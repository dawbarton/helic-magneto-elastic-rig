//! Acquisition, actuation, parameters, and core-1 hardware assembly.

use core::cell::RefCell;
use core::sync::atomic::Ordering;

use defmt::warn;
use embassy_embedded_hal::shared_bus::blocking::spi::SpiDeviceWithConfig;
use embassy_rp::gpio::Output;
use embassy_rp::peripherals::{PIN_8, PWM_SLICE4, SPI1};
use embassy_rp::pwm::{self, Pwm, Slice};
use embassy_rp::spi::{self, Blocking, Spi};
use embassy_rp::{pac, Peri};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use fixed::traits::ToFixed;
use helic_core::safety::{clamp_channel_command, StaleCounter};
#[cfg(not(feature = "diag-skip-dac"))]
use helic_drivers::ad5064::WORD_SETTLE_US;
use helic_drivers::ad5064::{Ad5064, ChannelPolarity};
use helic_drivers::ad7609::Ad7609;
use helic_fw_rt::analog_spi::{HotSpiConfig, RawSpiDevice, SramAd5064};
use helic_fw_rt::rig::BusyEdgeSpinTick;
use helic_rt::{Rig, SampleRate};
use static_cell::StaticCell;

use crate::board::MagnetoelasticParts;
// Rig-owned compile-time constants. Those whose names would otherwise collide
// with the atomics of the same meaning in `telemetry` are aliased, following
// the existing `LASER_RANGE_MM` precedent: the constant is the default, the
// atomic is the live value.
use crate::config::{
    DAC_OUT_CEILING_V, DAC_OUT_FLOOR_V, DISPLACEMENT_MAX_MM, DISPLACEMENT_MIN_MM,
    LASER_RANGE_MM as DEFAULT_LASER_RANGE_MM, LASER_STALE_AFTER_S, MAX_STATOR_BACKLASH_MM,
    MAX_STATOR_RATE_MM_S, OUTPUT_CHANNEL, STATOR_BACKLASH_MM as DEFAULT_STATOR_BACKLASH_MM,
    STATOR_DATUM_MM as DEFAULT_STATOR_DATUM_MM, STATOR_HOLD_DEFAULT,
    STATOR_RATE_MM_S as DEFAULT_STATOR_RATE_MM_S, STATOR_SEEK_MAX_MM,
};
use crate::stator::{issue_command, CMD_HOME, CMD_JOG, CMD_MOVE, CMD_STOP};
use crate::telemetry::{
    LASER_FRAMES_RECEIVED, LASER_RANGE_MM, LASER_VALUE, STATOR_BACKLASH_MM, STATOR_DATUM_MM,
    STATOR_HOLD, STATOR_JOG_MM, STATOR_POSITION_MM, STATOR_RATE_MM_S, STATOR_TARGET_MM,
};

/// Index of the laser distance within the measured input vector, after the
/// eight ADC channels. Mirrors the `INPUTS` order and `measure`'s `values[8]`.
const LASER_INPUT: usize = 8;

/// Index of the stator position, published by the core-0 stepper task.
const STATOR_INPUT: usize = 9;

/// DAC reference voltage fitted to the interim analogue board.
pub const DAC_VREF: f32 = 4.096;

/// Common-mode voltage for the exciter's differential current-controller
/// input. Both channels rest here so that a logical output of zero produces
/// zero differential drive. Exact half-scale of the unipolar DAC (2.048 V).
pub const MID_RAIL: f32 = DAC_VREF / 2.0;

/// DAC channel wired to the negative differential input of the exciter
/// current controller (AD5064 channel C). Driven symmetrically with the
/// positive channel (A) every tick: `MID_RAIL - out/2` against A's
/// `MID_RAIL + out/2`, so the differential swing (A - C) equals `out`
/// directly, matching what the `drive` loopback measures. Each channel still
/// uses the full 0-4.096 V unipolar DAC rail, so `out`'s achievable range is
/// twice what driving A alone against a fixed reference would give; only the
/// mapping from `out` to each channel changed on 2026-08-18 (previously
/// `MID_RAIL + out` / `MID_RAIL - out`, a differential of `2 * out`), not the
/// physical range.
pub const NEG_REF_CHANNEL: usize = 2;

