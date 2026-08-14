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
verified, and stepping leaves the real-time loop undisturbed. Nothing about the
mechanism is, and the axis has never been homed. See the standing constraints
below and `docs/stator-stage.md`.

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
  firmware emitted exactly the steps it should. Everything else about it is
  still an assumption: the fitted motor is unidentified, so 200 full steps per
  revolution and direct coupling to the barrel are guesses; the direction
  convention, the opto polarity, and the datum geometry are unconfirmed; and
  the axis has never been homed. `docs/stator-stage.md` holds the commissioning
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
