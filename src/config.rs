//! Compile-time choices for this rig.
//!
//! Start here when adapting the rig: most laboratory choices are constants,
//! while physical pin assignments and analogue polarity live in `board.rs`.
//! The host discovers the resulting parameter and source tables, so these
//! choices do not require host-side indices. See "Things you set at compile
//! time" in the platform's `docs/user_guide.md` and "Adding an experiment" in
//! its `docs/developer_guide.md`.

use helic_core::controller::PassThrough;
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

/// Upper bound on the DAC output voltage driven to either differential input
/// of the exciter current controller (channels A and C, driven symmetrically
/// about `MID_RAIL`). Set below the 4.096 V DAC rail. The gate clamps the
/// logical command so that neither `MID_RAIL + out` (A) nor `MID_RAIL - out`
/// (C) ever exceeds this.
pub const DAC_OUT_CEILING_V: f32 = 4.0;

/// Lower bound on the same channel voltages. Chosen symmetric about
/// `MID_RAIL` for the interim unipolar output stage, so the same bound on
/// `out` protects both channels. A future bipolar output stage will re-home
/// the common mode and turn these into independent ± limits.
pub const DAC_OUT_FLOOR_V: f32 = 0.096;

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
/// The fitted motor has not been identified, so this is an assumption until
/// the steps-per-millimetre check at commissioning confirms it.
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

/// Opto sensor level seen beyond the datum edge, that is on the short side
/// between the edge and the hard stop. The sensor is an edge rather than a
/// vane, so this level alone says which side of the datum the stage is on.
pub const STATOR_OPTO_HIGH_BEYOND_DATUM: bool = true;

/// True when advancing moves the stage toward the datum, and so toward the
/// hard stop just past it.
///
/// The datum sits at one **extreme** of travel, so one side of the edge is a
/// short clearance and the other is the whole working range. Which side is
/// which decides how homing can reach the edge, because the final motion onto
/// the datum has to be an advance for the unpreloaded stage to be positioned
/// by it:
///
/// - datum at the advanced extreme (`true`): advancing already points at the
///   datum, so the approach ends on the edge having intruded into the
///   clearance by one step;
/// - datum at the retracted extreme (`false`): advancing points away from the
///   datum, so homing has to enter the clearance first, by
///   `STATOR_HOME_BACKOFF_MM`, and advance back onto the edge. That case needs
///   `STATOR_DATUM_CLEARANCE_MM` to be comfortably the larger of the two, and
///   homing refuses to run when it is not.
///
/// Not yet established for this rig; confirm before the first home.
pub const STATOR_DATUM_AT_ADVANCED_EXTREME: bool = true;

/// Usable travel between the datum edge and the hard stop just past it.
/// Deliberately small until measured, because everything that intrudes into
/// this clearance is bounded against it.
pub const STATOR_DATUM_CLEARANCE_MM: f32 = 0.5;

/// Working travel on the far side of the datum, away from the hard stop.
/// Conservative and set before the geometry was known.
pub const STATOR_TRAVEL_RANGE_MM: f32 = 5.0;

/// Time for the MP6500 to wake from sleep before the first step. Generous
/// against the part's specification; this path is never time-critical.
pub const STATOR_WAKE_MS: u64 = 5;

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

/// Distance retracted past the datum edge before the final slow advance onto
/// it. Must exceed the sensor's hysteresis and the mechanism's lash.
pub const STATOR_HOME_BACKOFF_MM: f32 = 0.2;

/// Bound on any homing search. Exceeding it faults rather than continuing,
/// because the alternative is driving into a hard stop. It has to exceed the
/// full travel, so that the edge is reachable from wherever the stage happens
/// to be at power-on, which also means a failed or disconnected sensor buys
/// this much travel into a stop before homing gives up. The current-limit
/// potentiometer is what makes that a stall rather than damage.
pub const STATOR_SEEK_MAX_MM: f32 = STATOR_TRAVEL_RANGE_MM + STATOR_DATUM_CLEARANCE_MM + 1.0;

/// Default overshoot for the unidirectional approach. With no preload spring
/// the stage is only positioned by the spindle pushing it, so this is what
/// makes a retracting move deterministic at all rather than a refinement.
/// Measure the real figure at commissioning and replace this.
pub const STATOR_BACKLASH_MM: f32 = 0.25;

/// Soft travel window, as distances from the datum along the advance and
/// retract directions. Because the datum is at an extreme of travel the window
/// is one-sided by construction: all of the working range lies on one side of
/// the datum, and the other side is the clearance to the hard stop, which is
/// run-out for homing and not travel to operate in. Targets are therefore
/// bounded by the datum itself in that direction, and rejected rather than
/// clamped when they exceed it.
///
/// One consequence worth knowing: because a move approaches its target from
/// one backlash allowance below it, targets within `rig_stator_backlash` of
/// the datum-side limit cannot be reached, and are refused.
pub const STATOR_TRAVEL_ADVANCE_MM: f32 = if STATOR_DATUM_AT_ADVANCED_EXTREME {
    0.0
} else {
    STATOR_TRAVEL_RANGE_MM
};
pub const STATOR_TRAVEL_RETRACT_MM: f32 = if STATOR_DATUM_AT_ADVANCED_EXTREME {
    STATOR_TRAVEL_RANGE_MM
} else {
    0.0
};

/// Bounds accepted for the corresponding runtime parameters. A rate above a
/// few mm/s would need an acceleration ramp, which this axis does not
/// implement, and a backlash allowance of more than a couple of millimetres
/// means something mechanical is wrong rather than needing compensation.
pub const MAX_STATOR_RATE_MM_S: f32 = 3.0;
pub const MAX_STATOR_BACKLASH_MM: f32 = 2.0;

/// Barrel reading at the opto datum, in mm. Zero until measured by hand at
/// commissioning; `rig_stator_datum` overrides it at runtime, and like
/// `LASER_RANGE_MM` that override is not persisted across a reflash.
pub const STATOR_DATUM_MM: f32 = 0.0;

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

/// The controller that runs inside every sample tick.
///
/// `type` gives a concrete Rust type a local name. Selecting it at compile
/// time lets Rust specialise the real-time loop, avoiding dynamic dispatch in
/// the 125 microsecond tick budget. Swap this alias and `make_controller()`
/// together, for example:
///
/// ```ignore
/// pub type ActiveController = helic_core::controller::PidController;
/// pub fn make_controller() -> ActiveController {
///     PidController::new(Pid::new(PidConfig { kp: 1.0, ..Default::default() }), 0)
/// }
/// ```
pub type ActiveController = PassThrough;
/// Statically selected core-1 programme.
pub type ActiveProgram = helic_rt::StandardProgram<ActiveController, HARMONICS, TABLE_CAPACITY>;

/// Construct the one controller instance which is later moved to core 1.
///
/// Keep constructor defaults consistent with the controller's `param_value`
/// implementation so the host-visible parameter shadow starts correctly.
pub fn make_controller() -> ActiveController {
    PassThrough
}

/// Selected sample-rate preset. The preset supplies exact PWM divider values;
/// do not replace the hardware-timed clock with a software timer.
#[cfg(feature = "diag-sample-4k")]
pub const SAMPLE_RATE: SampleRate = SampleRate::Hz4000;
#[cfg(not(feature = "diag-sample-4k"))]
pub const SAMPLE_RATE: SampleRate = SampleRate::Hz8000;
