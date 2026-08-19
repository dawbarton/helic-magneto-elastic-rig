//! Compile-time choices for this rig.
//!
//! Start here when adapting the rig: most laboratory choices are constants,
//! while physical pin assignments and analogue polarity live in `board.rs`.
//! The host discovers the resulting parameter and source tables, so these
//! choices do not require host-side indices. See "Things you set at compile
//! time" in the platform's `docs/user_guide.md` and "Adding an experiment" in
//! its `docs/developer_guide.md`.

use fw_magnetoelastic_rig::control::MagnetoelasticControl;
#[allow(unused_imports)]
pub use fw_magnetoelastic_rig::safety_limits::{
    DAC_OUT_CEILING_V, DAC_OUT_FLOOR_V, DAC_VREF, MID_RAIL, SAFE_OUT_MAX_V, SAFE_OUT_MIN_V,
};
use helic_fw_support::net::NetConfig;
pub use helic_rt::SampleRate;

/// Name advertised during discovery. Protocol names are short ASCII strings.
pub const EXPERIMENT: &str = "magnetoelastic";

/// Maximum uploaded waveform length; storage is paid for by this rig.
pub const TABLE_CAPACITY: usize = 4096;

/// Fourier harmonics retained by each target and forcing generator.
pub const HARMONICS: usize = 16;

/// DAC channel driven by the control output. Its polarity is defined in
/// `board.rs` for the fitted analogue output stage.
pub const OUTPUT_CHANNEL: usize = 0;

/// Measuring range of the attached optoNCDT sensor in mm (model-dependent:
/// 10/25/50/100/200/500).
pub const LASER_RANGE_MM: f32 = 50.0;

// --- Output safety limits (enforced by the firmware safety gate) ---------
//
// These are hard constraints applied on core 1 after the controller/forcing/
// table sum, before the DAC write. They are compile-time here (edit and
// reflash to change). See "Output safety gate" in the platform's
// `docs/developer_guide.md`.

// The DAC channel bounds and the derived differential-command bounds live in
// `safety_limits.rs`, which is shared by the physical gate and PID residual
// limiter so those two constraints cannot diverge.

/// Safe tip-displacement window (laser, mm). Outside this the gate latches a
/// fault and holds the actuator quiet until the host re-arms. Conservative
/// bounds about the ~25 mm resting point.
pub const DISPLACEMENT_MIN_MM: f32 = 10.0;
pub const DISPLACEMENT_MAX_MM: f32 = 40.0;

/// Quiet the actuator if the laser has published no new frame for this long
/// (blind-feedback guard). Converted to a tick count from the sample rate in
/// `rig.rs`.
pub const LASER_STALE_AFTER_S: f32 = 0.02;

// --- Stator stage (stepper-driven micrometer) ----------------------------
//
// The stator's displacement is set by a two-phase stepper turning an imperial
// micrometer through an MP6500 carrier. Nothing here runs on core 1. See
// `docs/stator-stage.md` for the mechanism, the wiring, and the commissioning
// procedure; the constants below are the compile-time half of that document.

/// Micrometer barrel pitch. One turn advances the spindle by 1/40 inch, which
/// is 0.635 mm exactly, so the millimetre arithmetic downstream contains no
/// rounded conversion.
pub const MICROMETER_PITCH_MM: f32 = 25.4 / 40.0;

/// Full steps per motor revolution, and any reduction between motor and
/// barrel folded in. 200 is the standard 1.8 degree two-phase stepper.
///
/// **Measured on 2026-08-17**, not assumed: 800 commanded full steps moved the
/// barrel 0.100 inch exactly, four whole turns landing back on the same
/// graduation, and the return leg closed on the same line. That confirms 200
/// steps per revolution, direct coupling with no gearing, and
/// `STATOR_MICROSTEPS` at 1.0 all at once, since a 0.9 degree motor would have
/// given half the travel and strapped microstepping an eighth of it.
pub const STATOR_FULL_STEPS_PER_REV: f32 = 200.0;

/// Microsteps per full step, set by the MS1/MS2 strapping at the driver.
///
/// This must match the hardware, and the error is asymmetric: too low makes
/// every move proportionally short, which is harmless, while too high makes
/// every move proportionally long, into the end stop. The soft travel limits
/// do not protect against the second case, because they are converted to steps
/// with this same constant. It therefore stays at 1.0, matching an unmodified
/// MP6500 carrier (both MS pins are pulled low internally), and is raised to
/// 8.0 only once MS1/MS2 are confirmed strapped high by inspection.
///
/// The 1.0 was confirmed against the hardware on 2026-08-17 by the barrel
/// measurement described on `STATOR_FULL_STEPS_PER_REV`, so the counter's unit
/// is currently a full step of 3.175 um despite the name used throughout.
pub const STATOR_MICROSTEPS: f32 = 1.0;

