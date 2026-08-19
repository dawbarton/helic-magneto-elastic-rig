//! Run-time selection between open-loop, PID, and phase-locked excitation.

use helic_core::{Pid, PidConfig, Pll, PllConfig, PllState};
use helic_rt::{ControlStep, Payload, SampleRate, StandardControl, StandardControlInputs, StepCtx};

use crate::safety_limits::{SAFE_OUT_MAX_V, SAFE_OUT_MIN_V};

pub const COIL_INPUT: usize = 0;
pub const DRIVE_INPUT: usize = 1;
pub const LASER_INPUT: usize = 2;
pub const STATOR_INPUT: usize = 3;

pub const CONTROL_MODE_ID: u16 = 0;
pub const CTRL_RESET_ID: u16 = 1;
pub const PLL_REACQUIRE_ID: u16 = 2;
pub const PID_KP_ID: u16 = 3;
pub const PID_KI_ID: u16 = 4;
pub const PID_KD_ID: u16 = 5;
pub const PID_TAU_D_ID: u16 = 6;
pub const PLL_CENTRE_ID: u16 = 7;
pub const PLL_MIN_ID: u16 = 8;
pub const PLL_MAX_ID: u16 = 9;
pub const PLL_KP_ID: u16 = 10;
pub const PLL_KI_ID: u16 = 11;
pub const PLL_TARGET_PHASE_ID: u16 = 12;
pub const PLL_DELAY_ID: u16 = 13;
pub const PLL_DC_TAU_ID: u16 = 14;
pub const PLL_DEMOD_TAU_ID: u16 = 15;
pub const PLL_EXCITATION_MIN_ID: u16 = 16;
pub const PLL_RESPONSE_MIN_ID: u16 = 17;
pub const PLL_LOCK_PHASE_TOL_ID: u16 = 18;
pub const PLL_UNLOCK_PHASE_TOL_ID: u16 = 19;
pub const PLL_LOCK_FREQ_TOL_ID: u16 = 20;
pub const PLL_LOCK_DWELL_ID: u16 = 21;
pub const PLL_UNLOCK_DWELL_ID: u16 = 22;
pub const PLL_ACQUIRE_TIMEOUT_ID: u16 = 23;
pub const PLL_SATURATION_DWELL_ID: u16 = 24;
pub const CONTROL_PARAM_COUNT: usize = 25;

/// No PID-entry tolerance has yet been established on hardware. Leaving this
/// false makes PID selection testable but prevents it from energising the rig
/// until quietness and absolute-position evidence is recorded in `notes.md`.
pub const PID_ENTRY_LIMITS_VERIFIED: bool = false;
pub const PID_ENTRY_DWELL_S: f32 = 0.25;
pub const PID_ENTRY_QUIET_MM: f32 = 0.0;
pub const PID_ENTRY_ERROR_MAX_MM: f32 = 0.0;

/// Untuned but internally valid PLL defaults. Zero gains and a zero-width
/// frequency window ensure that construction cannot create active tracking.
pub const fn default_pll_config() -> PllConfig {
    PllConfig {
        centre_increment: 0,
        min_increment: 0,
        max_increment: 0,
        proportional_gain: 0.0,
        integral_gain: 0.0,
        target_phase_deg: -90.0,
        delay_s: 0.0,
        dc_time_constant_s: 1.0,
        demod_time_constant_s: 0.1,
        min_excitation_amplitude: 0.0,
        min_response_amplitude: 0.0,
        lock_phase_tolerance_deg: 5.0,
        unlock_phase_tolerance_deg: 10.0,
        lock_frequency_tolerance: 0.0,
        lock_dwell_s: 0.1,
        unlock_dwell_s: 0.1,
        acquire_timeout_s: 5.0,
        saturation_dwell_s: 0.1,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ControlMode {
    None = 0,
    Pid = 1,
    Pll = 2,
}

impl ControlMode {
    pub const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Pid),
            2 => Some(Self::Pll),
            _ => None,
        }
    }
}

const CONTROL_TELEMETRY: &[(&str, &str)] = &[
    ("control_mode", "enum"),
    ("pid_error", "mm"),
    ("pll_phase", "degree"),
    ("pll_phase_error", "degree"),
    ("pll_exc_amp", "V"),
    ("pll_resp_amp", "mm"),
    ("pll_freq_actual", "Hz"),
    ("pll_state", "enum"),
];

