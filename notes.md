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

**Hardware-verified through the pre-selectable-control firmware**, on a
W5500-EVB-Pico2 with the interim analogue cape: the 8 kHz hardware-clocked
acquisition path, the DAC output path, networking, discovery, the parameter
registry, and UDP streaming of the former 15-source table. The selectable
control build changes the source table and tick path and is software-verified
only; the 2026-08-19 entry records that boundary. The figures under "Bring-up
evidence" below remain the latest hardware acceptance record.

The stator stage is only partly covered. It was wired and first driven on
2026-08-14: the firmware's step generation, command surface, and telemetry are
verified, and stepping leaves the real-time loop undisturbed. Of the mechanism,
the datum edge and the reversal dead band are measured as of 2026-08-17, and
the datum geometry, the steps per millimetre and the travel limits are not. The
axis has never been homed. See the standing constraints below and
`docs/stator-stage.md`.

Exceptions, all electrical rather than real-time. The DAC output path is
verified as it was driven before 2026-08-12, with channel A alone against a
fixed channel C. The symmetric A/C drive that replaced it is timing-verified
and confirmed through the `drive` loopback, first at exactly double amplitude
(`out` as the per-channel offset) and then, after `out` was redefined as the
differential command on 2026-08-18 (firmware `a1e45da`), at unity (`drive` =
`out` to five figures) — still not independently observed on a scope with the
exciter powered, though. ADC channel 0 was rewired to a stator coil (`coil`)
and its calibration has not been established. ADC channel 1 (`drive`) is
wired and calibrated against `out`, with the exciter unpowered; see the last
four bring-up entries below.

The exciter drive is only as safe as the limits in `src/safety_limits.rs` and
`src/config.rs`, and those limits are compile-time constants that describe the
fitted hardware. Re-check them, and `DAC_POLARITY` in `src/rig.rs`, against the
hardware before every change of analogue board, specimen, or exciter.

## Standing hardware constraints

- **optoNCDT RX pull-up.** GP1 needs an external 10 kΩ pull-up to 3V3. Without
  it a disconnected or unpowered sensor leaves the line floating, and the UART
  free-runs into a framing/break interrupt storm that livelocks core 0. The
  symptom is a rig that boots and then stops answering on the network.
- **Analogue output stages.** The fitted cape is all-unipolar, and
  `DAC_POLARITY` in `src/rig.rs` is set to match. Channels A and C are the
  exciter's differential inputs and are driven symmetrically about the
  2.048 V common mode. **Since 2026-08-18** (`MID_RAIL + out/2` on A,
  `MID_RAIL - out/2` on C), so `out` is the differential command directly,
  equal to what `drive` measures; before that date it was `MID_RAIL + out` /
  `MID_RAIL - out`, a differential of `2 * out`. Either way channel B is
  broken and channel D is unused, so both rest at 0 V. Output routing is
  fixed to channel A and `rig_out_channel` will reject any other value. The
  physical range hasn't changed, only its label: the actual operating range
  stays clamped to ±1.952 V per channel about `MID_RAIL` by
  `DAC_OUT_FLOOR_V`/`DAC_OUT_CEILING_V` in `src/safety_limits.rs`, an unchanged,
  deliberate 0.096 V headroom below each rail, which is now **±3.904 V on
  `out`** (was ±1.952 V before 2026-08-18, since `out` used to be the
  per-channel deviation rather than the differential). Before 2026-08-12, C
  was held constant at `MID_RAIL` and only A varied, giving ±2.048 V.
- **Two DAC words per tick.** Driving A and C symmetrically means `actuate`
  writes two AD5064 words per tick, and the part needs ~3 µs between
  consecutive words. A single write per tick was spaced by the tick period
  itself; two are not, so `wait_word_settle` in `src/rig.rs` busy-waits on the
  always-on microsecond timer between them. It costs ~6 µs of the tick budget
  (see the 2026-08-12 bring-up below). Do not remove it, and do not replace it
  with an `embassy-time` delay: the tick path is Embassy-free and
  SRAM-resident.
- **ADC channel map, and a rename.** AD7609 channels 0 and 1 are named
  sources; 2-7 remain physically spare but are omitted from the current stream
  table. The converter still acquires all eight channels synchronously. Both
  named channels use the AD7609's true-bipolar differential inputs.
  - `coil` (channel 0) is a sense coil wound around the stator. Before
    2026-08-12 this channel was the actuator controller's current-sense
    output, so captures from before that date hold current, not coil voltage,
    and the two are not comparable.
  - `drive` (channel 1) is the exciter current controller's differential
    input. Declared in firmware on 2026-08-12; wired on 2026-08-18 and
    calibrated against `out` the same day, with the exciter unpowered.
  The wire-visible names changed with this: `adc0` became `coil`, `adc1`
  became `drive`, and the later selectable-control build removed `adc2` to
  `adc7`; `rig-profile.toml` follows. Sources are discovered by name, so host
  code and saved captures predating either change will not match the current
  table.
- **`drive` calibration against `out`, exciter unpowered: `drive` = 1.0000 ×
  `out` − 0.0002 V**, current since firmware `a1e45da` redefined `out` as the
  differential command (see "Analogue output stages" above). Measured
  2026-08-18 over an eleven-point sweep from -3.8 V to +3.8 V, ADC-side, well
  inside the AD7609's ±10 V range even at the DAC rails; residual against that
  fit was ≤32 µV, an order of magnitude below `coil`'s own noise floor. **This
  supersedes the 2.0000× figure measured the same day under the pre-`a1e45da`
  mapping**, which is no longer how the firmware behaves; both are correct for
  their own firmware. It taps the exciter controller's input rather than the
  DAC pins, so this is sensitive to cape changes in a way a direct DAC tap
  would not be, and should be rechecked after any analogue board swap. **The
  exciter was unpowered for this measurement.** If the tap point includes an
  actively buffered input stage inside the exciter's current controller, its
  gain could differ once that controller is powered; treat the ratio as
  provisional until repeated live, ideally cross-checked against a scope on
  channels A and C directly. See the two 2026-08-18 bring-up entries below.
- **The stator stage has moved, but is not calibrated or homed.** The axis was
  wired and first driven on 2026-08-14, two jogs of one revolution, and the
  firmware emitted exactly the steps it should. The direction convention and
  the opto polarity are now measured, and since the mechanical rework of
  2026-08-17 the stage follows a retraction with a 19-full-step reversal dead
  band and a datum edge repeatable to about one full step. The steps per
  millimetre, the absence of gearing, the microstepping constant and the datum
  reading were calibrated against the barrel the same day. What remains
  unmeasured is the **datum geometry**: which extreme of travel the datum sits
  at, and the clearance between it and the hard stop. The two constants
  describing that geometry contradict the measured opto polarity, and the axis
  has never been homed. `docs/stator-stage.md` holds the commissioning
  procedure, and none of it beyond first motion has been done.
- **Microstepping is not strapped**, so `STATOR_MICROSTEPS` stays at 1.0 and
  the driver is in full step, 3.175 µm per step. Every step count recorded in
  this file was therefore taken in full step, including the ones the earlier
  entries call microsteps, which is the firmware's name for the counter's unit
  rather than a statement about the strapping. Read them as full steps until an
  entry says otherwise, and prefer the millimetre and micrometre figures, which
  do not change meaning when MS1/MS2 are eventually strapped. Raising that constant without
  strapping MS1/MS2 makes every move eight times too long, into the end stop,
  and the soft travel limits do not protect against it because they are
  converted to steps with the same constant. The current-limit potentiometer is
  the real backstop and should be set low.
