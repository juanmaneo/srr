//! Procedurally generated music: 90s dark acid techno, "New York basement" style.
//!
//! Everything is synthesised at run time, deterministically, from a seed: no audio assets, no
//! I/O, no threads. The audio back-end (`platform::sound`, feature `client`) only pulls
//! samples out of [`Music::render`] and forwards the game's wishes from a [`MusicControl`].
//!
//! # The band
//!
//! * a pounding, distorted 909-style **kick** on every beat (pitch sweep ~150 → 45 Hz, click,
//!   long tail, soft-clip drive), with a doubled kick closing every phrase;
//! * the basement **rumble**: the kick echoed on the off-beat, smeared by feedback,
//!   low-passed into a sub-bass roar and ducked under the next kick;
//! * a TB-303 style **acid line**: band-limited saw into a resonant 24 dB ladder low-pass,
//!   per-note filter envelope, accents, slides (portamento without retrigger), a slow
//!   phrase-long cut-off sweep for the squelch, and saturation;
//! * high-passed noise **hats** (closed sixteenths, slightly panned, open hat on the off-beat)
//!   and a **clap** on 2 and 4;
//! * a dark detuned **drone** for the breakdowns.
//!
//! # The score
//!
//! The acid line plays a 16-step [`Pattern`] in A phrygian ([`SCALE`]), generated from the seed
//! by a [`Rng`] and [mutated](Pattern::mutate) every [`PHRASE_BARS`] bars: a few steps change,
//! so the line evolves endlessly but stays recognisable, and is reproducible from the seed.
//! The [`Mood`] picks the tempo and which instruments play; every change fades smoothly (no
//! clicks), tempo changes wait for the next bar. The intensity opens the filter and adds
//! resonance, accents and drive.
//!
//! ```
//! use srr::audio::{DEFAULT_SEED, Mood, Music};
//!
//! let mut music = Music::new(48_000, DEFAULT_SEED);
//! music.set_mood(Mood::Fight);
//! music.set_intensity(0.8);
//! let mut buffer = vec![0.0; 2 * 1_024]; // 1024 stereo frames
//! music.render(&mut buffer, 2);
//! assert!(buffer.iter().all(|sample| sample.abs() <= 1.0));
//! assert_eq!(music.bpm(), 145.0);
//! ```

mod dsp;
mod pattern;
mod rng;
mod voices;

use std::f32::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use dsp::{Ramp, soft_clip};
pub use pattern::{
    HIGHEST_NOTE, MUTATED_STEPS, OCTAVES, Pattern, ROOT_NOTE, SCALE, STEPS, Step, in_scale,
};
pub use rng::Rng;
use voices::{Acid, AcidParams, Clap, Hats, Kick, Pad, Rumble};

/// The seed of the game's music.
pub const DEFAULT_SEED: u64 = 0x0909_0303_D34D_B455;

/// The acid pattern mutates every `PHRASE_BARS` bars.
pub const PHRASE_BARS: u64 = 8;

/// The highest magnitude [`Music::render`] can output: about -3 dBFS, the ceiling of the
/// final soft clipper. The music sits under the game's sound effects.
pub const OUTPUT_PEAK: f32 = 0.7;

/// What the game is showing; the music follows it with smooth transitions (no clicks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mood {
    /// Everything fades out; once silent, the sequencer pauses.
    #[default]
    Silent,
    /// Title screen, 138 BPM: kick, rumble, hats and a filtered acid line slowly opening over
    /// 16 bars; no clap.
    Intro,
    /// A round is running, 145 BPM: the full groove.
    Fight,
    /// Round over / waiting, 145 BPM: the drums drop out, the acid line and a dark drone keep
    /// going.
    Breakdown,
}

/// The instrument groups whose level follows the mood, as indices into the gain table.
const KICK: usize = 0;
const RUMBLE: usize = 1;
const HATS: usize = 2;
const CLAP: usize = 3;
const ACID: usize = 4;
const PAD: usize = 5;
const GROUPS: usize = 6;

/// Gain of the mix into the final soft clipper: the kicks just touch its knee.
const MIX_GAIN: f32 = 1.5;
/// Seconds each group takes to fade from silence to full level (or back).
const FADE_SECONDS: [f32; GROUPS] = [0.35, 0.6, 0.5, 0.3, 1.5, 2.5];
/// Seconds the master level takes to fade in or out.
const MASTER_FADE_SECONDS: f32 = 1.5;
/// Seconds the intensity takes to move across its whole range.
const INTENSITY_SECONDS: f32 = 0.5;