/// Rig-local control policy moved to core 1 by `StandardProgram`.
pub struct MagnetoelasticControl<const H: usize> {
    mode: ControlMode,
    pending_mode: Option<ControlMode>,
    pid: Pid,
    pll: Pll<H>,
    output_enabled: bool,
    pid_error: f32,
    frequency_actual_hz: f32,
    entry_elapsed_s: f32,
    entry_sum_mm: f32,
    entry_samples: u32,
    entry_min_mm: f32,
    entry_max_mm: f32,
    qualified_reference_mean: f32,
    pid_qualified: bool,
    pid_entry_fault: bool,
    mode_change_fault: bool,
    internal_fault: bool,
}

impl<const H: usize> MagnetoelasticControl<H> {
    pub fn new() -> Self {
        Self {
            mode: ControlMode::None,
            pending_mode: None,
            pid: Pid::new(PidConfig::default()),
            pll: Pll::new(default_pll_config()),
            output_enabled: false,
            pid_error: f32::NAN,
            frequency_actual_hz: 0.0,
            entry_elapsed_s: 0.0,
            entry_sum_mm: 0.0,
            entry_samples: 0,
            entry_min_mm: f32::INFINITY,
            entry_max_mm: f32::NEG_INFINITY,
            qualified_reference_mean: f32::NAN,
            pid_qualified: false,
            pid_entry_fault: false,
            mode_change_fault: false,
            internal_fault: false,
        }
    }

    #[inline]
    #[cfg_attr(feature = "rt-sram", unsafe(link_section = ".data.ram_func"))]
    fn reset_pid_entry(&mut self) {
        self.pid.reset();
        self.pid_error = f32::NAN;
        self.entry_elapsed_s = 0.0;
        self.entry_sum_mm = 0.0;
        self.entry_samples = 0;
        self.entry_min_mm = f32::INFINITY;
        self.entry_max_mm = f32::NEG_INFINITY;
        self.qualified_reference_mean = f32::NAN;
        self.pid_qualified = false;
        self.pid_entry_fault = false;
    }

    #[inline]
    #[cfg_attr(feature = "rt-sram", unsafe(link_section = ".data.ram_func"))]
    fn full_reset(&mut self) {
        self.reset_pid_entry();
        self.pll.reset();
        self.mode_change_fault = false;
        self.internal_fault = false;
    }

    #[inline]
    #[cfg_attr(feature = "rt-sram", unsafe(link_section = ".data.ram_func"))]
    fn accept_pending_mode(&mut self) -> bool {
        self.mode_change_fault = false;
        let Some(mode) = self.pending_mode.take() else {
            return false;
        };
        self.mode = mode;
        self.full_reset();
        if self.output_enabled {
            self.mode_change_fault = true;
        }
        true
    }

    #[inline]
    #[cfg_attr(feature = "rt-sram", unsafe(link_section = ".data.ram_func"))]
    fn qualify_pid(&mut self, laser: f32, reference_mean: f32, dt_s: f32) -> bool {
        if !laser.is_finite() || !reference_mean.is_finite() {
            self.pid_entry_fault = true;
            return false;
        }
        self.entry_elapsed_s += dt_s;
        self.entry_sum_mm += laser;
        self.entry_samples = self.entry_samples.saturating_add(1);
        self.entry_min_mm = self.entry_min_mm.min(laser);
        self.entry_max_mm = self.entry_max_mm.max(laser);
        if self.entry_elapsed_s < PID_ENTRY_DWELL_S {
            return false;
        }
        let mean = self.entry_sum_mm / self.entry_samples as f32;
        let quiet = self.entry_max_mm - self.entry_min_mm <= PID_ENTRY_QUIET_MM;
        let aligned = (reference_mean - mean).abs() <= PID_ENTRY_ERROR_MAX_MM;
        if PID_ENTRY_LIMITS_VERIFIED && quiet && aligned {
            self.qualified_reference_mean = reference_mean;
            self.pid_qualified = true;
            true
        } else {
            self.pid_entry_fault = true;
            false
        }
    }

    #[inline]
    #[cfg_attr(feature = "rt-sram", unsafe(link_section = ".data.ram_func"))]
    fn apply_checked(&mut self, result: bool) {
        if !result {
            self.internal_fault = true;
        }
    }
}

impl<const H: usize> Default for MagnetoelasticControl<H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const H: usize> StandardControl<H> for MagnetoelasticControl<H> {
    const INPUTS_REQUIRED: usize = LASER_INPUT + 1;
    const REFERENCE_UNIT: &'static str = "mm";
    const TELEMETRY: &'static [(&'static str, &'static str)] = CONTROL_TELEMETRY;

