# Magneto-elastic rig notes

Hardware verification status and bring-up constraints for this rig. Read and
update this file when doing hardware work. Software checks establish neither
electrical nor real-time behaviour; only evidence recorded here does.

Platform-level notes (the shared real-time architecture, the embassy-time alarm
loss and its watchdog) stay in the HELIC-DAQ platform repository's `notes.md`.
Evidence recorded here starts at the creation of this repository on
2026-08-12; hardware evidence from before that date is held in the platform
repository and describes this rig under its former firmware name.

## Verification status

**Hardware-verified**, on a W5500-EVB-Pico2 with the interim analogue cape:
the 8 kHz hardware-clocked acquisition path, the DAC output path, networking,
discovery, the parameter registry, and UDP streaming of all 15 sources. The
figures under "Bring-up evidence" below are the current acceptance record.

The stator stage is only partly covered. It was wired and first driven on
2026-08-14: the firmware's step generation, command surface, and telemetry are
verified, and stepping leaves the real-time loop undisturbed. Of the mechanism,
the datum edge and the reversal dead band are measured as of 2026-08-17, and
the datum geometry, the steps per millimetre and the travel limits are not. The
axis has never been homed. See the standing constraints below and
`docs/stator-stage.md`.

Exceptions, all electrical rather than real-time. The DAC output path is
verified as it was driven before 2026-08-12, with channel A alone against a
fixed channel C; the symmetric A/C drive that replaced it is timing-verified
but has not been observed on a scope. ADC channel 0 was rewired to a stator
coil (`coil`) and its calibration has not been established. ADC channel 1
(`drive`) is declared in firmware but not yet wired. See the last two bring-up
entries below.

The exciter drive is only as safe as the limits in `src/config.rs`, and those
limits are compile-time constants that describe the fitted hardware. Re-check
them, and `DAC_POLARITY` in `src/rig.rs`, against the hardware before every
change of analogue board, specimen, or exciter.

## Standing hardware constraints

- **optoNCDT RX pull-up.** GP1 needs an external 10 kΩ pull-up to 3V3. Without
  it a disconnected or unpowered sensor leaves the line floating, and the UART
  free-runs into a framing/break interrupt storm that livelocks core 0. The
  symptom is a rig that boots and then stops answering on the network.
- **Analogue output stages.** The fitted cape is all-unipolar, and
  `DAC_POLARITY` in `src/rig.rs` is set to match. Channels A and C are the
  exciter's differential inputs and are driven symmetrically about the
  2.048 V common mode (`MID_RAIL + out` on A, `MID_RAIL - out` on C). This
  doubles the topology's achievable differential amplitude, from ±2.048 V
  (only A varying, over its own 0-4.096 V rail, against a fixed C) to the full
  ±4.096 V of the DAC reference; channel B is broken and channel D is unused,
  so both rest at 0 V. Output routing is fixed to channel A and
  `rig_out_channel` will reject any other value. The actual operating range
  stays clamped to ±1.952 V on `out` (±3.904 V differential) by
  `DAC_OUT_FLOOR_V`/`DAC_OUT_CEILING_V` in `src/config.rs`, an unchanged,
  deliberate 0.096 V headroom below each rail; only the topology ceiling
  doubled, not that software margin. Before 2026-08-12, C was held constant at
  `MID_RAIL` and only A varied.
- **Two DAC words per tick.** Driving A and C symmetrically means `actuate`
  writes two AD5064 words per tick, and the part needs ~3 µs between
  consecutive words. A single write per tick was spaced by the tick period
  itself; two are not, so `wait_word_settle` in `src/rig.rs` busy-waits on the
  always-on microsecond timer between them. It costs ~6 µs of the tick budget
  (see the 2026-08-12 bring-up below). Do not remove it, and do not replace it
  with an `embassy-time` delay: the tick path is Embassy-free and
  SRAM-resident.
- **ADC channel map, and a rename.** AD7609 channels 0 and 1 are named
  sources; 2-7 are spare and keep generic `adc2`-`adc7` names. Both named
  channels use the AD7609's true-bipolar differential inputs.
  - `coil` (channel 0) is a sense coil wound around the stator. Before
    2026-08-12 this channel was the actuator controller's current-sense
    output, so captures from before that date hold current, not coil voltage,
    and the two are not comparable.
  - `drive` (channel 1) is the exciter current controller's differential
    input. Declared in firmware on 2026-08-12; wiring due 2026-08-13. Until
    that is done the input is open and the source reads noise, so check it is
    actually connected before believing it.
  The wire-visible names changed with this: `adc0` became `coil` and `adc1`
  became `drive`, and `rig-profile.toml` follows. Sources are discovered by
  name, so host code and saved captures predating this will not match on the
  old names.
- **`drive` is uncalibrated.** It taps the exciter controller's input rather
  than the DAC pins, so any cape buffer gain or attenuation is inside the
  reading and `drive` is **not** a priori `2 * out`. Measure the ratio once
  against a scope and record it here before using `drive` quantitatively; until
  then it establishes only that the output path moves, not by how much. Note
  also that this makes `drive` sensitive to cape changes in a way a direct DAC
  tap would not be, so recheck it after any analogue board swap.
- **The stator stage has moved, but is not calibrated or homed.** The axis was
  wired and first driven on 2026-08-14, two jogs of one revolution, and the
  firmware emitted exactly the steps it should. The direction convention and
  the opto polarity are now measured, and since the mechanical rework of
  2026-08-17 the stage follows a retraction with a 19-microstep reversal dead
  band and a datum edge repeatable to under one microstep. Everything else
  about it is still an assumption: the fitted motor is unidentified, so 200
  full steps per revolution and direct coupling to the barrel are guesses; the
  datum geometry is unconfirmed, and the two constants describing it
  contradict the measured opto polarity; and the axis has never been homed. `docs/stator-stage.md` holds the commissioning
  procedure, and none of it beyond first motion has been done.
- **Microstepping is not strapped**, so `STATOR_MICROSTEPS` stays at 1.0 and
  the driver is in full step, 3.175 µm per step. Raising that constant without
  strapping MS1/MS2 makes every move eight times too long, into the end stop,
  and the soft travel limits do not protect against it because they are
  converted to steps with the same constant. The current-limit potentiometer is
  the real backstop and should be set low.
- **Do not home until the datum geometry is measured.** A homing search is
  bounded by `STATOR_SEEK_MAX_MM`, currently 6.5 mm, so it can travel far
  outside any envelope that has been shown to be safe by hand. It also needs
  `STATOR_DATUM_AT_ADVANCED_EXTREME` and `STATOR_DATUM_CLEARANCE_MM` to be
  right, and both are still guesses.
- **Jog order matters while the axis is unsettled.** A move approaches its
  target from one backlash allowance below it, so a retracting jog travels
  further than it asks: from rest, `jog -0.635` swings to -0.886 mm, which is
  1.4 revolutions, while `jog +0.635` is a pure advance of exactly one. When a
  bound on travel has been established by hand, advance into it first.
- **Open: confirm the parameter-write tick cost on the pre-stator firmware.**
  Applying any parameter write costs about 18 µs on the tick that applies it,
  taking `loop_time_max` from 45 µs to 63-64 µs and over the profile's 60 µs
  guard (2026-08-14 entry below). The evidence that this is pre-existing is
  circumstantial: it appears equally for a `rig_laser_range` write, a
  program-domain `table_mode` write, and a stator command, and the stator's own
  `set_param` adds nothing on top. That is strong but not proof, because adding
  the stator changed code layout and could in principle have moved something
  hot. The definitive test is to flash `e962260`, the last pre-stator image,
  `diag-reset`, write one parameter, and read `loop_time_max`. Do that before
  raising it against the platform.
- **Laser measuring rate.** The optoNCDT is configured at startup from
  `SAMPLE_RATE`, and expects the factory 921.6 kBaud setting.
- **Flashing.** Flash with `cargo run --release`, which uses `probe-rs run`.
  `probe-rs download` on its own has been observed to leave the target halted,
  which presents as a board that has vanished from the network; recover with
  `cargo run --release`.
