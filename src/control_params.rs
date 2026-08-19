//! Typed core-0 parameter group for the selectable rig control.

use helic_core::PhaseAccumulator;
use helic_proto::{ErrorCode, ParamType};
use helic_rt::params::{CommandTarget, ParamAction, ParamDef, ParamGroup, Staged};
use helic_rt::{Payload, RtShared, SampleRate, DOMAIN_CONTROLLER};

use crate::control::*;

pub const CONTROL_DEFAULTS: [f32; CONTROL_PARAM_COUNT] = [
    0.0,   // control_mode
    0.0,   // ctrl_reset
    0.0,   // pll_reacquire
    0.0,   // pid_kp
    0.0,   // pid_ki
    0.0,   // pid_kd
    0.0,   // pid_tau_d
    0.0,   // pll_centre_freq
    0.0,   // pll_freq_min
    0.0,   // pll_freq_max
    0.0,   // pll_kp
    0.0,   // pll_ki
    default_pll_config().target_phase_deg,
    default_pll_config().delay_s,
    default_pll_config().dc_time_constant_s,
    default_pll_config().demod_time_constant_s,
    default_pll_config().min_excitation_amplitude,
    default_pll_config().min_response_amplitude,
    default_pll_config().lock_phase_tolerance_deg,
    default_pll_config().unlock_phase_tolerance_deg,
    default_pll_config().lock_frequency_tolerance,
    default_pll_config().lock_dwell_s,
    default_pll_config().unlock_dwell_s,
    default_pll_config().acquire_timeout_s,
    default_pll_config().saturation_dwell_s,
];

pub const CONTROL_PARAM_DEFS: [ParamDef; CONTROL_PARAM_COUNT] = [
    ParamDef::writable("control_mode", ParamType::U32, 1),
    ParamDef::writable("ctrl_reset", ParamType::U32, 1),
    ParamDef::writable("pll_reacquire", ParamType::U32, 1),
    ParamDef::writable("pid_kp", ParamType::F32, 1),
    ParamDef::writable("pid_ki", ParamType::F32, 1),
    ParamDef::writable("pid_kd", ParamType::F32, 1),
    ParamDef::writable("pid_tau_d", ParamType::F32, 1),
    ParamDef::writable("pll_centre_freq", ParamType::F32, 1),
    ParamDef::writable("pll_freq_min", ParamType::F32, 1),
    ParamDef::writable("pll_freq_max", ParamType::F32, 1),
    ParamDef::writable("pll_kp", ParamType::F32, 1),
    ParamDef::writable("pll_ki", ParamType::F32, 1),
    ParamDef::writable("pll_target_phase", ParamType::F32, 1),
    ParamDef::writable("pll_delay_s", ParamType::F32, 1),
    ParamDef::writable("pll_dc_tau", ParamType::F32, 1),
    ParamDef::writable("pll_demod_tau", ParamType::F32, 1),
    ParamDef::writable("pll_excitation_min", ParamType::F32, 1),
    ParamDef::writable("pll_response_min", ParamType::F32, 1),
    ParamDef::writable("pll_lock_phase_tol", ParamType::F32, 1),
    ParamDef::writable("pll_unlock_phase_tol", ParamType::F32, 1),
    ParamDef::writable("pll_lock_freq_tol", ParamType::F32, 1),
    ParamDef::writable("pll_lock_dwell", ParamType::F32, 1),
    ParamDef::writable("pll_unlock_dwell", ParamType::F32, 1),
    ParamDef::writable("pll_acquire_timeout", ParamType::F32, 1),
    ParamDef::writable("pll_saturation_dwell", ParamType::F32, 1),
];

/// Accepted host shadows and sample-boundary conversion for the rig control.
pub struct MagnetoelasticControlGroup {
    shared: &'static RtShared,
    sample_rate: SampleRate,
    values: [f32; CONTROL_PARAM_COUNT],
    pending: Option<(usize, f32)>,
}