/// True when advancing the spindle increases the micrometer's barrel reading.
/// A hardware fact about the fitted micrometer, not a free choice.
pub const STATOR_ADVANCE_INCREASES_READING: bool = true;

/// Signed millimetres of barrel reading per microstep in the advance
/// direction, where advance is the direction in which the spindle pushes the
/// stage. Carrying the sign here keeps every conversion downstream a plain
/// multiply, and makes reversing the convention a one-line change.
pub const STATOR_MM_PER_MICROSTEP: f32 = if STATOR_ADVANCE_INCREASES_READING {
    MICROMETER_PITCH_MM / (STATOR_FULL_STEPS_PER_REV * STATOR_MICROSTEPS)
} else {
    -MICROMETER_PITCH_MM / (STATOR_FULL_STEPS_PER_REV * STATOR_MICROSTEPS)
};

/// DIR level that advances the spindle, pushing the stage. Hardware fact.
pub const STATOR_DIR_ADVANCE_HIGH: bool = true;

/// ENABLE level that energises the driver.
///
/// The fitted carrier uses the A4988-compatible **active-low** enable, so a low
/// level energises the motor. Measured on the bench on 2026-08-14, after an
/// initial guess of active-high left the driver disabled for the whole of every
/// commanded move and the motor did not turn at all. Like `DAC_POLARITY`, this
/// describes the fitted hardware and is not a free choice.
///
/// The pin therefore rests high, and the enable input has an internal
/// pull-down, so a microcontroller in reset or unpowered floats the driver
/// **enabled**. Fit an external pull-up to the 3.3 V the connector already
/// carries on pin 2, so the resting state of a dead controller is a
/// de-energised motor.
pub const STATOR_ENABLE_ACTIVE_HIGH: bool = false;

/// Opto sensor level seen **below** the datum edge, that is on the retracted
/// side. The sensor is an edge rather than a vane, so this level alone says
/// which side of the datum the stage is on.
///
/// Measured on 2026-08-14 and on every crossing since: advancing across the
/// edge takes the level from high to low, so high is the retracted side.
pub const STATOR_OPTO_HIGH_BELOW_DATUM: bool = true;

/// Soft travel window, as **barrel readings** rather than as distances from
/// the datum.
///
/// The readings are the physically fixed quantity: the hard stops sit at
/// particular points on the barrel, whereas the datum is an estimate that a
/// later home may revise. Expressing the window this way means the step bounds
/// follow any correction to `rig_stator_datum` and keep describing the same
/// two physical positions.
///
/// Measured by hand on 2026-08-17; `STATOR_TRAVEL_MIN_MM` widened to two
/// barrel turns clear of the hard stop on 2026-08-18 (was one turn, 3.175 mm):
///
/// | | Barrel | Note |
/// |---|---|---|
/// | Lower hard stop | 0.100 inch, 2.540 mm | measured |
/// | `STATOR_TRAVEL_MIN_MM` | 0.250 inch, 6.35 mm | two barrel turns clear of it |
/// | Datum | 0.4176875 inch, 10.609 mm | see `STATOR_DATUM_MM` |
/// | `STATOR_TRAVEL_MAX_MM` | 0.710 inch, 18.034 mm | chosen bound |
/// | Upper hard stop | 0.715 to 0.720 inch | measured, only 0.005 to 0.010 inch clear |
///
/// **The margins are still unequal, and the upper one is still the tight
/// one**: 3.81 mm of run-out below (was 0.635 mm), against 0.13 to 0.25 mm
/// above. The upper limit is close to the stop because that is where the
/// interesting operating case lies. Until the hardware is revised to give
/// more headroom, anything approaching `STATOR_TRAVEL_MAX_MM` deserves more
/// caution than the lower end, and the homing approach is bounded
/// accordingly; see `STATOR_APPROACH_MAX_MM`.
///
/// Note the datum no longer sits near the midpoint of this window after the
/// widening: it is now noticeably closer to `STATOR_TRAVEL_MIN_MM` (4.26 mm
/// away) than to `STATOR_TRAVEL_MAX_MM` (7.42 mm away). An earlier revision
/// of this file assumed the datum was at an extreme of travel and derived a
/// one-sided window from that assumption; it is not, at either width.
pub const STATOR_TRAVEL_MIN_MM: f32 = 6.35;
pub const STATOR_TRAVEL_MAX_MM: f32 = 18.034;