- **Sequential host access.** The control server is single-client. A host
  process killed mid-request leaves the connection held until it times out,
  and the next connection attempt will fail meanwhile.
- **A switched-off laser can take the rig off the network.** With the optoNCDT
  powered down, the platform driver probes baud rates about four times a second
  without bound. Observed on 2026-08-12: `records_dropped` climbed at roughly
  6000/s with streaming *off*, meaning core 0 was not draining the record ring
  and the network stack was not being serviced, so the rig went absent from the
  host (`No route to host`, or connections that time out) while defmt showed
  core 1 ticking normally at 8 kHz. A reset clears it. The probing alone is not
  sufficient, though: later the same afternoon, with the sensor still off, the
  probing continued while drops stayed at zero and the full regression passed.
  Switching the sensor off is a legitimate thing to do, so treat this as a
  platform defect rather than a rule about how to use the rig; the evidence is
  in the HELIC-DAQ repository's `notes.md`.
- **One unreachable episode is still unexplained.** An earlier episode the same
  day had the laser streaming normally and the record ring being drained, and
  the rig was invisible from the host anyway. If it recurs, attach the probe
  and inspect before resetting: a reset destroys the evidence.
- **Interrupted captures do not leave the rig streaming.** An earlier revision
  of this file said they did. They do not: killing a client sends a FIN and
  the control server stops the stream in the same defmt millisecond. Since
  platform `v0.1.3` the streamer also refuses to send at all without an open
  control connection.

## Bring-up evidence

### Repository split and firmware rename (2026-08-12)

This rig moved into its own repository, pinned to platform tag `v0.1.2`, and
its advertised experiment name changed. The rename is wire-visible: discovery
and the `experiment` parameter now report `magnetoelastic`, and the rig profile
selects it as `--rig magnetoelastic`. Nothing else about the firmware changed,
and this bring-up exists to show that.

Exact clean W5500 firmware `0.1.0 86a5262` reported protocol 3, 42 parameters,
15 sources at 8 kHz, and safety disarmed at boot (`arm = 0`).

- `helic-rt-regression --profile rig-profile.toml`, flashing the default build:
  idle, TCP-poll and capture phases at 7999.8–8000.2 ticks/s with zero
  overruns, tick timeouts, dropped records, lost packets, capture drops or
  index gaps. `loop_time_max` 38 µs against the profile's 60 µs limit;
  `wake_phase` 36 µs, `t_measure_max` 19 µs, `t_actuate_max` 4 µs,
  `t_rest_max` 16 µs.
- `--no-flash --capture-sources all --capture-samples 8000` on the same image:
  8000 records of all 15 sources (`adc0`–`adc7`, `laser`, `target`, `forcing`,
  `table`, `phase`, `out`, `cmd_epoch`) with the same zero counters,
  7999.5–8000.3 ticks/s and `loop_time_max` 38 µs.
- Static gates on the same tree: `cargo fmt`, `cargo clippy -D warnings`,
  `helic-deps-check`, and `helic-rt-layout` against `rig-profile.toml` all
  passed; the W6100 variant cross-built but was not flashed.

The specimen was not driven: the safety gate stayed disarmed throughout, so
this is evidence about the acquisition, timing and communications paths, not
about closed-loop behaviour.

### Platform upgrade to v0.1.3 (2026-08-12)

Repinned from `v0.1.2`. A patch release, so no rig code changed; the control
link now uses a 2 s TCP keep-alive with a 10 s timeout instead of a 30 s
timeout with no probes.

Exact clean W5500 firmware `0.1.0 a594cc5`, built against the tag:

- an idle control connection that this rig reset after exactly 30.0 s before
  the upgrade was still open after 90 s, so an arm-and-hold session no longer
  needs to poll to stay connected;
- `helic-rt-regression --profile rig-profile.toml --no-flash` passed every
  phase at 7999.9–8000.3 ticks/s with zero overruns, tick timeouts, dropped
  records, lost packets, capture drops or index gaps, and `loop_time_max`
  37 µs against the unchanged 60 µs limit;
- `cargo fmt`, release clippy, `helic-deps-check` and `helic-rt-layout` all
  passed against the repinned tree, and the W6100 variant cross-built.

The comms-loss window that disarms the output and stops the stream after a
host vanishes without closing its connection is now about 10 s rather than 30.
That path was not exercised directly: making this host stop answering without
closing the connection needs privileges not available here, so the bound rests
on the keep-alive mechanism, whose live half is what the 90 s test above
demonstrates.

### Symmetric A/C differential drive (2026-08-12)

Channel C stopped being a fixed reference and is now driven to `MID_RAIL - out`
against A's `MID_RAIL + out`. See the standing constraints above for what that
buys and what it costs.

Exact clean W5500 firmware `0.1.0 993b309`:

- `helic-rt-regression --profile rig-profile.toml --no-flash` passed every
  phase at 7999.5–8000.2 ticks/s with zero overruns, tick timeouts, dropped
  records, lost packets, capture drops or index gaps;
- `loop_time_max` 44 µs against the unchanged 60 µs limit, up from 37 µs before
  this change. The cost is where it should be: `t_actuate_max` went from 4 µs
  to 10 µs, consistent with one extra 2 µs AD5064 word plus the 3 µs
  `wait_word_settle`. `t_measure_max` (19 µs), `wake_phase` (36 µs) and
  `t_rest_max` (16 µs) are unchanged. 16 µs of headroom remains, so the two-word
  write is affordable at 8 kHz, but a third channel would not obviously be;
- `helic-rt-layout` passed, and `nm` shows no `wait_word_settle` symbol at all,
  so it inlined into the already-SRAM-resident `actuate_rig` rather than
  leaving a call into flash on the tick path. Worth re-checking by the same
  means after any future change to that function;
- `cargo fmt`, release clippy, `helic-deps-check` all passed, and the W6100
  variant cross-built.

**The differential swing itself is not verified.** The optoNCDT was switched
off for this session, so the blind-feedback guard latched at boot (`safety` 10,
`tripped` 1) and the gate held the actuator at `safe_output` throughout: `out`
was identically 0.0 across an 8000-sample capture, so no non-zero command ever
reached `actuate`. Confirming that A and C move oppositely and equally needs
the laser powered so the gate can arm, and then either a scope on the AD5064 A
and C outputs or the `drive` loopback described below. Until then, treat the
doubled range as designed and timing-verified but electrically unconfirmed.

A 1 s disarmed capture did show channel 0 sitting at a ±0.5 mV noise floor
about zero, which is what an unenergised coil should look like and is weak
evidence that the new input is wired and sane; it says nothing about its
calibration.

The laser was switched off partway through this session, so
`laser_frames_received` stayed at 0 and `safety` read 10: the gate had latched
a trip and was quieting the actuator, which is the designed response to a blind
feedback path. The figures above are therefore evidence about the acquisition,
timing and communications paths only, and the laser path is neither confirmed
nor called into question by them.

### `coil`/`drive` rename, `drive` wiring pending (2026-08-12)

ADC channels 0 and 1 became named sources, and channel 1 was added as `drive`,
a loopback from the exciter current controller's differential input. This
restores software observability of the output path that moving channel 0 to the
coil had removed. See the standing constraints above for what `drive` does and
does not mean.

**The wiring is not in yet**, so nothing here is evidence about the physical
loopback: channel 1 was an open differential input throughout. What this
records is that the firmware and profile changes are sound and the rename is
live on the wire.

Exact clean W5500 firmware `0.1.0 e962260`:

- `helic-daq sources` reports 15 sources with ids 0 and 1 as `coil` and
  `drive`, so the rename is wire-visible and discovery-clean;
- `helic-rt-regression --profile rig-profile.toml --no-flash` passed every
  phase at 7999.6–8000.3 ticks/s with zero overruns, tick timeouts, dropped
  records, lost packets, capture drops or index gaps, now capturing three
  sources (`coil`, `drive`, `out`) rather than two. `loop_time_max` stayed at
  44 µs and the phase breakdown was unchanged, so the extra capture source
  cost nothing measurable;