impl MagnetoelasticControlGroup {
    pub const fn new(shared: &'static RtShared, sample_rate: SampleRate) -> Self {
        Self {
            shared,
            sample_rate,
            values: CONTROL_DEFAULTS,
            pending: None,
        }
    }

    fn stage_f32(&mut self, id: u16, value: f32) -> Result<Staged, ErrorCode> {
        if !self.normalise(id, value) {
            return Err(ErrorCode::BadValue);
        }
        self.pending = Some((id as usize, value));
        let payload = match id {
            PLL_CENTRE_ID | PLL_MIN_ID | PLL_MAX_ID => Payload::U32(
                PhaseAccumulator::increment_for(value as f64, self.sample_rate.hz() as f64),
            ),
            PLL_KP_ID | PLL_KI_ID | PLL_LOCK_FREQ_TOL_ID => {
                Payload::F32(value * increments_per_hz(self.sample_rate))
            }
            _ => Payload::F32(value),
        };
        Ok(Staged::Rt(payload))
    }

    fn normalise(&self, id: u16, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        match id {
            PID_KP_ID | PID_KI_ID | PID_KD_ID | PLL_KP_ID | PLL_KI_ID | PLL_DELAY_ID => true,
            PID_TAU_D_ID => value >= 0.0,
            PLL_CENTRE_ID => {
                self.values[PLL_MIN_ID as usize] <= value
                    && value <= self.values[PLL_MAX_ID as usize]
            }
            PLL_MIN_ID => value >= 0.0 && value <= self.values[PLL_CENTRE_ID as usize],
            PLL_MAX_ID => {
                value >= self.values[PLL_CENTRE_ID as usize] && value < self.sample_rate.hz() / 2.0
            }
            PLL_TARGET_PHASE_ID => (-180.0..180.0).contains(&value),
            PLL_DC_TAU_ID => value > 0.0,
            PLL_DEMOD_TAU_ID
            | PLL_EXCITATION_MIN_ID
            | PLL_RESPONSE_MIN_ID
            | PLL_LOCK_FREQ_TOL_ID
            | PLL_LOCK_DWELL_ID
            | PLL_UNLOCK_DWELL_ID
            | PLL_ACQUIRE_TIMEOUT_ID
            | PLL_SATURATION_DWELL_ID => value >= 0.0,
            PLL_LOCK_PHASE_TOL_ID => {
                value >= 0.0 && value <= self.values[PLL_UNLOCK_PHASE_TOL_ID as usize]
            }
            PLL_UNLOCK_PHASE_TOL_ID => value >= self.values[PLL_LOCK_PHASE_TOL_ID as usize],
            _ => false,
        }
    }
}

impl ParamGroup for MagnetoelasticControlGroup {
    fn target(&self) -> CommandTarget {
        CommandTarget::Program(DOMAIN_CONTROLLER)
    }

    fn params(&self) -> &[ParamDef] {
        &CONTROL_PARAM_DEFS
    }

    fn get(&self, id: u16, out: &mut [u8]) -> Result<usize, ErrorCode> {
        let index = id as usize;
        let def = CONTROL_PARAM_DEFS.get(index).ok_or(ErrorCode::BadIndex)?;
        if out.len() < 4 {
            return Err(ErrorCode::BadLength);
        }
        if matches!(id, CTRL_RESET_ID | PLL_REACQUIRE_ID) {
            out[..4].copy_from_slice(&0_u32.to_le_bytes());
        } else if def.ty == ParamType::U32 {
            out[..4].copy_from_slice(&(self.values[index] as u32).to_le_bytes());
        } else {
            out[..4].copy_from_slice(&self.values[index].to_le_bytes());
        }
        Ok(4)
    }