/// How a playing mood sounds.
struct Scene {
    bpm: f32,
    /// Level of each group (indexed by `KICK`...`PAD`).
    levels: [f32; GROUPS],
    /// How far the acid filter opens (`0.0..=1.0`), and in how many steps from fully closed.
    opening: f32,
    opening_steps: f32,
}

impl Mood {
    /// Every mood, in declaration order.
    pub const ALL: [Mood; 4] = [Mood::Silent, Mood::Intro, Mood::Fight, Mood::Breakdown];

    /// The tempo of the mood, in beats per minute; `None` for [`Mood::Silent`], which keeps
    /// the current tempo.
    ///
    /// ```
    /// use srr::audio::Mood;
    ///
    /// assert_eq!(Mood::Intro.bpm(), Some(138.0));
    /// assert_eq!(Mood::Fight.bpm(), Some(145.0));
    /// assert_eq!(Mood::Silent.bpm(), None);
    /// ```
    pub fn bpm(self) -> Option<f32> {
        self.scene().map(|scene| scene.bpm)
    }

    fn scene(self) -> Option<Scene> {
        //                               kick rumble hats clap acid pad
        let (bpm, levels, opening, opening_steps) = match self {
            Mood::Silent => return None,
            Mood::Intro => (138.0, [1.0, 0.8, 0.6, 0.0, 0.8, 0.0], 1.0, 256.0),
            Mood::Fight => (145.0, [1.0, 1.0, 1.0, 1.0, 1.0, 0.0], 1.0, 32.0),
            Mood::Breakdown => (145.0, [0.0, 0.0, 0.0, 0.0, 1.0, 1.0], 0.75, 64.0),
        };
        Some(Scene {
            bpm,
            levels,
            opening,
            opening_steps,
        })
    }

    fn from_index(index: u32) -> Mood {
        match index {
            1 => Mood::Intro,
            2 => Mood::Fight,
            3 => Mood::Breakdown,
            _ => Mood::Silent,
        }
    }
}

/// Clamps an intensity to `0.0..=1.0`, mapping NaN to `0.0`.
fn clamp_intensity(intensity: f32) -> f32 {
    if intensity.is_nan() {
        0.0
    } else {
        intensity.clamp(0.0, 1.0)
    }
}

/// Thread-safe handle the game uses to steer the music playing on the audio thread.
///
/// Clones share the same state: the game keeps one, the audio callback another, and reads
/// the latest values at the start of every buffer. Lock-free (two atomics), so the audio
/// thread never waits for the game.
///
/// ```
/// use srr::audio::{Mood, MusicControl};
///
/// let control = MusicControl::new();
/// let audio_side = control.clone();
/// control.set_mood(Mood::Fight);
/// control.set_intensity(2.0);
/// assert_eq!(audio_side.mood(), Mood::Fight);
/// assert_eq!(audio_side.intensity(), 1.0);
/// ```
#[derive(Debug, Clone, Default)]
pub struct MusicControl {
    mood: Arc<AtomicU32>,
    intensity: Arc<AtomicU32>,
}

impl MusicControl {
    /// A control asking for [`Mood::Silent`] at intensity `0.0`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks for `mood`.
    pub fn set_mood(&self, mood: Mood) {
        self.mood.store(mood as u32, Ordering::Relaxed);
    }

    /// The mood last asked for.
    pub fn mood(&self) -> Mood {
        Mood::from_index(self.mood.load(Ordering::Relaxed))
    }

    /// How tense the fight is, `0.0..=1.0` (clamped; NaN counts as `0.0`), e.g. a fighter low
    /// on health or a submission. Opens the acid filter, adds resonance, accents and drive.
    pub fn set_intensity(&self, intensity: f32) {
        let intensity = clamp_intensity(intensity);
        self.intensity.store(intensity.to_bits(), Ordering::Relaxed);
    }

    /// The intensity last asked for, already clamped.
    pub fn intensity(&self) -> f32 {
        f32::from_bits(self.intensity.load(Ordering::Relaxed))
    }
}

/// The length of a sixteenth note in samples.
fn step_samples(sample_rate: f32, bpm: f32) -> f64 {
    f64::from(sample_rate) * 60.0 / (f64::from(bpm) * 4.0)
}

/// The synthesiser and its sequencer.
///
/// Deterministic: the same seed, sample rate and control changes (at the same sample
/// positions) produce the same samples. [`Music::render`] never allocates; the only buffer
/// (the rumble's echo, a quarter of a second) is allocated by [`Music::new`].
///
/// A new `Music` is [`Mood::Silent`]: it outputs silence until [`Music::set_mood`] asks for
/// something else, then fades in.
pub struct Music {
    sample_rate: u32,
    rate: f32,
    seed: u64,
    mood: Mood,
    intensity: f32,
    tension: Ramp,
    bpm: f32,
    target_bpm: f32,
    samples_per_step: f64,
    until_step: f64,
    steps_played: u64,
    rng: Rng,
    pattern: Pattern,
    opening: Ramp,
    gains: [Ramp; GROUPS],
    master: Ramp,
    kick: Kick,
    rumble: Rumble,
    acid: Acid,
    hats: Hats,
    clap: Clap,
    pad: Pad,
}