- `cargo fmt`, release clippy, `helic-deps-check`, `helic-rt-layout` passed,
  and the W6100 variant cross-built.

A 1 s disarmed capture as a pre-wiring baseline, worth keeping to compare
against once the loopback is connected: `coil` spanned -0.61 to +0.23 mV
(std 0.12 mV), `drive` 0.00 to +0.61 mV (std 0.08 mV), and `out` was
identically zero. Both ADC traces are at the converter's quantisation floor,
which for `drive` is what an open differential input should look like and is
therefore not evidence that anything is connected.

Two things to do when the `drive` wiring goes in:

1. Check the exciter input's levels before connecting. The pair should sit
   within the AD7609's ±10 V differential range at DAC levels, but if the cape
   has voltage gain ahead of the exciter it might not, and that is worth
   confirming rather than assuming.
2. Calibrate `drive` against `out` once, with a scope, and record the ratio in
   the standing constraints above. Until that is done `drive` shows only that
   the output moves. Doing this with the laser powered would also close the
   outstanding symmetric-drive question in the entry above, since a correct
   A/C pair gives `drive` twice the amplitude a stuck C would.

### Stator stage firmware, nothing wired (2026-08-12)

A stepper-driven micrometer that sets the stator gap. The design, the wiring
recommendations, and the commissioning procedure are in
`docs/stator-stage.md`; this entry records only what was established, which is
software and nothing else.

**No hardware exists yet.** The stepper, the MP6500 carrier, and the opto
sensor have not been connected, so nothing below is evidence about motion,
position, backlash, or noise. The rig was not flashed for this work.

What the static gates establish:

- `cargo fmt`, release clippy with `-D warnings`, `helic-deps-check`, and the
  W6100 cross-build all pass. No new crate dependency was needed: the PIO
  assembler is reached through embassy-rp's re-export, and `round` is
  implemented locally rather than pulling in `libm`, so
  `dependency-policy.toml` is unchanged;
- `helic-rt-layout` passes against `rig-profile.toml`, whose `capture_sources`
  gained `stator`;
- `set_rig_param` remains SRAM-resident at 0x2000093c and, by `llvm-objdump`,
  contains **no `bl` or `blx` at all**. `issue_command` therefore inlined
  completely rather than leaving a call into flash, the same outcome recorded
  for `wait_word_settle` and worth re-checking by the same means after any
  change to `set_param`. `measure_rig` likewise gained no branch to a flash
  address from the added atomic load.

Design decisions worth not relitigating:

- The axis is on core 0, with PIO0 generating the pulses. Core 1 was rejected
  because the stepper needs ramping, homing, timeouts, and aborts, all of which
  want `embassy-time`, and because the determinism argument for core 1 is
  answered by the PIO instead. The safety property that makes core 0 acceptable
  is that a starved TX FIFO **stretches one step interval** rather than losing
  or duplicating a pulse, so the step count stays exactly the number of words
  pushed even if core 0 stalls.
- The stage is not spring preloaded, so the micrometer can push but not pull.
  Every move therefore ends with an advance, and a retract leaves the position
  undefined until the following advance re-establishes contact. This is why
  `stator` reads NaN after an aborted retract rather than reporting a step
  count that no longer describes the mechanism.
- The opto datum is good to about 1/1000 inch, which is 8 full steps or 64
  microsteps, from previous testing. That is the dominant error term: absolute
  accuracy is 25 µm while relative resolution is 0.4 µm, so home once per
  session rather than between measurements, and read `stator_home_error` as an
  audit with a 64-microstep noise floor rather than a measurement.

Two facts settled the same day, after the first draft of the firmware:

1. **The stage moves orthogonally to the beam and to the laser axis.** Stator
   travel therefore does not shift the specimen's rest displacement, and
   `DISPLACEMENT_MIN_MM`/`DISPLACEMENT_MAX_MM` need no revision on account of
   the stage. An earlier revision of this entry recorded the opposite worry, and
   it does not apply. The interlock that refuses to move while the gate is armed
   stays, for the different reason that the gap is a parameter of the system:
   moving it under a running controller changes the dynamics beneath that
   controller, and injects the driver's switching noise into a live measurement.
2. **The datum is at one extreme of travel**, with a hard stop just past the
   edge. This forced a rewrite of homing before any of it ran. The datum is now
   always latched during an advance *and* the intrusion into the clearance is
   bounded. Which of those two is awkward depends on
   `STATOR_DATUM_AT_ADVANCED_EXTREME`: with the datum at the retracted extreme
   the entire final approach happens inside the clearance, so homing refuses to
   run unless `STATOR_DATUM_CLEARANCE_MM` exceeds `STATOR_HOME_BACKOFF_MM`
   rather than homing by driving into the stop. A blind home also always leaves
   the clearance first, so its first motion is away from the stop whichever side
   the stage powered up on. Both approaches now step one at a time, waiting for
   each pulse to execute: at the FIFO's eight queued steps the sensor would be
   read 25 µm ahead of the mechanism, which is the entire datum repeatability
   budget. The soft travel window became one-sided to match, since the clearance
   is run-out for homing and not travel to operate in.

Still to establish before the first home:

- **Which extreme, and how much clearance.** Both are guesses in
  `src/config.rs`, currently the advanced extreme and 0.5 mm. Measure the
  clearance first: it decides whether homing is safe at all in one of the two
  geometries, and the firmware can only refuse on the figure it is given.
- **Whether the opto flag rides on the stage or on the spindle.** If it rides on
  the spindle the datum is repeatable regardless of approach direction, because
  a screw is deterministic even when the unpreloaded stage it pushes is not, and
  the advance-only rule would be needed for positioning but not for homing. The
  firmware assumes the stricter case, which costs a little homing time and is
  correct either way. The 1/1000 inch figure is consistent with both and does
  not settle it.
- **Whether stepping during a capture is ever wanted.** The firmware does not
  forbid it, because the noise characterisation needs it, but the `stator`
  source makes a violation visible after the fact.

### Stator stage, first motion (2026-08-14)

The stepper, its MP6500 carrier, the opto sensor, and the ENABLE line on GP28
are wired. Microstepping is **not** strapped, so the driver is in full step and
`STATOR_MICROSTEPS` stays at 1.0, which is what the firmware already assumed.

Exact clean W5500 firmware `0.1.0 6f5da2f`, flashed with `cargo run --release`.
It came up reporting 16 sources and 57 parameters, logged `stator: axis ready,
not homed`, and the axis read `stator_state` 3 (not homed), `stator_steps` 0,
`stator_position_mm` NaN, `stator_faults` 0, with the gate disarmed.

The stage was in a position where one revolution either way was safe by hand,
so the test was two jogs of one revolution, 0.635 mm, advancing first:

- `rig_stator_jog 0.635`: `stator_steps` 0 to **exactly 200**, `stator_moves` 1,
  `stator_faults` 0. A pure advance, because the target sat above the unsettled
  start point;
- `rig_stator_jog -0.635`: back to **exactly 0**, `stator_moves` 2,
  `stator_faults` 0. A poll during the move caught `stator_state` 1 (moving) at
  step 24, mid-retract on the way to -79, which is the backlash undershoot
  before the final advance;
- 176 steps in the ~1.12 s to that poll is 157 steps/s, matching the configured
  0.5 mm/s exactly.

Excursion envelope was -0.2508 mm to +0.635 mm, that is -0.395 to +1.000
revolutions, inside the sanctioned one revolution either way. Advancing first is
what kept it there; see the standing constraint above on jog order.

**What this does and does not establish.** It establishes that the firmware
emits exactly the steps commanded, that direction changes and the
retract-then-advance approach sequence execute, that the rate is right, and that
the command and telemetry surface works end to end. It establishes **nothing**
about the mechanism: the step count is what the PIO was asked to emit, not
independent confirmation that the motor turned, that the coupling held, or which
physical direction "advance" is. The axis was deliberately not homed.