// Raw chip-select access is the one place pin identity cannot be recovered
// from Embassy's erased Output. Keep these beside the unsafe construction and
// in lockstep with board.rs's auditable pin map.
const ADC_CS_PIN: u8 = 13;
const DAC_CS_PIN: u8 = 9;

/// Output-stage polarity per DAC channel. The fitted interim board has four
/// unipolar outputs; this must change with the physical output stages.
pub const DAC_POLARITY: [ChannelPolarity; 4] = [
    ChannelPolarity::Unipolar,
    ChannelPolarity::Unipolar,
    ChannelPolarity::Unipolar,
    ChannelPolarity::Unipolar,
];

/// Busy-wait for the AD5064's inter-word settling time
/// ([`WORD_SETTLE_US`]) between the two channel writes `actuate` issues per
/// tick (see the timing note in the helic-drivers ad5064 module; a
/// single-channel-per-tick write was previously spaced by the tick period
/// itself, which no longer holds once two words are issued per tick). Reads
/// the RP2350's always-on, free-running microsecond timer directly, rather
/// than a flash-resident delay routine, so the wait stays exact regardless
/// of any XIP cache stall elsewhere in the tick (see analog_spi.rs's module
/// note on why the hot path avoids flash).
#[cfg(not(feature = "diag-skip-dac"))]
#[inline(always)]
#[unsafe(link_section = ".data.ram_func")]
fn wait_word_settle() {
    let start = pac::TIMER0.timerawl().read();
    while pac::TIMER0.timerawl().read().wrapping_sub(start) < WORD_SETTLE_US {}
}

type SpiBus = Mutex<NoopRawMutex, RefCell<Spi<'static, SPI1, Blocking>>>;
type SpiDevice =
    SpiDeviceWithConfig<'static, NoopRawMutex, Spi<'static, SPI1, Blocking>, Output<'static>>;
type Adc = Ad7609<SpiDevice, Output<'static>>;
type Dac = Ad5064<SpiDevice>;

// Both devices borrow the bus for the firmware lifetime. The NoopRawMutex is
// sound because MagnetoelasticParts is assembled only after it has moved to core 1.
static SPI_BUS: StaticCell<SpiBus> = StaticCell::new();

pub type Tick = BusyEdgeSpinTick;

/// Rig-specific mutable state reached by the generic bounded RT pipeline.
pub struct MagnetoelasticRig {
    adc: Adc,
    dac: Dac,
    tick_pin: Output<'static>,
    adc_raw: RawSpiDevice,
    dac_raw: SramAd5064,
    convst: Option<(Peri<'static, PWM_SLICE4>, Peri<'static, PIN_8>)>,
    convst_pwm: Option<Pwm<'static>>,
    pwm_slice: usize,
    pwm_divider: u32,
    sample_rate: SampleRate,
    adc_scale: f32,
    adc_last: [i32; 8],
    output_channel: usize,
    /// Blind-feedback guard: flags the laser feed as stale when its frame
    /// counter stops advancing (see `output_fault`).
    laser_stale: StaleCounter,
}

