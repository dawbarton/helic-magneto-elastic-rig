# Magneto-elastic rig

HELIC-DAQ firmware for the magneto-elastic rig, maintained separately from the
[HELIC-DAQ platform](https://github.com/dawbarton/helic-daq).

## Read before changing code

- `README.md`: hardware, safety limits, and the commands for building and
  checking.
- `notes.md`: hardware verification status and bring-up constraints. Read and
  update it when doing hardware work.
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
