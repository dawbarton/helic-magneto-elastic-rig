# Selectable control and phase-locked excitation

## Status and scope

This document proposes a run-time choice between no feedback controller, PID
displacement control, and phase-locked excitation for the magneto-elastic rig.
It covers the required changes to the HELIC-DAQ platform and this rig, including
an in-place revision of `helic_core::Pll`. It does not specify numerical gains
or operating bounds. Those require simulation and hardware evidence.

The phase-locked use case is the backbone and nonlinear frequency-response
method described by the CBC Duffing project's `docs/methods/phase-locked-loop.md`:
fundamental excitation-to-response phase is controlled through excitation
frequency, excitation amplitude is stepped for backbone tracing, and a slower
outer amplitude loop is used when a nonlinear FRF requires constant response
amplitude.

## Decision

`StandardProgram` should change, but it should not contain a PLL or know about
the rig's three control modes. It should acquire a generic, harmonic-frame-aware
control seam with the following responsibilities:

- receive the generated reference, the current harmonic frame, the measured
  inputs, forcing, table value, and the sample interval;
- return the complete raw programme output, so mode-specific composition rules
  can be enforced rather than left to operator discipline;
- optionally return the phase increment to use on the next tick;
- expose fixed telemetry, a reset hook, command application, and a fault;
- remain statically dispatched, allocation-free, and SRAM-resident; and
- adapt every existing `Controller` without changing its enabled-path
  behaviour or public configuration.

The selectable policy is specific to this rig and belongs here. The underlying
PID and PLL remain reusable primitives in `helic-core`. In particular, the
existing `helic_core::Pll` is to be changed in place rather than supplemented by
a second PLL implementation.

This split keeps the common target, forcing, table, phase, command-routing, and
telemetry machinery in the platform. It also avoids copying `StandardProgram`
into the rig merely to obtain access to its private phase accumulator.

## Control semantics

The proposed `control_mode` parameter is a writable `u32` enum:

| Value | Mode | Raw programme output | Master frequency |
|---:|---|---|---|
| 0 | `None` | `forcing + table` | standard `freq` parameter |
| 1 | `Pid` | `PID(target - laser) + forcing + table` | standard `freq` parameter |
| 2 | `Pll` | `forcing`; table is ignored | bounded output of the PLL |

Consequently:

- `target_coeffs` is always a displacement reference in millimetres. It is
  consumed only in PID mode.
- `forcing_coeffs` is always open-loop excitation in volts. It supplies the
  excitation in both `None` and `Pll` modes and remains additive feed-forward
  in PID mode.
- `table` remains an additive voltage in `None` and `Pid` modes. It is ignored
  in `Pll` mode, irrespective of `table_mode`, because a second waveform path
  must not silently invalidate the phase detector. A deliberately locked table
  may be admitted later, after its effect on the measured excitation reference
  is tested.

This gives each source one stable unit in every mode. It deliberately does not
preserve the present interpretation in which `PassThrough` sends `target`
straight to the actuator. Existing open-loop host code should use `forcing`, as
the current `sine` helper already does. Boot defaults remain zero, so this
change cannot energise the rig unexpectedly.

The outer amplitude controller needed for a constant-response-amplitude NLFRF
is not the PID mode above. It is a much slower host-side PI loop which adjusts
the fundamental coefficient in `forcing_coeffs`. Keeping it host-side makes the
required bandwidth separation explicit and leaves the firmware phase loop
deterministic. Fixed-excitation-amplitude backbone tracing needs only stepped
host writes to `forcing_coeffs`.

## Platform programme changes

### A harmonic-frame-aware control trait

Add a trait in `helic-rt`, provisionally named `StandardControl`, and make
`StandardProgram` generic over it. The exact Rust surface should be settled by
implementation, but the semantic contract is:

```rust
pub struct StandardControlInputs<'a, const H: usize> {
    pub measured: &'a [f32],
    pub reference: f32,
    pub reference_mean: f32,
    pub forcing: f32,
    pub table: f32,
    pub frame: &'a HarmonicFrame<H>,
    pub current_increment: u32,
}

pub struct ControlStep {
    /// Complete raw programme output, before the rig safety gate.
    pub output: f32,
    /// Increment for the next tick. `None` means the nominal `freq` increment,
    /// not "retain the last increment".
    pub next_increment: Option<u32>,
}

pub trait StandardControl<const H: usize> {
    const REFERENCE_UNIT: &'static str = "V";
    const TELEMETRY: &'static [(&'static str, &'static str)] = &[];

    fn step(&mut self, inputs: StandardControlInputs<'_, H>, ctx: &StepCtx<'_>)
        -> ControlStep;

    fn apply(&mut self, id: u16, payload: Payload);
    fn reset(&mut self) {}
    fn set_output_enabled(&mut self, _enabled: bool) {}
    fn telemetry(&self, _out: &mut [f32]) {}
    fn fault(&self) -> bool { false }
}
```

`None` has deliberately positive semantics: it restores the nominal increment.
If it meant "retain", leaving PLL mode could silently leave the generator at
the last locked frequency.

Provide a blanket adapter from every existing `C: Controller` to
`StandardControl<H>`. It delegates reset, telemetry, and `tick`, returns
`controller.tick(...) + forcing + table` with no frequency override, retains
`"V"` as the reference unit, and never faults. Existing rig type aliases and
`ControllerGroup<C>` therefore remain valid. A type must not implement both
`Controller` and `StandardControl`; a compile fixture has verified that a
downstream rig-local implementation does not overlap the blanket adapter.

`StandardProgram::apply` retains ownership of the standard controller command
mapping. It routes `command_id::controller::RESET` directly to `reset` and
forwards controller ids of one and above, without subtracting one, to
`StandardControl::apply`. The blanket adapter alone translates those forwarded
ids to the zero-based ids expected by `Controller::set_param`. The rig group
uses the same raw ids as its parameter definitions. Test `RESET`, the first
ordinary parameter, and the final ordinary parameter explicitly.

The selectable rig control uses a rig-specific parameter group because its
parameters mix `u32` commands and `f32` values. The existing
`ControllerGroup<C>` remains the simple adapter for ordinary controllers.

### Harmonic generation inside `StandardProgram`

Replace `StandardProgram`'s bare `PhaseAccumulator` with
`HarmonicGenerator<H>`. Per tick it should:

1. advance the generator once and borrow the resulting `HarmonicFrame<H>`;
2. project `target_coeffs` and `forcing_coeffs` through that shared frame;
3. step the table using the frame's phase and wrap flag;
4. call the selected control policy with the reference mean, forcing, table,
   and frame while the frame is borrowed;
5. record the increment which generated the current sample;
6. release the frame borrow and install either the returned next increment or
   the nominal `freq` increment; and
7. use the complete output returned by the control policy.

The returned increment takes effect on the following tick. `freq_actual`
telemetry must likewise describe the increment which generated the phase in
the current record, not the increment just installed for the next record.

This refactor should reduce duplicated sine/cosine lookups because target,
forcing, and PLL demodulation share one harmonic basis. That is an expectation,
not a timing result; the release build and hardware timing gate remain
authoritative.

### Output-enabled lifecycle

The current `Controller` documentation says reset occurs when control is
enabled or re-armed, but `StandardProgram` cannot presently see that lifecycle.
Keep `StepCtx` as the loop-invariant immutable services value it is today.
Instead, the RT loop loads one `SafetyInputs` snapshot per tick, calculates
`output_enabled` as:

```text
not safety-gated, or (armed and not tripped).
```

and passes the flag separately to `Program::step`. The same safety snapshot is
then used by the downstream gate, so a core-0 arm change cannot land between
two inconsistent reads. This keeps ungated rigs permanently enabled.
`StandardProgram` tracks the previous value and:

- does not advance controller or PLL state while output is disabled;
- calls `reset` on a disabled-to-enabled transition;
- calls `set_output_enabled` after transition handling; and
- uses zero raw programme output and the nominal frequency while disabled.

On the first enabled PLL tick, the phase increment may still be the nominal
`freq` value; the PLL centre increment takes effect on the following tick. The
resulting one-sample phase difference is
`(freq - pll_centre_freq) / sample_rate` turns, phase remains continuous, and
no second increment-override path is justified to remove it. Bound and test
this transient.

The forcing and table values may still be calculated for telemetry, but the
existing safety gate remains the sole authority which decides the applied
output. The programme observes the gate state and never arms or clears a trip.

### Source units and compatibility

`StandardProgram` currently labels `target` as volts. Its source discovery must
take the target unit from `StandardControl::REFERENCE_UNIT`; the blanket
adapter retains volts, while this rig declares millimetres. `forcing` and
`table` remain volts, and `phase` remains turns.

Add sample-for-sample regression tests showing that a blanket-adapted
`PassThrough` or `PidController` produces the same enabled outputs, commands,
table behaviour, signal ordering, and telemetry as the current
`StandardProgram`. The only intended common behaviour change is that a
safety-gated controller no longer integrates while its output is disabled and
is reset on re-arm, which is the already documented lifecycle.

## Changes to the existing `helic_core::Pll`

### Phase convention

Adopt the conventional Fourier phase used by the CBC host analysis:

```text
x(t) = mean + a cos(theta) + b sin(theta)
phase(x) = atan2(-b, a).
```

The measured PLL phase is then

```text
phase(response) - phase(excitation), wrapped to [-180, 180) degrees.
```

A displacement response lagging measured excitation by 90 degrees therefore
reports `-90 deg`, exactly matching the phase-resonance setpoint. The existing
demodulator uses `atan2(b, a)` and consequently reports the opposite sign; its
documentation and tests must change with the implementation.

Define the loop error as

```text
phase_error = corrected_measured_phase - target_phase.
```

The usual resonance has a negative phase-frequency slope. With this error
definition, positive `pll_kp` and `pll_ki` increase frequency when the measured
phase lies above the target, which is the expected feedback direction. Signed
gains remain valid for an experimentally established positive slope, but zero
is the unevidenced default and the ordinary case uses positive gains.

### DC rejection before synchronous demodulation

The laser reports an absolute distance with a large static component. Mixing
that value directly with sine and cosine leaves carrier-frequency leakage after
the first-order demodulation filter, potentially much larger than the dynamic
response.

Add one DC estimator to each input of the existing PLL demodulator. On the
first valid sample, seed the estimator from that sample. Thereafter use

```text
mean += beta * (sample - mean)
ac = sample - mean
beta = dt / (dc_time_constant + dt)
```

and demodulate `ac`. The same high-pass operation on excitation and response
preserves their relative steady-state phase. `dc_time_constant` must be
strictly positive and slow relative to the lowest permitted carrier. The
filtered PLL amplitude is used for qualification and telemetry, not as the
authoritative backbone amplitude; settled host-side harmonic fitting of the
captured raw response remains authoritative.

Resetting the PLL resets both mean estimators and seeds them afresh on the next
valid sample. Tests must cover a response of the form
`25 mm + small sinusoid`, non-zero excitation offset, and slow mean drift.

Seeding from one sample still leaves a mean error of order the oscillation
amplitude. After every full reset or explicit reacquisition, remain in
`Acquiring` and mark observations invalid for at least several times
`max(dc_time_constant, demod_time_constant)`. Use one named, tested warm-up
factor rather than relying on `lock_dwell` to hide filter transients. The
acquisition-timeout clock starts after warm-up, and telemetry distinguishes
warm-up from a valid observation. This conservative rule can be relaxed only
after simulation demonstrates an equivalent state-dependent criterion.

### Separate amplitude qualifications

Replace `min_amplitude` with:

- `min_excitation_amplitude`, in the excitation sensor's units; and
- `min_response_amplitude`, in the response sensor's units.

The demodulator should return an observation containing measured phase and both
fundamental amplitudes. Store these values in `Pll` and expose read-only
getters. An invalid or sub-threshold observation resets the consecutive
acquisition dwell. It continues to count towards loss of an established lock,
as it does now.

### PI loop filter and anti-windup

Replace the single `gain` with proportional and integral gains. Host-visible
gains use interpretable frequency units:

- `pll_kp`: Hz/degree; and
- `pll_ki`: Hz/(degree second).

The rig parameter group converts these once to phase-increment units using the
sample rate. No `f64` conversion occurs on the tick path.

The existing `Pll` retains a floating-point integral frequency correction. On
each valid observation it computes, schematically:

```text
i_candidate = i + Ki * phase_error * dt
f_unclamped = f_centre + Kp * phase_error + i_candidate
f_command = clamp(f_unclamped, f_min, f_max)
```

Use conditional-integration anti-windup: accept `i_candidate` if the command is
not saturated or if the error drives the command back into the admissible
window. Round to `u32` only after summing the centre, proportional, and integral
terms. Do not retain the current correction remainder: the floating-point
integrator already carries fractional information, and a remainder added to an
absolute PI reconstruction would double-count it.

