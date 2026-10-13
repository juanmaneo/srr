//! The instruments: a 909-style kick, its rumble, the 303 acid line, hats, clap and a drone.
//!
//! Every voice is a fixed-size state machine: `trigger` starts a note, `process` returns the
//! next sample. Envelopes are multiplicative decays so a sample costs a few multiplications.
//! Only [`Rumble`] owns a buffer, allocated once by its constructor.

use std::f32::consts::TAU;

use super::dsp::{Ladder, Noise, RawSaw, Saw, Svf, decay_coef, midi_to_hz, smooth_coef, soft_clip};
use super::pattern::Step;

/// A 909-style kick: sine with a fast pitch sweep (~150 Hz down to 45 Hz), a noisy click and
/// a long tail, driven into the soft clipper.
#[derive(Debug, Clone)]
pub(crate) struct Kick {
    sample_rate: f32,
    noise: Noise,
    phase: f32,
    amp: f32,
    amp_coef: f32,
    sweep: f32,
    sweep_coef: f32,
    click: f32,
    click_coef: f32,
}

impl Kick {
    /// The resting pitch of the tail, in hertz.
    const BASE_HZ: f32 = 45.0;
    /// Extra pitch at the start of the sweep (base + sweep = 150 Hz).
    const SWEEP_HZ: f32 = 105.0;
    /// Extra pitch of the very first milliseconds (the click).
    const CLICK_HZ: f32 = 450.0;
    /// Drive into the soft clipper: flattens the body into a pounding, distorted thump.
    const DRIVE: f32 = 2.4;

    pub(crate) fn new(sample_rate: f32, noise_seed: u32) -> Self {
        Kick {
            sample_rate,
            noise: Noise::new(noise_seed),
            phase: 0.0,
            amp: 0.0,
            amp_coef: decay_coef(0.3, sample_rate),
            sweep: 0.0,
            sweep_coef: decay_coef(0.032, sample_rate),
            click: 0.0,
            click_coef: decay_coef(0.0018, sample_rate),
        }
    }

    /// Starts a kick at `velocity` (`0.0..=1.0`).
    pub(crate) fn trigger(&mut self, velocity: f32) {
        self.phase = 0.0;
        self.amp = velocity;
        self.sweep = 1.0;
        self.click = velocity;
    }

    /// The amplitude envelope, used to duck the rumble under the kick.
    pub(crate) fn envelope(&self) -> f32 {
        self.amp
    }

    pub(crate) fn process(&mut self) -> f32 {
        let hz = Self::BASE_HZ + Self::SWEEP_HZ * self.sweep + Self::CLICK_HZ * self.click;
        self.phase = (self.phase + hz / self.sample_rate).fract();
        let body = (TAU * self.phase).sin() * self.amp;
        let click = self.noise.next() * self.click * 0.3;
        self.amp *= self.amp_coef;
        self.sweep *= self.sweep_coef;
        self.click *= self.click_coef;
        soft_clip((body + click) * Self::DRIVE)
    }
}

/// The basement rumble: the kick echoed an eighth note later (on the off-beat), smeared by
/// feedback, low-passed to a sub-bass roar, saturated and ducked under the next kick.
#[derive(Debug, Clone)]
pub(crate) struct Rumble {
    buffer: Vec<f32>,
    position: usize,
    delay: usize,
    low: [f32; 2],
    coef: f32,
}

impl Rumble {
    /// The longest echo, in seconds (an eighth note at 120 BPM).
    const MAX_DELAY_SECONDS: f32 = 0.25;
    /// Share of the echo fed back: each repeat smears into the next off-beat.
    const FEEDBACK: f32 = 0.5;

    pub(crate) fn new(sample_rate: f32) -> Self {
        let length = (Self::MAX_DELAY_SECONDS * sample_rate) as usize + 2;
        Rumble {
            buffer: vec![0.0; length],
            position: 0,
            delay: 1,
            low: [0.0; 2],
            coef: 1.0 - (-TAU * 110.0 / sample_rate).exp(),
        }
    }

    /// Sets the echo time in samples (clamped to the buffer).
    pub(crate) fn set_delay(&mut self, samples: f32) {
        self.delay = (samples.round() as usize).clamp(1, self.buffer.len() - 1);
    }

    /// Feeds `input` (the kick) and returns the rumble, ducked by `duck` (`0.0..=1.0`).
    pub(crate) fn process(&mut self, input: f32, duck: f32) -> f32 {
        let length = self.buffer.len();
        let echo = self.buffer[(self.position + length - self.delay) % length];
        self.buffer[self.position] = input + Self::FEEDBACK * echo;
        self.position = (self.position + 1) % length;
        self.low[0] += (echo - self.low[0]) * self.coef;
        self.low[1] += (self.low[0] - self.low[1]) * self.coef;
        soft_clip(self.low[1] * 2.0) * (1.0 - 0.85 * duck)
    }
}