    fn stage(&mut self, id: u16, data: &[u8]) -> Result<Staged, ErrorCode> {
        match id {
            CONTROL_MODE_ID => {
                let value = read_u32(data)?;
                if self.shared.safety.load_inputs().armed || ControlMode::from_u32(value).is_none()
                {
                    return Err(ErrorCode::BadValue);
                }
                self.pending = Some((id as usize, value as f32));
                Ok(Staged::Rt(Payload::U32(value)))
            }
            CTRL_RESET_ID | PLL_REACQUIRE_ID => {
                if read_u32(data)? == 0 {
                    Ok(Staged::Local(ParamAction::None))
                } else {
                    Ok(Staged::Rt(Payload::Unit))
                }
            }
            3..=24 => self.stage_f32(id, read_f32(data)?),
            _ => Err(ErrorCode::BadIndex),
        }
    }

    fn accept(&mut self, _id: u16) {
        if let Some((id, value)) = self.pending.take() {
            self.values[id] = value;
        }
    }

    fn reject(&mut self, _id: u16, _returned: Option<Payload>) {
        self.pending = None;
    }
}

fn read_f32(data: &[u8]) -> Result<f32, ErrorCode> {
    Ok(f32::from_le_bytes(
        data.try_into().map_err(|_| ErrorCode::BadLength)?,
    ))
}

fn read_u32(data: &[u8]) -> Result<u32, ErrorCode> {
    Ok(u32::from_le_bytes(
        data.try_into().map_err(|_| ErrorCode::BadLength)?,
    ))
}

#[cfg(test)]
mod tests {
    use std::boxed::Box;

    use super::*;

    fn group() -> MagnetoelasticControlGroup {
        MagnetoelasticControlGroup::new(Box::leak(Box::new(RtShared::new())), SampleRate::Hz8000)
    }

    #[test]
    fn raw_ids_and_payload_types_match_the_control_contract() {
        let mut group = group();
        assert_eq!(
            group.params()[CONTROL_MODE_ID as usize].name,
            "control_mode"
        );
        assert!(matches!(
            group.stage(CONTROL_MODE_ID, &2_u32.to_le_bytes()),
            Ok(Staged::Rt(Payload::U32(2)))
        ));
        group.accept(CONTROL_MODE_ID);
        assert!(matches!(
            group.stage(CTRL_RESET_ID, &1_u32.to_le_bytes()),
            Ok(Staged::Rt(Payload::Unit))
        ));
        assert!(matches!(
            group.stage(PID_KP_ID, &2.0_f32.to_le_bytes()),
            Ok(Staged::Rt(Payload::F32(2.0)))
        ));
    }

    #[test]
    fn frequency_window_is_valid_after_every_accepted_scalar_write() {
        let mut group = group();
        assert!(matches!(
            group.stage(PLL_CENTRE_ID, &20.0_f32.to_le_bytes()),
            Err(ErrorCode::BadValue)
        ));
        group.stage(PLL_MAX_ID, &40.0_f32.to_le_bytes()).unwrap();
        group.accept(PLL_MAX_ID);
        group.stage(PLL_CENTRE_ID, &20.0_f32.to_le_bytes()).unwrap();
        group.accept(PLL_CENTRE_ID);
        group.stage(PLL_MIN_ID, &10.0_f32.to_le_bytes()).unwrap();
        group.accept(PLL_MIN_ID);
        assert!(matches!(
            group.stage(PLL_MAX_ID, &19.0_f32.to_le_bytes()),
            Err(ErrorCode::BadValue)
        ));
    }

    #[test]
    fn mode_change_is_rejected_synchronously_while_armed() {
        let shared = Box::leak(Box::new(RtShared::new()));
        shared.safety.arm();
        let mut group = MagnetoelasticControlGroup::new(shared, SampleRate::Hz8000);
        assert!(matches!(
            group.stage(CONTROL_MODE_ID, &1_u32.to_le_bytes()),
            Err(ErrorCode::BadValue)
        ));
    }

    #[test]
    fn frequency_gains_are_converted_once_on_core_zero() {
        let mut group = group();
        let staged = group.stage(PLL_KP_ID, &1.0_f32.to_le_bytes()).unwrap();
        let Ok(Staged::Rt(Payload::F32(value))) = Ok::<_, ErrorCode>(staged) else {
            panic!("wrong payload");
        };
        assert_eq!(value, increments_per_hz(SampleRate::Hz8000));
    }
}