The gains may be signed because an unusual measured phase-frequency slope may
reverse the required direction. Validation must reject non-finite gains. The
slope and expected positive sign must be confirmed by a low-amplitude open-loop
frequency sweep before PLL operation.

### Frequency setpoint and bounds

The PLL has its own centre-frequency parameter, `pll_centre_freq`, matching the
VCO centre in the experimental method. In PLL mode it is the sole centre
frequency; the standard generator `freq` remains authoritative only in `None`
and `Pid` modes. This avoids cross-group validation between `freq` and the PLL
window.

The parameter group enforces

```text
0 <= pll_freq_min <= pll_centre_freq <= pll_freq_max < sample_rate / 2.
```

Scalar writes must preserve that invariant at every accepted intermediate
state. A host moving the complete window may widen it, move the centre, and
then narrow it. Public run-time setters in `Pll` must not panic on bad values;
use checked setters and share their validation rules with the core-0 parameter
group. A failure after a command was acknowledged indicates an internal
contract error and must produce a programme fault rather than a panic on core
1.

### Instrumentation delay and phase compensation

The excitation and response do not presently share an acquisition time base.
The excitation reference is sampled by the AD7609, including its oversampling
group delay. The laser is exposed and processed in the optoNCDT, transported by
UART on core 0, and read by core 1 through a latest-value atomic which may be up
to one sample old. These paths add a differential delay which is separate from
the question of whether `drive` represents force.

For a differential delay `tau_inst`, the raw measured phase contains a bias
`-360 f tau_inst` degrees. Add a signed `instrument_delay_s` to the existing
PLL and correct its measured phase by `+360 f tau_inst` before forming the
error. This constant-delay model must be fitted over the intended frequency
window during the open-loop commissioning sweep. If the residual is not
consistent with a pure delay, replace it with an evidenced calibration model
before interpreting quadrature as phase resonance. Quote the uncertainty in
the fitted delay as the systematic phase uncertainty
`360 f uncertainty(tau_inst)` on every backbone point.

### Loop-bandwidth hierarchy

Initial PI gains come from the measured phase-frequency slope and a deliberately
quasi-static hierarchy, not from trial-and-error simulation alone. In angular
frequency terms require

```text
omega_amplitude << omega_pll
omega_pll << 1 / demod_time_constant << omega_carrier
omega_pll << zeta * omega_n.
```

The host amplitude loop is at least five to ten times slower than the PLL. The
PLL is well below both the demodulator pole and the plant's resonant settling
rate, otherwise it acts on filter or mechanical transients as though phase were
a static function of frequency and can false-lock. Establish candidate gains
from this hierarchy, then test them in simulation and at low amplitude.

### Lock qualification

`Locked` must mean more than small phase error. Acquisition requires all of the
following continuously for `pll_lock_dwell`:

- a valid observation with both amplitudes above their thresholds;
- absolute phase error below `pll_lock_phase_tol`; and
- peak-to-peak unquantised frequency command over the candidate dwell below
  `pll_lock_freq_tol`, in Hz.

Any invalid sample or failed condition resets the acquisition dwell rather than
merely pausing it. Track stationarity from the floating-point PI command before
rounding to a phase increment. If its running span exceeds the tolerance,
restart the dwell and extrema at the current command. This measures
"stationary for the dwell" without the 0.0149 Hz/s single-tick floor produced
by one increment quantum at 8 kHz.

Once locked, retain separate unlock tolerance and dwell hysteresis. Loss of a
valid observation, excessive phase error, or sustained saturation enters the
latched `LockLost` state. Add loss on sustained frequency non-stationarity only
if simulation shows that it discriminates genuine loss of lock without
reacting to ordinary PI correction; frequency stationarity is mandatory for
declaring acquisition but not yet established as a useful trip condition.

Acquisition timeout remains non-faulting and returns to fixed operation at the
PLL centre frequency. `LockLost`, and only `LockLost`, contributes a programme
fault and trips the output safety gate.

### Reacquisition and parameter changes

Retain the replay-safe semantics of `set_enabled(true)`: replaying an unchanged
enable must not disturb an existing lock. Add an explicit `reacquire` operation
which:

- enters `Acquiring` from `Locked` or `Fixed`;
- preserves the current bounded frequency as the initial condition;
- clears acquisition, unlock, and saturation dwell timers; and
- retains useful demodulator state unless a full `reset` was requested.

Changing the target phase while PLL mode is active invokes `reacquire`
automatically. Forcing-coefficient changes do not have a platform callback:
sweep orchestration is host-side and explicitly pulses `pll_reacquire` after
the complete coefficient update. This avoids surprising lock loss after an
unrelated harmonic trim and keeps the platform control trait free of a
rig-shaped hook. `ctrl_reset` remains the full reset, returning to the centre
frequency and clearing filters and integrator state.

### Telemetry getters

The revised existing `Pll` exposes at least:

- state;
- measured excitation-to-response phase;
- phase error;
- excitation fundamental amplitude;
- response fundamental amplitude;
- commanded phase increment; and
- whether the frequency command is saturated.

Frequency conversion to hertz belongs in the programme, which knows the sample
rate.

## Rig-specific selectable control

### Structure

Add a dependency-light, host-testable rig control type, preferably in a local
library target rather than `main.rs`:

```rust
pub enum ControlMode { None = 0, Pid = 1, Pll = 2 }

pub struct MagnetoelasticControl<const H: usize> {
    mode: ControlMode,
    pid: Pid,
    pll: Pll<H>,
    output_enabled: bool,
    pid_error: f32,
    // Fixed telemetry and a bounded mode-transition fault pulse.
}
```

Its `StandardControl` implementation declares `REFERENCE_UNIT = "mm"` and:

- returns `forcing + table` and no frequency override in `None` mode;
- applies the PID to `target - laser`, returns
  `pid + forcing + table`, and returns no frequency override in `Pid` mode;
  and
- returns `forcing`, ignores the table contribution, and returns the PLL
  increment in `Pll` mode.

The control inputs include forcing and table so PID anti-windup can use the
remaining actuator headroom. On every PID tick set its internal output limits
to

```text
pid_min = SAFE_OUT_MIN - forcing - table
pid_max = SAFE_OUT_MAX - forcing - table,
```