impl MagnetoelasticParts {
    /// Assemble the core-1-only shared bus and its typed device drivers.
    pub fn build(self, sample_rate: SampleRate) -> (MagnetoelasticRig, Tick) {
        let tick = BusyEdgeSpinTick::new(self.adc_busy, sample_rate);
        let bus: &'static SpiBus = SPI_BUS.init(Mutex::new(RefCell::new(self.spi)));

        // AD7609 mode 2 at 12 MHz transfers an 18-byte frame in about 12 µs.
        let adc_config = HotSpiConfig::new(
            12_000_000,
            spi::Polarity::IdleHigh,
            spi::Phase::CaptureOnFirstTransition,
        );
        let adc_spi = SpiDeviceWithConfig::new(bus, self.adc_cs, adc_config.embassy());

        // AD5064 mode 1 at 16 MHz transfers one 32-bit word in 2 µs.
        let dac_config = HotSpiConfig::new(
            16_000_000,
            spi::Polarity::IdleLow,
            spi::Phase::CaptureOnSecondTransition,
        );
        let dac_spi = SpiDeviceWithConfig::new(bus, self.dac_cs, dac_config.embassy());

        // SAFETY: board.rs configures GP13 and GP9 as the live chip selects for
        // these SPI1 devices. The whole bundle moves to core 1, and no other
        // task accesses SPI1 or either CS output.
        let adc_raw = unsafe { RawSpiDevice::new(pac::SPI1, adc_config, ADC_CS_PIN) };
        let dac_raw = unsafe { RawSpiDevice::new(pac::SPI1, dac_config, DAC_CS_PIN) };

        // LDAC is a hardware strap in this operating mode. Keeping the Output
        // alive prevents its drop implementation from deconfiguring the pin.
        core::mem::forget(self.dac_ldac);

        let rig = MagnetoelasticRig {
            adc: Ad7609::new(adc_spi, self.adc_pins),
            dac: Ad5064::new(dac_spi, DAC_POLARITY, DAC_VREF),
            tick_pin: self.tick_pin,
            adc_raw,
            dac_raw: SramAd5064::new(dac_raw, DAC_POLARITY, DAC_VREF),
            pwm_slice: self.convst_slice.number(),
            convst: Some((self.convst_slice, self.convst_pin)),
            convst_pwm: None,
            pwm_divider: sample_rate.pwm_params().0 as u32,
            sample_rate,
            adc_scale: 0.0,
            adc_last: [0; 8],
            output_channel: OUTPUT_CHANNEL,
            // Tolerate this many unchanged laser frames before quieting. At
            // 8 kHz the laser publishes ~one frame per tick, so the window is
            // a small multiple of the frame interval.
            laser_stale: StaleCounter::new((LASER_STALE_AFTER_S * sample_rate.hz()) as u32),
        };
        (rig, tick)
    }
}