/// The per-sample controls of the [`Acid`] voice, computed by the conductor.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AcidParams {
    /// Cut-off with the envelope closed, in octaves above 50 Hz.
    pub(crate) base_octaves: f32,
    /// Octaves added by a fully open filter envelope.
    pub(crate) env_octaves: f32,
    /// Extra octaves added on accented notes.
    pub(crate) accent_octaves: f32,
    /// Ladder feedback (`0..4`).
    pub(crate) resonance: f32,
    /// Gain into the final saturator.
    pub(crate) drive: f32,
}

/// The TB-303 style acid voice: band-limited saw into the resonant ladder, per-note filter and
/// amplitude envelopes, accents, slides (portamento without retrigger) and saturation.
#[derive(Debug, Clone)]
pub(crate) struct Acid {
    sample_rate: f32,
    saw: Saw,
    ladder: Ladder,
    pitch: f32,
    target_pitch: f32,
    slide_coef: f32,
    gate_left: f32,
    amp: f32,
    level: f32,
    level_coef: f32,
    attack_coef: f32,
    release_coef: f32,
    filter_env: f32,
    filter_coef: f32,
    accent: f32,
    accent_cv: f32,
    accent_coef: f32,
}

impl Acid {
    /// Share of a step a normal (not sliding) note holds its gate.
    const GATE: f32 = 0.5;
    /// Samples a sliding note's gate overlaps the next step, so that step sees it held.
    const SLIDE_OVERLAP: f32 = 64.0;

    pub(crate) fn new(sample_rate: f32) -> Self {
        let root = f32::from(super::pattern::ROOT_NOTE);
        Acid {
            sample_rate,
            saw: Saw::default(),
            ladder: Ladder::default(),
            pitch: root,
            target_pitch: root,
            slide_coef: smooth_coef(0.035, sample_rate),
            gate_left: 0.0,
            amp: 0.0,
            level: 0.0,
            level_coef: decay_coef(1.5, sample_rate),
            attack_coef: smooth_coef(0.002, sample_rate),
            release_coef: smooth_coef(0.008, sample_rate),
            filter_env: 0.0,
            filter_coef: 0.0,
            accent: 0.0,
            accent_cv: 0.0,
            accent_coef: smooth_coef(0.03, sample_rate),
        }
    }

    /// Plays `step`. `next_plays` tells whether the following step is a note (a slide only
    /// holds into a note), `accent` whether this note is accented, `decay` the filter
    /// envelope's time constant in seconds and `step_samples` the length of the step.
    pub(crate) fn trigger(
        &mut self,
        step: Step,
        next_plays: bool,
        accent: bool,
        decay: f32,
        step_samples: f32,
    ) {
        if step.rest {
            self.gate_left = 0.0;
            return;
        }
        self.target_pitch = f32::from(step.note);
        // Still held by the previous step's slide: glide there, without retriggering.
        if self.gate_left <= 0.0 {
            self.pitch = self.target_pitch;
            self.filter_env = 1.0;
            self.accent = if accent { 1.0 } else { 0.0 };
            self.level = if accent { 1.0 } else { 0.62 };
            let decay = if accent { decay * 0.6 } else { decay };
            self.filter_coef = decay_coef(decay, self.sample_rate);
        }
        self.gate_left = if step.slide && next_plays {
            step_samples + Self::SLIDE_OVERLAP
        } else {
            step_samples * Self::GATE
        };
    }

    pub(crate) fn process(&mut self, params: &AcidParams) -> f32 {
        self.pitch += (self.target_pitch - self.pitch) * self.slide_coef;
        let dt = (midi_to_hz(self.pitch) / self.sample_rate).min(0.49);
        let oscillator = self.saw.next(dt);
        self.accent_cv += (self.accent * self.filter_env - self.accent_cv) * self.accent_coef;
        let octaves = params.base_octaves
            + params.env_octaves * self.filter_env
            + params.accent_octaves * self.accent_cv;
        let cutoff = 50.0 * octaves.exp2();
        let filtered =
            self.ladder
                .process(oscillator * 0.8, cutoff, params.resonance, self.sample_rate);
        if self.gate_left > 0.0 {
            self.amp += (self.level - self.amp) * self.attack_coef;
            self.gate_left -= 1.0;
        } else {
            self.amp -= self.amp * self.release_coef;
        }
        self.level *= self.level_coef;
        self.filter_env *= self.filter_coef;
        soft_clip(filtered * self.amp * params.drive)
    }
}

/// Noise hats: a high-passed noise source shaped by a short closed envelope and a longer open
/// one; a closed hat chokes the open one. Closed hats are panned for a little stereo width.
#[derive(Debug, Clone)]
pub(crate) struct Hats {
    noise: Noise,
    filter: Svf,
    closed: f32,
    closed_coef: f32,
    open: f32,
    open_coef: f32,
    open_decay: f32,
    choke: f32,
    pan: f32,
}