- **Do not raise `rig_stator_rate` above 0.5 mm/s.** Measured on 2026-08-17:
  0.5 mm/s loses no steps over some forty thousand, 1.5 mm/s lost 17 steps in
  one cycle of three, and 3.0 mm/s lost 16 every cycle. Nothing detects it, so
  the symptom is a position quietly wrong by tens of microns. The cause is the
  absence of an acceleration ramp, so the fix is a ramp rather than a smaller
  increase. `MAX_STATOR_RATE_MM_S` still permits 3.0 and should not be trusted
  as a safe bound.
- **The datum repeats to ±1 full step, 3.2 µm**, which is the opto sensor's own
  resolution rather than an error that grows with use: 57 600 steps at
  0.5 mm/s on 2026-08-17 lost none, and the crossing dithers between two
  adjacent steps instead of marching. A drift of about a step per five minutes
  seen earlier the same day stopped once the reworked coupling had been
  exercised, and is read as bedding in; re-measure after the rig has stood
  overnight, because its return would falsify that.
- **Travel limits, measured by hand on 2026-08-17.** The datum sits near the
  middle of the travel, not at an extreme. One full step is exactly
  0.000125 inch.

  | | Barrel | From the datum | Run-out |
  |---|---|---|---|
  | Lower hard stop | 0.100 inch, 2.540 mm | -2541 steps, -8.07 mm | |
  | Soft lower limit | 0.125 inch, 3.175 mm | -2341 steps, -7.43 mm | 0.635 mm |
  | Datum | 0.4176875 inch, 10.609 mm | 0 | |
  | Soft upper limit | 0.710 inch, 18.034 mm | +2338 steps, +7.42 mm | **0.13 to 0.25 mm** |
  | Upper hard stop | 0.715 to 0.720 inch | +2378 to +2418 steps | |

  **The upper run-out is the one to respect**: barely 0.005 to 0.010 inch,
  where the lower has a full barrel turn. The upper limit is close to its stop
  because that is where the interesting operating case lies, and David intends
  to revise the hardware for more headroom. Until then treat the top of the
  window with more caution than the bottom.

  Now in `src/config.rs` as `STATOR_TRAVEL_MIN_MM` and `STATOR_TRAVEL_MAX_MM`,
  expressed as barrel readings rather than distances from the datum, because
  the stops are fixed on the barrel while the datum is an estimate a later home
  may revise.

  **The firmware enforces this only once homed.** Before homing there is no
  window, and the only bound is `STATOR_SEEK_MAX_MM` on a single jog, so
  repeated jogs still walk anywhere. Until the axis has been homed the envelope
  remains the operator's discipline and belongs in any host script.
- **Homing has still never been run**, and the code that would run it was
  rewritten on 2026-08-17 and has not executed on hardware. The blockers that
  stood before are gone: the geometry is measured, the constants that described
  a datum at an extreme are deleted, and the backoff moves the flag now that
  the coupling follows a retraction. What remains is that a first home is an
  unexercised code path pointed at a mechanism with 0.13 mm of run-out at one
  end. Supervise it, with a hand on the supply, and check `stator_opto_edge`
  against the counter afterwards.
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

## The stage now follows a retraction: dead band 19 full steps (2026-08-17)

David reworked the coupling between the stage and the stepper, to get a better
mechanical connection than the push-only contact of 2026-08-14. The rework is
his to describe; what follows is only what the axis now does. Measured on the
image already flashed, `0.1.0 9be46f2`, which is one commit behind `main` and
differs from it only by the platform repin to v0.2.3, nothing in the axis.

**Counts here are full steps, 3.175 um each.** MS1/MS2 are still unstrapped, so
the driver is in full step and `STATOR_MICROSTEPS` is 1.0. The firmware calls
the counter's unit a microstep throughout, which is right in code because
`STATOR_MM_PER_MICROSTEP` scales with the constant, and wrong in recorded
evidence, where a bare count silently changes meaning by a factor of eight the
day the strapping is done. Every step count in this file predates that
strapping, so read "microstep" in the earlier entries as a full step of
3.175 um. The distances in millimetres and micrometres are unaffected either
way, and are the figures to trust.

**The datum edge is at 184 full steps** from the power-on counter zero, that
is 0.584 mm of advance from wherever the stage sat when it booted. Found by
advancing in 100-full-step jogs with the host polling during each move and
issuing `rig_stator_stop` on the first opto change, so the overshoot past the
edge was bounded by the poll interval rather than by the jog length. It turned
up in the second jog: the stage was already sitting just below the edge.

**The stage follows a retraction now.** This is the result that matters, and it
reverses the 2026-08-14 finding directly. With `rig_stator_backlash` set to
0.001 mm, which rounds to zero full steps and so makes a negative jog a pure
retract, 64 full steps of retraction (0.203 mm) brought the opto back to 1. On
2026-08-14 the same axis was retracted 503 full steps (1.6 mm) and crossed
nothing at all. The backlash compensation is therefore no longer inert, and
`move_to`'s retract-then-advance sequence does what it was written to do.

Note that `rig_stator_backlash` rejects an exact zero: it validates as
`> 0.0`, so 0.001 mm is the way to ask for a pure retract.

### The crossings, settle-stepped

Single-full-step jogs remove the FIFO lead, because each move drains the queue
before it reports, so the published level belongs to the settled step. Three
retract-advance cycles, then a further two as a logged sweep 45 full steps
either side of the edge:

| Quantity | Value |
|---|---|
| Retracting crossing | 165 full steps, all five cycles |
| Advancing crossing | 184 full steps, all five cycles |
| Reversal dead band | **19 full steps, 60.3 um** |
| Advancing spread | **0 full steps**, so under 3.175 um |

The sweep also shows the transition is clean: one change of level in each
direction over 45 full steps either side, no chatter near the edge. The figure
is `edge_sweep.png`, regenerated from `edge_sweep.csv` by the harness described
below.

Two consequences for the constants, and neither needs a change:

- `STATOR_BACKLASH_MM` at 0.25 mm is 79 full steps, four times the measured
  dead band. Correct as it stands, and the small value David preferred on
  2026-08-14 is now right for the right reason rather than by accident.
- `STATOR_HOME_BACKOFF_MM` at 0.2 mm is 63 full steps, comfortably past the
  dead band. Homing's back-off-and-reapproach would now actually move the
  flag, which it could not before. That removes one of the two reasons homing
  was blocked; the other is still open, below.

### The FIFO lead is 10 to 11 full steps, not 8

The coarse scan's latches can be differenced against the settle-stepped truth,
in both directions:

| Pass | Latched | Settled truth | Lead |
|---|---|---|---|
| Advancing, coarse scan | 194 | 184 | 10 |
| Advancing, dead-band run | 195 | 184 | 11 |
| Retracting, dead-band run | 154 | 165 | 11 |

So `stator_opto_edge` read from a normal move leads the mechanism by 10 to 11
full steps, 32 to 35 um, rather than the eight the design assumed. The extra
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

## Datum repeatability, a slow drift, and the rate at which steps go missing (2026-08-17)