/// Delay between energising the driver and the first step. The driver's own
/// wake is the smaller part of this: coil current is restored in well under a
/// millisecond, and the indexer keeps its phase while the outputs are off. The
/// limit is mechanical. While de-energised the rotor is held only by detent and
/// friction, so re-energising snaps it back to the held phase, and that snap
/// rings for tens of milliseconds against the rotor's inertia. Stepping into a
/// ringing rotor is how a move loses steps at its start. Paid at most once per
/// move, against a move that already takes seconds, so it is bought cheaply.
pub const STATOR_WAKE_MS: u64 = 100;

/// How long to hold current after a move before de-energising, when
/// `rig_stator_hold` is zero. Long enough for the mechanism to stop ringing.
pub const STATOR_HOLD_AFTER_MOVE_MS: u64 = 1000;

/// Default traverse rate. 0.5 mm/s is about 157 full steps per second, safely
/// below the pull-in rate of a small stepper under load, so no acceleration
/// ramp is needed. Raising this much will need one.
pub const STATOR_RATE_MM_S: f32 = 0.5;

/// Homing search rate, and the slow rate for the final approach to the datum.
/// The slow rate is what the datum's repeatability is bought with.
pub const STATOR_HOME_FAST_MM_S: f32 = 0.5;
pub const STATOR_HOME_SLOW_MM_S: f32 = 0.1;

/// Distance retracted below the datum edge before the final slow advance onto
/// it.
///
/// 0.2 mm is 63 full steps against a reversal dead band measured at 19 to 20
/// on 2026-08-17, so the backoff moves the flag properly with about three
/// times the margin it needs. Before the coupling rework of that day a
/// retraction transmitted no motion at all and this was inert at any setting;
/// that is no longer the case.
pub const STATOR_HOME_BACKOFF_MM: f32 = 0.2;

/// Bound on homing's **coarse** phase, the retract that gets the stage below
/// the datum before the final approach. It has to exceed the travel available
/// above the datum, 7.43 mm, so the datum is reachable from anywhere in the
/// window; retracting is the safe direction to overshoot in, because the lower
/// stop has 0.635 mm of run-out and lies 7.4 mm further on.
///
/// Also bounds a single jog on an unhomed axis, where no window is enforced,
/// so a mistyped distance cannot traverse the micrometer in one command.
pub const STATOR_SEEK_MAX_MM: f32 = 8.0;

/// Bound on homing's **final approach**, the slow advance from the backoff
/// position onto the datum edge. Separate from, and far smaller than,
/// [`STATOR_SEEK_MAX_MM`], because this is the only part of homing that
/// advances blind and the upper hard stop has barely 0.13 mm of run-out.
///
/// It must exceed **the FIFO lead, plus the backoff, plus the dead band**,
/// because the phase before it stops a FIFO lead past the retracting crossing
/// and then retreats by the backoff. Measured on the first home, 2026-08-17:
/// 11 + 63 + 20 = 94 full steps used of the 126 this allows, so the margin
/// covers the dead band growing from 20 steps to about 52 before homing starts
/// refusing. That refusal is informative rather than dangerous, and a dead
/// band that has more than doubled is something to know about.
///
/// At 0.4 mm it also caps the net upward excursion of a homing sequence at
/// 0.2 mm.
///
/// The residual hazard, stated plainly: a sensor that fails reading "below the
/// datum" skips the coarse phase, so homing advances the full 0.4 mm blind. If
/// the stage were already at `STATOR_TRAVEL_MAX_MM` that could touch the upper
/// stop, and the current-limit potentiometer is what makes it a stall rather
/// than damage. No bound can remove this: reaching the edge at all requires
/// advancing further than the backoff. It is the reason the potentiometer is
/// the axis's real mechanical fuse.
pub const STATOR_APPROACH_MAX_MM: f32 = 0.4;

/// Default overshoot for the unidirectional approach: how far below a target a
/// retracting move goes before advancing onto it, so that every move ends in
/// the same contact state.
///
/// 0.25 mm is 79 full steps against a reversal dead band measured at 19 to 20
/// on 2026-08-17, about four times what it needs. Before the coupling rework
/// of that day a retraction moved the stage not at all, and this compensation
/// was inert at any setting rather than merely undersized; it now does what it
/// was written to do. The measurement to repeat if the coupling is disturbed
/// again is the dead band, not this constant.
///
/// One consequence worth knowing: because a move approaches its target from
/// one backlash allowance below it, targets within `rig_stator_backlash` of
/// `STATOR_TRAVEL_MIN_MM` cannot be reached, and are refused.
pub const STATOR_BACKLASH_MM: f32 = 0.25;

