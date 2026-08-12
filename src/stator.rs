//! Stator stage: a core-0 stepper axis driving an imperial micrometer.
//!
//! The stage sets the stator/specimen gap, which is a parameter of the
//! experiment rather than a signal: set between measurements, held during a
//! capture, never swept continuously. `docs/stator-stage.md` is the user-facing
//! description; this module is its implementation.
//!
//! Two facts shape everything here.
//!
//! **The stage is not spring preloaded.** A micrometer spindle can push but not
//! pull, so the stage follows on an advance and is merely released on a
//! retract. Ending every move with an advance is therefore not a refinement to
//! trim a few microns of backlash, it is the only thing that makes the axis
//! deterministic. `settled` tracks whether the spindle is known to be in
//! contact, and the reported position is NaN when it is not.
//!
//! **The axis is open loop.** The opto sensor reports one edge, not a
//! continuous measurement, so lost steps are invisible until the next home.
//! Re-homing publishes the discrepancy through `stator_home_error`, whose noise
//! floor is the datum's own repeatability of about 64 microsteps.
//!
//! Nothing in this module runs on core 1, and nothing it calls may be reached
//! from a tick. The one exception is [`issue_command`], which `set_param` calls
//! from the real-time core and which is inlined for that reason.

use core::sync::atomic::Ordering;

use defmt::{info, warn};
use embassy_rp::gpio::{Input, Level, Output};
use embassy_rp::peripherals::PIO0;
use embassy_rp::pio::Pio;
use embassy_time::{Instant, Timer};
use helic_rt::RtShared;

use crate::board::StatorParts;
use crate::config;
use crate::step_pio::StepPulser;
use crate::telemetry::{
    StatorState, STATOR_BACKLASH_MM, STATOR_COMMAND, STATOR_DATUM_MM, STATOR_FAULTS, STATOR_HOLD,
    STATOR_HOMED, STATOR_HOME_ERROR, STATOR_JOG_MM, STATOR_MOVES, STATOR_POSITION_MM,
    STATOR_RATE_MM_S, STATOR_STATE, STATOR_STEPS, STATOR_TARGET_MM,
};

/// Command kinds packed into the low byte of [`STATOR_COMMAND`].
pub const CMD_MOVE: u32 = 1;
pub const CMD_JOG: u32 = 2;
pub const CMD_HOME: u32 = 3;
pub const CMD_STOP: u32 = 4;

/// Publish a command from core 1's `set_param`.
///
/// The sequence number in the upper bits makes every parameter write a distinct
/// word, so writing the same target twice re-issues the move and the task acts
/// on each write exactly once. Core 1 is the only writer, so the
/// read-modify-write needs no atomicity beyond the individual accesses.
///
/// Inlined into the caller for the same reason as `wait_word_settle` in
/// `rig.rs`: `set_param` is SRAM-resident and must not call into flash.
#[inline(always)]
#[unsafe(link_section = ".data.ram_func")]
pub fn issue_command(kind: u32) {
    let previous = STATOR_COMMAND.load(Ordering::Relaxed);
    STATOR_COMMAND.store(
        (previous.wrapping_add(0x100) & !0xff) | (kind & 0xff),
        Ordering::Relaxed,
    );
}

/// Why a motion stopped early. Each leaves the axis quiet, and the caller
/// decides whether that is a fault or the expected handover to a new command.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Abort {
    /// A newer command arrived and supersedes the one in progress.
    Command,
    /// The output safety gate armed. The stage moves orthogonally to the beam
    /// and the laser axis, so this is not about the displacement guard; it is
    /// that the gap is a parameter of the system, and moving it under a running
    /// controller changes the dynamics beneath it while injecting the driver's
    /// switching noise into the measurement.
    Armed,
    /// A reboot was requested.
    Reboot,
}

pub struct StatorAxis {
    pulses: StepPulser<'static, PIO0, 0>,
    dir: Output<'static>,
    enable: Output<'static>,
    opto: Input<'static>,
    shared: &'static RtShared,
    /// Signed microsteps from the datum, in the advance direction. Counts words
    /// queued rather than pulses emitted, so during a move it leads the
    /// mechanism by at most the FIFO depth; `drain` closes that gap before any
    /// position is reported as final.
    steps: i32,
    homed: bool,
    /// True while the spindle is known to be in contact with the stage, which
    /// after a retract it is not.
    settled: bool,
    energised: bool,
    seen_command: u32,
}

