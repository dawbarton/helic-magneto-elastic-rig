//! PIO step-pulse generator: one STEP pulse per FIFO word.
//!
//! A state machine emits one pulse for each 32-bit word pushed to its TX FIFO,
//! and the word sets the interval that follows the pulse. Two properties earn
//! the PIO its place here, over toggling a pin from a timer on core 0:
//!
//! - Pulse timing does not depend on core-0 load, which on this rig includes
//!   the network stack and the laser UART.
//! - A starved FIFO **stretches one step interval**. It cannot lose a pulse or
//!   emit a spurious one, so the step count stays exactly the number of words
//!   pushed. That is what makes it safe to drive an open-loop axis from a core
//!   that has no real-time guarantee.
//!
//! Nothing here is specific to this rig; the stator's calibration, homing, and
//! backlash handling live in `stator.rs`. This module is a candidate for
//! `helic-fw-rt` alongside `ssi_pio` and `pulse_pio` once a second rig wants
//! it. See "Where this code should eventually live" in
//! `docs/stator-stage.md`.

use embassy_rp::clocks::clk_sys_freq;
use embassy_rp::pio::{
    Common, Config, Direction, FifoJoin, Instance, PioPin, ShiftConfig, ShiftDirection,
    StateMachine,
};
use embassy_rp::Peri;
use fixed::traits::ToFixed;

/// PIO instruction clock. One instruction per microsecond makes each FIFO word
/// a direct microsecond count, which keeps the arithmetic auditable at the
/// cost of an upper step rate far above anything a micrometer wants.
const TICK_HZ: u32 = 1_000_000;

/// STEP high time, fixed by the side-set delay in the program. Well above the
/// MP6500's 1 µs minimum, and irrelevant to the step rate at these speeds.
const PULSE_HIGH_US: u32 = 8;

/// Instruction cycles one pulse costs outside its delay loop: the blocking
/// `out`, the eight-cycle high phase, and the final non-jumping `jmp`.
const PULSE_OVERHEAD_US: u32 = PULSE_HIGH_US + 2;

/// Shortest interval the program can express, and hence the fastest step rate.
pub const MIN_PERIOD_US: u32 = PULSE_OVERHEAD_US + 1;

/// Longest interval, bounded by the 32-bit delay counter.
pub const MAX_PERIOD_US: u32 = u32::MAX / 2;

/// One state machine driving a single STEP output.
pub struct StepPulser<'d, PIO: Instance, const SM: usize> {
    sm: StateMachine<'d, PIO, SM>,
}

impl<'d, PIO: Instance, const SM: usize> StepPulser<'d, PIO, SM> {
    /// Load the program and start the state machine, idle and waiting on its
    /// FIFO. No pulse is emitted until a word is pushed.
    pub fn new(
        common: &mut Common<'d, PIO>,
        mut sm: StateMachine<'d, PIO, SM>,
        step: Peri<'d, impl PioPin + 'd>,
    ) -> Self {
        // `out x, 32` blocks on an empty FIFO with autopull enabled, which is
        // the idle state. The delay loop runs x+1 times, so the total interval
        // is x + PULSE_OVERHEAD_US instruction cycles.
        let program = embassy_rp::pio::program::pio_asm!(
            r#"
                .side_set 1
            .wrap_target
                out x, 32           side 0
                nop                 side 1 [7]
            low:
                jmp x--, low        side 0
            .wrap
            "#
        );
        let program = common.load_program(&program.program);
        let step = common.make_pio_pin(step);

        let mut config = Config::default();
        config.use_program(&program, &[&step]);
        config.shift_out = ShiftConfig {
            threshold: 32,
            direction: ShiftDirection::Right,
            auto_fill: true,
        };
        // The RX FIFO is unused, so join it to the TX side for eight words of
        // slack. Deeper buffering means fewer wakeups per step; it also sets
        // how far an aborted move overruns, which `drain` then resolves.
        config.fifo_join = FifoJoin::TxOnly;
        let sys_hz = clk_sys_freq().to_fixed::<fixed::FixedU64<fixed::types::extra::U8>>();
        let tick_hz = TICK_HZ.to_fixed::<fixed::FixedU64<fixed::types::extra::U8>>();
        config.clock_divider = (sys_hz / tick_hz).to_fixed();

        sm.set_config(&config);
        sm.set_pin_dirs(Direction::Out, &[&step]);
        sm.set_enable(true);
        Self { sm }
    }

    /// Queue one pulse followed by an interval of `period_us`, waiting for FIFO
    /// space. Returns once the word is accepted, which precedes the pulse
    /// itself by however much is already queued.
    pub async fn emit(&mut self, period_us: u32) {
        let period = period_us.clamp(MIN_PERIOD_US, MAX_PERIOD_US);
        self.sm.tx().wait_push(period - PULSE_OVERHEAD_US).await;
    }

    /// True once every queued word has been consumed. The final pulse may still
    /// be in flight, so `stator.rs` follows this with one period of settling.
    pub fn queue_empty(&mut self) -> bool {
        self.sm.tx().empty()
    }
}