impl Rig for MagnetoelasticRig {
    // `measure` fills this exact order. The common loop appends controller
    // telemetry and generated signals without experiment-specific indices.
    //
    // AD7609 channels 0 and 1 carry named physical measurements; 2-7 are
    // spare and keep generic names. Both named channels are differential
    // pairs, as the AD7609's inputs are true-bipolar differential:
    //
    // - `coil`: voltage across a sense coil wound around the stator. This is
    //   the specimen-side measurement, not the actuator drive.
    // - `drive`: the exciter current controller's own differential input,
    //   which is the DAC output after whatever the analogue cape does to it.
    //   Since 2026-08-18 `out` is defined as the differential command, so
    //   `drive` is nominally `out`, but it is still NOT a priori exactly
    //   equal: any cape buffer gain or attenuation sits inside this reading,
    //   so the volts-per-volt factor against `out` has to be measured and
    //   recorded in `notes.md` before this source is used quantitatively.
    //   Its purpose is to make the output path observable in software at
    //   all, which it was not once channel 0 moved to the coil.
    const INPUTS: &'static [(&'static str, &'static str)] = &[
        ("coil", "V"),
        ("drive", "V"),
        ("adc2", "V"),
        ("adc3", "V"),
        ("adc4", "V"),
        ("adc5", "V"),
        ("adc6", "V"),
        ("adc7", "V"),
        ("laser", "mm"),
        // Stator position as a micrometer barrel reading, published by the
        // core-0 stepper task, so every capture records the gap it was taken
        // at. Reads NaN until the axis is homed, or after a move was abandoned
        // part-way through a retract; see `stator.rs`. One relaxed atomic load
        // per tick is the whole of the stage's cost to the real-time path.
        ("stator", "mm"),
    ];
    const ACTUATORS: &'static [(&'static str, &'static str)] = &[("out", "V")];

    // The exciter is driven through a feedback path that can go unstable, so
    // this rig opts into the shared per-tick output safety gate.
    const SAFETY_GATED: bool = true;

    fn init(&mut self) {
        // Slow reset delays and fail-safe zeroing happen before the sample
        // clock starts, never on the bounded per-tick path.
        self.adc.init(
            helic_drivers::ad7609::InputRange::Bipolar10V,
            helic_drivers::ad7609::Oversampling::for_sample_rate(self.sample_rate.hz()),
            &mut embassy_time::Delay,
        );
        self.adc_scale = self.adc.scale();
        // Define every DAC output before the RT loop starts, spacing the writes
        // for the AD5064's inter-word settling time (see the timing note in the
        // helic-drivers ad5064 module). Channels A and C (the exciter's
        // differential inputs) both rest at the common-mode reference so the
        // differential drive (A - C) is zero until the first `actuate`; unused B
        // and D rest at 0 V. C is written before A so the driven channel settles
        // to match the reference last. Output routing is locked to A, so the
        // remaining unused channels are the fixed indices 1 (B) and 3 (D).
        let startup_setpoints = [
            (NEG_REF_CHANNEL, MID_RAIL),     // C: negative differential channel
            (self.output_channel, MID_RAIL), // A: positive differential channel
            (1, 0.0),                        // B: broken, held defined
            (3, 0.0),                        // D: unused
        ];
        if self
            .dac
            .write_volts_with_delay(&startup_setpoints, &mut embassy_time::Delay)
            .is_err()
        {
            warn!("DAC startup setup failed");
        }
        let (divider, top) = self.sample_rate.pwm_params();
        self.convst_pwm = Some(self.start_convst_pwm(divider, top));
    }

    #[unsafe(link_section = ".data.ram_func")]
    fn measure(&mut self, values: &mut [f32]) {
        #[cfg(not(feature = "diag-skip-adc"))]
        {
            let mut raw = [0u8; 18];
            self.adc_raw.transfer(&mut raw);
            self.adc_last = helic_drivers::ad7609::decode_frame(&raw);
        }
        for (value, raw) in values[..8].iter_mut().zip(self.adc_last) {
            *value = raw as f32 * self.adc_scale;
        }
        values[LASER_INPUT] = f32::from_bits(LASER_VALUE.load(Ordering::Relaxed));
        values[STATOR_INPUT] = f32::from_bits(STATOR_POSITION_MM.load(Ordering::Relaxed));
    }

    #[unsafe(link_section = ".data.ram_func")]
    fn actuate(&mut self, outputs: &[f32]) {
        let out = outputs[0];
        #[cfg(feature = "diag-skip-dac")]
        let _ = out;
        // Drive A and C symmetrically about the common-mode reference so the
        // signed differential drive (A - C) equals `out` directly, matching
        // what the `drive` loopback measures: each channel moves by half of
        // `out`. out = 0 rests both channels at MID_RAIL. No sign inversion
        // (A up, C down = more drive). Each channel is independently clamped
        // into the unipolar 0-4.096 V range by the DAC driver; `clamp_output`
        // keeps `out` within the margin that keeps both half-swung channels
        // inside that range. The two words are spaced by `wait_word_settle`
        // to respect the AD5064's inter-word settling time.
        #[cfg(not(feature = "diag-skip-dac"))]
        {
            let half = out * 0.5;
            self.dac_raw
                .write_volts(self.output_channel, MID_RAIL + half);
            wait_word_settle();
            self.dac_raw.write_volts(NEG_REF_CHANNEL, MID_RAIL - half);
        }
    }

    #[unsafe(link_section = ".data.ram_func")]
    fn prepare_reboot(&mut self, step: u8) -> bool {
        // One transfer per sample boundary preserves the AD5064 inter-word
        // timing while reproducing the audited power-on safe state. Writing
        // both differential inputs covers any runtime output routing.
        let (channel, volts, complete) = match step {
            0 => (NEG_REF_CHANNEL, MID_RAIL, false),
            1 => (OUTPUT_CHANNEL, MID_RAIL, false),
            2 => (1, 0.0, false),
            _ => (3, 0.0, true),
        };
        #[cfg(not(feature = "diag-skip-dac"))]
        self.dac_raw.write_volts(channel, volts);
        #[cfg(feature = "diag-skip-dac")]
        let _ = (channel, volts);
        complete
    }

    #[inline]
    #[unsafe(link_section = ".data.ram_func")]
    fn clamp_output(&self, actuator: usize, out: f32) -> f32 {
        debug_assert_eq!(actuator, 0);
        // Hard amplitude ceiling: clamp the logical command so both driven
        // channel voltages, `MID_RAIL + out/2` (A) and `MID_RAIL - out/2`
        // (C), stay inside the safe DAC window set by `DAC_OUT_FLOOR_V`/
        // `DAC_OUT_CEILING_V`. `clamp_channel_command` clamps a per-channel
        // deviation from `MID_RAIL`, so it is applied to `out/2` and the
        // clamped half doubled back into `out`'s own (differential) units.
        // Applied after the controller/forcing/table sum, so no single stage
        // can push the exciter past it. The AD5064 driver's own 0-4.096 V
        // clamp remains as a final backstop.
        clamp_channel_command(out * 0.5, MID_RAIL, DAC_OUT_FLOOR_V, DAC_OUT_CEILING_V) * 2.0
    }

    #[inline]
    #[unsafe(link_section = ".data.ram_func")]
    fn safe_output(&self, actuator: usize) -> f32 {
        debug_assert_eq!(actuator, 0);
        // Logical zero → A and C both at MID_RAIL → zero differential drive.
        0.0
    }

    #[unsafe(link_section = ".data.ram_func")]
    fn output_fault(&mut self, inputs: &[f32]) -> bool {
        // Blind-feedback guard: the laser frame counter must keep advancing.
        let stale = self
            .laser_stale
            .observe(LASER_FRAMES_RECEIVED.load(Ordering::Relaxed));
        // Displacement excursion: laser distance out of the safe window (or
        // non-finite) trips the gate.
        let d = inputs[LASER_INPUT];
        let out_of_range =
            !d.is_finite() || !(DISPLACEMENT_MIN_MM..=DISPLACEMENT_MAX_MM).contains(&d);
        stale || out_of_range
    }

    #[unsafe(link_section = ".data.ram_func")]
    fn tick_start(&mut self) {
        self.tick_pin.set_high();
    }

    #[unsafe(link_section = ".data.ram_func")]
    fn tick_phase_us(&self) -> Option<u32> {
        // The CONVST PWM slice wraps at CONVST's rising edge. With a 150 MHz
        // system clock, the divider converts its counter directly to elapsed
        // µs.
        let ctr = pac::PWM.ch(self.pwm_slice).ctr().read().ctr() as u32;
        Some(ctr * self.pwm_divider / 150)
    }

    #[unsafe(link_section = ".data.ram_func")]
    fn tick_end(&mut self) {
        self.tick_pin.set_low();
    }

    // Parameters 2 onwards belong to the stator stage, whose task runs on core
    // 0. They are routed through the rig group only because that is where a
    // rig's writable parameters live; `set_param` does no more than store an
    // atomic and, for the two command parameters, bump a sequence number. See
    // `stator.rs` and `docs/stator-stage.md`.
    fn param_names() -> &'static [&'static str] {
        &[
            "rig_laser_range",
            "rig_out_channel",
            "rig_stator_target",
            "rig_stator_jog",
            "rig_stator_home",
            "rig_stator_stop",
            "rig_stator_rate",
            "rig_stator_backlash",
            "rig_stator_datum",
            "rig_stator_hold",
        ]
    }

    fn param_defaults() -> &'static [f32] {
        &[
            DEFAULT_LASER_RANGE_MM,
            OUTPUT_CHANNEL as f32,
            0.0,
            0.0,
            0.0,
            0.0,
            DEFAULT_STATOR_RATE_MM_S,
            DEFAULT_STATOR_BACKLASH_MM,
            DEFAULT_STATOR_DATUM_MM,
            STATOR_HOLD_DEFAULT,
        ]
    }

    // Only range validation belongs here, because this is a static function
    // with no view of the axis. Rejections that depend on state, such as an
    // absolute move before homing, are made by the stepper task and counted in
    // `stator_faults` rather than returned as a write error.
    fn normalise_param(id: u16, value: f32) -> Option<f32> {
        match id {
            0 if value.is_finite() && value > 0.0 => Some(value),
            // Output routing is fixed to channel A (`OUTPUT_CHANNEL`): channel
            // B is broken and channel C holds the differential reference, so
            // neither may be selected as the driven output. Only the wired
            // channel is accepted; any other request is rejected.
            1 if value == OUTPUT_CHANNEL as f32 => Some(value),
            // Target and datum are barrel readings, so any finite value is
            // syntactically fine; the soft travel window is enforced against
            // the datum by the task, which knows where the axis actually is.
            2 | 8 if value.is_finite() => Some(value),
            // A jog is bounded by the same distance that bounds a homing
            // search, so a mistyped one cannot traverse the micrometer.
            3 if value.is_finite() && value.abs() <= STATOR_SEEK_MAX_MM => Some(value),
            // Home and stop are triggers: any finite value is accepted and a
            // non-zero one acts.
            4 | 5 if value.is_finite() => Some(value),
            // Rates above a few mm/s would need an acceleration ramp, which
            // this axis does not implement.
            6 if value.is_finite() && value > 0.0 && value <= MAX_STATOR_RATE_MM_S => Some(value),
            // Zero is not allowed: the final advance onto the target is what
            // makes an unpreloaded stage deterministic, so there is no such
            // thing as an approach with no overshoot.
            7 if value.is_finite() && value > 0.0 && value <= MAX_STATOR_BACKLASH_MM => Some(value),
            9 if value == 0.0 || value == 1.0 => Some(value),
            _ => None,
        }
    }

    #[unsafe(link_section = ".data.ram_func")]
    fn set_param(&mut self, id: u16, value: f32) {
        match id {
            0 => LASER_RANGE_MM.store(value.to_bits(), Ordering::Relaxed),
            1 => self.output_channel = value as usize,
            2 => {
                STATOR_TARGET_MM.store(value.to_bits(), Ordering::Relaxed);
                issue_command(CMD_MOVE);
            }
            3 => {
                STATOR_JOG_MM.store(value.to_bits(), Ordering::Relaxed);
                issue_command(CMD_JOG);
            }
            4 if value != 0.0 => issue_command(CMD_HOME),
            5 if value != 0.0 => issue_command(CMD_STOP),
            6 => STATOR_RATE_MM_S.store(value.to_bits(), Ordering::Relaxed),
            7 => STATOR_BACKLASH_MM.store(value.to_bits(), Ordering::Relaxed),
            8 => STATOR_DATUM_MM.store(value.to_bits(), Ordering::Relaxed),
            9 => STATOR_HOLD.store(value.to_bits(), Ordering::Relaxed),
            _ => {}
        }
    }
}

impl MagnetoelasticRig {
    /// Start the crystal-timed CONVST output after ADC and DAC setup.
    fn start_convst_pwm(&mut self, divider: u8, top: u16) -> Pwm<'static> {
        let (slice, pin) = self.convst.take().expect("CONVST PWM already started");
        let mut config = pwm::Config::default();
        config.divider = divider.to_fixed();
        config.top = top;
        config.compare_a = top / 2;
        Pwm::new_output_a(slice, pin, config)
    }
}
