//! The small signal-processing building blocks shared by the voices.
//!
//! Everything here is plain `f32` arithmetic on fixed-size state: no allocation, no I/O. Every
//! block stays bounded for any parameter it is given: cut-off frequencies are clamped below
//! Nyquist, resonance below self-oscillation, and the non-linearities saturate.

use std::f32::consts::PI;

/// Highest cut-off of every filter, as a fraction of the sample rate (just below Nyquist).
const MAX_CUTOFF_RATIO: f32 = 0.45;
/// Lowest cut-off of every filter, in hertz.
const MIN_CUTOFF_HZ: f32 = 10.0;
/// Highest feedback of the ladder filter: screaming, but still below self-oscillation (4).
pub(crate) const MAX_RESONANCE: f32 = 3.9;

/// A cheap, smooth saturator: identity-like near zero, reaching exactly ±1 at ±3 and beyond.
///
/// Rational approximation of `tanh` (monotonic, odd, never above 1 in magnitude, no
/// transcendental function, so it is cheap in WASM and identical on every platform).
pub(crate) fn soft_clip(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    // The final clamp only removes rounding (up to 1e-7 above 1 near ±3).
    (x * (27.0 + x * x) / (27.0 + 9.0 * x * x)).clamp(-1.0, 1.0)
}

/// The per-sample multiplier of an exponential decay with time constant `seconds`.
pub(crate) fn decay_coef(seconds: f32, sample_rate: f32) -> f32 {
    (-1.0 / (seconds * sample_rate)).exp()
}

/// The per-sample step of a one-pole smoother with time constant `seconds`.
pub(crate) fn smooth_coef(seconds: f32, sample_rate: f32) -> f32 {
    1.0 - decay_coef(seconds, sample_rate)
}

/// The frequency in hertz of a (fractional) MIDI note number; A4 = 69 = 440 Hz.
pub(crate) fn midi_to_hz(note: f32) -> f32 {
    440.0 * ((note - 69.0) / 12.0).exp2()
}

/// The bilinear-prewarped integrator gain of a filter at `cutoff` hertz (clamped to a safe
/// range).
fn prewarp(cutoff: f32, sample_rate: f32) -> f32 {
    // Not `clamp`: at absurdly low sample rates the ceiling is below the floor.
    let cutoff = cutoff
        .max(MIN_CUTOFF_HZ)
        .min(MAX_CUTOFF_RATIO * sample_rate);
    (PI * cutoff / sample_rate).tan()
}

/// A value moving linearly towards a target at a fixed rate per call: click-free gain changes
/// that reach their target exactly (so a fade-out ends in true silence).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ramp {
    value: f32,
    target: f32,
    rate: f32,
}

impl Ramp {
    /// A ramp resting at `value` that moves by at most `rate` per [`Ramp::next`].
    pub(crate) fn new(value: f32, rate: f32) -> Self {
        Ramp {
            value,
            target: value,
            rate,
        }
    }

    /// The current value.
    pub(crate) fn value(&self) -> f32 {
        self.value
    }

    /// Moves towards `target` from now on.
    pub(crate) fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Changes the speed of the ramp.
    pub(crate) fn set_rate(&mut self, rate: f32) {
        self.rate = rate;
    }

    /// Advances one call and returns the new value.
    pub(crate) fn next(&mut self) -> f32 {
        let distance = self.target - self.value;
        if distance.abs() <= self.rate {
            self.value = self.target;
        } else {
            self.value += self.rate.copysign(distance);
        }
        self.value
    }

    /// Whether the ramp rests at zero (a muted voice can be skipped).
    pub(crate) fn is_silent(&self) -> bool {
        self.value == 0.0 && self.target == 0.0
    }
}

/// White noise from a 32-bit xorshift generator.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Noise(u32);

impl Noise {
    /// A generator seeded with `seed` (zero, which would stick, is replaced).
    pub(crate) fn new(seed: u32) -> Self {
        Noise(seed.max(1))
    }

    /// The next sample, uniform in `-1.0..1.0`.
    pub(crate) fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x.cast_signed() as f32 / 2_147_483_648.0
    }
}

