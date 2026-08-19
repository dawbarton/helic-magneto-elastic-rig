//! One definition of the fitted differential output stage's safe window.

/// DAC reference voltage fitted to the interim analogue board.
pub const DAC_VREF: f32 = 4.096;
pub const MID_RAIL: f32 = DAC_VREF / 2.0;
pub const DAC_OUT_CEILING_V: f32 = 4.0;
pub const DAC_OUT_FLOOR_V: f32 = 0.096;

const POSITIVE_HALF_SWING_V: f32 = DAC_OUT_CEILING_V - MID_RAIL;
const NEGATIVE_HALF_SWING_V: f32 = MID_RAIL - DAC_OUT_FLOOR_V;
const SAFE_HALF_SWING_V: f32 = if POSITIVE_HALF_SWING_V < NEGATIVE_HALF_SWING_V {
    POSITIVE_HALF_SWING_V
} else {
    NEGATIVE_HALF_SWING_V
};

/// Safe differential command bounds used by both PID and the final rig clamp.
pub const SAFE_OUT_MIN_V: f32 = -2.0 * SAFE_HALF_SWING_V;
pub const SAFE_OUT_MAX_V: f32 = 2.0 * SAFE_HALF_SWING_V;