Three runs on the same flashed image, 3285 moves in total, `stator_faults` 0
throughout, `loop_time_max` 45 us against the 60 us limit, `overruns` 0 and
`records_dropped` unchanged at its pre-session 497. Counts are full steps of
3.175 um.

Each cycle measures both datum crossings by single-stepping, with a bulk
excursion in between: creep down through the retracting crossing, retract the
rest of the excursion, come back to a guard position 60 steps below the edge,
then creep up through the advancing crossing. Both crossings are therefore
settle-stepped and neither carries the FIFO lead. Because the retracting
crossing is recorded before the bulk moves and the advancing one after, **the
dead band measured within a cycle is a lost-step detector**: it is 19 whenever
the bulk moves were faithful, and departs from 19 by exactly the number of
steps they lost, signed by which direction lost them.

### Repeatability at the working rate is one full step

Sixteen cycles at 0.5 mm/s, retractions of 200, 400, 600 and 800 full steps
(0.64 to 2.54 mm) interleaved rather than blocked, then ten more cycles later
in the session:

| Quantity | Value |
|---|---|
| Advancing crossing, 26 cycles | 184 to 186, **spread 1 to 2 full steps** |
| Standard deviation, first 16 | 0.50 full steps, 1.6 um |
| Dependence on excursion length | **none detectable** |
| Dead band | 19 in every cycle |
| Faults | 0 |

So the axis returns to the datum within about one step, 3.2 um, after
excursions of up to four barrel revolutions. That is eight times inside the
25.4 um the design budgeted, and it holds equally at 0.64 and 2.54 mm of
excursion.

### The spread is a slow monotone drift, and it is not reversible

The one-step spread is not scatter. Over the first sixteen cycles the advancing
crossing steps 184 to 185 at cycle 7 and the retracting crossing 165 to 166 at
cycle 10. **A lost step would move both at the same instant**, since both are
read from the same counter. They moved three cycles apart, and the dead band
read 20 only in the window between the two transitions before returning to 19,
which is the signature of a continuous drift quantised at two different
sub-step phases rather than of a counter slip.

The obvious explanation was thermal, the motor being energised almost
continuously through a run. That is wrong, or at least incomplete. After eight
minutes idle with the driver de-energised, the crossings did not return: they
had gone one step **further**, to 186 and 167, and stayed there for ten
cycles. The drift runs at roughly one full step per five minutes, about
0.5 um/min, and it continues while nothing is moving.

Three candidates remain, not separated:

- the reworked coupling bedding in, which should decay and is the easiest to
  test, by repeating this measurement after some hours;
- a slow thermal equilibration of the whole assembly driven by something other
  than the motor, the laser and the analogue cape being the obvious sources;
- drift in the opto sensor's own trip point, which would masquerade as
  position drift.

**This measurement cannot tell a moving stage from a moving trip point**, and
the distinction matters: the first corrupts position and the second only
corrupts the datum. Separating them needs an independent displacement measure
against the barrel.

Practically, at 0.5 um/min the drift reaches the 25.4 um budget in about fifty
minutes, so re-datum within a session if better than a few microns is wanted,
and do not expect the datum to hold to a single step across a long one.

### Raising the traverse rate loses steps, silently

Nine cycles at a fixed 800-step excursion with the rate interleaved across
0.5, 1.5 and 3.0 mm/s. The creeps that measure the crossings always ran at
0.5 mm/s, so any loss is attributable to the bulk move rather than to the
measurement. Steps lost per retract-and-return, from the dead-band diagnostic:

| Bulk rate | Full steps/s | Steps lost per cycle |
|---|---|---|
| 0.5 mm/s | 157 | 0, 0, +1 |
| 1.5 mm/s | 472 | +1, +3, **+17** |
| 3.0 mm/s | 945 | **-16, -16, -16** |

Over the nine cycles the advancing crossing walked from 186 to 159, 27 full
steps, 86 um, entirely through the fast cycles. `stator_faults` stayed at 0 the
whole time: nothing in the firmware notices, and nothing can, because the axis
is open loop between datum crossings.

This is what "no acceleration ramp" costs. The first step of every move is
issued at the full traverse rate from standstill, so 3.0 mm/s asks a small
stepper for 945 steps/s from rest under load, well past any plausible pull-in
rate. The reliability of the -16 at 3.0 mm/s suggests a repeatable slip early
in each move rather than random loss.

Consequences:

- **`STATOR_RATE_MM_S` stays at 0.5 mm/s.** It is verified faithful over some
  forty thousand steps across this session.
- **1.5 mm/s is not a safe intermediate.** Two of three cycles were clean and
  the third lost 17 steps, which is the worst available failure mode:
  intermittent, silent, and invisible until the next home.
- **`MAX_STATOR_RATE_MM_S` at 3.0 permits a rate that reliably corrupts
  position.** It is a validation bound rather than a recommendation, but it
  currently lets a host set a value that silently loses a step per sixty. Worth
  lowering to about 1.0 until an acceleration ramp exists. Not changed here:
  it is a hardware judgement, and the ramp is the real fix.

## The residual step is dither, not lost steps, and the drift has stopped (2026-08-17)

David's question about the entry above: is the single step of spread the opto
sensor's resolution, or steps genuinely being lost? His discriminator was to
run longer and see whether the error accumulates.

That needs one modification before it separates anything. A longer run
accumulates steps **and** elapsed time together, and the cooldown result had
already shown the crossing moving while the axis was idle, so accumulation on
its own cannot say which of the two is responsible. The runs are therefore
blocks of equal wall-clock duration, about 100 s each, differing in what the
axis does, rotated rather than blocked so a background drift cannot masquerade
as an effect of any one condition:

| Condition | Steps | Motor |
|---|---|---|
| `busy` | 14 400, at 0.5 mm/s | energised, warming |
| `hold` | none | energised via `rig_stator_hold`, warming |
| `idle` | none | de-energised, cooling |

Both crossings are measured identically at every block boundary, costing about
forty steps, so the measurement contributes equally to all three conditions.
Four rotations: **57 600 full steps, 183 mm of travel, 19.8 minutes, zero
faults.**

### It is not lost steps

The advancing crossing over the whole run stayed within one step, and the shift
does not follow the stepping:

| After a block of | Shifts | Net |
|---|---|---|
| `busy`, all 57 600 steps | 0, 0, 0, -1 | **-1** |
| `hold` | 0, 0, +1, +1 | +2 |
| `idle` | 0, 0, 0, 0 | **0** |

The four `busy` blocks carried every one of the 57 600 steps and contributed
nothing, once with the wrong sign. Had the axis been losing steps at the rate
the earlier session's drift would imply, two steps per forty thousand, these
blocks would have shown about three steps of monotone accumulation. They showed
none.

The `hold` total of +2 is not worth a mechanism. Three of twelve boundaries
moved at all, and with a one-step quantiser and four observations per
condition, that distribution is what dither looks like rather than an effect of
holding current.

### It is the sensor's resolution, and the earlier drift was bedding in

The crossing sat at 160 for the first six boundaries, ten minutes and 28 800
steps, then moved between 160 and 161 for the rest: 161, 161, 160, 161, 161.
**Non-monotone**, dithering between two adjacent steps about a stable value.
That is the signature of a threshold sitting between two step positions, which
is exactly what "the resolution of the opto sensor" means, and it is the benign
answer.

