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
  inputs, and the sample interval;
- return a scalar controller contribution to the actuator command;
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

| Value | Mode | Controller contribution | Master frequency |
|---:|---|---|---|
| 0 | `None` | zero | standard `freq` parameter |
| 1 | `Pid` | PID output from `target - laser` | standard `freq` parameter |
| 2 | `Pll` | zero | bounded output of the PLL |

The complete programme output remains

```text
out_raw = controller_contribution + forcing + table.
```

Consequently:

- `target_coeffs` is always a displacement reference in millimetres. It is
  consumed only in PID mode.
- `forcing_coeffs` is always open-loop excitation in volts. It supplies the
  excitation in both `None` and `Pll` modes and remains additive feed-forward
  in PID mode.
- `table` remains an additive voltage. A free-running table must be off in PLL
  mode because it is not coherent with the phase detector. A table deliberately
  locked to the master phase may be admitted later, after its effect on the
  measured excitation reference is tested.

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
pub struct ControlStep {
    /// Contribution summed with forcing and table, in actuator units.
    pub contribution: f32,
    /// Increment for the next tick. `None` means the nominal `freq` increment,
    /// not "retain the last increment".
    pub next_increment: Option<u32>,
}

pub trait StandardControl<const H: usize> {
    const REFERENCE_UNIT: &'static str = "V";
    const TELEMETRY: &'static [(&'static str, &'static str)] = &[];

    fn step(
        &mut self,
        inputs: &[f32],
        reference: f32,
        frame: &HarmonicFrame<H>,
        current_increment: u32,
        nominal_increment: u32,
        dt: f32,
        sample_rate: SampleRate,
    ) -> ControlStep;