Real-time behaviour was untouched by the stepping, which is the architectural
claim the axis rests on:

- `loop_time_max` 45 µs at rest, against 44 µs recorded before the stage
  existed. The extra atomic load in `measure` costs about 1 µs, as expected;
- zero overruns, zero tick timeouts, and jitter of at most 1 µs across the whole
  run, including while stepping.

**An incidental finding, not caused by this work.** Any parameter write pushes
`loop_time_max` from 45 µs to 63-64 µs, which is over the profile's 60 µs guard.
It is not stator-specific and not new: after a `diag-reset` the same 63-64 µs
appears from a plain `rig_laser_range` write and from a program-domain
`table_mode` write, as well as from a stator command, and the stator's own
`set_param` adds nothing measurable on top. So applying a parameter write costs
about 18 µs on the tick that applies it. No overrun or tick timeout resulted,
and against the 125 µs period at 8 kHz there is ample real margin; the 60 µs
figure is a self-imposed guard rather than a deadline. It is probably invisible
to `helic-rt-regression` because that resets diagnostics after its own quieting
writes. Worth raising against the platform rather than working around here.

Next, in order: confirm by hand that the motor actually turned and in which
direction, so `STATOR_DIR_ADVANCE_HIGH` and `STATOR_ADVANCE_INCREASES_READING`
stop being guesses; check the barrel returns to its starting reading, which is
the first evidence about lost steps and coupling slip; then measure the datum
geometry before any homing, and only then the steps-per-millimetre check.

### Stator ENABLE polarity, and a second attempt (2026-08-14)

The 2026-08-14 first-motion test above emitted its steps correctly but **the
motor did not turn at all**. Bench meter readings at rest diagnosed it, and are
worth keeping because between them they exonerate everything except the one
constant that was wrong:

| Pin | Signal | Reading at rest | Reading |
|---|---|---|---|
| GP28 | ENABLE | low | the driver is **enabled** by a low level |
| GP26 | DIR | high | the last motion was the closing advance, as designed |
| GP22 | STEP | low | correct park state between pulses |
| GP27 | opto | low | against an internal pull-up, so the sensor is connected and actively driving |

The enable is **active low**, and the firmware had assumed active high. It was
therefore driving the pin high for the whole of every move, disabling the
driver, and low at rest, holding the motor energised continuously. Exactly
backwards in both states. `STATOR_ENABLE_ACTIVE_HIGH` is now `false`.

Two things follow. The motor sat energised from the first flash until the fix,
about twenty minutes, which is the state the ENABLE line exists to avoid. And
the external resistor wanted on that line is a **pull-up** to the 3.3 V on
connector pin 2, not the pull-down recommended earlier: the enable input has an
internal pull-down, so a microcontroller in reset or unpowered otherwise floats
the driver enabled.

**The step pulses were real, and this is worth recording because it is not
obvious.** The firmware's step counter only proves what the PIO was asked to
emit. But `emit` waits on FIFO space, so had the state machine not been
executing, the FIFO would have filled and the move would have stalled and taken
far longer than commanded. Instead both moves ran at 157 steps/s, matching the
configured 0.5 mm/s to the sample. The state machine was therefore consuming
words at exactly the programmed rate, which it can only do by executing the
delay loop, so STEP was toggling on GP22 throughout. The fault was downstream of
the pin.

Exact clean W5500 firmware `0.1.0 a37f46d`, the same two jogs repeated:
`stator_steps` 0 to exactly 200 and back to exactly 0, `stator_moves` 2,
`stator_faults` 0, a mid-move poll at step 17 retracting toward -79, zero
overruns and zero tick timeouts. Identical to the previous attempt, as expected:
the firmware could not tell the difference, which is the point.

**Still unconfirmed by observation**: whether the motor turned this time, in
which direction, and whether the barrel returned to its starting reading. The
firmware evidence cannot settle any of the three.

### Stator motion confirmed, and a watchable repeat (2026-08-14)

With `STATOR_ENABLE_ACTIVE_HIGH` corrected to `false`, **the motor turns**.
GP28 reads high at rest, confirming the driver is now released between moves
rather than held energised. The first confirmed motion was too quick to observe
by eye, so `rig_stator_dwell` was added: a diagnostic hold at each direction
reversal, zero in normal use.

Exact clean W5500 firmware `0.1.0 7017c41`, with `rig_stator_dwell` 5 s and
`rig_stator_rate` 0.15 mm/s, repeating the same two jogs. Polled `stator_steps`
tracks the whole sequence:

| Phase | Steps | Observed |
|---|---|---|
| dwell | held at 0 | no motion for the first 5 s |
| advance | 0 to 200 | 200 steps in 4.2 s |
| dwell | held at 200 | 5 s |
| retract | 200 to -79 | 279 steps in 5.9 s |
| dwell | held at -79 | 5 s, seen at two consecutive polls |
| advance | -79 to 0 | 79 steps in 1.7 s |

Both moves ran at 47.5 steps/s against the 47.24 steps/s that 0.15 mm/s implies,
so the rate parameter is honoured at this setting as well as at 0.5 mm/s. The
retract-then-advance approach is now directly observable: the axis really does
overshoot its target by the backlash allowance and come back onto it advancing.

The excursion envelope was unchanged, -0.395 to +1.000 revolutions, and
`stator_steps` returned to exactly 0 with `stator_faults` 0.

The rig was left with `rig_stator_dwell` 5 and `rig_stator_rate` 0.15 so the
demonstration can be repeated immediately. Neither survives a reflash, and the
compile-time defaults remain 0 and 0.5.

**Still unconfirmed**: which physical direction the advance corresponds to, and
whether the barrel returns to its starting reading. Both need an eye on the
mechanism, not the telemetry.

## Enable-to-first-step delay raised to 100 ms (2026-08-14)

`STATOR_WAKE_MS` was 5 ms, which is generous against the MP6500's own wake but
misses the constraint that actually matters. The driver's electrical restore is
fast: the indexer keeps its phase while the outputs are disabled, and coil
current comes back in well under a millisecond. The rotor is the slow part.
De-energised, it is held only by detent and friction, so it can sit off the
phase the indexer still holds; re-energising snaps it back, and that snap is a
lightly damped ring against the rotor's inertia lasting tens of milliseconds.
A 5 ms wait starts stepping inside the ring, which is exactly the condition for
losing steps at the start of a move.

Now 100 ms. It is paid at most once per move, and only on a transition from
de-energised, against moves that already take seconds.

Two things this does not fix, both worth watching:

- The snap-back is uncounted. The step counter assumes the rotor does not move
  while the driver is off. Returning to the same phase keeps the count true, and
  it should, but a detent strong enough to pull the rotor a full step over would
  leave the count silently wrong by one full step, 3.175 um. That is well under
  the datum's 25.4 um repeatability, so it would only ever show up cumulatively,
  in `stator_home_error` over many de-energise cycles.
- Nothing ramps. The first step is issued at the full traverse rate from
  standstill. At 0.5 mm/s that is 157 steps/s, comfortably inside the pull-in
  rate of a small stepper, so it is sound on its own; it is only the combination
  with a still-ringing rotor that ate the margin.

Not yet flashed: the rig is still running `7017c41` with the demonstration
settings, and a reflash would clear them.

## Wake fix confirmed, advance direction confirmed, opto scan inconclusive (2026-08-14)

Three results, in descending order of confidence.

**The 100 ms wake fixed a real fault.** Before it, moves of fewer than about
four steps were variably successful; after it they are reliable, with no
evidence of missed steps. This is the symptom the mechanical argument predicted:
a short move is all start transient, so a rotor still ringing from
re-energisation swallows it entirely, while a long move loses only its first few
steps and hides them in the noise. The 5 ms wake was the defect.

**`STATOR_ADVANCE_INCREASES_READING = true` is confirmed.** A `+0.635` jog
increases the micrometer reading, measured by eye. The constant was a guess and
is now a measurement.