where `SAFE_OUT_MIN` and `SAFE_OUT_MAX` are derived from the same compile-time
DAC window used by `Rig::clamp_output`. Do not expose softer run-time PID limits
which can disagree with the hardware decision. The downstream safety clamp
remains the final backstop, but ordinary feed-forward saturation now freezes an
integrator which would drive further into saturation.

The PID feedback channel is fixed to the named laser input at compile time.
Do not retain a freely writable numeric feedback index: it permits
dimensionally invalid feedback and makes saved parameter sets dependent on
source ordering.

PID entry is also firmware-checked. On the first enabled PID tick, and on the
first tick after activating replacement target coefficients in PID mode,
compare the active `target_coeffs.mean` with the current laser distance. If
their absolute difference exceeds an evidence-backed
`PID_ENTRY_ERROR_MAX_MM`, raise a programme fault before applying output. The
rig control can detect the latter case from the `reference_mean` input, without
a target-specific platform callback. This turns the 25 mm absolute-laser offset
trap into an enforced condition. A bumpless initial PID contribution is
desirable, but it does not replace the mean check because an integral term
would otherwise drive persistently towards a dimensionally valid but unsafe
zero-millimetre target.

The PLL response channel is likewise the laser. The excitation reference is a
hardware decision which remains unresolved:

- `drive` is a command-path loopback, not measured force; and
- `coil` is a specimen-side pickup, not an established force measurement.

Using `drive` would control command-to-displacement phase. It supports a
repeatable engineering PLL only after its powered phase transfer is measured,
but a `-90 deg` lock would not yet be evidence of force appropriation. A
scientifically defensible phase-resonance result requires a measured exciter
current or force channel, or an experimentally validated correction from
`drive` phase to applied-force phase. The selected source and evidence must be
recorded in `notes.md` before enabling PLL hardware acceptance.

### Parameter group

Replace `ControllerGroup<ActiveController>` in this rig with a fixed-capacity
`MagnetoelasticControlGroup`. It retains accepted core-0 shadows, validates
before acknowledgement, converts frequency quantities to phase-increment
units, and sends commands to `DOMAIN_CONTROLLER` for sample-boundary
application.

The proposed host-visible parameters are:

| Parameter | Type | Unit or values |
|---|---|---|
| `control_mode` | `u32` | 0 none, 1 PID, 2 PLL |
| `ctrl_reset` | `u32` | non-zero pulse |
| `pll_reacquire` | `u32` | non-zero pulse |
| `pid_kp` | `f32` | V/mm |
| `pid_ki` | `f32` | V/(mm s) |
| `pid_kd` | `f32` | V s/mm |
| `pid_tau_d` | `f32` | s |
| `pll_centre_freq` | `f32` | Hz |
| `pll_freq_min` | `f32` | Hz |
| `pll_freq_max` | `f32` | Hz |
| `pll_kp` | `f32` | Hz/degree, normally positive |
| `pll_ki` | `f32` | Hz/(degree s), normally positive |
| `pll_target_phase` | `f32` | degree, conventional response minus excitation |
| `pll_delay` | `f32` | s, signed response-path minus excitation-path delay |
| `pll_dc_tau` | `f32` | s |
| `pll_demod_tau` | `f32` | s |
| `pll_excitation_min` | `f32` | excitation-source unit |
| `pll_response_min` | `f32` | mm |
| `pll_lock_phase_tol` | `f32` | degree |
| `pll_unlock_phase_tol` | `f32` | degree |
| `pll_lock_freq_tol` | `f32` | Hz peak-to-peak over lock dwell |
| `pll_lock_dwell` | `f32` | s |
| `pll_unlock_dwell` | `f32` | s |
| `pll_acquire_timeout` | `f32` | s |
| `pll_saturation_dwell` | `f32` | s |

All time constants, amplitudes, tolerances, and dwell times are finite and
non-negative, except `pll_dc_tau`, which is strictly positive. Phase lies in
`[-180, 180)`. Unlock phase tolerance is not smaller than lock phase
tolerance. Defaults must be identical in the core-0 shadow and the constructed
core-1 object. PID gains default to zero. No unevidenced PLL gain or frequency
window is made a production default merely to make acquisition appear to work.

The core-0 group rejects `control_mode` with `BadValue` while its safety
snapshot is armed. This makes the ordinary error synchronous and informative.
There remains a race in which the mode write is accepted while disarmed but the
host arms before core 1 consumes it. The RT backstop handles that case: `apply`
sets `mode_change_pending`; `step` observes that pending change together with
enabled output, moves it into `mode_change_fault`, and emits safe raw output;
`Program::fault` reads the flag later in the same tick; and only the following
`step` clears it. Test this exact apply, step, fault, gate ordering.

Every accepted mode change resets both algorithms. A raced armed change
therefore latches the safety trip and requires explicit re-arm, while an
ordinary disarmed change does not turn routine configuration into a safety
recovery. Only the host arms or clears the trip.

### Fixed telemetry and source budget

Source discovery cannot change with mode. Expose this fixed control telemetry
in every mode:

| Source | Unit | Inactive value |
|---|---|---|
| `control_mode` | enum | current mode |
| `pid_error` | mm | `NaN` outside PID mode |
| `pll_phase` | degree | `NaN` before a valid observation |
| `pll_phase_error` | degree | `NaN` before a valid observation |
| `pll_exc_amp` | V | `NaN` before valid observation |
| `pll_resp_amp` | mm | `NaN` before valid observation |
| `pll_freq_actual` | Hz | current programme frequency |
| `pll_state` | enum | current PLL state |

Do not spend six stream slots on physically unused `adc2` to `adc7`. The ADC
frame is still acquired in full, but `Rig::INPUTS` publishes only named,
connected measurements. Reserve one spare physical ADC input for a measured
exciter-current or force signal and name it when wired; this replaces a spare
rather than growing the source table. A likely commissioned set is `coil`,
`drive`, `current`, `laser`, and `stator`, giving five rig inputs, twelve
programme signals, one actuator, and `cmd_epoch`: 19 of the 24 reviewed slots.
Until `current` is physically present and calibrated, omit it rather than
publishing a plausible zero.

Keep the eight control signals coherent in the capture for initial
commissioning. If later source pressure is real, the slowly varying amplitude
and state views can move to typed `ExtraParam` atomics, but that is not needed
to create room for the already anticipated current channel.

`pll_freq_actual` describes the frequency used for the current record. The
shortened amplitude names respect the protocol's 15-character source-name
limit; parameter names have the separate 23-character limit.