    #[inline]
    #[cfg_attr(feature = "rt-sram", unsafe(link_section = ".data.ram_func"))]
    fn step(&mut self, inputs: StandardControlInputs<'_, H>, ctx: &StepCtx<'_>) -> ControlStep {
        let mode_changed = self.accept_pending_mode();
        self.frequency_actual_hz =
            inputs.current_increment as f32 * (ctx.sample_rate.hz() / 4_294_967_296.0);
        if mode_changed && self.mode_change_fault {
            return ControlStep {
                output: 0.0,
                next_increment: None,
            };
        }
        if !self.output_enabled {
            self.pid_error = f32::NAN;
            self.pll.set_enabled(false);
            return ControlStep {
                output: 0.0,
                next_increment: None,
            };
        }

        match self.mode {
            ControlMode::None => {
                self.pid_error = f32::NAN;
                self.pll.set_enabled(false);
                ControlStep {
                    output: inputs.forcing + inputs.table,
                    next_increment: None,
                }
            }
            ControlMode::Pid => {
                self.pll.set_enabled(false);
                let laser = inputs.measured[LASER_INPUT];
                if self.pid_entry_fault
                    || (self.pid_qualified
                        && inputs.reference_mean != self.qualified_reference_mean)
                {
                    self.pid_entry_fault = true;
                    return ControlStep {
                        output: 0.0,
                        next_increment: None,
                    };
                }
                if !self.pid_qualified
                    && !self.qualify_pid(laser, inputs.reference_mean, ctx.sample_rate.dt())
                {
                    return ControlStep {
                        output: 0.0,
                        next_increment: None,
                    };
                }
                let feed_forward = inputs.forcing + inputs.table;
                if !feed_forward.is_finite() {
                    self.internal_fault = true;
                    return ControlStep {
                        output: 0.0,
                        next_increment: None,
                    };
                }
                self.pid.config.out_min = (SAFE_OUT_MIN_V - feed_forward).min(0.0);
                self.pid.config.out_max = (SAFE_OUT_MAX_V - feed_forward).max(0.0);
                self.pid_error = inputs.reference - laser;
                ControlStep {
                    output: self.pid.update(self.pid_error, ctx.sample_rate.dt()) + feed_forward,
                    next_increment: None,
                }
            }
            ControlMode::Pll => {
                self.pid_error = f32::NAN;
                self.pll.set_enabled(true);
                let increment = self.pll.update(
                    inputs.frame,
                    inputs.measured[DRIVE_INPUT],
                    inputs.measured[LASER_INPUT],
                    inputs.current_increment,
                    ctx.sample_rate.dt(),
                );
                ControlStep {
                    output: inputs.forcing,
                    next_increment: Some(increment),
                }
            }
        }
    }