/// Read a commanded `f32`, falling back to its compile-time default if the
/// host has not written one or wrote something unusable.
fn commanded(cell: &core::sync::atomic::AtomicU32, fallback: f32) -> f32 {
    let value = f32::from_bits(cell.load(Ordering::Relaxed));
    if value.is_finite() && value != 0.0 {
        value
    } else {
        fallback
    }
}

/// Round half away from zero. `f32::round` is not in `core`, and pulling in
/// `libm` for one operation on a path that runs at a few hundred hertz is not
/// worth a new entry in `dependency-policy.toml`. The `as` cast truncates
/// toward zero and saturates, so a non-finite input lands on zero rather than
/// wrapping.
fn round_to_i32(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

/// Millimetres of travel converted to a count of microsteps, sign discarded.
fn magnitude_steps(mm: f32) -> u32 {
    round_to_i32(mm.abs() / config::STATOR_MM_PER_MICROSTEP.abs()).unsigned_abs()
}

impl StatorAxis {
    /// Step interval for a rate in mm/s, bounded so that a mistyped rate
    /// cannot ask the PIO for an interval it can neither express nor survive.
    fn period_us(rate_mm_s: f32) -> u32 {
        let steps_per_s =
            (rate_mm_s.abs() / config::STATOR_MM_PER_MICROSTEP.abs()).clamp(1.0, 20_000.0);
        (1_000_000.0 / steps_per_s) as u32
    }

    fn datum_mm(&self) -> f32 {
        commanded(&STATOR_DATUM_MM, config::STATOR_DATUM_MM)
    }

    /// Barrel reading for a step count, and the inverse. The sign convention
    /// lives entirely in `STATOR_MM_PER_MICROSTEP`, so both are plain
    /// arithmetic.
    fn reading_for(&self, steps: i32) -> f32 {
        self.datum_mm() + steps as f32 * config::STATOR_MM_PER_MICROSTEP
    }

    fn steps_for(&self, reading_mm: f32) -> i32 {
        round_to_i32((reading_mm - self.datum_mm()) / config::STATOR_MM_PER_MICROSTEP)
    }

    /// Soft travel window in advance-direction microsteps, measured from the
    /// datum. Asymmetric by construction: the datum is at an extreme, so one
    /// side is the working range and the other only the clearance to the hard
    /// stop. Both configured distances are already along the advance and
    /// retract directions, so no sign convention enters here.
    fn travel_window(&self) -> (i32, i32) {
        let advance = magnitude_steps(config::STATOR_TRAVEL_ADVANCE_MM) as i32;
        let retract = magnitude_steps(config::STATOR_TRAVEL_RETRACT_MM) as i32;
        (-retract, advance)
    }

    /// True when the stage is past the datum edge, in the short clearance
    /// between it and the hard stop.
    fn beyond_datum(&self) -> bool {
        self.opto.is_high() == config::STATOR_OPTO_HIGH_BEYOND_DATUM
    }

    fn abort_reason(&self) -> Option<Abort> {
        if STATOR_COMMAND.load(Ordering::Relaxed) != self.seen_command {
            Some(Abort::Command)
        } else if self.shared.safety.load_inputs().armed {
            Some(Abort::Armed)
        } else if self.shared.reboot.is_requested() {
            Some(Abort::Reboot)
        } else {
            None
        }
    }

    fn publish(&self) {
        STATOR_STEPS.store((self.steps as f32).to_bits(), Ordering::Relaxed);
        STATOR_HOMED.store(self.homed as u32, Ordering::Relaxed);
        // Position is meaningful only while the spindle is known to be in
        // contact with the stage. A capture whose `stator` column is NaN is one
        // whose gap should not be trusted.
        let reading = if self.homed && self.settled {
            self.reading_for(self.steps)
        } else {
            f32::NAN
        };
        STATOR_POSITION_MM.store(reading.to_bits(), Ordering::Relaxed);
    }

    fn set_state(&self, state: StatorState) {
        STATOR_STATE.store(state as u32, Ordering::Relaxed);
    }

    fn idle_state(&self) -> StatorState {
        if self.homed {
            StatorState::Idle
        } else {
            StatorState::NotHomed
        }
    }

    async fn energise(&mut self) {
        if !self.energised {
            self.enable
                .set_level(level(config::STATOR_ENABLE_ACTIVE_HIGH));
            self.energised = true;
            Timer::after_millis(config::STATOR_WAKE_MS).await;
        }
    }

    fn de_energise(&mut self) {
        if self.energised {
            self.enable
                .set_level(level(!config::STATOR_ENABLE_ACTIVE_HIGH));
            self.energised = false;
        }
    }

    /// Wait for every queued word to be consumed and the last pulse to finish.
    /// After this the step count describes the mechanism exactly.
    async fn drain(&mut self, period_us: u32) {
        while !self.pulses.queue_empty() {
            Timer::after_micros(period_us as u64).await;
        }
        Timer::after_micros(2 * period_us as u64).await;
    }

    /// Point DIR at the requested direction. The driver latches DIR on each
    /// STEP edge, so the queue is drained first and given a settling gap;
    /// milliseconds here are free and the MP6500 needs nanoseconds.
    async fn set_direction(&mut self, advance: bool, period_us: u32) {
        self.drain(period_us).await;
        self.dir
            .set_level(level(advance == config::STATOR_DIR_ADVANCE_HIGH));
        Timer::after_millis(1).await;
    }

    /// Queue one step. With `settle`, wait for it to be executed before
    /// returning, which is what keeps the homing approach's sensor reads in
    /// step with the mechanism rather than up to a FIFO depth ahead of it.
    async fn one_step(&mut self, advance: bool, period_us: u32, settle: bool) -> Result<(), Abort> {
        if let Some(abort) = self.abort_reason() {
            return Err(abort);
        }
        self.pulses.emit(period_us).await;
        self.steps += if advance { 1 } else { -1 };
        if settle {
            self.drain(period_us).await;
        }
        self.publish();
        Ok(())
    }

    async fn step_fixed(&mut self, count: u32, advance: bool, period_us: u32) -> Result<(), Abort> {
        for _ in 0..count {
            self.one_step(advance, period_us, false).await?;
        }
        self.drain(period_us).await;
        Ok(())
    }

    /// Step until the opto sensor reports the requested side of the datum
    /// edge, or until `limit` steps have been taken. Returns whether the edge
    /// was found; not finding it within the bound is a fault rather than a
    /// reason to keep going, because the alternative is a hard stop.
    async fn step_until_beyond(
        &mut self,
        want_beyond: bool,
        advance: bool,
        period_us: u32,
        limit: u32,
        settle: bool,
    ) -> Result<bool, Abort> {
        for _ in 0..limit {
            if self.beyond_datum() == want_beyond {
                return Ok(true);
            }
            self.one_step(advance, period_us, settle).await?;
        }
        self.drain(period_us).await;
        Ok(self.beyond_datum() == want_beyond)
    }

    /// Find the datum and zero the step counter there.
    ///
    /// The datum is always latched **during an advance**, because that is the
    /// only motion that positions an unpreloaded stage. The datum also sits at
    /// one extreme of travel, with a hard stop just past it, so the sequence
    /// also has to bound how far it ever intrudes into that clearance. Which
    /// of those two constraints is easy depends on the geometry:
    ///
    /// - datum at the advanced extreme: advancing points at the edge, so the
    ///   approach runs from the working range and stops one step past the edge.
    ///   The retreat that clears the edge beforehand happens in the open range;
    /// - datum at the retracted extreme: advancing points away from the edge,
    ///   so homing must first enter the clearance, retreat into it by the
    ///   backoff, and advance back out onto the edge. Every part of that is
    ///   inside the clearance, which is why it is refused unless the clearance
    ///   is comfortably bigger than the backoff.
    ///
    /// Both approaches step one at a time, waiting for each pulse to be
    /// executed, so the sensor is read in step with the mechanism rather than a
    /// FIFO depth ahead of it. That matters here: at eight queued steps the
    /// overshoot would be the same size as the datum's whole repeatability
    /// budget.
    async fn home(&mut self) -> Result<(), Abort> {
        self.set_state(StatorState::Homing);

        let fast = Self::period_us(config::STATOR_HOME_FAST_MM_S);
        let slow = Self::period_us(config::STATOR_HOME_SLOW_MM_S);
        let seek = magnitude_steps(config::STATOR_SEEK_MAX_MM);
        let backoff = magnitude_steps(config::STATOR_HOME_BACKOFF_MM);
        let predicted = self.homed.then_some(self.steps);

        // The direction that moves toward the datum, and so toward the hard
        // stop just past it, expressed as an `advance` flag.
        let toward_datum = config::STATOR_DATUM_AT_ADVANCED_EXTREME;

        if !toward_datum && config::STATOR_HOME_BACKOFF_MM >= config::STATOR_DATUM_CLEARANCE_MM {
            // With the datum at the retracted extreme the whole approach runs
            // inside the clearance, so a backoff that does not fit in it would
            // home by driving into the stop.
            return self.lost("stator: homing backoff does not fit the datum clearance");
        }

        self.energise().await;
        self.settled = false;
        self.publish();

        // Leave the clearance if we start in it, moving away from the hard
        // stop, and then stand clear of the edge in the open range.
        if self.beyond_datum() {
            self.set_direction(!toward_datum, fast).await;
            if !self
                .step_until_beyond(false, !toward_datum, slow, seek, true)
                .await?
            {
                return self.lost("stator: homing could not leave the datum clearance");
            }
        }
        self.set_direction(!toward_datum, fast).await;
        self.step_fixed(backoff, !toward_datum, fast).await?;

        // Approach the edge from the open range.
        self.set_direction(toward_datum, slow).await;
        if !self
            .step_until_beyond(true, toward_datum, slow, seek, true)
            .await?
        {
            return self.lost("stator: homing search found no datum edge");
        }

        if toward_datum {
            // The approach was itself an advance, so the edge crossing already
            // sits at the end of one and defines the datum.
        } else {
            // The approach was a retract, which does not position the stage.
            // Retreat further into the clearance and come back out advancing,
            // so the crossing that defines the datum is made under contact.
            self.step_fixed(backoff, toward_datum, slow).await?;
            self.set_direction(true, slow).await;
            if !self
                .step_until_beyond(false, true, slow, seek, true)
                .await?
            {
                return self.lost("stator: homing approach did not reach the datum edge");
            }
        }
        self.drain(slow).await;

        if let Some(predicted) = predicted {
            // Lost-step audit: what the counter said the datum was, against
            // where it actually is. Only meaningful well outside the datum's
            // own repeatability.
            let error = self.steps - predicted;
            STATOR_HOME_ERROR.store((error as f32).to_bits(), Ordering::Relaxed);
            info!("stator: re-homed, {} microsteps from prediction", error);
        }
        self.steps = 0;
        self.homed = true;
        self.settled = true;
        self.publish();
        self.set_state(StatorState::Idle);
        Ok(())
    }

    /// Move to an absolute step count, always finishing with an advance.
    async fn move_to(&mut self, target: i32, enforce_window: bool) -> Result<(), Abort> {
        let rate = Self::period_us(commanded(&STATOR_RATE_MM_S, config::STATOR_RATE_MM_S));
        let backlash =
            magnitude_steps(commanded(&STATOR_BACKLASH_MM, config::STATOR_BACKLASH_MM)) as i32;
        let approach_from = target - backlash;

        if enforce_window {
            let (low, high) = self.travel_window();
            if target < low || target > high {
                return self.fail("stator: target outside the soft travel window");
            }
            // The retract undershoot has to fit too, so the usable target range
            // is the window narrowed by one backlash allowance at the low end.
            if approach_from < low {
                return self.fail("stator: no room below the target for the backlash approach");
            }
        }

        self.set_state(StatorState::Moving);
        self.energise().await;

        // A retract is needed when the target is behind us, and also when a
        // previous move was abandoned part-way through one: the spindle may
        // have parted from the stage, so contact has to be re-made from below
        // regardless of which way the target lies.
        if !self.settled || target < self.steps {
            if approach_from != self.steps {
                let advance = approach_from > self.steps;
                if !advance {
                    self.settled = false;
                    self.publish();
                }
                self.set_direction(advance, rate).await;
                self.step_fixed(self.steps.abs_diff(approach_from), advance, rate)
                    .await?;
            }
            self.set_direction(true, rate).await;
            self.step_fixed(backlash as u32, true, rate).await?;
        } else if target > self.steps {
            self.set_direction(true, rate).await;
            self.step_fixed(self.steps.abs_diff(target), true, rate)
                .await?;
        }

        self.settled = true;
        self.publish();
        STATOR_MOVES.fetch_add(1, Ordering::Relaxed);
        self.set_state(self.idle_state());
        Ok(())
    }

    async fn command_move(&mut self) -> Result<(), Abort> {
        if !self.homed {
            return self.fail("stator: absolute move refused, axis is not homed");
        }
        let target_mm = f32::from_bits(STATOR_TARGET_MM.load(Ordering::Relaxed));
        if !target_mm.is_finite() {
            return self.fail("stator: target is not a finite reading");
        }
        let target = self.steps_for(target_mm);
        self.move_to(target, true).await
    }

    async fn command_jog(&mut self) -> Result<(), Abort> {
        let jog_mm = f32::from_bits(STATOR_JOG_MM.load(Ordering::Relaxed));
        if !jog_mm.is_finite() {
            return self.fail("stator: jog is not a finite distance");
        }
        // Before homing there is no window to enforce, so bound the jog itself:
        // a mistyped distance should not be able to traverse the micrometer.
        if !self.homed && jog_mm.abs() > config::STATOR_SEEK_MAX_MM {
            return self.fail("stator: jog too large for an unhomed axis");
        }
        let target = self.steps + round_to_i32(jog_mm / config::STATOR_MM_PER_MICROSTEP);
        self.move_to(target, self.homed).await
    }

    /// Record a rejected command and return to a quiet idle state. The axis
    /// still knows where it is, so this is a refusal rather than a fault state.
    fn fail(&self, reason: &'static str) -> Result<(), Abort> {
        warn!("{}", reason);
        STATOR_FAULTS.fetch_add(1, Ordering::Relaxed);
        self.set_state(self.idle_state());
        Ok(())
    }

    /// Record a homing failure. A search that ran past its bound means the axis
    /// no longer knows where it is, so the datum is discarded and absolute
    /// moves are refused until it is homed successfully.
    fn lost(&mut self, reason: &'static str) -> Result<(), Abort> {
        warn!("{}", reason);
        STATOR_FAULTS.fetch_add(1, Ordering::Relaxed);
        self.homed = false;
        self.settled = false;
        self.publish();
        self.set_state(StatorState::Faulted);
        Ok(())
    }

    async fn run(&mut self) -> ! {
        self.publish();
        self.set_state(self.idle_state());
        let mut idle_since = Instant::now();

        loop {
            let command = STATOR_COMMAND.load(Ordering::Relaxed);
            if command != self.seen_command {
                self.seen_command = command;
                let outcome = match command & 0xff {
                    CMD_MOVE => self.command_move().await,
                    CMD_JOG => self.command_jog().await,
                    CMD_HOME => self.home().await,
                    // A stop needs no action of its own: issuing any command
                    // already aborted whatever was running.
                    _ => Ok(()),
                };
                if let Err(abort) = outcome {
                    match abort {
                        // The newer command is still pending and is picked up
                        // on the next pass, so this is a handover, not a fault.
                        Abort::Command => {}
                        Abort::Armed => {
                            warn!("stator: move abandoned, output safety gate armed");
                            STATOR_FAULTS.fetch_add(1, Ordering::Relaxed);
                        }
                        Abort::Reboot => {
                            warn!("stator: move abandoned, reboot requested");
                        }
                    }
                    self.set_state(self.idle_state());
                }
                idle_since = Instant::now();
            }

            // Hold current only while asked to, or briefly after a move so the
            // mechanism stops ringing against a held rotor. De-energised is the
            // quieter state for the sense coil, and the micrometer's thread is
            // self-locking, so the stage does not move when the motor lets go.
            let hold = f32::from_bits(STATOR_HOLD.load(Ordering::Relaxed)) != 0.0;
            if self.energised
                && !hold
                && idle_since.elapsed().as_millis() > config::STATOR_HOLD_AFTER_MOVE_MS
            {
                self.de_energise();
            }
            if self.shared.reboot.is_requested() {
                self.de_energise();
            }

            Timer::after_millis(20).await;
        }
    }
}

fn level(high: bool) -> Level {
    if high {
        Level::High
    } else {
        Level::Low
    }
}

/// Core 0: own the stepper and serve commands issued from the parameter
/// registry. Runs for the firmware's lifetime.
#[embassy_executor::task]
pub async fn stator_task(parts: StatorParts, shared: &'static RtShared) -> ! {
    let mut pio = Pio::new(parts.pio, crate::Irqs);
    let pulses = StepPulser::new(&mut pio.common, pio.sm0, parts.step);
    let mut axis = StatorAxis {
        pulses,
        dir: parts.dir,
        enable: parts.enable,
        opto: parts.opto,
        shared,
        steps: 0,
        // Step counts are RAM state, so a reboot loses the datum and the axis
        // comes up refusing absolute moves until it is homed again.
        homed: false,
        settled: false,
        energised: false,
        seen_command: STATOR_COMMAND.load(Ordering::Relaxed),
    };
    info!("stator: axis ready, not homed");
    axis.run().await
}