The authoritative applied excitation remains `out`, after the safety gate.
Captures used as evidence include at least the excitation reference, `laser`,
`forcing`, `phase`, all PLL telemetry, `out`, and `cmd_epoch`.

## Safety and operating sequence

Boot selects `None`, with zero target, forcing, and table, and remains
disarmed. Before first entering PID mode in a session, measure the laser resting
position and set the mean of `target_coeffs` to it while PID gains remain zero.
A zero-mean displacement target against an absolute laser reading near 25 mm
would otherwise command a large, dimensionally valid error as soon as non-zero
gain is applied. The firmware entry check is authoritative; this operating
sequence explains how to satisfy it.

The normal PLL sequence is:

1. establish the laser resting point and validate the chosen excitation-phase
   measurement;
2. configure the PLL while disarmed, including a conservative frequency
   window, evidenced instrumentation-delay compensation, and small forcing
   amplitude;
3. select PLL mode, clear diagnostics, and arm through the persistent host;
4. observe `Acquiring`, and accept a point only after `Locked` and independent
   host settling checks;
5. step forcing amplitude for a backbone, or target phase plus the slow outer
   amplitude loop for an NLFRF;
6. pulse `pll_reacquire` after each completed forcing-coefficient update; and
7. on `LockLost`, displacement trip, current excursion, or communication loss,
   remain quiet until the cause is understood, reset, and explicitly re-arm.

The existing per-tick voltage clamp, displacement window, laser-staleness
guard, non-finite output guard, arm state, and communication-loss disarm remain
downstream of the complete programme output. PLL frequency bounds supplement
these protections but are not a substitute for output and displacement limits.

`LockLost` remains a programme fault which trips the output. Near a fold, loss
of phase lock can precede a jump to a distant, high-amplitude attractor, so
falling back to fixed excitation while hoping the displacement window catches
the jump is not the conservative choice. The operational cost is deliberate:
any established-lock loss aborts an unattended sweep, leaves the output quiet,
and requires diagnosis and explicit host re-arm. Acquisition failure before
lock remains non-faulting at the bounded centre frequency.

The firmware currently has no calibrated current safety input. Until one is
available, the host sweep must retain its guarded, short-capture current and
displacement checks. Do not represent `drive` as measured current.

## Verification

### Pure PLL tests

Extend the existing `helic-core` tests to cover:

- the conventional phase sign at `-90`, `0`, and `+90 deg`;
- wrapping immediately either side of `-180/180 deg`;
- a small sinusoid about a 25 mm response mean;
- non-zero and slowly drifting means on both inputs;
- warm-up after reset and reacquisition, including timeout starting only after
  warm-up;
- independent excitation and response amplitude thresholds;
- constant instrumentation-delay compensation, sign, and uncertainty;
- PI convergence for both signs of a monotone phase-frequency slope;
- proportional response, integral convergence, conditional anti-windup, and
  recovery from both frequency limits;
- phase-increment quantisation while the floating-point integrator retains
  sub-quantum corrections;
- continuous valid dwell, with one invalid sample resetting acquisition dwell;
- phase and unquantised frequency-span lock qualification;
- lock/unlock hysteresis, acquisition timeout, saturation dwell, and latched
  lock loss;
- replayed enable preserving lock and explicit reacquisition preserving the
  current frequency; and
- every public run-time setter rejecting invalid values without panicking.

### Programme and registry tests

Add host tests for:

- sample-for-sample compatibility of blanket-adapted existing controllers;
- shared harmonic projection matching the previous target and forcing values;
- an increment returned on tick `n` first affecting tick `n + 1`;
- leaving PLL mode restoring the standard nominal frequency;
- no control-state evolution while output is disabled and reset on re-arm;
- the exact output expression in all three modes;
- table suppression in PLL mode;
- residual PID output limits after forcing and table, PID anti-windup, and the
  target-mean interlock on entry and coefficient replacement;
- armed mode-write rejection, the arm/write race, fault-pulse ordering, and
  explicit re-arm behaviour;
- exact controller reset and first/last parameter-id routing;
- parameter type, bound, cross-parameter, shadow, and command-epoch semantics;
- fixed source names, units, ordering, omission of unused ADC sources, and the
  24-source limit; and
- `Program::fault` being true only for the mode-transition pulse, internal
  contract failure, or `PllState::LockLost`.

### Simulation

Before hardware operation, close the revised PLL around both a linear
second-order plant and the project's Duffing virtual rig. Generate time-series
figures of displacement, excitation, measured phase, phase error, commanded
frequency, amplitudes, state, saturation, and applied output. Use the
simulations to establish conservative starting gains and confirm:

- acquisition from both sides of resonance;
- no false lock during slow frequency drift;
- stable reacquisition following an amplitude or phase step;
- the PI bandwidth remaining below both demodulator and mechanical settling
  bandwidths;
- at least a factor of five to ten between phase-loop and host amplitude-loop
  bandwidths; and
- correct behaviour near a frequency bound and on observation loss.

The simulation establishes plausibility, not hardware stability.

### Software and hardware gates

Run the complete check set in `README.md` for both supported boards. Update
`rig-profile.toml` for every new required hot symbol and for the revised capture
source set. Inspect compiler-generated calls reachable from the new control
path, then run `helic-rt-layout`.

On hardware, commission sequentially at the existing safe starting amplitude:

1. verify `None` mode, mode-change quieting, and all source metadata;
2. verify PID reset, sign, low gains, clamp, trip, and re-arm;
3. measure the powered excitation-reference transfer and the open-loop
   phase-frequency slope;
4. acquire PLL lock at one low-amplitude point from above and below resonance;
5. provoke acquisition timeout, frequency saturation, observation loss, and
   lock loss, verifying the intended safe output each time;
6. run a short stepped-amplitude backbone segment with independent host
   settling and spectrum estimation; and
7. record loop timing, overruns, drops, source captures, and electrical evidence
   in `notes.md`.

The 8 kHz worst-case PLL path, including command-application ticks, must remain
below the existing 60 microsecond hardware regression limit with zero overruns.
The former 63 to 64 microsecond command-copy cost was removed in platform
v0.2.1; this rig subsequently measured quiet and hundreds of write ticks at
about 44 to 45 microseconds, and v0.2.5 retains that compact command envelope.
The 60 microsecond acceptance is therefore current and testable, not merely a
quiet-tick limit. Software checks alone do not establish the revised PLL path.

## Delivery sequence and repository ownership