/// A band-limited (PolyBLEP) sawtooth oscillator.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Saw {
    phase: f32,
}

impl Saw {
    /// The next sample in `-1.0..=1.0` for a frequency of `dt` cycles per sample
    /// (`0.0 < dt < 0.5`).
    pub(crate) fn next(&mut self, dt: f32) -> f32 {
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let t = self.phase;
        let blep = if t < dt {
            let x = t / dt;
            2.0 * x - x * x - 1.0
        } else if t > 1.0 - dt {
            let x = (t - 1.0) / dt;
            x * x + 2.0 * x + 1.0
        } else {
            0.0
        };
        2.0 * t - 1.0 - blep
    }
}

/// A naive sawtooth (for the heavily low-passed pad, where aliasing is inaudible).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RawSaw {
    phase: f32,
}

impl RawSaw {
    /// The next sample in `-1.0..1.0` for a frequency of `dt` cycles per sample.
    pub(crate) fn next(&mut self, dt: f32) -> f32 {
        self.phase = (self.phase + dt).fract();
        2.0 * self.phase - 1.0
    }
}

/// The three outputs of a [`Svf`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct SvfOut {
    /// Low-pass output.
    pub(crate) low: f32,
    /// Band-pass output.
    pub(crate) band: f32,
    /// High-pass output.
    pub(crate) high: f32,
}

/// A 2-pole state-variable filter (topology-preserving transform, after A. Simper): stable
/// for every cut-off and any positive Q.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Svf {
    /// A filter at `cutoff` hertz with quality `q` (> 0).
    pub(crate) fn new(cutoff: f32, q: f32, sample_rate: f32) -> Self {
        let mut svf = Svf {
            ic1: 0.0,
            ic2: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            k: 0.0,
        };
        svf.tune(cutoff, q, sample_rate);
        svf
    }

    /// Moves the cut-off to `cutoff` hertz with quality `q` (> 0), keeping the state.
    pub(crate) fn tune(&mut self, cutoff: f32, q: f32, sample_rate: f32) {
        let g = prewarp(cutoff, sample_rate);
        self.k = 1.0 / q;
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    /// Filters one sample.
    pub(crate) fn process(&mut self, input: f32) -> SvfOut {
        let v3 = input - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        SvfOut {
            low: v2,
            band: v1,
            high: input - self.k * v1 - v2,
        }
    }
}

/// A 4-pole (24 dB/octave) resonant ladder low-pass, the heart of the acid squelch.
///
/// Zero-delay-feedback (topology-preserving) cascade of four one-poles with the feedback
/// solved exactly, then saturated at the input. Each stage is a stable linear filter fed with a
/// signal bounded by the saturator, so the output stays bounded for every cut-off and every
/// resonance up to [`MAX_RESONANCE`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Ladder {
    stages: [f32; 4],
}