    #[inline]
    #[cfg_attr(feature = "rt-sram", unsafe(link_section = ".data.ram_func"))]
    fn apply(&mut self, id: u16, payload: Payload) {
        match (id, payload) {
            (CONTROL_MODE_ID, Payload::U32(value)) => {
                if let Some(mode) = ControlMode::from_u32(value) {
                    self.pending_mode = Some(mode);
                } else {
                    self.internal_fault = true;
                }
            }
            (CTRL_RESET_ID, Payload::Unit) => self.full_reset(),
            (PLL_REACQUIRE_ID, Payload::Unit) => self.pll.reacquire(),
            (PID_KP_ID, Payload::F32(value)) => self.pid.config.kp = value,
            (PID_KI_ID, Payload::F32(value)) => self.pid.config.ki = value,
            (PID_KD_ID, Payload::F32(value)) => self.pid.config.kd = value,
            (PID_TAU_D_ID, Payload::F32(value)) => self.pid.config.tau_d = value,
            (PLL_CENTRE_ID, Payload::U32(value)) => {
                let result = self.pll.set_centre_increment(value);
                self.apply_checked(result);
            }
            (PLL_MIN_ID, Payload::U32(value)) => {
                let result = self.pll.set_min_increment(value);
                self.apply_checked(result);
            }
            (PLL_MAX_ID, Payload::U32(value)) => {
                let result = self.pll.set_max_increment(value);
                self.apply_checked(result);
            }
            (PLL_KP_ID, Payload::F32(value)) => {
                let result = self.pll.set_proportional_gain(value);
                self.apply_checked(result);
            }
            (PLL_KI_ID, Payload::F32(value)) => {
                let result = self.pll.set_integral_gain(value);
                self.apply_checked(result);
            }
            (PLL_TARGET_PHASE_ID, Payload::F32(value)) => {
                let result = self.pll.set_target_phase(value);
                self.apply_checked(result);
            }
            (PLL_DELAY_ID, Payload::F32(value)) => {
                let result = self.pll.set_delay(value);
                self.apply_checked(result);
            }
            (PLL_DC_TAU_ID, Payload::F32(value)) => {
                let result = self.pll.set_dc_time_constant(value);
                self.apply_checked(result);
            }
            (PLL_DEMOD_TAU_ID, Payload::F32(value)) => {
                let result = self.pll.set_demod_time_constant(value);
                self.apply_checked(result);
            }
            (PLL_EXCITATION_MIN_ID, Payload::F32(value)) => {
                let result = self.pll.set_min_excitation_amplitude(value);
                self.apply_checked(result);
            }
            (PLL_RESPONSE_MIN_ID, Payload::F32(value)) => {
                let result = self.pll.set_min_response_amplitude(value);
                self.apply_checked(result);
            }
            (PLL_LOCK_PHASE_TOL_ID, Payload::F32(value)) => {
                let result = self.pll.set_lock_phase_tolerance(value);
                self.apply_checked(result);
            }
            (PLL_UNLOCK_PHASE_TOL_ID, Payload::F32(value)) => {
                let result = self.pll.set_unlock_phase_tolerance(value);
                self.apply_checked(result);
            }
            (PLL_LOCK_FREQ_TOL_ID, Payload::F32(value)) => {
                let result = self.pll.set_lock_frequency_tolerance(value);
                self.apply_checked(result);
            }
            (PLL_LOCK_DWELL_ID, Payload::F32(value)) => {
                let result = self.pll.set_lock_dwell(value);
                self.apply_checked(result);
            }
            (PLL_UNLOCK_DWELL_ID, Payload::F32(value)) => {
                let result = self.pll.set_unlock_dwell(value);
                self.apply_checked(result);
            }
            (PLL_ACQUIRE_TIMEOUT_ID, Payload::F32(value)) => {
                let result = self.pll.set_acquire_timeout(value);
                self.apply_checked(result);
            }
            (PLL_SATURATION_DWELL_ID, Payload::F32(value)) => {
                let result = self.pll.set_saturation_dwell(value);
                self.apply_checked(result);
            }
            _ => self.internal_fault = true,
        }
    }

    #[inline]
    #[cfg_attr(feature = "rt-sram", unsafe(link_section = ".data.ram_func"))]
    fn reset(&mut self) {
        self.reset_pid_entry();
        if self.pll.state() != PllState::LockLost {
            self.pll.reset();
        }
        self.mode_change_fault = false;
        self.internal_fault = false;
    }

    #[inline]
    #[cfg_attr(feature = "rt-sram", unsafe(link_section = ".data.ram_func"))]
    fn set_output_enabled(&mut self, enabled: bool) {
        self.output_enabled = enabled;
    }

    #[inline]
    #[cfg_attr(feature = "rt-sram", unsafe(link_section = ".data.ram_func"))]
    fn telemetry(&self, out: &mut [f32]) {
        if out.len() < CONTROL_TELEMETRY.len() {
            return;
        }
        out[0] = self.mode as u32 as f32;
        out[1] = if self.mode == ControlMode::Pid {
            self.pid_error
        } else {
            f32::NAN
        };
        out[2] = self.pll.measured_phase();
        out[3] = self.pll.phase_error();
        out[4] = self.pll.excitation_amplitude();
        out[5] = self.pll.response_amplitude();
        out[6] = self.frequency_actual_hz;
        out[7] = self.pll.state() as u8 as f32;
    }

    #[inline(always)]
    #[cfg_attr(feature = "rt-sram", unsafe(link_section = ".data.ram_func"))]
    fn fault(&self) -> bool {
        self.mode_change_fault
            || self.pid_entry_fault
            || self.internal_fault
            || self.pll.state() == PllState::LockLost
    }
}

/// Convert a host-visible frequency gain or tolerance to increment quanta.
pub fn increments_per_hz(sample_rate: SampleRate) -> f32 {
    4_294_967_296.0 / sample_rate.hz()
}

#[cfg(test)]
mod tests {
    use helic_core::{HarmonicFrame, SinLut};