Contrast the earlier runs, which moved 184 to 186 monotonically and did not
come back over an eight-minute de-energised soak. That drift has stopped: zero
net movement over the first ten minutes here, against a step per five minutes
before. The best reading is that the reworked coupling was bedding in and has
now settled, after some four thousand moves of exercise. It is a reading rather
than a demonstration, since nothing was measured independently of the datum,
but the change in behaviour is unambiguous.

One incidental confirmation. Block 7 ended with the opto reading 1 where 0 was
expected, and the harness flagged it. Every measurement parks the axis one step
past the advancing crossing, which is to say **on the threshold**, so the level
there is ambiguous by one step and occasionally reads the other way. That is
the same dither seen from a different angle, and it argues for parking a few
steps clear of the edge rather than one.

### What this settles

- **The step counter is faithful at 0.5 mm/s.** 57 600 steps with no
  detectable loss, on top of the 40 000 of the earlier runs. Combined with the
  rate sweep, the working rate is sound and the fault is entirely in raising it.
- **The datum is repeatable to ±1 full step, 3.2 µm**, which is the sensor's
  resolution and not an error budget that grows with use. Eight times inside
  the 25.4 µm the design assumed.
- **The drift is no longer a live concern**, but it is worth re-measuring after
  the rig has been left overnight. If it returns after a period at rest, it is
  not bedding in and the reading above is wrong.

## Steps per millimetre and the datum, calibrated against the barrel (2026-08-17)

The three assumptions the axis was built on are now measurements. David read the
micrometer at both ends of a commanded leg, with the axis held still and
de-energised for each reading.

### The leg

The axis was parked on the settle-stepped advancing crossing, then advanced two
steps so the barrel sat on a graduation line, and read. It was then moved 800
full steps down and read again, and returned and read a third time. Both ends
were approached by an advance, the second via the standard 0.25 mm backlash
take-up, so every reading is in the same contact state.

| Counter | Barrel | |
|---|---|---|
| 163 | 0.418 inch, on the line | start |
| -637 | **0.318 inch, exactly on the line** | 800 steps down |
| 163 | **0.418 inch, back on the line** | closure |

**800 full steps = 0.100 inch = 2.540 mm exactly, so 3.175 um per full step**,
which is the value the firmware has assumed all along.

Landing on a graduation at both ends matters more than the two absolute
readings do. It is a null measurement: it says the 800 steps produced a whole
number of turns, and the sleeve says that number was four. The angular
precision of a thimble line is well under a division, so this pins the
steps-per-revolution far tighter than reading two distances a division apart
could.

What that settles, all previously flagged as assumptions:

- `STATOR_FULL_STEPS_PER_REV = 200` is **confirmed**. A 0.9 degree motor would
  have moved 0.368 inch.
- **Direct coupling, no gearing or belt reduction**, is confirmed by the same
  measurement.
- `STATOR_MICROSTEPS = 1.0` is **confirmed against the hardware** rather than
  against the MS1/MS2 strapping by inspection. Strapped 1/8 microstepping would
  have moved 0.406 inch.
- **No steps lost over the 1600-step round trip**, by barrel and by counter
  together, which agrees with the electrical evidence from the same day.

### The datum

The advancing crossing dithers between counter 160 and 161, as established
earlier, so the sensor's trip point lies between two step positions rather than
on one. Taking the midpoint, the datum edge is 2.5 full steps below the 163 at
which 0.418 inch was read. One full step is exactly 0.000125 inch, from the
calibration above, so:

**Datum = 0.418 - 0.0003125 = 0.4176875 inch = 10.609 mm.**

Now written to `rig_stator_datum` at runtime and to `STATOR_DATUM_MM` in
`src/config.rs`, since the runtime value does not survive a reflash.

Two things to record with it, neither a defect:

- **The uncertainty is about one full step, 3.2 um.** An earlier draft of this
  entry said ±13 um, half a barrel division, and David corrected it. Nothing
  was interpolated between graduations: the axis was stepped until the barrel
  coincided with a line, so the reading is a null measurement whose precision
  is how well coincidence can be judged, and the exactly known two-step offset
  then carries it back to the datum. The residual is the half-step dither of
  the trip point, not the division width. This is the same trick as the
  calibration leg, and it is worth remembering as the general method: **move
  the axis onto the graduation rather than estimating between graduations.**
- **The absolute accuracy of the barrel does not enter.** Nothing depends on
  it. The datum fixes an origin, and every quantity the experiment uses is a
  relative move from that origin.
- **This datum was not produced by homing.** It comes from a settle-stepped
  advancing crossing, which is the same approach homing uses and the same
  quantity it latches, but homing has still never been run and the datum
  geometry is still unmeasured. If the first home disagrees with 10.609 mm by
  much more than 13 um, believe the home and suspect the geometry.

## Homing is implemented, and what now blocks it is the travel model (2026-08-17)

Homing exists in full. `home()` in `src/stator.rs`, reached by writing any
non-zero value to `rig_stator_home`, and described in `docs/stator-stage.md`.
It has never been run on this rig. Nothing in the platform is involved: the
whole axis is rig-specific, and only `src/step_pio.rs`, the pulse generator, is
portable enough to belong upstream one day.

What it does, in order: refuse outright if the geometry is the awkward one and
the backoff will not fit the clearance; leave the clearance if it started in
one, moving away from the hard stop; stand off by `STATOR_HOME_BACKOFF_MM`;
approach the edge at `STATOR_HOME_SLOW_MM_S`, stepping one at a time and
waiting for each pulse so the sensor is read in step with the mechanism rather
than a FIFO ahead of it; and, in the geometry where that approach was a
retract, retreat and come back out advancing so the crossing that defines the
datum is made under contact. Every search is bounded by `STATOR_SEEK_MAX_MM`
and a search that runs past it faults rather than continuing into a stop. It
then zeroes the counter and, on a re-home, publishes `stator_home_error`, the
lost-step audit.

The mechanical objection is gone. Homing's back-off-and-reapproach was inert
before the coupling rework, because a retraction moved nothing; the backoff of
0.2 mm is now 63 full steps against a measured dead band of 19 to 20, so it
moves the flag properly.

**What blocks it now is that the travel model does not match the rig.** The
firmware assumes the datum sits at one extreme of travel with a hard stop just
past it, and derives a one-sided soft window from that:

| Constant | Value | What it asserts |
|---|---|---|
| `STATOR_DATUM_AT_ADVANCED_EXTREME` | `true` | the datum is at the advanced end |
| `STATOR_DATUM_CLEARANCE_MM` | 0.5 | a hard stop 0.5 mm above the datum |
| `STATOR_TRAVEL_RANGE_MM` | 5.0 | all working travel lies below the datum |
| `STATOR_OPTO_HIGH_BEYOND_DATUM` | `true` | the opto reads high above the datum |

Three of those four are contradicted by measurement. The opto reads **low**
above the datum, on every crossing since 2026-08-14, so the last row is wrong
as written; `travel_window()` currently yields -1575 to 0 steps, permitting no
travel at all above the datum, while the sanctioned envelope allows +2458; and
a hard stop 0.5 mm above the datum cannot be reconciled with an envelope that
goes 7.8 mm above it.

The honest conclusion is that **the datum is somewhere in the middle of the
travel, not at an extreme**, which is a case the design did not contemplate. It
is a benign case, being the one with a hard stop nowhere near, but the code
expresses it badly: `beyond_datum()` is documented as "in the short clearance
between the edge and the hard stop" when it now only means "on the advanced
side", and the refusal that protects the awkward geometry guards a hazard that
may not exist.