impl Ladder {
    /// Filters one sample at `cutoff` hertz with feedback `resonance` (clamped to
    /// `0.0..=MAX_RESONANCE`).
    pub(crate) fn process(
        &mut self,
        input: f32,
        cutoff: f32,
        resonance: f32,
        sample_rate: f32,
    ) -> f32 {
        let g = prewarp(cutoff, sample_rate);
        let k = resonance.clamp(0.0, MAX_RESONANCE);
        let a = g / (1.0 + g);
        let b = 1.0 - a;
        let [s0, s1, s2, s3] = self.stages;
        // Output of the cascade = gain * u + state, solved for the feedback input u.
        let gain = a * a * a * a;
        let state = b * (a * a * a * s0 + a * a * s1 + a * s2 + s3);
        let mut u = soft_clip((input - k * state) / (1.0 + k * gain));
        for stage in &mut self.stages {
            let v = (u - *stage) * a;
            let y = v + *stage;
            *stage = y + v;
            u = y;
        }
        // Compensates the loss of pass-band level as the resonance rises.
        u * (1.0 + 0.5 * k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_clip_is_odd_monotonic_and_bounded() {
        assert_eq!(soft_clip(0.0), 0.0);
        assert_eq!(soft_clip(3.0), 1.0);
        assert_eq!(soft_clip(-1e9), -1.0);
        assert_eq!(soft_clip(f32::INFINITY), 1.0);
        let mut previous = -1.0;
        for i in -400..=400 {
            let y = soft_clip(i as f32 / 100.0);
            assert!(y >= previous && y.abs() <= 1.0, "{i} {y} {previous}");
            assert_eq!(soft_clip(-(i as f32) / 100.0), -y);
            previous = y;
        }
    }

    #[test]
    fn the_ladder_stays_bounded_for_extreme_settings() {
        for sample_rate in [1.0, 8_000.0, 48_000.0, 192_000.0] {
            for cutoff in [-5.0, 0.0, 20.0, 1_000.0, 1e9, f32::INFINITY] {
                for resonance in [-1.0, 0.0, 2.0, 3.9, 100.0] {
                    let mut ladder = Ladder::default();
                    let mut noise = Noise::new(7);
                    for i in 0..4_000 {
                        let input = if i % 50 < 25 { 10.0 } else { -10.0 } + noise.next();
                        let y = ladder.process(input, cutoff, resonance, sample_rate);
                        assert!(y.is_finite() && y.abs() < 100.0, "{cutoff} {resonance} {y}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_ladder_passes_the_bass_and_cuts_the_highs() {
        let rms = |hz: f32| {
            let mut ladder = Ladder::default();
            let mut sum = 0.0;
            for i in 0..48_000 {
                let x = 0.1 * (2.0 * PI * hz * i as f32 / 48_000.0).sin();
                let y = ladder.process(x, 500.0, 0.0, 48_000.0);
                if i >= 24_000 {
                    sum += y * y;
                }
            }
            (sum / 24_000.0).sqrt()
        };
        let (low, high) = (rms(100.0), rms(4_000.0));
        assert!(low > 0.06, "{low}");
        assert!(high < low / 1_000.0, "{high}");
    }

    #[test]
    fn the_svf_splits_low_band_and_high() {
        let mut svf = Svf::new(1_000.0, 0.7, 48_000.0);
        let mut dc = SvfOut {
            low: 0.0,
            band: 0.0,
            high: 0.0,
        };
        for _ in 0..48_000 {
            dc = svf.process(1.0);
        }
        assert!((dc.low - 1.0).abs() < 1e-3 && dc.band.abs() < 1e-3 && dc.high.abs() < 1e-3);
    }

    #[test]
    fn the_saws_stay_in_range() {
        let (mut saw, mut raw) = (Saw::default(), RawSaw::default());
        for _ in 0..10_000 {
            assert!(saw.next(0.013).abs() <= 1.0);
            assert!(raw.next(0.013).abs() <= 1.0);
        }
    }

    #[test]
    fn ramps_reach_their_target_exactly() {
        let mut ramp = Ramp::new(0.0, 0.3);
        ramp.set_target(1.0);

        assert!(!ramp.is_silent());
        for expected in [0.3, 0.6, 0.9] {
            assert!((ramp.next() - expected).abs() < 1e-6);
        }
        assert_eq!(ramp.next(), 1.0);
        ramp.set_rate(10.0);
        ramp.set_target(0.0);
        assert_eq!(ramp.next(), 0.0);
        assert!(ramp.is_silent());
        assert_eq!(ramp.value(), 0.0);
    }

    #[test]
    fn noise_is_never_stuck_and_stays_in_range() {
        let mut noise = Noise::new(0);
        let samples: Vec<f32> = (0..1_000).map(|_| noise.next()).collect();
        assert!(samples.iter().all(|s| (-1.0..1.0).contains(s)));
        assert!(samples.iter().any(|&s| s > 0.5) && samples.iter().any(|&s| s < -0.5));
    }

    #[test]
    fn coefficients_match_their_definitions() {
        assert!((midi_to_hz(69.0) - 440.0).abs() < 1e-3);
        assert!((midi_to_hz(33.0) - 55.0).abs() < 1e-3);
        assert!((decay_coef(1.0, 100.0) - (-0.01f32).exp()).abs() < 1e-7);
        assert!((smooth_coef(1.0, 100.0) + decay_coef(1.0, 100.0) - 1.0).abs() < 1e-7);
    }
}
