//! The procedurally generated music: determinism, safety of the output, moods, tempo, the
//! acid pattern and its mutations, and the thread-safe control handle.
//!
//! Most tests run at a low sample rate ([`RATE`]) to stay fast: the synthesis is the same at
//! any rate, only the clamped filter cut-offs differ.

use srr::audio::{
    DEFAULT_SEED, HIGHEST_NOTE, MUTATED_STEPS, Mood, Music, MusicControl, OCTAVES, OUTPUT_PEAK,
    PHRASE_BARS, Pattern, ROOT_NOTE, Rng, SCALE, STEPS, in_scale,
};

/// The sample rate of most tests.
const RATE: u32 = 8_000;

/// A music already playing `mood` at intensity 0.
fn playing(mood: Mood) -> Music {
    let mut music = Music::new(RATE, DEFAULT_SEED);
    music.set_mood(mood);
    music
}

/// Renders `frames` frames of `channels` channels.
fn render(music: &mut Music, frames: usize, channels: usize) -> Vec<f32> {
    let mut out = vec![f32::NAN; frames * channels];
    music.render(&mut out, channels);
    out
}

/// Renders `seconds` of mono.
fn render_seconds(music: &mut Music, seconds: f32) -> Vec<f32> {
    render(music, (seconds * music.sample_rate() as f32) as usize, 1)
}

/// Frames in `bars` bars at `bpm`.
fn bar_frames(rate: u32, bpm: f32, bars: u64) -> usize {
    (rate as f64 * 60.0 / bpm as f64 * 4.0 * bars as f64).ceil() as usize + 1
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |peak, s| peak.max(s.abs()))
}

fn energy(samples: &[f32]) -> f64 {
    samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum()
}

fn assert_safe(samples: &[f32], context: &str) {
    for (i, s) in samples.iter().enumerate() {
        assert!(
            s.is_finite() && s.abs() <= OUTPUT_PEAK,
            "{context}: sample {i} is {s}"
        );
    }
}