This document is the cross-repository design record requested before work
begins. Before implementation, split the normative platform half into
HELIC-DAQ beside `docs/rt_program_proposal.md` and
`docs/rig_decoupling_proposal.md`; retain here only the rig composition,
parameters, source choices, safety decisions, and commissioning contract. The
platform pull request is reviewed and merged before the rig is repinned.

`Pll` does not yet have two firmware consumers, so this work does not satisfy
the platform's usual placement test by inventing one. Revising it in place is
nevertheless preferable because the already exported implementation and its
tests are in `helic-core`; moving it into this rig merely to move it back on a
second use would be churn. No second PLL implementation is created.

1. In the HELIC-DAQ platform proposal and implementation, introduce
   `StandardControl`, the existing
   `Controller` adapter, shared-frame `StandardProgram`, coherent per-tick
   output-enabled flag, and compatibility tests.
2. In the same platform, revise the existing `helic_core::Pll` and its tests.
   Do not add a second PLL type.
3. Release a platform tag. Update this repository's `Cargo.toml`, CI workflow,
   and README installation command together, after grepping for the old tag.
4. In this repository, add the selectable control and parameter group, compose
   it in `main.rs`, update source metadata and `rig-profile.toml`, and add host
   tests in the local library target.
5. Add the slow outer amplitude controller and PLL orchestration to the CBC
   Duffing host library, then update its method note to say the phase detector
   and VCO now run in firmware while sweep orchestration remains host-side.
6. Simulate, plot, commission on hardware, and record evidence before declaring
   PID or PLL mode verified.

The platform tag also corrects two documentation debts: `Controller::reset`
must describe the lifecycle actually implemented, and the developer guide's
standard-signal ordering and target-unit description must reflect the new
control-provided reference unit. Both changes are called out in the tag
message.

If the rig work must begin before the platform release, use a temporary
`[patch.crates-io]` against the platform branch, record why it exists, and
remove it immediately after repinning to the released tag.

## Deferred extensions

The first implementation is a fundamental, linear-PI PLL. It deliberately
defers:

- nonlinear-controller PLL laws;
- multi-harmonic phase criteria or force appropriation;
- automatic continuation and sweep scheduling in firmware;
- a firmware response-amplitude loop; and
- dynamic selection of measurement channels.

Strong odd harmonics are expected from the magnetic nonlinearity. Every locked
point therefore retains full-spectrum host analysis. If PLL and CBC backbone
results disagree beyond experimental uncertainty, the fundamental phase
criterion and excitation-force measurement are investigated before adding an
NCPLL or multi-harmonic controller.

## Review, 2026-08-18

Read against the platform at the pinned tag: `helic-core`'s `pll.rs`,
`harmonics.rs`, `pid.rs`, and `controller.rs`; `helic-rt`'s `program.rs`,
`params/groups.rs`, `rig.rs`, and `safety.rs`; `firmware/rt/src/rt_loop.rs`;
this rig's `config.rs`, `rig.rs`, and `main.rs`; and both `AGENTS.md` files.

The architecture holds and the repository split follows the platform's own
rules. The loop design has correctness gaps that are cheap to close now and
expensive later, and the safety story leans on operator prose in the two places
where this rig has most recently decided the opposite.

### Verified, so it need not be rechecked

- **The blanket adapter compiles.** A two-crate fixture with an upstream
  blanket `impl<C: Controller, const H: usize> StandardControl<H> for C`, a
  downstream local implementation, and both `StandardProgram<PassThrough, 16>`
  and `StandardProgram<RigControl<16>, 16>` builds without a coherence error.
  A downstream local type cannot acquire a `Controller` implementation from
  elsewhere, so the overlap check is satisfied. Existing rig aliases survive.
  The one lasting constraint is that no type may implement both traits.
- **The shared-frame refactor completes an existing intent rather than adding
  machinery.** `HarmonicFrame`, `HarmonicGenerator`, and `Pll` are all
  currently dead code in `helic-core`: exported from `lib.rs` and used by
  nothing. This proposal is their first consumer.
- **The sine and cosine saving is real.** `FourierCoeffs::evaluate` walks all
  `H` harmonics per call, so target and forcing cost 64 lookups per tick at
  `HARMONICS = 16`. One shared frame costs 32.
- **The budget arithmetic is right.** Ten inputs, twelve programme signals, one
  actuator, and `cmd_epoch` is exactly `MAX_SOURCES = 24`. `pll_freq_actual`
  and `pll_phase_error` are exactly 15 characters; `pll_lock_freq_rate_tol` is
  22 against the 23-character parameter limit. `MAX_CTRL_PARAMS = 17` does not
  bind a rig-owned group, and `MAX_GROUPS = 8` is not reached at six groups.
- **The DC-rejection argument is quantitatively necessary, not defensive.** A
  mean `D` leaks through synchronous demodulation as roughly
  `2 D / (omega tau_demod)`, so 25 mm at 30 Hz with `tau_demod = 0.1 s` gives
  about 2.7 mm of carrier-rate ripple against a sub-millimetre response.

### Correctness gaps

**The mode-change fault pulse can be cleared before it is ever read.** One tick
runs as apply commands, `step_program`, `program_fault`, then the gate. If
`apply` sets the pulse and `step` clears it, `Program::fault` never observes it
and the interlock is decorative. The rule must be stated and tested: `apply`
sets pending, `step` moves pending into the flag `fault` reads, and the
following `step` clears that flag.

**The PID's own anti-windup is left disarmed.** `Pid` implements conditional
integration against `config.out_min` and `out_max`, which default to plus and
minus `f32::MAX`. The parameter table exposes the four gains and no limits, so
whenever the safety gate is clamping but not tripped the integrator winds up
without bound. Reset on re-arm does not cover that case, because no transition
occurs. Bind the limits to `DAC_OUT_FLOOR_V` and `DAC_OUT_CEILING_V` at
construction, or expose them as parameters.

**Instrumentation phase bias is unaccounted for and lands on the phase
criterion.** The excitation reference reaches the loop through the AD7609 at
OS8, a group delay of tens of microseconds, under a degree at tens of hertz.
The response reaches it from the optoNCDT over UART, published by core 0 as a
latest-value atomic, at most one tick stale, plus the sensor's own exposure and
processing latency, which is nowhere measured in this repository. The two
channels therefore do not share a time base, and the measured
excitation-to-response phase carries an unknown fixed delay. A pure delay
`tau` biases the phase by `-360 f tau` degrees, growing linearly along the
backbone. This is separate from, and additional to, the `drive`-is-not-force
problem already recorded above. The open-loop sweep of commissioning step 3
already produces the necessary data; what is missing is the requirement that
the fitted phase be decomposed into plant phase plus instrumentation delay
before `-90 deg` means anything, and that the delay be either compensated in
`pll_target_phase` or bounded and quoted as a systematic uncertainty on every
backbone point.