impl Music {
    /// A silent band at `sample_rate` hertz (0 counts as 1) playing the tune of `seed`.
    pub fn new(sample_rate: u32, seed: u64) -> Self {
        let sample_rate = sample_rate.max(1);
        let rate = sample_rate as f32;
        let per_sample = |seconds: f32| 1.0 / (seconds * rate);
        let mut rng = Rng::new(seed);
        let pattern = Pattern::generate(&mut rng);
        let mut noise_seeds = Rng::new(seed ^ 0x6E6F_6973_6520_2020);
        let mut noise_seed = || (noise_seeds.next_u64() >> 32) as u32;
        let bpm = Mood::Intro.bpm().unwrap_or_default();
        Music {
            sample_rate,
            rate,
            seed,
            mood: Mood::Silent,
            intensity: 0.0,
            tension: Ramp::new(0.0, per_sample(INTENSITY_SECONDS)),
            bpm,
            target_bpm: bpm,
            samples_per_step: step_samples(rate, bpm),
            until_step: 0.0,
            steps_played: 0,
            rng,
            pattern,
            opening: Ramp::new(0.0, 0.0),
            gains: FADE_SECONDS.map(|seconds| Ramp::new(0.0, per_sample(seconds))),
            master: Ramp::new(0.0, per_sample(MASTER_FADE_SECONDS)),
            kick: Kick::new(rate, noise_seed()),
            rumble: Rumble::new(rate),
            acid: Acid::new(rate),
            hats: Hats::new(rate, noise_seed()),
            clap: Clap::new(rate, noise_seed()),
            pad: Pad::new(rate),
        }
    }

    /// Follows `mood` from now on: levels fade, the tempo changes at the next bar.
    /// Setting the current mood again changes nothing.
    pub fn set_mood(&mut self, mood: Mood) {
        self.mood = mood;
        let Some(scene) = mood.scene() else {
            self.master.set_target(0.0);
            return;
        };
        self.master.set_target(1.0);
        for (gain, level) in self.gains.iter_mut().zip(scene.levels) {
            gain.set_target(level);
        }
        self.target_bpm = scene.bpm;
        self.opening.set_target(scene.opening);
        self.opening.set_rate(1.0 / scene.opening_steps);
    }

    /// The mood the music follows.
    pub fn mood(&self) -> Mood {
        self.mood
    }

    /// Sets how tense the fight is, `0.0..=1.0` (clamped; NaN counts as `0.0`). The sound
    /// follows within half a second.
    pub fn set_intensity(&mut self, intensity: f32) {
        self.intensity = clamp_intensity(intensity);
        self.tension.set_target(self.intensity);
    }

    /// The intensity last set, already clamped.
    pub fn intensity(&self) -> f32 {
        self.intensity
    }

    /// Fills `out` with the next frames: `channels` interleaved samples per frame (0 counts as
    /// 1). Every sample is finite and within `-OUTPUT_PEAK..=OUTPUT_PEAK`.
    ///
    /// Mono gets the mix, stereo gets the mix with slightly panned hats, extra channels (5.1...)
    /// get the mono mix. A trailing partial frame is filled with the mono mix of one more
    /// frame.
    pub fn render(&mut self, out: &mut [f32], channels: usize) {
        for frame in out.chunks_mut(channels.max(1)) {
            let (left, right) = self.next_frame();
            frame.fill(0.5 * (left + right));
            if let [first, second, ..] = frame {
                *first = left;
                *second = right;
            }
        }
    }

    /// The current tempo in beats per minute.
    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    /// The length of a sixteenth note at the current tempo, in samples (fractional: steps
    /// start on the first sample at or after their exact time).
    pub fn samples_per_step(&self) -> f64 {
        self.samples_per_step
    }

    /// The sample rate in hertz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The seed of the tune.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The acid pattern currently playing.
    pub fn pattern(&self) -> &Pattern {
        &self.pattern
    }

    /// Sixteenth notes started since the creation (the sequencer pauses while silent).
    pub fn steps_played(&self) -> u64 {
        self.steps_played
    }

    /// The bar of the latest step started (`0` before the first one).
    pub fn bar(&self) -> u64 {
        self.steps_played.saturating_sub(1) / STEPS as u64
    }

