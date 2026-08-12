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

Two things to do when the wiring goes in:

1. Check the exciter input's levels before connecting. The pair should sit
   within the AD7609's ±10 V differential range at DAC levels, but if the cape
   has voltage gain ahead of the exciter it might not, and that is worth
   confirming rather than assuming.
2. Calibrate `drive` against `out` once, with a scope, and record the ratio in
   the standing constraints above. Until that is done `drive` shows only that
   the output moves. Doing this with the laser powered would also close the
   outstanding symmetric-drive question in the entry above, since a correct
   A/C pair gives `drive` twice the amplitude a stuck C would.