    use super::*;

    const H: usize = 2;

    fn run(
        control: &mut MagnetoelasticControl<H>,
        measured: &[f32; 4],
        reference: f32,
        reference_mean: f32,
        forcing: f32,
        table: f32,
    ) -> ControlStep {
        let frame = HarmonicFrame::zero();
        let lut = SinLut::new();
        <MagnetoelasticControl<H> as StandardControl<H>>::step(
            control,
            StandardControlInputs {
                measured,
                reference,
                reference_mean,
                forcing,
                table,
                frame: &frame,
                current_increment: 123,
            },
            &StepCtx {
                lut: &lut,
                sample_rate: SampleRate::Hz8000,
            },
        )
    }

    fn select(control: &mut MagnetoelasticControl<H>, mode: ControlMode) {
        control.set_output_enabled(false);
        control.apply(CONTROL_MODE_ID, Payload::U32(mode as u32));
        run(control, &[0.0; 4], 0.0, 0.0, 0.0, 0.0);
        control.reset();
        control.set_output_enabled(true);
    }

    #[test]
    fn each_mode_owns_its_complete_output_expression() {
        let mut control = MagnetoelasticControl::<H>::new();
        control.set_output_enabled(true);
        assert_eq!(
            run(&mut control, &[0.0; 4], 99.0, 0.0, 1.0, 2.0).output,
            3.0
        );

        select(&mut control, ControlMode::Pid);
        control.pid_qualified = true;
        control.qualified_reference_mean = 25.0;
        control.pid.config.kp = 2.0;
        let pid = run(&mut control, &[0.0, 0.0, 24.5, 0.0], 25.0, 25.0, 0.5, 0.5);
        assert_eq!(pid.output, 2.0);
        assert_eq!(pid.next_increment, None);

        select(&mut control, ControlMode::Pll);
        let pll = run(&mut control, &[0.0, 0.1, 25.0, 0.0], 99.0, 0.0, 1.0, 200.0);
        assert_eq!(pll.output, 1.0, "table must be diagnostic-only in PLL mode");
        assert!(pll.next_increment.is_some());
    }

    #[test]
    fn pid_residual_limits_are_finite_and_include_zero_beyond_both_rails() {
        let mut control = MagnetoelasticControl::<H>::new();
        select(&mut control, ControlMode::Pid);
        control.pid_qualified = true;
        control.qualified_reference_mean = 25.0;
        run(&mut control, &[0.0, 0.0, 25.0, 0.0], 25.0, 25.0, 10.0, 0.0);
        assert!(control.pid.config.out_min <= 0.0 && control.pid.config.out_max >= 0.0);
        run(&mut control, &[0.0, 0.0, 25.0, 0.0], 25.0, 25.0, -10.0, 0.0);
        assert!(control.pid.config.out_min <= 0.0 && control.pid.config.out_max >= 0.0);
    }

    #[test]
    fn non_finite_pid_feed_forward_faults_without_advancing_pid() {
        let mut control = MagnetoelasticControl::<H>::new();
        select(&mut control, ControlMode::Pid);
        control.pid_qualified = true;
        control.qualified_reference_mean = 25.0;
        let result = run(
            &mut control,
            &[0.0, 0.0, 25.0, 0.0],
            25.0,
            25.0,
            f32::NAN,
            0.0,
        );
        assert_eq!(result.output, 0.0);
        assert!(control.fault());
    }

    #[test]
    fn armed_mode_change_fault_is_visible_for_exactly_one_step() {
        let mut control = MagnetoelasticControl::<H>::new();
        control.set_output_enabled(true);
        control.apply(CONTROL_MODE_ID, Payload::U32(ControlMode::Pll as u32));
        assert_eq!(run(&mut control, &[0.0; 4], 0.0, 0.0, 1.0, 0.0).output, 0.0);
        assert!(control.fault());
        run(&mut control, &[0.0; 4], 0.0, 0.0, 1.0, 0.0);
        assert!(!control.mode_change_fault);
    }

    #[test]
    fn unevidenced_pid_entry_limits_fail_safe() {
        assert!(!PID_ENTRY_LIMITS_VERIFIED);
        let mut control = MagnetoelasticControl::<H>::new();
        select(&mut control, ControlMode::Pid);
        for _ in 0..2100 {
            run(&mut control, &[0.0, 0.0, 25.0, 0.0], 25.0, 25.0, 0.0, 0.0);
        }
        assert!(control.fault());
    }
}