/// Bounds accepted for the corresponding runtime parameters.
///
/// `MAX_STATOR_RATE_MM_S` was 3.0 until 2026-08-18; **measured on 2026-08-17,
/// 1.5 mm/s lost 17 steps in one cycle of three and 3.0 mm/s lost 16 every
/// cycle, silently, with no fault raised**, because this axis has no
/// acceleration ramp. Only 0.5 mm/s, the default `STATOR_RATE_MM_S`, was
/// measured clean (zero lost steps over some forty thousand). The bound now
/// enforces that evidence in firmware rather than leaving it as an operator
/// instruction in `notes.md`/`AGENTS.md`; raise it only alongside an
/// acceleration ramp and fresh loss measurements at the higher rate.
///
/// A backlash allowance of more than a couple of millimetres means something
/// mechanical is wrong rather than needing compensation.
pub const MAX_STATOR_RATE_MM_S: f32 = 0.5;
pub const MAX_STATOR_BACKLASH_MM: f32 = 2.0;

/// Barrel reading at the opto datum, in mm. `rig_stator_datum` overrides it at
/// runtime, and like `LASER_RANGE_MM` that override is not persisted across a
/// reflash, which is why the measured value lives here too.
///
/// Measured 2026-08-17: 0.4176875 inch, from a barrel reading of 0.418 inch
/// taken with the counter parked 2.5 full steps above the datum edge. The
/// half step is not spurious precision: the sensor's trip point sits between
/// two step positions, so the advancing crossing dithers between counter 160
/// and 161, and the midpoint is the value homing will produce on average.
///
/// The uncertainty is about **one full step, 3.2 um**, not the half-division
/// a barrel reading would normally carry. Nothing was interpolated between
/// graduations: the axis was stepped until the barrel coincided with a line,
/// which makes it a null reading whose precision is how well coincidence can
/// be seen, and the exactly known step offset then carries it back to the
/// datum. The residual is the half-step dither of the trip point.
///
/// The *absolute* accuracy of the barrel does not enter, because nothing
/// depends on it. This constant only fixes an origin; every quantity the
/// experiment uses is a relative move from the datum.
pub const STATOR_DATUM_MM: f32 = 10.609;

/// Hold current between moves by default. Zero de-energises, which is the
/// quieter state for a rig measuring a sense coil at the microvolt level, and
/// is safe because the micrometer's thread is self-locking.
pub const STATOR_HOLD_DEFAULT: f32 = 0.0;

/// optoNCDT measuring-rate command matched to the hardware sample clock.
///
/// The sensor command uses kHz, and must end in LF.
pub const LASER_MEASRATE_COMMAND: &[u8] = match SAMPLE_RATE {
    SampleRate::Hz1000 => b"MEASRATE 1\n",
    SampleRate::Hz2000 => b"MEASRATE 2\n",
    SampleRate::Hz4000 => b"MEASRATE 4\n",
    SampleRate::Hz8000 => b"MEASRATE 8\n",
};

/// Static IPv4 address and prefix length. Configuration is not persisted;
/// edit and reflash to change it.
pub const NET_CONFIG: NetConfig = NetConfig::Static {
    address: [192, 168, 1, 235],
    prefix: 24,
};

/// Locally administered MAC address.
pub const MAC_ADDR: [u8; 6] = [0x02, 0x48, 0x4C, 0x00, 0x00, 0x01];

/// Run-time-selectable, statically dispatched control policy.
pub type ActiveController = MagnetoelasticControl<HARMONICS>;
/// Statically selected core-1 programme.
pub type ActiveProgram = helic_rt::StandardProgram<ActiveController, HARMONICS, TABLE_CAPACITY>;

/// Construct the one controller instance which is later moved to core 1.
///
/// Keep constructor defaults consistent with the controller's `param_value`
/// implementation so the host-visible parameter shadow starts correctly.
pub fn make_controller() -> ActiveController {
    MagnetoelasticControl::new()
}

/// Selected sample-rate preset. The preset supplies exact PWM divider values;
/// do not replace the hardware-timed clock with a software timer.
#[cfg(feature = "diag-sample-4k")]
pub const SAMPLE_RATE: SampleRate = SampleRate::Hz4000;
#[cfg(not(feature = "diag-sample-4k"))]
pub const SAMPLE_RATE: SampleRate = SampleRate::Hz8000;