**The loop-bandwidth hierarchy is under-specified.** Only the factor of five to
ten between phase and amplitude loops is given. Two further constraints matter
more, and both belong in the tuning section:

- `omega_loop` well below `1 / tau_demod`, itself well below the carrier, or
  the demodulator pole sits inside the loop; and
- `omega_loop` well below `zeta omega_n`, the plant's own settling rate.

The PI acts on the assumption that response phase is a static function of
excitation frequency. That holds only quasi-statically. A loop faster than the
resonance settles chases its own transient, which is the classic false-lock
mechanism the simulation section is asked to rule out but is given no criterion
for. Starting gains should be derived from this hierarchy and then confirmed by
simulation, not chosen by simulation alone.

**The frequency-slew lock criterion has a quantisation floor.** One
phase-increment quantum at 8 kHz is `8000 / 2^32`, that is 1.86 microhertz, so
a single-tick slew estimated from the quantised increment reads 0.0149 Hz/s
whenever the command moves at all. Computing the slew from the unquantised PI
output in hertz, and across the lock dwell rather than one tick, removes the
floor entirely and measures what "stationary for the dwell" actually means.

**The DC estimator's warm-up is a real transient.** Seeding from the first
sample sets the mean to a point on the waveform, leaving an error of order the
signal amplitude which decays over `dc_time_constant`. With that constant slow
relative to the carrier, as required, the transient outlives a 0.1 s lock
dwell. Require an explicit warm-up of several times
`max(dc_time_constant, demod_time_constant)` during which no observation is
valid, and test it. Otherwise the first `Locked` after every arm or reacquire
rests on corrupted phase.

### Simplifications

- **Drop the correction remainder.** With `f = f_centre + Kp e + I`
  reconstructed absolutely each tick, rounding once loses nothing: the
  integrator is floating point and carries the fractional information, and the
  quantum is 1.86 microhertz. The remainder is a leftover of the current
  incremental design, `commanded_increment += gain * error * dt`, and keeping
  it on top of an absolute computation double-counts. The incremental
  formulation is discarded, not adapted.
- **Drop `forcing_changed`.** It is a rig-shaped hook in a platform trait,
  redundant with the `pll_reacquire` pulse specified in the same document, and
  surprising in use, since any coefficient write including a small harmonic
  trim would drop the lock. Sweep orchestration is already host-side; let the
  host pulse `pll_reacquire`.
- **Drop `disabled_increment`.** It buys one sample of frequency accuracy at
  arm. Phase is continuous across an increment change, so one sample at the
  nominal increment is a phase error of `(f_nom - f_centre) / f_s` turns, which
  is negligible. It costs a second increment-override path whose semantics
  differ from `next_increment`.
- **Shrink the `step` signature.** Seven positional arguments, two of them
  redundant, since `dt` is `sample_rate.dt()`. Pass `&StepCtx` plus one small
  inputs struct. Redundant parameters that must agree are how they eventually
  disagree.
- **Say what happens to `StepCtx`.** It is currently loop-invariant, built once
  in `run_rt_loop` and stored in `RtLoopState`, and is documented as immutable
  services. Adding `output_enabled` makes it per-tick state. Either construct
  it per tick, which costs two words, or pass the flag separately, but choose
  deliberately rather than contradicting the type's stated character.
- **Specify who handles `command_id::controller::RESET`.** Today
  `StandardProgram::apply` special-cases it and offsets every other id by one.
  If `StandardControl::apply` sees raw ids, a rig control can silently fail to
  honour `ctrl_reset`. State that the programme keeps routing `RESET` to
  `reset`, forwards ids of one and above unchanged, and test that mapping
  specifically rather than under the general heading of commands.
- **Fix the sign convention so that positive gains are the expected case.**
  Signed gains are correct, but an operator entering the wrong sign gets a
  runaway to a frequency bound. Define the error so that a system with the
  usual negative phase-frequency slope needs positive `pll_kp` and `pll_ki`,
  and say so beside the parameters.

### Two decisions worth arguing

**Using the safety latch as a state-machine device.** A mode change while armed
raising a fault works, but it overloads a mechanism whose documented meaning is
that something unsafe happened, and it turns a routine host action into an
operator recovery. Core 0 can read `shared.safety.load_inputs().armed`, so the
natural design is to reject the write with `BadValue` while armed, which is
synchronous and informative, and keep the real-time fault only as the backstop
for the case where arming races the write. Same guarantee, better ergonomics.

**`LockLost` tripping the output.** Probably right for a hardening system,
where losing lock near a fold can precipitate a jump to a large-amplitude
branch, but the document asserts it rather than arguing it, and does not state
the operational consequence: every lost lock during an amplitude step ends an
unattended sweep until a human re-arms. Either justify it on the fold hazard
explicitly, or make lock loss a latched status with a fallback to fixed
operation at the last bounded frequency, leaving the displacement window to do
the safety work it already does.

### Rules that should be firmware, not prose

This rig's most recent precedent is `MAX_STATOR_RATE_MM_S`, where measured
evidence moved out of `notes.md` into an enforced bound with the reasoning
recorded beside the constant. Two rules here are left as operator instructions,
and both bite hard:

- **A free-running table must be off in PLL mode.** Nothing enforces it. Either
  ignore the table contribution in PLL mode, or refuse entry to PLL mode while
  `table_mode` is non-zero.
- **The target mean must be set to the laser resting position before PID
  mode.** This is the largest single hazard in the document. A zero-mean target
  against a 25 mm absolute reading is a dimensionally valid 25 mm error that
  commands full output the instant a gain becomes non-zero. Enforce it with an
  entry check that `|target_mean - laser|` lies within a bound, or with a
  bumpless transfer that initialises the integrator so the output is continuous
  at mode entry.

### Budget and gates