    fn apply(&mut self, id: u16, payload: Payload);
    fn reset(&mut self) {}
    fn set_output_enabled(&mut self, _enabled: bool) {}
    fn disabled_increment(&self, _nominal: u32) -> Option<u32> { None }
    fn forcing_changed(&mut self) {}
    fn telemetry(&self, _out: &mut [f32]) {}
    fn fault(&self) -> bool { false }
}
```

`None` has deliberately positive semantics: it restores the nominal increment.
If it meant "retain", leaving PLL mode could silently leave the generator at
the last locked frequency.

Provide a blanket adapter from every existing `C: Controller` to
`StandardControl<H>`. It delegates scalar controller parameters, reset,
telemetry, and `tick`, returns no frequency override, retains `"V"` as the
reference unit, and never faults. Existing rig type aliases and
`ControllerGroup<C>` therefore remain valid.

The selectable rig control uses a rig-specific parameter group because its
parameters mix `u32` commands and `f32` values. The existing
`ControllerGroup<C>` remains the simple adapter for ordinary controllers.

### Harmonic generation inside `StandardProgram`

Replace `StandardProgram`'s bare `PhaseAccumulator` with
`HarmonicGenerator<H>`. Per tick it should:

1. advance the generator once and borrow the resulting `HarmonicFrame<H>`;
2. project `target_coeffs` and `forcing_coeffs` through that shared frame;
3. step the table using the frame's phase and wrap flag;
4. call the selected control policy while the frame is borrowed;
5. record the increment which generated the current sample;
6. release the frame borrow and install either the returned next increment or
   the nominal `freq` increment; and
7. sum the control contribution, forcing, and table.

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
Extend `StepCtx` with `output_enabled`, calculated by the RT loop as:

```text
not safety-gated, or (armed and not tripped).
```

This keeps ungated rigs permanently enabled. `StandardProgram` tracks the
previous value and:

- does not advance controller or PLL state while output is disabled;
- calls `reset` on a disabled-to-enabled transition;
- calls `set_output_enabled` after transition handling; and
- uses zero controller contribution while disabled.

While disabled, `StandardProgram` still installs the value returned by
`disabled_increment`, falling back to the nominal frequency. This rig's PLL
mode returns its centre increment, whereas the other modes and every existing
controller return the nominal increment. Selecting PLL while disarmed therefore
places the generator at the PLL centre before the first armed sample, without
running the estimator or applying an output.

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
terms. Retain a fractional remainder where needed so small integral corrections
are not lost to phase-increment quantisation.

The gains may be signed because the required feedback direction depends on the
measured phase-frequency slope. Validation must reject non-finite gains. The
sign must be established from a low-amplitude open-loop frequency sweep before
PLL operation.

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

### Lock qualification

`Locked` must mean more than small phase error. Acquisition requires all of the
following continuously for `pll_lock_dwell`:

- a valid observation with both amplitudes above their thresholds;
- absolute phase error below `pll_lock_phase_tol`; and
- absolute frequency slew below `pll_lock_freq_rate_tol`, in Hz/s.

Any invalid sample or failed condition resets the acquisition dwell rather than
merely pausing it. The frequency-slew calculation must account for one
phase-increment step at the configured sample rate so an unattainable tolerance
cannot be accepted.

Once locked, retain separate unlock tolerance and dwell hysteresis. Loss of a
valid observation, excessive phase error, or sustained saturation enters the
latched `LockLost` state. Add frequency-slew loss only if simulation shows that
it discriminates genuine loss of lock without reacting to ordinary quantised
frequency correction; it is mandatory for declaring acquisition but not yet
established as a useful trip condition.

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
automatically. Activating new forcing coefficients invokes the control
policy's `forcing_changed` hook and likewise reacquires. This supports backbone
amplitude steps and NLFRF target-phase steps without first returning to the
centre frequency. `ctrl_reset` remains the full reset, returning to the centre
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

- returns zero contribution and no frequency override in `None` mode;
- applies the PID to `target - laser` and returns no frequency override in
  `Pid` mode; and
- returns zero contribution and the PLL increment in `Pll` mode.

The PID feedback channel is fixed to the named laser input at compile time.
Do not retain a freely writable numeric feedback index: it permits
dimensionally invalid feedback and makes saved parameter sets dependent on
source ordering.

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
| `pll_kp` | `f32` | Hz/degree |
| `pll_ki` | `f32` | Hz/(degree s) |
| `pll_target_phase` | `f32` | degree, conventional response minus excitation |
| `pll_dc_tau` | `f32` | s |
| `pll_demod_tau` | `f32` | s |
| `pll_excitation_min` | `f32` | excitation-source unit |
| `pll_response_min` | `f32` | mm |
| `pll_lock_phase_tol` | `f32` | degree |
| `pll_unlock_phase_tol` | `f32` | degree |
| `pll_lock_freq_rate_tol` | `f32` | Hz/s |
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

Changing mode resets both algorithms. A mode change while output is enabled
produces a one-tick programme fault, thereby latching the existing safety trip;
the host must explicitly re-arm. The fault pulse then clears, but the safety
trip does not. This guarantees that a PID integrator or PLL state acquired in
one mode cannot be carried silently into another and preserves the rule that
only the host arms the gate.

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

With ten rig inputs, these eight control signals, the four standard programme
signals, one actuator, and `cmd_epoch`, this rig uses all 24 reviewed source
slots. No further source can be added without either removing one of these or
deliberately changing the platform capacity. `pll_freq_actual` describes the
frequency used for the current record. The shortened amplitude names respect
the protocol's 15-character source-name limit; parameter names have the
separate 23-character limit.

The authoritative applied excitation remains `out`, after the safety gate.
Captures used as evidence include at least the excitation reference, `laser`,
`forcing`, `phase`, all PLL telemetry, `out`, and `cmd_epoch`.

## Safety and operating sequence

Boot selects `None`, with zero target, forcing, and table, and remains
disarmed. Before first entering PID mode in a session, measure the laser resting
position and set the mean of `target_coeffs` to it while PID gains remain zero.
A zero-mean displacement target against an absolute laser reading near 25 mm
would otherwise command a large, dimensionally valid error as soon as non-zero
gain is applied.

The normal PLL sequence is:

1. establish the laser resting point and validate the chosen excitation-phase
   measurement;
2. configure the PLL while disarmed, including a conservative frequency
   window and small forcing amplitude;
3. select PLL mode, clear diagnostics, and arm through the persistent host;
4. observe `Acquiring`, and accept a point only after `Locked` and independent
   host settling checks;
5. step forcing amplitude for a backbone, or target phase plus the slow outer
   amplitude loop for an NLFRF;
6. call `pll_reacquire` when an excitation change was not automatically
   observed by the programme; and
7. on `LockLost`, displacement trip, current excursion, or communication loss,
   remain quiet until the cause is understood, reset, and explicitly re-arm.

The existing per-tick voltage clamp, displacement window, laser-staleness
guard, non-finite output guard, arm state, and communication-loss disarm remain
downstream of the complete programme output. PLL frequency bounds supplement
these protections but are not a substitute for output and displacement limits.

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
- independent excitation and response amplitude thresholds;
- PI convergence for both signs of a monotone phase-frequency slope;
- proportional response, integral convergence, conditional anti-windup, and
  recovery from both frequency limits;
- phase-increment quantisation and fractional integral accumulation;
- continuous valid dwell, with one invalid sample resetting acquisition dwell;
- phase and frequency-stationarity lock qualification;
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
- mode-change trip and explicit re-arm behaviour;
- parameter type, bound, cross-parameter, shadow, and command-epoch semantics;
- fixed source names, units, ordering, and the 24-source limit; and
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

The 8 kHz worst-case PLL path must remain below the existing 60 microsecond
hardware regression limit with zero overruns. Software checks alone do not
establish this.

## Delivery sequence and repository ownership

1. In the HELIC-DAQ platform, introduce `StandardControl`, the existing
   `Controller` adapter, shared-frame `StandardProgram`, output-enabled context,
   and compatibility tests.
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