So this is a firmware change rather than a measurement, and it should not be
made by guessing which of the two booleans to flip. What is needed first:

1. **Where are the hard stops?** Both of them, by hand, with the motor
   uncoupled, in barrel readings. That is the measurement the envelope is
   standing in for.
2. **Then decide the travel model.** If the datum really is mid-travel, the
   window wants to become genuinely two-sided, with a signed range either side
   of the datum, and the clearance concept drops out. That is a small change to
   `travel_window()` and the four constants above, and it makes homing's
   two-geometry branch mostly redundant, since the approach can always be an
   advance from below.

Until then homing stays unrun, and the envelope stays the operator's
responsibility.

## The travel model is two-sided now, and the spent diagnostics are gone (2026-08-17)

David measured the hard stops, at 0.100 inch and 0.715 to 0.720 inch, and
revised the upper working limit to 0.710. That settles the geometry and
contradicts the assumption the axis was designed around, which was his own and
which he retracted: **the datum is not at an extreme of travel.** It sits within
two full steps of the midpoint of the window.

### What changed in the firmware

The one-sided travel model is gone, replaced by two barrel readings.

| Removed | Replaced by |
|---|---|
| `STATOR_DATUM_AT_ADVANCED_EXTREME` | nothing; there is no geometry case left |
| `STATOR_DATUM_CLEARANCE_MM` | nothing; there is no short clearance |
| `STATOR_TRAVEL_RANGE_MM` | `STATOR_TRAVEL_MIN_MM`, `STATOR_TRAVEL_MAX_MM` |
| `STATOR_TRAVEL_ADVANCE_MM`, `STATOR_TRAVEL_RETRACT_MM` | as above |
| `STATOR_OPTO_HIGH_BEYOND_DATUM` | `STATOR_OPTO_HIGH_BELOW_DATUM`, still `true` |

The limits are configured as **barrel readings**, not as distances from the
datum, and `travel_window()` converts them through the runtime datum. The stops
are fixed on the barrel; the datum is an estimate a later home may revise. This
way a correction to `rig_stator_datum` moves the step bounds so they keep
describing the same two physical positions, which is the behaviour that cannot
surprise anyone.

`beyond_datum()` became `below_datum()`. The old name meant "in the short
clearance between the edge and the hard stop", which was never what the sensor
said and is now not a thing that exists.

### Homing lost a whole branch, and gained a second bound

With no extremum there is no awkward geometry, so the two-case approach
collapsed to one: get below the datum retracting, stand off by the backoff,
advance slowly onto the edge. The refusal that guarded the retracted-extreme
case went with it.

More important is the new `STATOR_APPROACH_MAX_MM`, 0.4 mm, bounding the final
advance separately from the 8 mm `STATOR_SEEK_MAX_MM` that bounds the retract.
The old code used the same bound for both, which with the measured geometry
would have let a failed sensor advance 6.5 mm from just below the datum: past
the upper stop, by an order of magnitude. The asymmetry in the code now matches
the asymmetry in the run-out, 0.635 mm below against 0.13 mm above.

One hazard survives and cannot be designed out. A sensor failing in the "below
the datum" state makes homing skip the retract and advance 0.4 mm blind, which
from the upper soft limit could touch the stop. Reaching the edge requires
advancing further than the backoff, so no bound removes it. The current-limit
potentiometer is the fuse, which is the argument for setting it low.

### Diagnostics removed

`rig_stator_dwell` and its `STATOR_DWELL_S`, `MAX_STATOR_DWELL_S`, the atomic,
the parameter, its validation, and the polled hold inside `set_direction`. It
existed to make a direction reversal watchable by eye during bring-up, when it
was not yet known which phase of a retract-then-advance was which. That question
is long answered, and it was costing a branch and a poll loop on the move path.
The parameter count drops from 60 to 59 and the name disappears from the wire.

Kept, with the reasoning recorded on the declarations so it is not revisited
from scratch:

- **`stator_opto`** is not a diagnostic. On an unhomed axis it is the only
  position feedback there is, and with the datum mid-travel "which side am I
  on" is operationally useful rather than merely interesting.
- **`stator_opto_edge`** stays until homing has been commissioned, because it
  is the only lost-step check available before `stator_home_error` works, and
  it is what the first home should be validated against. Marked in
  `telemetry.rs` as a candidate for removal after that.

The `diag-*` build features were reviewed and all eight are live: each is
referenced from `src/`, and the two that forward to the platform still name
features that exist at the pinned v0.2.3. Nothing stale there.

### Checks

All six software gates pass: `cargo fmt --check`, `clippy -D warnings`, both
board builds, `helic-deps-check`, and `helic-rt-layout`. **Not flashed**, so
none of this is hardware-verified: the rig is still running `0.1.0 9be46f2`,
which predates every change in this entry.

## Platform v0.2.4, flashed, and a homing bug caught before it ran (2026-08-17)

Repinned from v0.2.3 to v0.2.4 and flashed. Patch bump: the only consumer
change is `DualSsiReader` sampling one PIO cycle into the clock-high phase, a
fix for an RLS RMB20/AM4096 encoder. This rig does not use that codepath at
all, reading an AD7609, driving an AD5064, and generating steps from its own
PIO program, so David's expectation that the change was inert here is correct.
No crate API or wire-protocol change, and the Embassy versions are unchanged
from v0.2.3, checked against the platform's `firmware/Cargo.toml` at the tag.

**Two of the three pins had drifted.** The crates were at v0.2.3, but
`.github/workflows/ci.yml` and the README install line still said **v0.1.3**.
So since the v0.2.0 upgrade, CI had been installing host verification gates
three minor versions behind the firmware they check, which is exactly the
failure `AGENTS.md` warns about and it arrived silently. All three now say
v0.2.4. Worth a habit: grep the whole repository for the old tag when
upgrading, not just `Cargo.toml`.

### A homing bug, found by reading rather than by running

The homing rewritten earlier the same day was wrong for the case where the
stage starts **below** the datum, which is half the travel. It skipped the seek
phase and left the final approach, deliberately bounded at 0.4 mm, to cover a
gap of up to 7.4 mm. Homing would have faulted rather than worked. It fails
safe, so this was a correctness bug rather than a hazard, but it would have
been the first thing the first home did.

Homing now seeks **up** through the edge when it starts below it, comes back
down through it, backs off, and makes the tight final approach. The approach
therefore always begins one backoff below the edge whatever the starting
position, which is also what makes the datum repeatable from anywhere. The two
coarse phases run at 0.5 mm/s without settle-stepping, since the ten-step FIFO
lead is irrelevant before a sixty-three-step backoff; only the final approach
settle-steps, at 0.1 mm/s.

The residual hazard moved with the fix and is documented on the function: a
sensor stuck reading "below the datum" makes the upward seek run its full 8 mm
bound, which from just under the datum reaches past the upper stop. Every other
failure mode either retracts, toward 0.635 mm of run-out, or is bounded far
shorter than the distance to a stop. This is why the commissioning procedure
starts by confirming the sensor **changes state**.

### Build identity does not mark a dirty tree