- **The source budget is exactly full, and this document says another channel
  is needed.** A calibrated excitation-current measurement is required before
  the science is defensible, and there is no slot for it. Six of the ten inputs
  are unused spare ADC channels, `adc2` to `adc7`, streamed at 8 kHz. Free
  them, or move the slow PLL telemetry, `pll_state`, `pll_exc_amp`, and
  `pll_resp_amp`, which evolve on `demod_time_constant`, to `ExtraParam`
  atomics, which is the platform's idiom for slow read-only values. Arriving at
  exactly the cap while knowing another channel is coming is a decision to make
  now rather than during commissioning.
- **The 60 microsecond acceptance is stated against a limit already
  exceeded.** `notes.md` records that `max_loop_us = 60` is exceeded by any
  parameter write, at 63 to 64 microseconds, from the fixed 140-byte
  `RtCommand` copy, against 44 to 45 quiet. As written the acceptance is
  untestable. Phrase it as quiet-tick `loop_time_max` with the known write-tick
  exception, and quote both.

### Repository ownership

- **The platform half of this document belongs in `helic-daq`**, beside
  `docs/rt_program_proposal.md` and `docs/rig_decoupling_proposal.md`. This
  repository's `AGENTS.md` is explicit that platform mechanisms go upstream as
  a pull request rather than being designed here. Split it: `StandardControl`,
  the `StandardProgram` refactor, `StepCtx`, and the `Pll` revision upstream;
  the selectable mode, parameter group, telemetry, and commissioning sequence
  stay.
- **The `helic-core` placement rule is two actual consumers.** `Pll` has none
  today and one after this work, and the CBC Duffing contribution in the
  delivery sequence is host-side, so it does not become the second firmware
  consumer. Revising in place remains the right call, because the code is
  already there and moving it is churn, but say so rather than leaving the rule
  apparently satisfied.
- **Two documentation debts travel with the platform tag.**
  `Controller::reset` claims a lifecycle the platform does not currently
  implement, and the developer guide's description of the standard signal
  ordering changes with the reference unit. Both belong in the tag message.

### Suggested order of work

1. The three that change behaviour rather than shape: fault-pulse ordering, PID
   output limits, and the slew criterion computed from the unquantised command.
2. The bandwidth hierarchy and the instrumentation-delay decomposition, making
   commissioning step 3 explicitly responsible for measuring the delay.
3. Prune `forcing_changed`, `disabled_increment`, and the correction remainder,
   after which the trait has one increment path and no rig-shaped hooks.
4. Settle the source budget before implementation rather than after.
5. Split the document and open the platform half upstream.

## Response to review, 2026-08-18

The review materially improves the design. The proposal above has been revised
rather than leaving accepted comments as implementation folklore. Dispositions
follow in the review's order.

### Correctness gaps

- **Fault-pulse ordering: accepted.** The normative sequence is now
  `apply: pending`, `step: pending -> visible fault`, `fault: read`, and only
  the following `step: clear`. It is the race backstop rather than the ordinary
  mode-change path.
- **PID anti-windup: accepted and strengthened.** The PID limit is the residual
  actuator headroom after forcing and table, derived every tick from the
  compile-time safe output window. This covers additive feed-forward saturation
  which binding the PID to the full window alone would miss.
- **Instrumentation delay: accepted.** The revised existing PLL has a signed
  delay compensation, commissioning fits the delay over frequency, and every
  backbone result carries its systematic phase uncertainty. This remains
  distinct from whether the excitation channel measures force.
- **Bandwidth hierarchy: accepted.** Gain selection now begins from the
  demodulator, mechanical-settling, carrier, and outer-amplitude bandwidth
  ordering and is subsequently tested by simulation.
- **Quantised slew: accepted.** Lock qualification now uses the span of the
  unquantised floating-point frequency command across a continuous dwell. The
  `pll_lock_freq_rate_tol` parameter is replaced by `pll_lock_freq_tol` in Hz.
- **DC warm-up: accepted.** Reset and reacquisition have an explicit filter
  warm-up, observations are invalid during it, and acquisition timeout begins
  afterwards.

### Simplifications

All six are accepted. The correction remainder, `forcing_changed`, and
`disabled_increment` are removed. The control call takes one inputs struct and
the immutable `StepCtx`; output-enabled state is a separate per-tick flag from
one safety snapshot. `StandardProgram` retains and tests reset/id routing. The
phase error is measured minus target, making positive gains the expected case
for the usual negative phase-frequency slope.

The simplifications reveal a cleaner output contract: the control policy
returns the complete raw programme output rather than only a contribution.
That lets PLL mode enforce `output = forcing` and ignore table playback without
a rig-shaped table hook in the platform.

### Safety decisions

- **Mode change: revised as suggested.** Core 0 rejects an armed write
  synchronously. The RT fault pulse covers only the arm/write race, where the
  write was valid when accepted but output is enabled before core 1 applies it.
- **Lock loss: trip retained and justified.** Near a fold, fixed-frequency
  fallback can drive a jump to a remote high-amplitude attractor before the
  displacement guard reacts. An established-lock loss therefore aborts an
  unattended sweep and requires explicit diagnosis and re-arm. Acquisition
  failure before lock remains non-faulting.

### Firmware-enforced rules

Both comments are accepted. PLL mode ignores table output in firmware. PID
entry checks the active target mean against the live laser with an
evidence-backed bound and faults before output if it fails. Operator prose now
explains how to satisfy these interlocks rather than carrying the safety case.

### Source budget and timing gate

- **Source budget: accepted.** Unconnected ADC channels are omitted from
  `Rig::INPUTS`, and a physical spare is reserved for a named measured-current
  channel. The likely commissioned source set uses 19 of 24 slots, so coherent
  PLL telemetry need not be demoted prematurely to atomics.
- **Timing claim: corrected, not accepted.** The review cites valid measurements
  from platform v0.1.3, but the later entry `Platform upgraded to v0.2.1` records
  the command-envelope fix and 366 write ticks at 45 microseconds. The v0.2.3
  regression repeated 606 writes at 45 microseconds, and this repository is now
  pinned to v0.2.5. The 60 microsecond gate is current, includes a write phase,
  and remains the acceptance threshold for both quiet and command ticks. The
  proposal now states that provenance explicitly.

### Repository ownership

Accepted. This file remains the requested cross-repository decision record
while the design is being settled, but its normative platform half is split
into HELIC-DAQ before implementation and reviewed there. The rig half remains
here. The existing PLL is revised in place despite having only one immediate
firmware consumer because it is already an exported, tested `helic-core`
primitive; this is avoiding relocation churn, not pretending a second consumer
exists. The tag also fixes the `Controller::reset` lifecycle and developer-guide
signal-unit documentation debts identified by the review.