**The opto scan did not find the edge.** Firmware `d600966` plus the new opto
telemetry was flashed, the stage was manually positioned so the sensor was
triggering, and the axis advanced 200 steps in ten 20-step jogs at 0.15 mm/s.
`stator_opto` read 1 throughout and `stator_opto_edge` stayed NaN, so no
transition occurred within 0.635 mm of advance. Zero faults.

Two readings of that, not yet separated:

- The edge lies further along than one revolution in the increasing direction.
- The triggered half-plane extends in the increasing direction, so the edge lies
  *below* the manual start position and advancing will never reach it. The opto
  is an edge, not a vane, so this is a live possibility rather than a pedantic
  one, and it is the dangerous one: continuing to advance would then be driving
  toward whichever extreme carries the hard stop.

Scanning stopped at +200 steps pending a decision, because the direction of the
hard stop relative to increasing reading is still unknown. Nothing about the
datum geometry, `STATOR_DATUM_AT_ADVANCED_EXTREME` or
`STATOR_DATUM_CLEARANCE_MM`, is settled by this run.

Also now measured: `stator_opto` reads 1 with the flag in the sensor, so
triggered is high. The earlier multimeter reading of GP27 low was the untriggered
state, not a wiring fault.

## Opto edge located at 2794 steps, and it did not come back (2026-08-14)

Continuing the advancing scan from +1000 steps, sanctioned to a further ten
revolutions. The opto released during the ninth, with `stator_opto` going 1 to 0
and `stator_opto_edge` latching **2794 steps** from the manual start position.
Zero faults over 27 moves.

That settles the polarity question the other way from my reading of it: high is
the state at lower readings and low at higher, so the earlier multimeter low at
the original position simply means that position was above the edge. The manual
reposition had put the stage well below it. Advancing was the right direction
after all; it was just 14 revolutions away rather than the one or two expected.

In distance, 2794 steps is 8.871 mm, 0.3492 inch, 13.97 barrel revolutions. The
latch fires on the counted step, which leads the mechanism by up to the eight
word FIFO, so the true crossing lies in [2786, 2794] steps, a 25 um window.

**The refinement then failed, informatively.** To pin the edge without the FIFO
lead, the axis backed off 30 steps below the latch and crept up in single
settle-stepped jogs. At 2770 steps the opto still read **0**, and the first
1-step advance kept it at 0. Backing off 24 steps below the crossing did not
restore the untriggered state.

Two candidate explanations, not yet separated, and the distinction matters more
than the edge value does:

- **The stage did not follow the retract.** With no preload the spindle can back
  away and leave the stage behind, so the flag never moved below the edge, and
  the backlash take-up then simply re-contacted it where it already was. This is
  the central mechanical assumption of the whole axis design, and this would be
  the first direct evidence against the retract-then-advance scheme doing
  anything at all.
- **Sensor hysteresis.** An opto interrupter with a Schmitt output has some, but
  95 um of it would be a great deal.

The test that separates them: retract several hundred steps, well past any
plausible hysteresis, and advance back. If the opto returns to 1 and releases
again near 2794 the sensor has modest hysteresis and the stage does follow. If
it never returns to 1, the stage is not following retraction at all.

Until that is answered, no position derived from a retract should be trusted,
and the `move_to` backlash compensation is unproven.

## Reversal dead band measured at 0.697 mm, and the datum edge is sharp (2026-08-14)

The retract test answered both questions, and the answer to the second was not
the one the failed refinement suggested.

**The stage does follow a retract.** Retracting 500 microsteps from 2771 brought
the opto back to 1, latched at **2575 microsteps**. So the spindle is not simply
walking away from a stationary stage, and the retract-then-advance scheme is
mechanically sound in principle.

**The dead band is large.** Advancing back released the opto again at **2795**,
against 2794 on the first crossing.

| Crossing | Latched microsteps |
|---|---|
| Advancing, first pass | 2794 |
| Retracting | 2575 |
| Advancing, second pass | 2795 |

The reversal dead band is 2794.5 - 2575 = **219.5 microsteps, 0.697 mm**, about
1.1 barrel revolutions. This is why the earlier refinement failed: backing off 30
microsteps was entirely inside it, so the flag never moved and the opto never
returned. The observation was right and the hysteresis reading of it was wrong.

The figure lumps mechanical lash with any sensor hysteresis. Separating them
needs an independent displacement measure against the barrel, and for setting
the overshoot the lumped figure is the conservative one to use anyway.

**The datum edge itself is excellent.** Two advancing crossings, one microstep
apart, 3.175 um. That is eight times better than the 25.4 um the design budgeted
from the earlier manual testing, and it is the quantity the datum's repeatability
actually rests on. Worth confirming over more than two crossings before relying
on it, but the sign is good.

### Consequences, both now in config.rs

- `STATOR_BACKLASH_MM` 0.25 to **1.0**. The old value was less than half the dead
  band, so every retracting move would have stopped short of contact and reported
  a position wrong by up to the shortfall. Also set at runtime immediately, since
  `rig_stator_backlash` does not need a reflash.
- `STATOR_HOME_BACKOFF_MM` 0.2 to **1.5**. The old value sat inside the dead band,
  so homing's back-off-and-reapproach would not have moved the flag and the final
  approach would have re-crossed nothing. Homing as previously written could not
  have worked, independently of the datum geometry being unknown.

That second change interacts with `STATOR_DATUM_CLEARANCE_MM`, still a guessed
0.5 mm. If the datum turns out to be at the retracted extreme, `home` refuses to
run when the backoff does not fit inside the clearance, which it now does not.
That refusal is correct rather than a regression: it says the clearance must be
measured before homing can be trusted, which was already true.

Still open: whether the opto flag rides on the stage or on the spindle. It
matters more now. If the flag is on the spindle, 0.697 mm is the lash upstream
of it only, and the spindle-to-stage contact adds more on top.

## The retract does not move the stage: backlash compensation is inert (2026-08-14)

**Superseded on 2026-08-17 by the mechanical rework**, after which the stage
does follow a retraction and the dead band is 19 microsteps. The section is
kept because it is what the axis did before the rework, and because the
harness bug it identifies is a real one. See the last entry in this file.

The 0.697 mm dead band recorded above is wrong, and so was the reasoning that
raised the constants. Retracting it.

Two problems with that measurement. First, the wait-for-idle loop in the test
harness compared `stator_state` against 1 and broke out on anything else, but
the idle state here is 3, not homed, so it returned immediately and the next
command was issued into a still-running move, aborting it. The single-step
refinement was therefore issuing aborts, not steps. Second, and because of that,
the 579-step retract that produced the 2575 crossing started from a position
left by those aborted moves, so its initial state was unknown.

Repeated cleanly, from a position reached by a long monotonic advance, with the
completed-move counter as the wait signal:

| Retraction from 2850 | Opto returned to 1? |
|---|---|
| 62 microsteps, 0.2 mm | no |
| 125 microsteps, 0.4 mm | no |
| 251 microsteps, 0.8 mm | no |
| 503 microsteps, 1.6 mm | no |
| 797 microsteps, 2.53 mm | yes, crossing at 2053 |

The datum edge sat 54 microsteps, 0.17 mm, below the start. So while the spindle
withdrew 2.53 mm the stage crept back 0.17 mm, and four retractions of up to
1.6 mm moved it past nothing at all.

**The stage does not follow the spindle on a retraction.** With no preload
nothing pulls it, so retracting opens a gap and leaves the stage where it was.
The little movement seen at 2.53 mm looks like creep, which would also explain
why the contaminated earlier run gave a different figure: how far it creeps
depends on time and disturbance, not on how far the spindle went.

Consequences:

- The backlash compensation is **inert**, not undersized. No setting fixes it,
  because the problem is that retraction transmits no motion. `STATOR_BACKLASH_MM`
  is back to 0.25 and `STATOR_HOME_BACKOFF_MM` to 0.2, since a larger value costs
  travel and buys nothing. David's judgement that the raised values were overly
  large was right, though not for the reason either of us had.
