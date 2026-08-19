# Selectable control: implemented contract

The firmware exposes one run-time `control_mode`:

| Value | Mode | Raw programme output | Frequency |
|---:|---|---|---|
| 0 | none | `forcing + table` | nominal `freq` |
| 1 | PID | `PID(target - laser) + forcing + table` | nominal `freq` |
| 2 | PLL | `forcing` | bounded PLL command |

`target` is millimetres, while `forcing`, `table`, and `out` are volts. In PLL
mode the table player continues to advance and its candidate remains in the
capture, but it is not included in `out`. Generator phase also continues while
disarmed. A PLL increment calculated in record (n) first generates record
(n+1); `pll_freq_actual` describes record (n).

## Inputs and signals

The compact measured-input order is `coil`, `drive`, `laser`, and `stator`.
All eight AD7609 channels are still converted synchronously, but unused
channels 2 to 7 are not copied into the stream. The PLL currently treats
`drive` as excitation and `laser` as response. `drive` is a command-path
loopback, not measured force.

The fixed control signals precede the standard `target`, `forcing`, `table`,
and `phase` signals:

| Signal | Unit | Meaning |
|---|---|---|
| `control_mode` | enum | 0 none, 1 PID, 2 PLL |
| `pid_error` | mm | target minus laser, NaN outside PID |
| `pll_phase` | degree | corrected response minus excitation |
| `pll_phase_error` | degree | measured phase minus target |
| `pll_exc_amp` | V | demodulated excitation fundamental |
| `pll_resp_amp` | mm | demodulated response fundamental |
| `pll_freq_actual` | Hz | frequency used by the current record |
| `pll_state` | enum | 0 fixed, 1 acquiring, 2 locked, 3 lock lost |

## Parameters

`control_mode`, `ctrl_reset`, and `pll_reacquire` are `u32`; the remaining
control parameters are `f32`:

- PID: `pid_kp`, `pid_ki`, `pid_kd`, and `pid_tau_d`;
- PLL frequency and gains: `pll_centre_freq`, `pll_freq_min`,
  `pll_freq_max`, `pll_kp`, and `pll_ki`;
- phase detector: `pll_target_phase`, `pll_delay_s`, `pll_dc_tau`,
  `pll_demod_tau`, `pll_excitation_min`, and `pll_response_min`; and
- lock qualification: `pll_lock_phase_tol`, `pll_unlock_phase_tol`,
  `pll_lock_freq_tol`, `pll_lock_dwell`, `pll_unlock_dwell`,
  `pll_acquire_timeout`, and `pll_saturation_dwell`.

Frequency parameters and gains are host-visible in Hz, Hz/degree, and
Hz/(degree second), then converted once on core 0. Every scalar write must
preserve `0 <= min <= centre <= max < sample_freq/2`. To move a window, widen
the maximum first, move the centre, move the minimum, and finally narrow the
maximum.

Mode changes are rejected synchronously while armed. The real-time side also
faults an arm/write race. Every accepted disarmed mode change resets both
algorithms. `ctrl_reset` is a full reset; `pll_reacquire` retains the bounded
frequency and useful demodulator state, but cannot clear latched `LockLost`.

## Commissioning blockers

PID entry observes the resting laser for 0.25 s, requires a quiet peak-to-peak
window and agreement between laser mean and target mean, and holds raw output
at zero throughout. There is no measurement in `notes.md` from which to set
those two tolerances. Consequently `PID_ENTRY_LIMITS_VERIFIED` is false and
PID entry deliberately faults. Measure and record conservative values before
enabling it in `src/control.rs`.

PLL defaults have zero gains and a zero-width frequency window. Before changing
them, measure the powered excitation-reference transfer, establish whether
`drive` can be corrected to applied force or add a measured current/force
input, fit `pll_delay_s`, and measure the open-loop phase-frequency slope. Tune
the PLL well below the demodulator pole and mechanical settling rate. `LockLost`
trips the output and requires full reset, diagnosis, and explicit re-arm.

Neither host tests nor a successful release build establish electrical sign,
stability, phase accuracy, or the 8 kHz timing margin. Follow the staged
hardware sequence in `selectable-control-and-pll.md`, and append the evidence
to `notes.md`.