impl Hats {
    pub(crate) fn new(sample_rate: f32, noise_seed: u32) -> Self {
        let open_decay = decay_coef(0.09, sample_rate);
        Hats {
            noise: Noise::new(noise_seed),
            filter: Svf::new(7_500.0, 0.8, sample_rate),
            closed: 0.0,
            closed_coef: decay_coef(0.014, sample_rate),
            open: 0.0,
            open_coef: open_decay,
            open_decay,
            choke: decay_coef(0.004, sample_rate),
            pan: 0.0,
        }
    }

    /// A closed hat at `velocity`, panned by `pan` (`-1.0..=1.0`, right is positive).
    pub(crate) fn trigger_closed(&mut self, velocity: f32, pan: f32) {
        self.closed = velocity;
        self.pan = pan;
        self.open_coef = self.choke;
    }

    /// An open hat at `velocity`.
    pub(crate) fn trigger_open(&mut self, velocity: f32) {
        self.open = velocity;
        self.open_coef = self.open_decay;
    }

    /// The next `(mid, side)` sample pair.
    pub(crate) fn process(&mut self) -> (f32, f32) {
        let noise = self.filter.process(self.noise.next()).high;
        let closed = noise * self.closed;
        let open = noise * self.open * 0.7;
        self.closed *= self.closed_coef;
        self.open *= self.open_coef;
        (closed + open, closed * self.pan)
    }
}

/// A 909-ish clap / snare: three quick band-passed noise bursts, a noise tail and a short
/// pitched body.
#[derive(Debug, Clone)]
pub(crate) struct Clap {
    sample_rate: f32,
    noise: Noise,
    filter: Svf,
    burst_samples: u32,
    elapsed: u32,
    burst: f32,
    burst_coef: f32,
    tail: f32,
    tail_coef: f32,
    body_phase: f32,
    body: f32,
    body_coef: f32,
}

impl Clap {
    /// Number of quick bursts before the tail (the "hands" of the clap).
    const BURSTS: u32 = 3;
    /// Pitch of the snare body, in hertz.
    const BODY_HZ: f32 = 185.0;

    pub(crate) fn new(sample_rate: f32, noise_seed: u32) -> Self {
        Clap {
            sample_rate,
            noise: Noise::new(noise_seed),
            filter: Svf::new(1_200.0, 1.4, sample_rate),
            burst_samples: ((0.011 * sample_rate) as u32).max(1),
            elapsed: u32::MAX,
            burst: 0.0,
            burst_coef: decay_coef(0.004, sample_rate),
            tail: 0.0,
            tail_coef: decay_coef(0.13, sample_rate),
            body_phase: 0.0,
            body: 0.0,
            body_coef: decay_coef(0.04, sample_rate),
        }
    }

    pub(crate) fn trigger(&mut self) {
        self.elapsed = 0;
        self.tail = 1.0;
        self.body = 1.0;
        self.body_phase = 0.0;
    }

    pub(crate) fn process(&mut self) -> f32 {
        let in_bursts = self.elapsed < Self::BURSTS * self.burst_samples;
        if in_bursts && self.elapsed.is_multiple_of(self.burst_samples) {
            self.burst = 1.0;
        }
        let envelope = if in_bursts { self.burst } else { self.tail };
        self.elapsed = self.elapsed.saturating_add(1);
        self.burst *= self.burst_coef;
        self.tail *= self.tail_coef;
        // The band-pass keeps a small share of the white noise: make up for it.
        let noise = self.filter.process(self.noise.next()).band * envelope * 5.0;
        self.body_phase = (self.body_phase + Self::BODY_HZ / self.sample_rate).fract();
        let body = (TAU * self.body_phase).sin() * self.body * 0.5;
        self.body *= self.body_coef;
        noise + body
    }
}

/// The breakdown drone: a dark, detuned A minor cluster of saws through a slowly breathing
/// low-pass, kept out of the bass so the breakdown really drops the low end.
#[derive(Debug, Clone)]
pub(crate) struct Pad {
    sample_rate: f32,
    saws: [RawSaw; 4],
    filter: Svf,
}

impl Pad {
    /// Detuned A3, A3, C4 and E4, in hertz: above the acid line's register.
    const FREQUENCIES: [f32; 4] = [219.2, 221.0, 261.2, 330.2];

    pub(crate) fn new(sample_rate: f32) -> Self {
        Pad {
            sample_rate,
            saws: [RawSaw::default(); 4],
            filter: Svf::new(300.0, 0.9, sample_rate),
        }
    }

    /// The next sample; `brightness` (`0.0..=1.0`) opens the filter.
    pub(crate) fn process(&mut self, brightness: f32) -> f32 {
        let mut sum = 0.0;
        for (saw, hz) in self.saws.iter_mut().zip(Self::FREQUENCIES) {
            sum += saw.next(hz / self.sample_rate);
        }
        self.filter
            .tune(350.0 + 650.0 * brightness, 0.9, self.sample_rate);
        self.filter.process(sum * 0.25).low
    }
}
