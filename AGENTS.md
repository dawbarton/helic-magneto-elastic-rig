# Magneto-elastic rig

HELIC-DAQ firmware for the magneto-elastic rig, maintained separately from the
[HELIC-DAQ platform](https://github.com/dawbarton/helic-daq).

## Read before changing code

- `README.md`: hardware, safety limits, and the commands for building and
  checking.
- `notes.md`: hardware verification status and bring-up constraints. Read and
  update it when doing hardware work.
- `docs/stator-stage.md`: the stator axis in full. Before touching the axis
  read "The stator stage: units first" below, which is short and is the part
  that causes silent errors.
- The platform's `docs/developer_guide.md`, section "Adding a rig in its own
  repository", is the contract this repository implements; its "Extending",
  "Timing" and "Output safety gate" sections govern the code here.
- The platform's `docs/protocol.md` is the authoritative wire protocol.

## What belongs here, and what does not

This repository holds only what is specific to this rig: the pin map, the
analogue acquisition and actuation semantics, compile-time configuration
including the safety limits, telemetry declarations, and the verification
contracts.

Do not add general-purpose mechanisms here. An algorithm with a second real
consumer, a portable device driver, a cross-core contract or a core-0 service
belongs in the platform, as a pull request against it, not copied into this
repository. If a platform change is needed urgently, a temporary
`[patch.crates-io]` is the honest mechanism, but record why and remove it once
the change lands upstream: a patched fork that outlives its reason is how this
arrangement silently diverges from the platform.

- `src/board.rs` owns only pins and unassembled peripheral parts.
- `src/config.rs` owns compile-time choices, including `SAMPLE_RATE`,
  `HARMONICS`, `TABLE_CAPACITY`, the output safety limits, the network
  configuration and the active controller and programme.
- `src/rig.rs` assembles core-1 hardware and implements `Rig`.
- `src/telemetry.rs` declares atomic-backed read-only values.
- `src/main.rs` binds interrupts, assigns cores, and composes the platform's
  runners. It is auditable glue, not logic.

## Safety-critical constraints

This rig sets `SAFETY_GATED = true` and drives an exciter that can be commanded
into an unstable feedback loop. Treat the following as evidence-backed
constants rather than tuning knobs:

- `DAC_POLARITY` in `src/rig.rs` must match the fitted output stages. A
  mismatch drives the exciter with the wrong sign or scale and is not visible
  in software checks.
- `DAC_OUT_FLOOR_V`/`DAC_OUT_CEILING_V` bound the driven channel about
  `MID_RAIL`; `DISPLACEMENT_MIN_MM`/`DISPLACEMENT_MAX_MM` bound the tip travel;
  `LASER_STALE_AFTER_S` is the blind-feedback guard. Changing any of them is a
  hardware decision, recorded in `notes.md`.
- The `arm` parameter, and only the host, arms the gate. Firmware may latch a
  trip but never clear one.

## The stator stage: units first

`docs/stator-stage.md` is the full description and `notes.md` holds the
evidence. This section is the part that bites.

### Inches on the instrument, millimetres in the software

**The micrometer is imperial. Everything in the firmware, the wire protocol and
the host tooling is metric.** The conversion is exact in both directions, so
there is never a rounding argument, only a units one:

| Quantity | Millimetres | Inches |
|---|---|---|
| One full step | 0.003175 | 0.000125 exactly |
| One barrel turn, 200 full steps | 0.635 exactly | 0.025, that is 1/40 |
| The datum | 10.609 | 0.4176875 |
| Soft travel window | 3.175 to 18.034 | 0.125 to 0.710 |

**The trap is writing a barrel reading straight into a millimetre parameter.**
Every stator parameter that carries a position, `rig_stator_target`,
`rig_stator_jog` and `rig_stator_datum`, is in **millimetres**. A barrel reading
of 0.418 written to `rig_stator_datum` is not a small error, it is a factor of
25.4, and it is silent: 0.418 is an entirely plausible-looking millimetre value,
it is inside the parameter's accepted range, and nothing downstream can tell it
from a real one. Multiply by 25.4 on the way in, every time, and say which unit
you mean in any note, commit message or comment.

The same applies in reverse when reporting. Prefer quoting both, as the table
above does, whenever a number is going to be read next to a barrel.

Step counts are a third unit and are **full steps of 3.175 µm**, not
microsteps, because MS1/MS2 are unstrapped and `STATOR_MICROSTEPS` is 1.0. The
firmware's identifiers say "microstep" throughout, which stays correct if the
carrier is ever strapped; recorded evidence must say full steps, or it silently
changes meaning by a factor of eight on the day that happens.

### Using the axis

- **Home once per session, then trust the counter.** The datum repeats to
  better than one full step, and re-homing costs travel and time without
  buying accuracy. Write any non-zero value to `rig_stator_home`.
- **Prove the sensor changes state before the first home of a session.** Jog
  across the datum and watch `stator_opto` go 1 to 0 and back. A sensor stuck
  reading "below the datum" is the one failure that can drive this axis into a
  hard stop, and this check takes a minute.
  `data/stator-2026-08-17/commission_home.py --check` does it.
- **After a reflash the counter is zero and means nothing.** It is not a
  position until the axis is homed, however plausible it looks, and this has
  already caused one false fault report. The same applies to anything moved by
  hand.
- **Never raise `rig_stator_rate` above 0.5 mm/s.** Measured: 1.5 mm/s lost 17
  steps in one cycle of three and 3.0 mm/s lost 16 every cycle, silently, with
  no fault raised. The cause is the absence of an acceleration ramp, so the fix
  is a ramp, not a smaller increase.
- **The upper limit has almost no run-out.** 0.13 to 0.25 mm to the hard stop,
  against 0.635 mm at the lower end. Treat the top of the window with more care
  than the bottom.
- **Only advancing moves position the stage**, so every move ends with an
  advance and a target within `rig_stator_backlash` of the lower limit is
  refused rather than clamped.
- **The soft window is enforced only once homed.** Before that a single jog is
  bounded but repeated jogs walk anywhere, so the travel limits are the
  operator's responsibility until a home has run.
- **Do not step during a capture.** The `stator` sample source makes any
  violation visible after the fact: a capture whose `stator` column is constant
  is quiescent by inspection.

### Checking a session's work

`stator_home_error` is the lost-step audit: re-home at the end of a session and
it reports, in full steps, how far the datum turned out to be from where the
counter predicted. Zero is the expected answer and what ten consecutive homes
gave on 2026-08-17. Anything else means steps were lost, and the first thing to
suspect is a rate that was raised.

## Platform constraints that still apply

- Keep the core-1 tick path SRAM-resident and Embassy-free. Anything reachable
  per tick must carry `#[unsafe(link_section = ".data.ram_func")]`, or inline
  into something that does, and must not call `embassy-time`, async GPIO/SPI,
  `defmt`, or anything taking a critical section. `helic-rt-layout` is the
  gate, and it is a minimum named-symbol guard rather than a proof: inspect new
  compiler-generated calls after material tick-path changes.
- Keep the tick source's latch continuously armed. Re-arming per wait loses
  edges that arrive while a tick body runs.
- No allocation, blocking cross-core locks or `f64` on the real-time path.
- Parameters and stream sources are discovered by name. Never hard-code
  registry or source indices in host code.
- Update `rig-profile.toml` whenever the rig or its hot-path boundary changes.

## Upgrading the platform

The platform crates and the verification tools are pinned to one exact tag, in
`Cargo.toml` and `.github/workflows/ci.yml` respectively. Pin a tag and never a
branch: a git dependency cannot express a version range, and a moving pin
removes the isolation this repository exists to provide. Upgrade both together,
because a gate from a different platform version checks the wrong contract.

The platform is at `0.x`, where the minor position is the breaking one:

| Bump | What to expect |
|---|---|
| Patch, `0.1.1` to `0.1.2` | No crate API change. Repin, run the full check set, done. |
| Minor, `0.1.x` to `0.2.0` | Breaking. Expect to change code here. |

Breaking means a changed signature, a new trait method without a default, a
wire-visible name or semantic change, or a changed platform capacity. Read the
tag message before every upgrade regardless: it records what changed for
consumers. Also re-check the Embassy versions against the platform's
`firmware/Cargo.toml` at the new tag, since a mismatch presents as type errors
on identically named types rather than as a version conflict.

This rig's version is independent of the platform's. Do not sync them. Build
identity is rig-owned: the firmware reports this package's version with this
repository's revision, and the platform version separately, which is what makes
a flashed image identifiable.

The weekly `platform-drift` CI job builds against the platform's `main`. A
failure there is advance warning about the next upgrade, not a defect in the
pinned build.

## Working conventions

- Use British English in prose with Oxford commas.
- Give every new source or configuration file a file-level comment describing
  its purpose.
- Comment non-obvious timing, safety, or hardware constraints, not the obvious.
- Keep commits to one logical unit, in the `<Area>: <what and why>` style.
- Communicate with real DAQ hardware sequentially; the control server is
  single-client.
- Diagnostic (`diag-*`) builds are not production images. Never leave one
  flashed on the rig, and never record evidence from one as acceptance.

Before declaring a change complete, run the check set in `README.md`. Software
checks do not establish real-time, electrical or throughput behaviour; record
hardware evidence in `notes.md`.