/// A 2-pole Butterworth low-pass (RBJ biquad), to measure the bass of the mix.
struct LowPass {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl LowPass {
    fn new(cutoff: f32, rate: u32) -> Self {
        let w = std::f32::consts::TAU * cutoff / rate as f32;
        let alpha = w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let (c, a0) = (w.cos(), 1.0 + alpha);
        let b0 = (1.0 - c) / 2.0 / a0;
        LowPass {
            b: [b0, 2.0 * b0, b0],
            a: [-2.0 * c / a0, (1.0 - alpha) / a0],
            z: [0.0; 2],
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// Renders one frame at a time for `frames` frames; returns the mono samples and the frame
/// index at which each step started.
fn render_tracking_steps(music: &mut Music, frames: usize) -> (Vec<f32>, Vec<usize>) {
    let mut samples = Vec::with_capacity(frames);
    let mut onsets = Vec::new();
    for frame in 0..frames {
        let before = music.steps_played();
        let mut out = [0.0];
        music.render(&mut out, 1);
        if music.steps_played() != before {
            onsets.push(frame);
        }
        samples.push(out[0]);
    }
    (samples, onsets)
}

// ---------------------------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------------------------

/// Plays a scripted session (every mood, intensity changes, odd buffer sizes) and returns the
/// output.
fn session(seed: u64) -> Vec<f32> {
    let mut music = Music::new(RATE, seed);
    let mut out = Vec::new();
    let script = [
        (Mood::Intro, 0.0, 333),
        (Mood::Fight, 0.3, 1_021),
        (Mood::Fight, 1.0, 7),
        (Mood::Breakdown, 0.6, 4_096),
        (Mood::Silent, 0.0, 1),
        (Mood::Fight, f32::NAN, 2_000),
    ];
    for (mood, intensity, chunk) in script {
        music.set_mood(mood);
        music.set_intensity(intensity);
        for _ in 0..(12_000 / chunk).max(5) {
            out.extend(render(&mut music, chunk, 2));
        }
    }
    out
}

#[test]
fn the_same_seed_renders_bit_identical_samples() {
    let (a, b) = (session(DEFAULT_SEED), session(DEFAULT_SEED));
    assert_eq!(a.len(), b.len());
    assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
    assert!(peak(&a) > 0.1, "the session is not silent");
}

#[test]
fn different_seeds_play_different_patterns_and_samples() {
    let (a, b) = (Music::new(RATE, 1), Music::new(RATE, 2));
    assert_ne!(a.pattern(), b.pattern());
    assert_ne!(session(1), session(2));
}

#[test]
fn the_pattern_comes_from_the_seed() {
    for seed in [0, 1, DEFAULT_SEED, u64::MAX] {
        let music = Music::new(RATE, seed);
        assert_eq!(music.seed(), seed);
        assert_eq!(*music.pattern(), Pattern::generate(&mut Rng::new(seed)));
    }
}

#[test]
fn the_buffer_size_does_not_change_the_music() {
    let mut whole = playing(Mood::Fight);
    let mut sliced = playing(Mood::Fight);
    let expected = render(&mut whole, 20_000, 2);
    let mut actual = Vec::new();
    for size in [1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987] {
        actual.extend(render(&mut sliced, size, 2));
    }
    actual.extend(render(&mut sliced, 20_000 - actual.len() / 2, 2));
    assert_eq!(expected, actual);
}

// ---------------------------------------------------------------------------------------------
// Output safety and channels
// ---------------------------------------------------------------------------------------------

#[test]
fn every_sample_is_finite_and_within_the_output_peak_in_every_mood() {
    let mut chunks = [1_usize, 7, 333, 1_021].into_iter().cycle();
    let mut channel_counts = [1_usize, 2, 6].into_iter().cycle();
    for mood in Mood::ALL {
        for intensity in [0.0_f32, 0.5, 1.0] {
            let mut music = Music::new(RATE, DEFAULT_SEED ^ u64::from(intensity.to_bits()));
            music.set_mood(mood);
            music.set_intensity(intensity);
            // Past the first mutation (bar 8) and the doubled kick closing the phrase.
            let frames = bar_frames(RATE, 138.0, PHRASE_BARS + 1);
            let mut rendered = 0;
            while rendered < frames {
                let (chunk, channels) = (chunks.next().unwrap(), channel_counts.next().unwrap());
                assert_safe(&render(&mut music, chunk, channels), &format!("{mood:?}"));
                rendered += chunk;
            }
        }
    }
}

#[test]
fn the_output_is_safe_at_extreme_sample_rates() {
    for rate in [0, 1, 100, 22_050, 192_000] {
        let mut music = Music::new(rate, DEFAULT_SEED);
        assert_eq!(music.sample_rate(), rate.max(1));
        music.set_mood(Mood::Fight);
        music.set_intensity(1.0);
        let frames = (music.sample_rate() as usize * 3).max(200);
        assert_safe(&render(&mut music, frames, 2), &format!("{rate} Hz"));
        music.set_mood(Mood::Breakdown);
        assert_safe(&render(&mut music, frames, 2), &format!("{rate} Hz"));
    }
}

#[test]
fn the_mix_peaks_near_minus_three_dbfs_and_is_bass_heavy() {
    let mut music = playing(Mood::Fight);
    render_seconds(&mut music, 3.0);
    let samples = render_seconds(&mut music, 10.0);
    let level = peak(&samples);
    assert!(level > 0.5 && level <= OUTPUT_PEAK, "peak {level}");

    let (mut first, mut second) = (LowPass::new(120.0, RATE), LowPass::new(120.0, RATE));
    let low: Vec<f32> = samples
        .iter()
        .map(|&s| second.process(first.process(s)))
        .collect();
    let share = energy(&low) / energy(&samples);
    assert!(share > 0.6, "only {share} of the energy below 120 Hz");
}

#[test]
fn stereo_pans_the_hats_and_other_layouts_get_the_mono_mix() {
    let frames = 5_000;
    let stereo = render(&mut playing(Mood::Fight), frames, 2);
    let mono = render(&mut playing(Mood::Fight), frames, 1);
    let surround = render(&mut playing(Mood::Fight), frames, 6);
    let mut panned = 0;
    for i in 0..frames {
        let (left, right) = (stereo[2 * i], stereo[2 * i + 1]);
        let mix = 0.5 * (left + right);
        assert_eq!(mono[i], mix);
        assert_eq!(&surround[6 * i..6 * i + 2], &[left, right]);
        assert!(surround[6 * i + 2..6 * i + 6].iter().all(|&s| s == mix));
        if left != right {
            panned += 1;
        }
    }
    assert!(panned > frames / 2, "the hats are not panned");
}

#[test]
fn zero_channels_count_as_one_and_a_partial_frame_gets_the_mono_mix() {
    let zero = render(&mut playing(Mood::Fight), 1_000, 1);
    let mut music = playing(Mood::Fight);
    let mut out = vec![0.0; 1_000];
    music.render(&mut out, 0);
    assert_eq!(out, zero);

    let stereo = render(&mut playing(Mood::Fight), 3, 2);
    let mut odd = [0.0; 5];
    playing(Mood::Fight).render(&mut odd, 2);
    assert_eq!(&odd[..4], &stereo[..4]);
    assert_eq!(odd[4], 0.5 * (stereo[4] + stereo[5]));

    playing(Mood::Fight).render(&mut [], 2);
}

// ---------------------------------------------------------------------------------------------
// Moods and transitions
// ---------------------------------------------------------------------------------------------

#[test]
fn a_new_music_is_silent_and_paused() {
    let mut music = Music::new(RATE, DEFAULT_SEED);
    assert_eq!(music.mood(), Mood::Silent);
    assert!(render(&mut music, 10_000, 2).iter().all(|&s| s == 0.0));
    assert_eq!(music.steps_played(), 0);
    assert_eq!((music.bar(), music.step()), (0, 0));
    assert_eq!(music.bpm(), 138.0, "starts at the intro tempo");
}

#[test]
fn moods_fade_in_instead_of_starting_with_a_jump() {
    let mut music = playing(Mood::Fight);
    let start = render(&mut music, 80, 1);
    assert!(peak(&start) < 0.02, "{}", peak(&start));
    render_seconds(&mut music, 2.0);
    assert!(peak(&render_seconds(&mut music, 1.0)) > 0.4);
}

#[test]
fn silent_fades_out_then_outputs_true_silence_and_pauses() {
    let mut music = playing(Mood::Fight);
    render_seconds(&mut music, 2.0);
    music.set_mood(Mood::Silent);
    assert_eq!(music.mood(), Mood::Silent);
    let fading = render_seconds(&mut music, 1.6);
    let tenth = RATE as usize / 10;
    assert!(peak(&fading[..tenth]) > 0.3, "cut instead of faded");
    assert!(peak(&fading[..tenth]) > peak(&fading[12 * tenth..14 * tenth]));
    let steps = music.steps_played();
    assert!(render_seconds(&mut music, 2.0).iter().all(|&s| s == 0.0));
    assert_eq!(
        music.steps_played(),
        steps,
        "the sequencer pauses while silent"
    );
    assert_eq!(music.bpm(), 145.0, "silence keeps the tempo");

    music.set_mood(Mood::Fight);
    render_seconds(&mut music, 2.0);
    assert!(music.steps_played() > steps, "and resumes");
}

/// The energy of the bass (below 150 Hz) in the 40 ms after each beat, over 4 bars once the
/// mood has settled.
fn bass_on_the_beats(mood: Mood) -> f64 {
    let mut music = playing(mood);
    render_seconds(&mut music, 4.0);
    let (samples, onsets) = render_tracking_steps(&mut music, bar_frames(RATE, 145.0, 4));
    let steps_before = music.steps_played() - onsets.len() as u64;
    let mut low_pass = LowPass::new(150.0, RATE);
    let bass: Vec<f32> = samples.iter().map(|&s| low_pass.process(s)).collect();
    let window = RATE as usize / 25;
    let beats: Vec<usize> = onsets
        .iter()
        .enumerate()
        .filter(|&(i, _)| (steps_before + i as u64).is_multiple_of(4))
        .map(|(_, &frame)| frame)
        .collect();
    assert!(beats.len() >= 15, "{} beats", beats.len());
    beats
        .iter()
        .map(|&b| energy(&bass[b..(b + window).min(bass.len())]))
        .sum()
}

#[test]
fn the_breakdown_drops_the_kick() {
    let fight = bass_on_the_beats(Mood::Fight);
    let breakdown = bass_on_the_beats(Mood::Breakdown);
    assert!(
        breakdown < fight / 8.0,
        "breakdown {breakdown} fight {fight}"
    );
}

#[test]
fn the_intro_has_kick_but_no_clap() {
    let intro = bass_on_the_beats(Mood::Intro);
    let fight = bass_on_the_beats(Mood::Fight);
    assert!(intro > fight * 0.5, "intro {intro} fight {fight}");

    // The clap sits on beats 2 and 4 (steps 4 and 12): compare the mid band there with the
    // mid band on beats 1 and 3 (steps 0 and 8), where the clap never plays.
    let backbeat_ratio = |mood: Mood| {
        let mut music = playing(mood);
        render_seconds(&mut music, 3.0);
        let (samples, onsets) = render_tracking_steps(&mut music, bar_frames(RATE, 145.0, 2));
        let (mut low, mut band) = (LowPass::new(2_500.0, RATE), LowPass::new(700.0, RATE));
        let mids: Vec<f32> = samples
            .iter()
            .map(|&s| {
                let below = low.process(s);
                below - band.process(below)
            })
            .collect();
        let first = music.steps_played() - onsets.len() as u64;
        let mids_on = |steps: [u64; 2]| {
            onsets
                .iter()
                .enumerate()
                .filter(|&(i, _)| steps.contains(&((first + i as u64) % 16)))
                .map(|(_, &frame)| energy(&mids[frame..frame + 300]))
                .sum::<f64>()
        };
        mids_on([4, 12]) / mids_on([0, 8])
    };
    let (intro, fight) = (backbeat_ratio(Mood::Intro), backbeat_ratio(Mood::Fight));
    assert!(fight > intro * 2.0, "intro {intro} fight {fight}");
}

// ---------------------------------------------------------------------------------------------
// Tempo
// ---------------------------------------------------------------------------------------------

#[test]
fn each_mood_plays_at_its_tempo() {
    for (mood, bpm) in [
        (Mood::Intro, 138.0),
        (Mood::Fight, 145.0),
        (Mood::Breakdown, 145.0),
    ] {
        assert_eq!(mood.bpm(), Some(bpm));
        let mut music = Music::new(48_000, DEFAULT_SEED);
        music.set_mood(mood);
        let (_, onsets) = render_tracking_steps(&mut music, bar_frames(48_000, bpm, 2));
        assert_eq!(music.bpm(), bpm);
        let expected = 48_000.0 * 60.0 / (f64::from(bpm) * 4.0);
        assert_eq!(music.samples_per_step(), expected);
        assert_eq!(onsets.len(), 2 * STEPS + 1);
        for (k, &frame) in onsets.iter().enumerate() {
            let exact = k as f64 * expected;
            assert!(
                frame as f64 >= exact - 1e-6 && (frame as f64) < exact + 1.0 + 1e-6,
                "{mood:?}: step {k} at frame {frame}, expected {exact}"
            );
        }
    }
    assert_eq!(Mood::Silent.bpm(), None);
}

#[test]
fn the_tempo_changes_on_the_next_bar() {
    let mut music = playing(Mood::Fight);
    render(&mut music, bar_frames(RATE, 145.0, 1) + 3 * 828, 1);
    assert_eq!(music.bar(), 1);
    assert_eq!(music.bpm(), 145.0);
    music.set_mood(Mood::Intro);
    while music.bar() == 1 {
        render(&mut music, 1, 1);
        if music.bar() == 1 {
            assert_eq!(music.bpm(), 145.0, "changed in the middle of a bar");
        }
    }
    assert_eq!((music.bar(), music.step()), (2, 0));
    assert_eq!(music.bpm(), 138.0);
}

// ---------------------------------------------------------------------------------------------
// The acid pattern
// ---------------------------------------------------------------------------------------------

#[test]
fn the_scale_is_a_phrygian_from_a1() {
    assert_eq!(SCALE, [0, 1, 3, 5, 7, 8, 10]);
    assert_eq!(ROOT_NOTE, 33);
    assert_eq!(HIGHEST_NOTE, ROOT_NOTE + 12 * (OCTAVES - 1) + 10);
    let in_range: Vec<u8> = (0..=127).filter(|&n| in_scale(n)).collect();
    assert_eq!(in_range.len(), SCALE.len() * OCTAVES as usize);
    assert_eq!(in_range.first(), Some(&ROOT_NOTE));
    assert_eq!(in_range.last(), Some(&HIGHEST_NOTE));
}

#[test]
fn pattern_notes_stay_in_the_scale_and_the_line_is_root_heavy() {
    let mut roots = 0;
    let mut notes = 0;
    for seed in 0..300 {
        let mut rng = Rng::new(seed);
        let mut pattern = Pattern::generate(&mut rng);
        for _ in 0..40 {
            let steps = pattern.steps();
            assert!(steps.iter().all(|s| in_scale(s.note)), "{pattern:?}");
            assert_eq!((steps[0].note, steps[0].rest), (ROOT_NOTE, false));
            notes += steps.len();
            roots += steps
                .iter()
                .filter(|s| (s.note - ROOT_NOTE).is_multiple_of(12))
                .count();
            pattern.mutate(&mut rng);
        }
    }
    let share = roots as f64 / notes as f64;
    assert!(share > 0.3, "only {share} roots");
}

#[test]
fn generated_patterns_have_notes_rests_accents_and_slides() {
    let (mut rests, mut accents, mut slides, mut total) = (0, 0, 0, 0);
    for seed in 0..200 {
        for step in Pattern::generate(&mut Rng::new(seed)).steps() {
            total += 1;
            rests += usize::from(step.rest);
            accents += usize::from(step.accent);
            slides += usize::from(step.slide);
        }
    }
    let share = |count: usize| count as f64 / total as f64;
    assert!((0.15..0.4).contains(&share(rests)), "{}", share(rests));
    assert!((0.2..0.4).contains(&share(accents)), "{}", share(accents));
    assert!((0.1..0.3).contains(&share(slides)), "{}", share(slides));
}

#[test]
fn a_mutation_changes_exactly_a_few_steps_but_never_the_anchor() {
    for seed in 0..500 {
        let mut rng = Rng::new(seed);
        let original = Pattern::generate(&mut rng);
        let mut mutated = original;
        mutated.mutate(&mut rng);
        assert_eq!(original.differences(&mutated), MUTATED_STEPS, "seed {seed}");
        assert_eq!(original.steps()[0], mutated.steps()[0]);
    }
    const { assert!(MUTATED_STEPS * 4 <= STEPS, "most steps are kept") };
}

#[test]
fn the_pattern_mutates_every_phrase_of_eight_bars() {
    let mut music = playing(Mood::Fight);
    let mut previous = *music.pattern();
    let mut bar = 0;
    let mut mutations = 0;
    let frames = bar_frames(RATE, 145.0, 2 * PHRASE_BARS);
    for _ in 0..frames {
        render(&mut music, 1, 1);
        if music.bar() != bar {
            bar = music.bar();
            let changed = previous.differences(music.pattern());
            if bar.is_multiple_of(PHRASE_BARS) {
                assert_eq!(changed, MUTATED_STEPS, "bar {bar}");
                mutations += 1;
            } else {
                assert_eq!(changed, 0, "bar {bar}");
            }
            previous = *music.pattern();
        }
    }
    assert_eq!((bar, mutations), (2 * PHRASE_BARS, 2));
}

// ---------------------------------------------------------------------------------------------
// Intensity
// ---------------------------------------------------------------------------------------------

#[test]
fn intensity_is_clamped_and_nan_counts_as_zero() {
    let mut music = Music::new(RATE, DEFAULT_SEED);
    let control = MusicControl::new();
    for (asked, expected) in [
        (0.25, 0.25),
        (2.0, 1.0),
        (-1.0, 0.0),
        (f32::INFINITY, 1.0),
        (f32::NEG_INFINITY, 0.0),
        (f32::NAN, 0.0),
    ] {
        music.set_intensity(asked);
        control.set_intensity(asked);
        assert_eq!(music.intensity(), expected, "{asked}");
        assert_eq!(control.intensity(), expected, "{asked}");
    }
}

/// The energy of the sample-to-sample differences: a crude measure of brightness.
fn brightness(samples: &[f32]) -> f64 {
    samples
        .windows(2)
        .map(|w| f64::from(w[1] - w[0]).powi(2))
        .sum()
}

#[test]
fn intensity_opens_the_acid_filter() {
    let breakdown_at = |intensity: f32| {
        let mut music = Music::new(22_050, DEFAULT_SEED);
        music.set_mood(Mood::Breakdown);
        music.set_intensity(intensity);
        render_seconds(&mut music, 3.0);
        brightness(&render_seconds(&mut music, 6.0))
    };
    let (calm, tense) = (breakdown_at(0.0), breakdown_at(1.0));
    assert!(tense > calm * 2.0, "calm {calm} tense {tense}");
}

#[test]
fn the_intro_line_opens_slowly() {
    // In the breakdown only the acid line and the drone play; start it from the closed intro
    // filter and compare its brightness right away and once the filter opened.
    let mut music = Music::new(22_050, DEFAULT_SEED);
    music.set_mood(Mood::Intro);
    render_seconds(&mut music, 0.5);
    music.set_mood(Mood::Breakdown);
    let early = brightness(&render_seconds(&mut music, 2.0));
    render_seconds(&mut music, 6.0);
    let late = brightness(&render_seconds(&mut music, 2.0));
    assert!(late > early * 2.0, "early {early} late {late}");
}

// ---------------------------------------------------------------------------------------------
// MusicControl
// ---------------------------------------------------------------------------------------------

#[test]
fn the_control_starts_silent_and_calm() {
    let control = MusicControl::new();
    assert_eq!(control.mood(), Mood::Silent);
    assert_eq!(control.intensity(), 0.0);
    assert_eq!(Mood::default(), Mood::Silent);
}

#[test]
fn the_control_round_trips_every_mood_and_is_shared_between_clones() {
    let control = MusicControl::default();
    let audio_thread = control.clone();
    for mood in Mood::ALL {
        control.set_mood(mood);
        assert_eq!(control.mood(), mood);
        assert_eq!(audio_thread.mood(), mood);
    }
    audio_thread.set_intensity(0.75);
    assert_eq!(control.intensity(), 0.75);
    let unrelated = MusicControl::new();
    assert_eq!(unrelated.intensity(), 0.0);
}

#[test]
fn the_control_works_across_threads() {
    let control = MusicControl::new();
    let game = control.clone();
    std::thread::spawn(move || {
        game.set_mood(Mood::Breakdown);
        game.set_intensity(0.5);
    })
    .join()
    .unwrap();
    let mut music = Music::new(RATE, DEFAULT_SEED);
    music.set_mood(control.mood());
    music.set_intensity(control.intensity());
    assert_eq!((music.mood(), music.intensity()), (Mood::Breakdown, 0.5));
}

#[test]
fn setting_the_same_mood_again_changes_nothing() {
    let mut once = playing(Mood::Fight);
    let mut repeated = playing(Mood::Fight);
    let expected = render(&mut once, 9_000, 1);
    let mut actual = Vec::new();
    for _ in 0..90 {
        repeated.set_mood(Mood::Fight);
        actual.extend(render(&mut repeated, 100, 1));
    }
    assert_eq!(expected, actual);
}

// ---------------------------------------------------------------------------------------------
// Rng
// ---------------------------------------------------------------------------------------------

#[test]
fn the_rng_is_deterministic_and_seed_dependent() {
    let sequence = |seed| {
        let mut rng = Rng::new(seed);
        (0..8).map(|_| rng.next_u64()).collect::<Vec<_>>()
    };
    assert_eq!(sequence(5), sequence(5));
    assert_ne!(sequence(5), sequence(6));
    assert_eq!(Rng::new(9), Rng::new(9).clone());
}

#[test]
fn the_rng_helpers_stay_in_their_ranges() {
    let mut rng = Rng::new(DEFAULT_SEED);
    let mut seen = [false; 7];
    for _ in 0..10_000 {
        let n = rng.below(7);
        seen[n as usize] = true;
        let u = rng.unit();
        assert!((0.0..1.0).contains(&u));
    }
    assert!(seen.iter().all(|&s| s));
    assert_eq!(rng.below(0), 0);
    assert!((0..100).all(|_| !rng.chance(0.0) && rng.chance(1.0)));
}

#[test]
fn weighted_picks_follow_their_weights() {
    let mut rng = Rng::new(3);
    let mut counts = [0; 3];
    for _ in 0..30_000 {
        counts[rng.weighted(&[1, 0, 2])] += 1;
    }
    assert_eq!(counts[1], 0);
    assert!((9_000..11_000).contains(&counts[0]), "{counts:?}");
    assert_eq!(rng.weighted(&[0, 0, 0]), 2);
    assert_eq!(rng.weighted(&[]), 0);
}