Noticed while flashing: the first flash reported `0.1.0 ff55589`, a clean
commit hash, from a working tree that carried the repin and the homing fix
uncommitted. The platform's `emit_identity` builds `HELIC_GIT_DESCRIBE` with
`git describe --always --dirty`, but the wire identity, which is what
`helic-daq status` reports and what `AGENTS.md` calls "what makes a flashed
image identifiable", comes from `git rev-parse --short=7 HEAD` and carries no
dirty marker. A modified tree therefore flashes an image that names a commit it
is not built from.

Committed and reflashed so the identity is true, but the gap is a platform one
and worth raising upstream: the wire identity should carry the dirty marker, or
refuse to build. Until then, **commit before flashing** anything whose identity
is going to be quoted as evidence.

### Verified after the flash

`firmware: 0.1.0 4f782a4`, 59 parameters where there were 60, since
`rig_stator_dwell` is gone, and `rig_stator_datum` comes up at 10.609 mm from
the new compile-time default rather than needing a runtime write.

All six software gates, and the full default `helic-rt-regression` with no
probe attached: no acceptance errors, `loop_time_max` 45 us against the 60 us
limit, 8000 records, no lost packets, no index gaps, no dropped records, jitter
0, wake phase 36/36.

**Homing itself is still unexercised.** Nothing above ran it.

## Unresolved: the opto stopped responding to motion after the reflash (2026-08-17)

**Homing was not run.** Step 1 of the commissioning procedure, proving the
sensor changes state, failed, which is the check that exists to stop exactly
this becoming a crash.

After flashing `4f782a4` the stage sat physically on the datum edge, where the
previous image had parked it, and the counter came up at 0 as expected.
Retracting from there should cross the edge within about twenty full steps: the
reversal dead band measured 19 to 20 earlier the same day, and 64 steps crossed
it reliably in dozens of cycles.

It did not cross in **640 full steps, 2.03 mm**, issued as 150 completed moves
with `stator_faults` 0 throughout. `stator_opto` stayed at 0 and
`stator_opto_edge` is still NaN, meaning the level has not changed once since
boot. Even the pre-rework behaviour, when a retraction moved the stage barely
at all, crossed the edge by 797 steps.

The axis was left where it stopped, at counter -640, deliberately: the barrel
reading there is the discriminating measurement and moving again would destroy
it.

### What the evidence already narrows

**A disconnected sensor is unlikely.** GP27 is an input with a pull-up, so an
open or unpowered sensor floats **high**, reading 1. The stuck level is **0**,
which is the flag-out-of-slot state and has to be actively driven. The sensor
looks alive and looks as though it is being told there is no flag.

**The firmware change is an unlikely cause, though not excluded.** Between the
last image that worked and this one, the only code a jog can reach that changed
at all is `set_direction`, which lost the dwell block; the rewritten `home()` is
not called by a jog, and `travel_window()` and `below_datum()` are reached only
from homing or from a window that an unhomed axis does not enforce.
`stator_opto` publishes the raw pin level and that path is untouched.

So the weight is on the mechanism: the counter is advancing and the flag is not
following it.

### The measurement that decides it

Read the barrel. The axis has commanded 640 steps of retraction from a datum at
0.4176875 inch, and one full step is exactly 0.000125 inch:

| Barrel now reads | Conclusion |
|---|---|
| about **0.338 inch** | the drive moved the stage; the fault is in the sensor or the flag |
| about **0.418 inch**, unchanged | the drive did not move the stage; the coupling, the motor, or ENABLE |

A useful second check, if the first is ambiguous: write `rig_stator_hold = 1`
and feel whether the motor develops holding torque against detent alone. That
separates a de-energised driver from a slipping coupling.

Until this is understood, **do not home**: the sequence depends on the sensor
terminating both searches, and a sensor stuck reading "below the datum" is the
one failure mode that can drive this axis into the upper stop. The level is
currently stuck the other way, reading "above the datum", which would fault
safely on the downward seek rather than run away, but that is not a reason to
try it.

## Correction: there was no fault, and the axis validated end to end (2026-08-17)

The entry above is **retracted**. There is nothing wrong with the sensor, the
drive, or the coupling. David had moved the stage by hand while finding the
travel limits, so the counter's zero, set by the reflash, corresponded to an
unknown barrel position rather than to the datum edge where the previous image
had parked it. Every reading was truthful; my premise was wrong.

The opto reading 0 at a hand-set 0.515 inch is exactly correct: that is above
the datum at 0.4177, and above the datum reads low. It had been telling the
truth throughout, and the sensor being stuck at the *low* level was the clue I
under-weighted, having already reasoned that the pull-up makes a disconnected
sensor read high.

**The lesson is about the counter, not the sensor.** After a reflash the step
counter is zero and the axis is unhomed, so the counter carries no information
about where the stage is. It is easy to keep treating it as though it does,
because it reads plausibly and the last known position is fresh in mind. Anything
moved by hand, and anything at all across a reflash, needs the barrel or the
opto to re-establish position before a step count means anything.

### What the false alarm turned into: a clean end-to-end validation

Given the barrel at 0.515 inch and the datum at 0.4176875, the datum should lie
(0.515 - 0.4176875) / 0.000125 = **779 full steps** below, with the retracting
crossing about 19 steps further on. Retracted in 50-step blocks, the opto
crossed at **800 steps**, and correcting for the 10 to 11 step FIFO lead of a
coarse move puts the true crossing at about 789 against 798 predicted.

Settle-stepped immediately afterwards, the crossings came out at:

| Quantity | Value |
|---|---|
| Advancing crossing | counter -1418 |
| Retracting crossing | counter -1438 |
| Dead band | **20 full steps**, matching the 19 to 20 measured this morning |
| Datum below the 0.515 inch reading | **778 steps**, against 779 predicted |

So an independent hand reading of the barrel and a datum measured hours earlier
agree to **one full step, 3.2 um**. That exercises the whole chain at once: the
datum value, the steps-per-inch calibration, the dead band, the sensor, the
drive and the coupling. It is a better check than the one I was trying to run
when it failed.

### Commissioning step 1 passed

The sensor changes state in both directions, 1 to 0 advancing and 0 to 1
retracting, on the flashed image `4f782a4`. That eliminates the one failure mode
that can drive this axis into the upper stop, a sensor stuck reading "below the
datum" with nothing to terminate homing's upward seek.

The stage is parked 100 full steps below the datum edge, so the first home's
upward seek has 0.32 mm to run against its 8 mm bound. Homing has still not been
run.

## Homing commissioned: ten homes, zero error, zero faults (2026-08-17)

The axis has been homed. Commissioning steps 3 to 7 of `docs/stator-stage.md`
are done, on firmware `ef798b2`.

### The first home, and what its trace showed

From 100 full steps below the datum edge, the four phases behaved exactly as
designed. Read off the counter trace:

| Phase | Counter | Note |
|---|---|---|
| 1, seek up | -1518 to -1408 | crossed the edge, stopping 10 steps past the settle-stepped crossing at -1418, which is the FIFO lead on a coarse move |
| 2, come back down | to about -1448 | crossed back, again a FIFO lead past |
| 3, back off | to -1511 | 63 full steps, as configured |
| 4, slow approach | -1511 to -1418 | 93 single settle-stepped advances, then the counter zeroed |

**Phase 4 used 93 full steps of the 126 its bound allows.** That is the real
constraint on `STATOR_APPROACH_MAX_MM` and it is now recorded on the constant:
the approach has to cover the FIFO lead, plus the backoff, plus the dead band,
11 + 63 + 20. The remaining margin covers the dead band roughly doubling before
homing starts refusing, and that refusal is informative rather than dangerous.