- **A retracting move does not reposition the stage, and the position reported
  after one is not to be believed.** Only advancing moves position the axis.
- **Homing must not be run.** Its approach depends on backing off past the edge
  and coming back, and the backoff moves nothing, so it would latch a datum from
  a stationary stage.
- The fix is mechanical: preload the stage against the spindle. Then the
  compensation becomes meaningful and all of this needs re-measuring.

What survives from the earlier work is the advancing behaviour, which is
excellent. Three separate advancing crossings of the datum edge, separated by
retractions of hundreds of microsteps and hundreds of moves, landed at 2794,
2795, and 2796 microsteps: about one microstep, 3.2 um, per cycle. Whether that
one-per-cycle progression is a slow real drift or a single lost step is not yet
separable, but either way it is eight times inside the 25.4 um the design
budgeted.

## The 18 us parameter-write cost is a fixed-size command copy (2026-08-14)

Every parameter write, of any kind, adds about 19 us to the tick that applies
it. This closes the open item recorded earlier: it is not the stator, it is not
this rig, and it is not the handler for the parameter written. It is the
platform copying a fixed 140-byte `RtCommand` out of the cross-core queue,
whatever the command actually carries.

The delay is by now well constrained. All measurements at 8 kHz with the laser,
exciter, and stepper powered down, so only the analogue path and the network
were live.

| Condition | `loop_time_max` | `t_rest_max` |
|---|---|---|
| Quiet, no traffic | 44 us | 15 us |
| 20 reads over TCP, no command | 45 us | 16 us |
| Writes, rig domain (`rig_stator_dwell`) | 64 us | 35 us |
| Writes, program domain (`table_gain`, `table_interp`) | 63 us | 35 us |
| Writes, 33-float block (`target_coeffs`, `forcing_coeffs`) | 64 us | 35 us |

Four things fall out of that table. Reads cost nothing, so it is not core-0
network activity: a GET and a SET differ on core 0 only by the enqueue.
`t_measure_max` and `t_actuate_max` never move, so all of it is in the rest of
the tick, not in the ADC or DAC transfers. Rig-domain and program-domain writes
cost the same, so it is common dispatch rather than either handler. And a
33-float write costs exactly what a 1-float write costs, which is the tell: the
cost does not depend on how much data the command carries.

The mechanism is visible in the disassembly. `Payload::Values` is a fixed
`[f32; 33]` inline array, so `RtCommand` is 140 bytes regardless of variant, and
the dispatch path in `run_rt_tick` copies that struct roughly three times per
command through a runtime-dispatched `__aeabi_memcpy`. `set_rig_param` and
`apply_program` themselves are a few tens of instructions and entirely SRAM
resident; there is no flash execution anywhere on the path, and the layout gate
is not being evaded.

Confirmed by scaling rather than by reading code. The platform has a
`diag-wide-command-payload` feature that widens `MAX_RT_VALUES` from 33 to 132,
taking `RtCommand` from 140 to 536 bytes and changing nothing else:

| Build | `RtCommand` | Quiet | Write | Extra |
|---|---|---|---|---|
| Default | 140 bytes | 44 us | 64 us | 20 us |
| `diag-wide-command-payload` | 536 bytes | 44 us | 118 us | 74 us |

Extra cost against struct size is a straight line through those two points at
0.136 us/byte, about 20 cycles per byte at 150 MHz, with an intercept of 0.9 us.
So under a microsecond of the write is real work, and essentially all of the
rest is the copy. The platform already knows this: the comment on
`MAX_RT_VALUES` records that "hardware timing rejected copying 132 values
through this envelope", which is the 118 us build, 7 us short of the 125 us
period.

**It predates the stator.** Flashing `dd215ec`, the last commit before the axis
existed, and repeating: quiet 43 us, any write 63 us, `t_rest` 14 us to 35 us.
Identical within resolution. What the stator did add is about 1 us to the quiet
baseline, 43 to 44 us, which is the extra atomic load publishing
`stator_position_mm` as a source, exactly as its commit message claimed.

Two practical points, neither urgent.

The realised worst case is one command per tick, not the two
`COMMANDS_PER_TICK` allows. Writes through the control protocol are
request/response, so core 0 cannot enqueue faster than about 1 kHz; pipelining
four SET frames into a single TCP write still left `cmd_backlog_max` at 1, and
sustained writing gave no overruns and no clock jitter. The bound is therefore
64 us in practice against a 125 us period, and 85 us in principle if two ever
did land together.

`max_loop_us = 60` in `rig-profile.toml` is exceeded by any parameter write.
The regression does not catch this because `measure_phase` resets diagnostics
and then measures a window containing no writes, so the limit is in effect a
quiescent limit that is never tested against a command tick. Left as it is
rather than quietly raised: what that number is asserting is a decision worth
making deliberately, since the honest WCET is 85 us.

The fix, if it is ever worth making, is upstream and structural: make the queue
element small, either by moving the inline `Values` array behind the
`ValueBuffer` mechanism the platform already has for wider vectors, or by
sizing the payload to the harmonics a given experiment actually uses. Nothing
in this repository can address it, and at 64 us against 125 us there is no
operational reason to.

## What an upstream fix would cost: the command envelope (2026-08-14)

Follow-up to the section above, prototyped and measured rather than argued.
Not a limit for this experiment, but it will be for a faster one, so the
options are worth having on record.

Each candidate was built against a local copy of the platform at v0.1.3,
flashed, and measured with the same harness. The rig's own `Cargo.toml` is
unchanged and still pins the tag; the prototypes lived in a scratch copy.

### The finding that reframes it

`Payload::Values`, the 132-byte inline array that sets the size of every
command, **has no production consumer**. Its only constructor,
`ParamStore::enqueue_max_command_burst`, and its only match arm in
`StandardProgram::apply` are both behind `#[cfg(feature =
"diag-max-command-burst")]`. Every vector parameter that a host can actually
write, `target_coeffs`, `forcing_coeffs`, and `table`, travels as
`Payload::Buffer(CommitToken)`; rig and controller parameters are scalar `f32`
and nothing else. So in a production build that array is dead weight, and it
is charged to every scalar write.

Worse, and this is the part worth remembering: **the buffered path did not
escape the copy cost, it only avoided making it worse.** A 33-float
`target_coeffs` write moves a pointer-sized token and still costs the same
64 us as writing one float, because an enum is as large as its largest
variant whether or not that variant is in use.

### Measured cost against envelope size

| Variant | `RtCommand` | Write | Extra over quiet |
|---|---|---|---|
| `Values` gated out | 16 B | 45 us | 2 us |
| `MAX_RT_VALUES = 8` | 44 B | 50 us | 6 us |
| Shipped, `MAX_RT_VALUES = 33` | 140 B | 64 us | 20 us |
| `diag-wide-command-payload`, 132 | 536 B | 118 us | 74 us |

Linear through the origin at **0.139 us per byte of `RtCommand`**, about 21
cycles per byte at 150 MHz. Four points, no curvature. That is the design law:
the tick cost of a command is set by the size of the queue element and by
nothing else about the command.

### The options

**A. Gate or delete `Values`.** One line. 20 us to 2 us, and 3968 bytes of
`.bss` returned, exactly `COMMAND_QUEUE_LEN * (140 - 16)`, for 80 bytes of
flash. All 173 platform tests pass with the variant gated, both with and
without `diag-max-command-burst`. The platform's own rule, that a type earns
its place by having two actual consumers, already argues for it: this one has
none. Against: it is breaking under their versioning rules, which make a
capacity change breaking in either direction, and it removes an extension
point a future experiment might want.

**B. Make the payload width a const generic per experiment.** The developer
guide already recommends exactly this shape, "prefer a const generic with a
default over a shared constant, so each rig pays only for what it uses".
Measured proxy at `MAX_RT_VALUES = 8`: 6 us, so it works. Against: the
parameter infects `RtCommand`, `Payload`, both queue endpoints, `RtChannels`,
`ParamStore`, `Program::apply`, and `Rig`, which is a wide refactor touching
every rig; and a default still has to be chosen, so it fixes nothing for a rig
that takes it.

