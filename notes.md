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
  `DAC_POLARITY` in `src/rig.rs` is set to match. Channel A drives the
  exciter's positive differential input and channel C holds the negative
  reference at the 2.048 V common mode; channel B is broken and channel D is
  unused, so both rest at 0 V. Output routing is fixed to channel A and
  `rig_out_channel` will reject any other value.
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

The laser was switched off partway through this session, so
`laser_frames_received` stayed at 0 and `safety` read 10: the gate had latched
a trip and was quieting the actuator, which is the designed response to a blind
feedback path. The figures above are therefore evidence about the acquisition,
timing and communications paths only, and the laser path is neither confirmed
nor called into question by them.
