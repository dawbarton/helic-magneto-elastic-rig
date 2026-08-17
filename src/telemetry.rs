//! Scalar state shared between the RT core, laser task, and parameter view.

use core::sync::atomic::AtomicU32;

use helic_rt::params::ExtraParam;

pub static LASER_VALUE: AtomicU32 = AtomicU32::new(0);
pub static LASER_RANGE_MM: AtomicU32 = AtomicU32::new(0);
pub static LASER_FRAMES_RECEIVED: AtomicU32 = AtomicU32::new(0);
pub static LASER_UART_ERRORS: AtomicU32 = AtomicU32::new(0);
pub static LASER_PARSE_ERRORS: AtomicU32 = AtomicU32::new(0);
pub static LASER_INVALID_FRAMES: AtomicU32 = AtomicU32::new(0);
pub static LASER_UNEXPECTED_VALUES: AtomicU32 = AtomicU32::new(0);
pub static LASER_SYNC_ERRORS: AtomicU32 = AtomicU32::new(0);

// --- Stator stage --------------------------------------------------------
//
// Published by the core-0 stepper task in `stator.rs`. `STATOR_POSITION_MM` is
// the one word core 1 reads, as the `stator` sample source; everything else is
// host-visible telemetry only. Step counts are held as `f32` rather than a
// bit-cast `i32` because the axis has at most a few tens of thousands of
// microsteps of travel, well inside `f32`'s exact-integer range.

/// Current position as a micrometer barrel reading, or NaN when unknown.
pub static STATOR_POSITION_MM: AtomicU32 = AtomicU32::new(NAN_BITS);
/// Signed microsteps from the datum, in the advance direction.
pub static STATOR_STEPS: AtomicU32 = AtomicU32::new(0);
/// 0 idle, 1 moving, 2 homing, 3 not homed, 4 faulted.
pub static STATOR_STATE: AtomicU32 = AtomicU32::new(StatorState::NotHomed as u32);
/// Set once a homing sequence has completed.
pub static STATOR_HOMED: AtomicU32 = AtomicU32::new(0);
/// Steps between the predicted and actual datum at the last re-home. The
/// lost-step audit; its noise floor is the datum's own repeatability, measured
/// at about one full step on 2026-08-17. See `docs/stator-stage.md`.
pub static STATOR_HOME_ERROR: AtomicU32 = AtomicU32::new(0);
/// Raw opto sensor level, 1 when the input reads high. Deliberately the pin
/// level rather than `below_datum`, which made the sensor's polarity
/// measurable at commissioning rather than assumed. It stays because on an
/// unhomed axis it is the only position feedback there is: it says which side
/// of the datum the stage sits on, and with the datum near the middle of the
/// travel that is genuinely useful rather than merely diagnostic.
pub static STATOR_OPTO: AtomicU32 = AtomicU32::new(0);
/// Step count at which the opto level last changed. Latched in the stepping
/// path, so the edge is located to the step that crossed it rather than to
/// whenever the host next asked. Meaningless until an edge has been seen, and
/// during a move it leads the mechanism by the ten or eleven steps of FIFO
/// measured on 2026-08-17.
///
/// Retained deliberately after the bring-up it was added for: until homing has
/// been commissioned it is the only lost-step check available, and it is what
/// the first home should be validated against. Once `stator_home_error` has
/// been shown to work, this is a candidate for removal.
pub static STATOR_OPTO_EDGE: AtomicU32 = AtomicU32::new(NAN_BITS);

/// Completed moves, for wear and audit.
pub static STATOR_MOVES: AtomicU32 = AtomicU32::new(0);
/// Commands rejected or abandoned: out of range, not homed, gate armed, or a
/// homing search that ran past its bound.
pub static STATOR_FAULTS: AtomicU32 = AtomicU32::new(0);

// Commanded state, written by `set_param` on core 1 and read by the stepper
// task on core 0. Single-writer, so the packed command word below needs no
// read-modify-write atomicity.

pub static STATOR_TARGET_MM: AtomicU32 = AtomicU32::new(0);
pub static STATOR_JOG_MM: AtomicU32 = AtomicU32::new(0);
pub static STATOR_RATE_MM_S: AtomicU32 = AtomicU32::new(0);
pub static STATOR_BACKLASH_MM: AtomicU32 = AtomicU32::new(0);
pub static STATOR_DATUM_MM: AtomicU32 = AtomicU32::new(0);
pub static STATOR_HOLD: AtomicU32 = AtomicU32::new(0);

/// Packed `(sequence << 8) | kind`. A parameter write changes the whole word,
/// so the task can tell a repeated command from a stale one and act on each
/// write exactly once, which a bare value comparison cannot do.
pub static STATOR_COMMAND: AtomicU32 = AtomicU32::new(0);

/// Bit pattern of `f32::NAN`, needed because `f32::to_bits` is not usable in a
/// static initialiser.
const NAN_BITS: u32 = 0x7fc0_0000;

/// Reported through `stator_state`, and the numbering is wire-visible.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StatorState {
    Idle = 0,
    Moving = 1,
    Homing = 2,
    NotHomed = 3,
    Faulted = 4,
}

pub const EXTRA_PARAMS: &[ExtraParam] = &[
    ExtraParam::f32("laser", &LASER_VALUE),
    ExtraParam::u32("laser_frames_received", &LASER_FRAMES_RECEIVED),
    ExtraParam::u32_event("laser_uart_errors", &LASER_UART_ERRORS),
    ExtraParam::u32_event("laser_parse_errors", &LASER_PARSE_ERRORS),
    ExtraParam::u32_event("laser_invalid_frames", &LASER_INVALID_FRAMES),
    ExtraParam::u32_event("laser_unexpected_values", &LASER_UNEXPECTED_VALUES),
    ExtraParam::u32_event("laser_sync_errors", &LASER_SYNC_ERRORS),
    ExtraParam::f32("stator_position_mm", &STATOR_POSITION_MM),
    ExtraParam::f32("stator_steps", &STATOR_STEPS),
    ExtraParam::u32("stator_state", &STATOR_STATE),
    ExtraParam::u32("stator_homed", &STATOR_HOMED),
    ExtraParam::f32("stator_home_error", &STATOR_HOME_ERROR),
    ExtraParam::u32("stator_opto", &STATOR_OPTO),
    ExtraParam::f32("stator_opto_edge", &STATOR_OPTO_EDGE),
    ExtraParam::u32("stator_moves", &STATOR_MOVES),
    ExtraParam::u32_event("stator_faults", &STATOR_FAULTS),
];