**C. Route array parameters through `ValueBuffer`, then delete `Values`.** The
mechanism exists and is proven, since the coefficient parameters already use
it, and the numbers collapse to A's. Against: each array parameter needs its
own statically allocated double buffer, so SRAM is paid per parameter rather
than per queue slot, and the semantics differ. A buffered commit allows one
outstanding activation and returns `Busy` if written again before the tick
consumes it, whereas copied commands pipeline 32 deep. That difference is the
only real argument for keeping a copied path at all.

**D. Cut the number of copies rather than the size.** Dequeue in place,
dispatch by reference. The measured 0.139 us/byte is spread across roughly
three copies of the struct, so even a perfect single-copy dispatch leaves
about 7 us at 140 bytes, an order worse than A. It also breaks the `Program`
trait signature, and `Payload::Buffer` holds a linear token that must move, so
in-place dispatch needs `unsafe` or an `Option::take` dance. Dominated.

**E. Alignment or padding. Tested and rejected.** `#[repr(align(8))]` on both
`Payload` and `RtCommand` changed nothing: 64 us before, 64 to 65 us after.
The copies were never on a misaligned byte-wise path. Recording it because it
is the obvious cheap non-breaking guess, and it does not work.

Preference is A, with C as the answer for anyone who later needs a copied
array, and B only if an experiment turns up that needs one and genuinely
cannot use a buffer.

### When this stops being free

At 8 kHz there is 81 us of headroom over a 44 us quiet tick and a command
spends 20 of it, so nothing here matters yet. It matters when the period
shrinks. At 16 kHz the period is 62.5 us against the same 44 us quiet tick,
leaving 18.5 us of margin for a command that needs 20: **a single parameter
write would overrun outright.** So the present envelope forecloses 16 kHz
operation for any experiment that writes parameters while running, which is
every continuation or live-tuning experiment. It would also bite if
`COMMANDS_PER_TICK` were raised, since two commands cost 85 us today, or on a
rig with a heavier measure or actuate path than this one's 29 us.

## What `Values` was for, and why deletion beats gating (2026-08-14)

### What it was for

The platform's own history answers this, and it is a single afternoon.

| Time, 2026-08-11 | Commit | What happened |
|---|---|---|
| 13:09 | `117f3fd` | `Payload::Values` introduced at 132 values wide, replacing operation-specific commands with domain/id routing. It was the general mechanism for every vector real-time parameter. |
| 13:17 | `af90fea` | Hardware measured two 132-value commands at 74 us against CBC's 60 us gate. Narrowed to 33; force vectors moved to the owner-checked double buffer. `Values` still carried the target and forcing coefficients. |
| 13:21, 13:24 | `2d2b0ed`, `19e659c` | The maximum-command-burst diagnostic wired up to exercise `Values`. |
| 13:30 | `a3bf233` | The same measurement at the narrowed width, two 33-value commands at 73 us, moved the coefficients to buffers too. **`Values` lost its last production consumer here**, twenty-one minutes after being narrowed for it. |

So `Values` is the vestige of the original copied-payload design, kept at the
width of its final user after that user was moved off it precisely because
copying was too slow. It was never deleted, and the cost was never
reattributed. The platform concluded that the copy was too slow for wide
payloads and moved wide payloads away, without noticing that an enum charges
its largest variant to every command. Since 13:30 that afternoon the only
thing keeping it alive has been the diagnostic built to measure it.

### Gate or delete

Measured both. They are numerically identical: 45 us write, 2 us over quiet,
134584 bytes of `.bss`. So the choice is hygiene, not performance, and hygiene
says delete.

The decisive argument is what gating does to the diagnostic. Gate the variant
and `size_of::<RtCommand>()` becomes feature-dependent, so
`diag-max-command-burst` measures a command shape that no shipped firmware
has: a probe that only measures itself. Delete it, point the burst probe at
`Payload::F32`, and it measures the real production envelope for the first
time. That is a better diagnostic than the one being given up.

Three supporting reasons. A gated variant is dead in every build anyone ships,
so it rots unexercised. The two diagnostic axes would multiply. And deletion
buys a const assert tight enough to be worth having:

```rust
const _: () = assert!(core::mem::size_of::<RtCommand>() <= 4 * core::mem::size_of::<usize>());
```

Sixteen bytes on the target and thirty-two on a host test runner, expressed in
pointer widths because `CommitToken` carries its owner address. That turns the
whole class of regression from a timing failure found on hardware into a
compile error. The bound it replaces, 160 bytes, permits exactly the mistake
that produced this in the first place.

Costs of deleting, stated fairly. `diag-wide-command-payload` disappears, and
every rig forwards that feature in its own `Cargo.toml`, so each consumer
needs a one-line edit; this repository needed exactly that. `MAX_RT_VALUES`
leaves the public API. And the copied path goes entirely, so a future
requirement for one means reintroducing it rather than flipping a feature.

The `Busy` objection to relying only on buffers is weaker than it looks.
Buffers are per parameter, not per command stream: `target_coeffs`,
`forcing_coeffs`, and the table each own a separate `DoubleBuffer`, so the
one-outstanding-commit restriction serialises a single parameter against
itself and nothing else.

Verified on the deleted version: the platform's full test suite passes, 173
tests, with and without `diag-max-command-burst`; `cargo clippy --workspace
--all-targets` is clean; the rig builds, flashes, and measures 45 us.

## Platform upgraded to v0.2.1, and the control link has a fifteen-second clock (2026-08-14)

The command-envelope fix landed upstream and this rig is repinned from v0.1.3
to v0.2.1. The rig's only source change was deleting the
`diag-wide-command-payload` feature forward, since the feature no longer
exists; nothing in `src/` needed touching.

Verified on hardware after the upgrade, all six gates plus the regression:
quiet tick 44 us, a parameter write 45 us against the profile's 60 us limit,
366 writes in the new write phase, no overruns, no jitter, no dropped records,
no acceptance errors. Before the upgrade that phase read 64 us and would have
failed the gate it had never previously been tested against.

**A separate and more serious defect turned up while doing this, and it is not
fixed.** The control connection stops being answered about fifteen seconds
after it is opened, after which new connections time out rather than being
refused. Core 1 is untouched throughout, ticking at exactly 8 kHz with no
overruns, and core 0's status task, laser probe, and record drain all keep
running; only the network path dies. It reproduces on v0.1.3, so it is neither
the envelope change nor anything this rig did, and it is measured across seven
runs at 15.06 to 16.02 s independent of traffic direction, rate, and volume.
The platform's `notes.md` at v0.2.1 carries the full characterisation and the
suggested bisect.

What it means for working here, until it is fixed:

- Keep any single control session under about twelve seconds. Every
  `helic-daq` command is connect, request, disconnect, so ordinary use is
  unaffected; it is scripts holding one connection that break.
- The default `helic-rt-regression` run exceeds the window and will report
  `control link lost`. Use `--idle-seconds 2 --poll-seconds 2
  --write-seconds 3 --capture-samples 4000`, which fits inside it and is what
  the acceptance figures above were measured with.
- The stepper work in the sections above was done in short bursts and is
  unaffected, but a long automated move sequence would need reconnecting
  rather than holding one session open.

## Correction: the fifteen-second failure was blocking RTT, not the control link (2026-08-14)

The section above is wrong in its diagnosis and is retracted. There is no
control-link defect and the platform's keep-alive constants are not
implicated. **`defmt-rtt` blocks when its buffer fills, and it fills whenever
no debugger is draining it.** The status task logs once a second, so it fills
in roughly fifteen seconds, and the next `info!` blocks inside a critical
section and stops core 0: network, record drain, laser probe, all of it. Core
1 is Embassy-free and never logs, which is why it went on ticking at exactly
8 kHz with no overruns while the board was invisible.