### Two bugs the commissioning found

**`stator_home_error` was not measuring an error.** It subtracted the position
homing started from, so it reported the distance homing travelled. Tested on
hardware before the fix: homing from counter +500 with nothing lost reported
**-500**. The datum defines counter zero, so the counter's reading when the
datum is found back is itself the accumulated error, and that is what it stores
now. The exactness of that -500 is incidentally the evidence that nothing was
lost over the excursion.

**`stator_opto_edge` kept a stale frame.** The latch held the value recorded
before `home()` re-zeroed the counter, so after the first home it read -1418: an
edge more than four millimetres from a datum it was sitting on. It is now
carried into the new frame with the counter and reads about zero after a home,
which makes it a usable check that the approach landed where the settle-stepped
measurement did.

Both were only findable by running the thing. Neither would have shown up in any
software gate.

### Ten homes

Alternating the starting side so both paths through `home()` are exercised: from
300 steps **below** the datum, which runs the upward seek, and from 700 steps
**above** it, which skips the seek and takes the long retract instead.

| Quantity | Result |
|---|---|
| Homes | 10, plus the first |
| `stator_home_error` | **0.0 on every one**, spread 0, sd 0 |
| `stator_opto_edge` after each | **0.0 on every one** |
| `stator_faults` | 0 |
| Both homing paths | exercised, five each |

So: **no lost steps at all** across ten excursions of 300 to 700 full steps, and
a datum repeatability of **zero full steps**, which bounds it below one step,
3.175 um. The design budgeted 25.4 um and the audit's noise floor was assumed to
be 64 full steps; the real figures are at least eight and sixty-four times
better respectively.

Worth noting against this morning: the advancing crossing dithered between two
adjacent steps then, and did not dither at all here. `stator_home_error` is
sensitive to exactly that, since each home re-zeroes on the crossing it finds,
so ten identical zeroes is a real result rather than a blind spot. The likely
reason is that homing's approach is fixed, always the same rate and the same
standoff, where this morning's crossings were taken at a mix of rates and
approaches.

### State

`stator_position_mm` reads 10.6090 mm, 0.41768 inch, against the datum measured
by hand this morning. `loop_time_max` 44 us against the 60 us limit, no
overruns, no new dropped records.

Still outstanding: **the barrel has not been read by eye since homing.** That is
the one check that does not go through the same sensor, and it is what would
catch the datum being self-consistently wrong.

## DAC output path checked, `drive` wired and calibrated, exciter unpowered (2026-08-18)

David wired the DAC outputs to the exciter input, and looped that input back to
ADC channel 1 (`drive`), closing the two outstanding items from the 2026-08-12
`coil`/`drive` bring-up entry. The exciter itself was unpowered throughout, so
the full commanded range, including deliberately over-range commands, was safe
to exercise. Firmware `0.1.0 346a445`, script and figure at
`data/dac-check-2026-08-18/`.

### Method

A persistent Python session (`helic_daq.Device`), not the one-shot CLI: `helic-daq set arm 1`
refuses on principle (`the one-shot CLI cannot keep the output armed`), because
arming does not survive a dropped control connection and the CLI closes its
connection after every invocation. Worth remembering for any future scripted
check that needs the output live for more than one request.

With the rig armed, `forcing_coeffs`' DC term was swept over
-1.9, -1.5, -1.0, -0.5, -0.2, 0, 0.2, 0.5, 1.0, 1.5, 1.9 V, plus ±5 V
deliberately outside the software clamp, each level held 0.1 s to settle then
captured for 0.2 s (`coil`, `drive`, `out`) at the full 8 kHz rate. Forcing and
target were zeroed and the rig disarmed again afterwards.

### Results

**`out` tracks the commanded value exactly** up to the clamp, and the clamp
lands exactly on `DAC_OUT_CEILING_V − MID_RAIL` and `MID_RAIL − DAC_OUT_FLOOR_V`,
±1.952 V, with the `clamped` safety bit set only on the two over-range points.
The safety gate is doing precisely what `src/config.rs` says it does.

**`drive` = 2.0000 × `out` − 0.0002 V**, fit over the eleven in-range points,
residual ≤36 µV. That is the calibration ratio the 2026-08-12 entry asked for,
and it is almost exactly the topology's nominal doubling from driving A and C
symmetrically about `MID_RAIL` (`MID_RAIL + out` / `MID_RAIL − out`). A stuck C
channel would have given a ratio near 1, not 2, so this is also the first
evidence beyond timing that the symmetric A/C drive is real, though it is
still an ADC measurement rather than an independent scope trace on A and C.