    /// The index in the bar (`0..STEPS`) of the latest step started (`0` before the first).
    pub fn step(&self) -> usize {
        (self.steps_played.saturating_sub(1) % STEPS as u64) as usize
    }

    /// Starts the next sixteenth note: tempo and pattern changes at bar boundaries, then the
    /// triggers of every instrument.
    fn play_step(&mut self) {
        let index = self.steps_played as usize % STEPS;
        let bar = self.steps_played / STEPS as u64;
        if index == 0 {
            self.bpm = self.target_bpm;
            self.samples_per_step = step_samples(self.rate, self.bpm);
            self.rumble.set_delay(2.0 * self.samples_per_step as f32);
            if bar > 0 && bar.is_multiple_of(PHRASE_BARS) {
                self.pattern.mutate(&mut self.rng);
            }
        }
        self.until_step += self.samples_per_step;
        self.steps_played += 1;
        self.opening.next();

        let tension = self.tension.value();
        let step = self.pattern.steps()[index];
        let next = self.pattern.steps()[(index + 1) % STEPS];
        let accent = step.accent || (tension >= 0.5 && index % 4 == 2);
        let decay = 0.28 - 0.1 * tension;
        let step_length = self.samples_per_step as f32;
        self.acid
            .trigger(step, !next.rest, accent, decay, step_length);

        let phrase_end = bar % PHRASE_BARS == PHRASE_BARS - 1;
        if index.is_multiple_of(4) {
            self.kick.trigger(1.0);
        } else if phrase_end && index == 14 {
            self.kick.trigger(0.6);
        }
        if index % 4 == 2 {
            self.hats.trigger_open(0.8);
        } else {
            let velocity = if index.is_multiple_of(4) { 0.6 } else { 0.32 };
            let accent = if step.accent { 0.15 } else { 0.0 };
            let pan = if index % 4 == 1 { -0.35 } else { 0.35 };
            self.hats.trigger_closed(velocity + accent, pan);
        }
        if index == 4 || index == 12 {
            self.clap.trigger();
        }
    }

    /// The slow modulations, both `0.0..=1.0`: the 8-bar cut-off sweep and a 32-bar drift.
    fn lfos(&self) -> (f32, f32) {
        let within = (1.0 - self.until_step / self.samples_per_step).clamp(0.0, 1.0) as f32;
        let steps = (self.steps_played.saturating_sub(1) % (32 * STEPS as u64)) as f32 + within;
        let phrase = steps / (PHRASE_BARS as f32 * STEPS as f32);
        let drift = steps / (32.0 * STEPS as f32);
        (
            0.5 - 0.5 * (TAU * phrase).cos(),
            0.5 - 0.5 * (TAU * drift).cos(),
        )
    }

    /// The next `(left, right)` frame.
    fn next_frame(&mut self) -> (f32, f32) {
        if self.master.is_silent() {
            return (0.0, 0.0);
        }
        if self.until_step <= 0.0 {
            self.play_step();
        }
        self.until_step -= 1.0;

        let tension = self.tension.next();
        let gains = self.gains.each_mut().map(Ramp::next);
        let master = self.master.next();
        let (sweep, drift) = self.lfos();
        let opening = self.opening.value();

        let kick = self.kick.process();
        let rumble = self.rumble.process(kick, self.kick.envelope());
        let acid = self.acid.process(&AcidParams {
            base_octaves: 0.5 + opening * (1.3 + 1.9 * sweep) + 1.2 * tension,
            env_octaves: 2.0 + opening + 1.3 * tension,
            accent_octaves: 1.2,
            resonance: 4.0 * (0.6 + 0.12 * sweep + 0.05 * drift + 0.2 * tension),
            drive: 1.4 + 2.6 * tension,
        });
        let (hats_mid, hats_side) = self.hats.process();
        let clap = self.clap.process();
        let pad = if self.gains[PAD].is_silent() {
            0.0
        } else {
            self.pad.process(drift)
        };

        let hats_level = 0.09 * gains[HATS] * (1.0 + 0.3 * tension);
        let mid = 0.55 * gains[KICK] * kick
            + 0.32 * gains[RUMBLE] * rumble
            + 0.3 * gains[ACID] * acid
            + 0.3 * gains[CLAP] * clap
            + 0.7 * gains[PAD] * pad
            + hats_level * hats_mid;
        let side = hats_level * hats_side;
        let master = MIX_GAIN * master;
        (
            OUTPUT_PEAK * soft_clip((mid + side) * master),
            OUTPUT_PEAK * soft_clip((mid - side) * master),
        )
    }
}
