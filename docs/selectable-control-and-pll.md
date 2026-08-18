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