**The pair never approached the AD7609's ±10 V differential range**: ±3.9 V
at full DAC swing, so item 1 of the 2026-08-12 to-do list (check the exciter
input's levels before connecting) is closed too, at least for the unpowered
case.

**`coil` stayed at its pre-existing noise floor throughout**, a band of about
40 µV, roughly an order of magnitude below `drive`'s residual and far below
its step size: no measurable crosstalk from the drive path onto the sense
coil.

### What this does and does not establish

The exciter was unpowered for all of it. This is good evidence for the DAC,
the cape, and the loopback wiring, but not that the 2.0000 ratio holds once
the exciter's current controller is powered: `drive` taps that controller's
input rather than the DAC pins directly (see the standing constraint above),
and if that input stage buffers actively rather than passively, its gain could
change with power applied. **Recalibrate once the exciter is live**, ideally
cross-checked against a scope on channels A and C, before trusting `drive`
quantitatively during a real experiment.

### A safety-bitfield reading worth remembering

`clamped` and `quieted` in the `safety` word are cumulative since the last
`diag_reset`, not instantaneous: the `clamped` bit stayed set after the output
was zeroed and the rig disarmed again, because the ±5 V over-range commands
had clamped earlier in the same session. Matches `RtShared`'s implementation
(`safety_clamp_ticks`/`safety_quiet_ticks` counters) rather than being a
surprise, but it is easy to misread a stale bit as a live condition
mid-session; `diag_reset` clears it.

## `out` redefined as the differential DAC command, platform repinned to v0.2.5, reflashed and revalidated (2026-08-18)

Following on from the check above: David asked for `out = drive` rather than
`drive = 2*out`, and to repin the platform first. Both landed in one flash,
firmware `0.1.0 a1e45da`.

### Platform repin, v0.2.4 → v0.2.5

Patch bump, no public Rust API or wire-packet layout change per the tag
message. Directly closes the gap this file raised on 2026-08-17
(`helic-daq#3`): the compact firmware identity now appends `+` when tracked
content differs from HEAD and `?` when cleanliness can't be established,
instead of silently naming a commit the image wasn't built from. Embassy
versions in the platform's `firmware/Cargo.toml` at the new tag already
matched ours, so no transitive bump was needed. Moved in all three places
(`Cargo.toml`, `.github/workflows/ci.yml`, `README.md`) per `AGENTS.md`, and
`Cargo.lock` updated. Static gates pass unchanged: `cargo fmt`, clippy, both
board builds, `helic-deps-check`, `helic-rt-layout`.

### `out` = A − C directly

`src/rig.rs`'s `actuate` now writes `MID_RAIL + out/2` to A and
`MID_RAIL - out/2` to C, instead of `MID_RAIL + out` / `MID_RAIL - out`.
`clamp_output` matches: it clamps `out/2` against `DAC_OUT_FLOOR_V`/
`DAC_OUT_CEILING_V` and doubles the clamped half back into `out`'s own units.
Net effect: `out`'s achievable range doubled in its own units, ±1.952 V to
±3.904 V, for an unchanged physical DAC rail, and the differential drive
equals `out` directly rather than `2 * out`. See the updated "Analogue output
stages" and `drive` calibration entries in the standing constraints above.
Static gates pass: `cargo fmt`, clippy, both board builds,
`helic-deps-check`, `helic-rt-layout` (the tick-path SRAM-residency check,
relevant since `actuate` and `clamp_output` both changed).

Committed before flashing, as always, so the wire identity is trustworthy —
doubly worth doing given this flash is also what first exercises the v0.2.5
dirty-marker fix.

### Reflash and hardware verification

Flashed via `probe-rs run`; came up as `0.1.0 a1e45da`, matching HEAD exactly
with no `+` suffix, confirming the tree was clean at build time (the first
real-world exercise of the v0.2.5 identity fix). `helic-rt-regression` passed
every phase with zero overruns, tick timeouts, dropped records, lost packets
or index gaps, `loop_time_max` 45 µs against the 60 µs limit — unchanged from
before this session's firmware changes, as expected: `actuate` and
`clamp_output` only gained an `f32` multiply each.

Reran the DAC sweep from earlier today (`data/dac-check-2026-08-18/
dac_check.py`, rerun writes `dac_check_2026-08-18b.npz`/`.png`), exciter still
unpowered, over -3.8 V to +3.8 V plus a deliberate ±10 V over-range pair:

- **`drive` = 1.0000 × `out` − 0.0002 V**, residual ≤32 µV against that fit —
  `out` and `drive` now agree to five figures, which is what was asked for.
  This supersedes the 2.0000× figure from the pre-`a1e45da` mapping measured
  earlier the same day; neither is wrong, they describe different firmware.
- The clamp now lands at exactly **±3.904 V** on `out` (was ±1.952 V), as
  `DAC_OUT_CEILING_V`/`FLOOR_V`'s doc comments predict, with the `clamped`
  safety bit set only on the two over-range points.
- `coil` stayed within its usual ~40 µV noise floor throughout: no crosstalk
  introduced by the remapping.

Same caveat as before carries over unchanged: **the exciter was unpowered**
throughout, so the 1.0000× ratio is provisional until repeated live, ideally
against a scope on channels A and C directly.

### A live operational trap this creates, worth flagging now

**`out`'s numeric meaning changed.** A `forcing_coeffs`/`target_coeffs` value
that used to drive a channel close to its rail now only drives it to half
that, and conversely a value chosen under the new convention would have
undershot the old one by half. Nothing in this repository depends on `out`'s
absolute scale today (`ActiveController` is `PassThrough`, and both `notes.md`
sweeps were commanded explicitly), so the blast radius is contained, but any
future controller gain, saved waveform, or mental "V equals volts on the
channel" habit carried over from before 2026-08-18 will be wrong by a factor
of two. Worth a line in `AGENTS.md` if this rig starts seeing tuned
controller gains that depend on `out`'s scale.

## Stator soft lower limit widened, rate cap now enforced in firmware (2026-08-18)

David asked for `STATOR_TRAVEL_MIN_MM = 6.35 mm` (was 3.175 mm) and
`MAX_STATOR_RATE_MM_S = 0.5 mm/s` (was 3.0). Both are hardware-decision
constants per `AGENTS.md`, so recorded here.

**`STATOR_TRAVEL_MIN_MM`: one barrel turn clear of the lower hard stop to
two.** Lower run-out goes from 0.635 mm to 3.81 mm; the datum sits at 4.26 mm
above the new limit and 7.42 mm below `STATOR_TRAVEL_MAX_MM`, so it is now
closer to the lower limit than the upper, where it used to sit within two
full steps of the midpoint. The usable travel below the datum shrinks by the
same 3.175 mm the run-out gained. `docs/stator-stage.md`'s travel table and
homing safety-argument figures updated to match.

**`MAX_STATOR_RATE_MM_S`: 3.0 → 0.5 mm/s.** This had been an operator
instruction only (see the standing constraint above and the 2026-08-17
lost-step measurements: 1.5 mm/s lost 17 steps in one cycle of three, 3.0
lost 16 every cycle, silently); the accepted parameter range now matches the
only rate measured clean. Verified live: `helic-daq set rig_stator_rate 1.0`
now returns `bad value`, `rig_stator_rate` unchanged at 0.5; setting 0.5
succeeds.

Firmware `0.1.0 80a222c`, flashed clean (no `+` suffix). `helic-rt-regression`
passed with an empty `acceptance_errors` list, unchanged from before this
change. Neither constant is on the tick path, so no timing effect was
expected or seen.

Not exercised: an actual jog against the new `STATOR_TRAVEL_MIN_MM`, since
that requires homing first (the soft window is enforced only once homed) and
homing was out of scope for this change. Worth doing before the lower limit
is trusted operationally, alongside the barrel-by-eye check still outstanding
from the homing commissioning entry above.

## 2026-08-19T11:38+00:00 Selectable control implemented, hardware enablement blocked on evidence

- The firmware now has run-time `None`, PID, and PLL modes through the
  platform's breaking `StandardControl` API. The existing platform `Pll` was
  revised in place. Host tests cover composition, typed command routing,
  frequency-window invariants, mode-change races, phase convention, DC
  rejection, warm-up, stationarity, anti-windup, delay wrapping, and latched
  lock loss.
- Unused ADC channels 2 to 7 are no longer streamed. The hardware still
  acquires the complete AD7609 frame; the compact input order is now `coil`,
  `drive`, `laser`, `stator`. This leaves six stream slots free, including one
  for a measured exciter-current or force channel when it is physically wired.
- **PID output is deliberately inhibited pending measurement.** This file has
  no evidence for a quiescent laser peak-to-peak limit or an acceptable error
  between target mean and resting laser mean. `PID_ENTRY_LIMITS_VERIFIED`
  therefore remains false, and PID entry faults after its 0.25 s observation
  window rather than energising with invented limits. Measure both quantities,
  choose conservative `PID_ENTRY_QUIET_MM` and `PID_ENTRY_ERROR_MAX_MM`, and
  record the data here before setting the flag true.
- **PLL defaults are deliberately inert:** zero proportional and integral
  gains, and a zero-width frequency window. `drive` is currently the phase
  detector's excitation input, but its 1.0000× calibration was measured with
  the exciter unpowered and it is not measured force. Before scientific PLL
  use, measure the powered drive-to-force/current phase transfer, fit
  `pll_delay_s`, establish the open-loop phase-frequency slope and safe
  frequency window, and then tune the loop below both demodulator and plant
  settling bandwidths.
- The release ELF initially exposed a flash-resident `libm::sqrtf` call from
  the PLL telemetry path. It was replaced with a tested SRAM-resident `f32`
  square root; the named hot symbols now lie in the executable SRAM section.
- Software completion is against a temporary sibling-checkout Cargo patch to
  unreleased platform 0.3.0. Remove it only after all platform crates, CI gate,
  and README installation command can be repinned together to the release tag.
  No hardware timing, electrical, PID, or PLL acceptance is claimed by this
  entry.