Two things hid it. My harness killed the tmux flashing session to free the
probe before each measurement, so every failing run had no RTT reader and
every passing run had one; the fifteen seconds is a buffer fill time at a
fixed logging rate, which is why it looked like a clock and did not depend on
traffic. And attaching a probe drains the buffer and releases the block, so
the fault removes itself the moment you look at it. My own probe capture
showed a healthy core 0 for that reason, not because core 0 was ever fine.

Settled on one firmware with no reset in between:

| Condition | 20 ms reads |
|---|---|
| Probe attached | survives, ended at 90 s |
| Probe killed | dies after 455 reads, 12.83 s |
| Probe re-attached, no reset | answers immediately, `ticks` 12462784 |

Fixed at v0.2.2 by adding `features = ["disable-blocking-mode"]` to this rig's
own `defmt-rtt` declaration, so a full buffer drops frames rather than halting
the core. Every rig must make that edit itself; nothing catches its absence.

**The operating advice in the section above is withdrawn.** Sessions no longer
need to be kept under twelve seconds, and the default `helic-rt-regression`
now runs to completion: 630 writes in the write phase at 45 us against the
60 us limit, 8000 records, no lost packets, no index gaps, no acceptance
errors, with no probe attached. Long automated stepper sequences over a single
held connection are fine again.

This is almost certainly what the two earlier unexplained unreachable episodes
were, and what the stale connection at the start of this session was.

## The blocking-RTT trap is now a gate, and the hazard is narrower than stated (2026-08-14)

Two refinements to the correction above.

The firmware does not default to blocking. `defmt-rtt` initialises its control
block to non-blocking, and **probe-rs writes the blocking mode when it
attaches**; nothing rewrites it when the debugger goes away. So the hazard is
specifically a probe that attached and detached without a reset, not the mere
absence of one. Measured here on a build without the feature, same image:

| Boot | 20 ms reads |
|---|---|
| Flashed by `cargo run`, probe then killed | dies after 486 reads, 13.27 s |
| `probe-rs reset`, which never attaches RTT | survives 4214 reads, 90 s |

Every `cargo run` does it, and the regression detaches deliberately before
opening its host connection, so it covers the whole workflow here. It does
mean a rig power-cycled since its last flash was never at risk.

It is now gated rather than remembered. `helic-deps-check` fails any workspace
whose `defmt-rtt` resolves without `disable-blocking-mode`, unconditionally
and with no entry in `dependency-policy.toml`, so this rig and any future one
inherit it for free. Confirmed here both ways: removing the feature fails the
gate with the fix quoted in the message, restoring it passes.

Repinned to v0.2.3 and re-verified end to end: all six gates, and the full
default `helic-rt-regression` with no probe attached reports no acceptance
errors, 606 writes in the write phase at 45 us against the 60 us limit, 8000
records, no lost packets, no index gaps.

## The stage now follows a retraction: dead band 19 microsteps (2026-08-17)

David reworked the coupling between the stage and the stepper, to get a better
mechanical connection than the push-only contact of 2026-08-14. The rework is
his to describe; what follows is only what the axis now does. Measured on the
image already flashed, `0.1.0 9be46f2`, which is one commit behind `main` and
differs from it only by the platform repin to v0.2.3, nothing in the axis.

**The datum edge is at 184 microsteps** from the power-on counter zero, that
is 0.584 mm of advance from wherever the stage sat when it booted. Found by
advancing in 100-microstep jogs with the host polling during each move and
issuing `rig_stator_stop` on the first opto change, so the overshoot past the
edge was bounded by the poll interval rather than by the jog length. It turned
up in the second jog: the stage was already sitting just below the edge.

**The stage follows a retraction now.** This is the result that matters, and it
reverses the 2026-08-14 finding directly. With `rig_stator_backlash` set to
0.001 mm, which rounds to zero microsteps and so makes a negative jog a pure
retract, 64 microsteps of retraction (0.203 mm) brought the opto back to 1. On
2026-08-14 the same axis was retracted 503 microsteps (1.6 mm) and crossed
nothing at all. The backlash compensation is therefore no longer inert, and
`move_to`'s retract-then-advance sequence does what it was written to do.

Note that `rig_stator_backlash` rejects an exact zero: it validates as
`> 0.0`, so 0.001 mm is the way to ask for a pure retract.

### The crossings, settle-stepped

Single-microstep jogs remove the FIFO lead, because each move drains the queue
before it reports, so the published level belongs to the settled step. Three
retract-advance cycles, then a further two as a logged sweep 45 microsteps
either side of the edge:

| Quantity | Value |
|---|---|
| Retracting crossing | 165 microsteps, all five cycles |
| Advancing crossing | 184 microsteps, all five cycles |
| Reversal dead band | **19 microsteps, 60.3 um** |
| Advancing spread | **0 microsteps**, so under 3.175 um |

The sweep also shows the transition is clean: one change of level in each
direction over 45 microsteps either side, no chatter near the edge. The figure
is `edge_sweep.png`, regenerated from `edge_sweep.csv` by the harness described
below.

Two consequences for the constants, and neither needs a change:

- `STATOR_BACKLASH_MM` at 0.25 mm is 79 microsteps, four times the measured
  dead band. Correct as it stands, and the small value David preferred on
  2026-08-14 is now right for the right reason rather than by accident.
- `STATOR_HOME_BACKOFF_MM` at 0.2 mm is 63 microsteps, comfortably past the
  dead band. Homing's back-off-and-reapproach would now actually move the
  flag, which it could not before. That removes one of the two reasons homing
  was blocked; the other is still open, below.

### The FIFO lead is 10 to 11 microsteps, not 8

The coarse scan's latches can be differenced against the settle-stepped truth,
in both directions:

| Pass | Latched | Settled truth | Lead |
|---|---|---|---|
| Advancing, coarse scan | 194 | 184 | 10 |
| Advancing, dead-band run | 195 | 184 | 11 |
| Retracting, dead-band run | 154 | 165 | 11 |

So `stator_opto_edge` read from a normal move leads the mechanism by 10 to 11
microsteps, 32 to 35 um, rather than the eight the design assumed. The extra
words are the OSR and the pulse in flight on top of the eight-word joined FIFO.
It does not affect homing, which settle-steps its approach for exactly this
reason, but it does mean an edge latched during an ordinary move is worth about
35 um, not 25, and `docs/stator-stage.md` understates it in two places.

### Still blocked: the datum geometry, and a constant that contradicts it

Homing must still not be run, for the one remaining reason. Which extreme of
travel the datum sits at, and how much clearance lies between the edge and the
hard stop, are both still unmeasured guesses.

The measurements do, though, expose an inconsistency in the pair of constants
that describe the geometry. `STATOR_OPTO_HIGH_BEYOND_DATUM` is `true` and
`STATOR_DATUM_AT_ADVANCED_EXTREME` is `true`, which together assert that the
opto reads high on the advanced side of the edge. It reads **low** there:
advancing takes it 1 to 0, on every crossing recorded since 2026-08-14. So at
least one of the two is wrong, and `beyond_datum()` currently returns `false`
while the stage sits on the advanced side of the edge. Which one to change
depends on which side carries the hard stop, which is the same physical
measurement homing is already waiting on. Do not resolve it by flipping a
constant until the stop has been found by hand.

### Zero disturbance to the real-time loop

320 completed moves over the session, `stator_faults` 0 throughout,
`loop_time_max` 45 us against the 60 us profile limit, `overruns` 0, and
`records_dropped` unchanged at its pre-session 497. The architectural claim
that the axis cannot disturb core 1 continues to hold under a few hundred
moves.

### Harness

Four short host scripts, kept in the session scratchpad rather than the
repository: an advancing scan with stop-on-edge, a dead-band ladder, a
settle-stepped refinement, and a logged sweep with its plot. All wait on
`stator_moves`, the completed-move counter, which is the fix for the
2026-08-14 harness bug that waited on `stator_state` and issued its next
command into a still-running move. Ask before relying on them again: they are
diagnostic scaffolding, not a committed tool.
