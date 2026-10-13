//! The 16-step TB-303 style acid pattern: seeded generation and gradual mutation.

use super::rng::Rng;

/// Steps in a pattern: one bar of sixteenth notes.
pub const STEPS: usize = 16;

/// The dark scale of every note, in semitones above the root: A phrygian
/// (A B♭ C D E F G), the minor scale with a flat second.
pub const SCALE: [u8; 7] = [0, 1, 3, 5, 7, 8, 10];

/// The root and lowest note of the acid line, as a MIDI note number: A1 (55 Hz).
pub const ROOT_NOTE: u8 = 33;

/// Octaves spanned by the acid line, starting at [`ROOT_NOTE`].
pub const OCTAVES: u8 = 3;

/// The highest note the acid line can play: the top of [`SCALE`] in the last octave
/// (G4, MIDI 67).
pub const HIGHEST_NOTE: u8 = ROOT_NOTE + 12 * (OCTAVES - 1) + SCALE[SCALE.len() - 1];

/// Steps changed by each [`Pattern::mutate`]: few enough that the groove stays recognisable.
pub const MUTATED_STEPS: usize = 3;

/// How likely each [`SCALE`] degree is: root-heavy, then the dark b2, b3 and fifth.
const DEGREE_WEIGHTS: [u32; 7] = [12, 3, 3, 2, 3, 2, 2];
/// How likely each octave is: mostly the low two.
const OCTAVE_WEIGHTS: [u32; OCTAVES as usize] = [9, 7, 3];

/// Whether `note` (MIDI) belongs to the acid line's range and scale.
///
/// ```
/// use srr::audio::{in_scale, ROOT_NOTE};
///
/// assert!(in_scale(ROOT_NOTE));       // A1
/// assert!(in_scale(ROOT_NOTE + 1));   // B♭1, the flat second
/// assert!(!in_scale(ROOT_NOTE + 2));  // B1 is not in A phrygian
/// assert!(!in_scale(ROOT_NOTE - 12)); // A0 is below the range
/// ```
pub fn in_scale(note: u8) -> bool {
    (ROOT_NOTE..=HIGHEST_NOTE).contains(&note) && SCALE.contains(&((note - ROOT_NOTE) % 12))
}

/// One sixteenth of the acid line, as on a TB-303 sequencer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Step {
    /// The MIDI note; always satisfies [`in_scale`], even on a rest (so a mutation that
    /// un-rests the step has a note to play).
    pub note: u8,
    /// Silence instead of a note.
    pub rest: bool,
    /// Louder, with a stronger and snappier filter envelope.
    pub accent: bool,
    /// Holds the note into the next step and glides to its pitch without retriggering the
    /// envelopes (only when the next step is not a rest).
    pub slide: bool,
}

impl Step {
    /// The index of the note in [`SCALE`].
    fn degree(self) -> usize {
        let semitones = (self.note - ROOT_NOTE) % 12;
        SCALE.iter().position(|&s| s == semitones).unwrap_or(0)
    }

    /// The octave of the note above [`ROOT_NOTE`], `0..OCTAVES`.
    fn octave(self) -> u8 {
        (self.note - ROOT_NOTE) / 12
    }
}

/// The note of `degree` (index into [`SCALE`]) in `octave` (`0..OCTAVES`).
fn note(degree: usize, octave: u8) -> u8 {
    ROOT_NOTE + 12 * octave + SCALE[degree]
}

/// A random root-heavy note of the scale.
fn random_note(rng: &mut Rng) -> u8 {
    let degree = rng.weighted(&DEGREE_WEIGHTS);
    let octave = rng.weighted(&OCTAVE_WEIGHTS) as u8;
    note(degree, octave)
}

/// A 16-step acid pattern.
///
/// The first step is the anchor: always the root note in the lowest octave, never a rest, and
/// never mutated, so the line keeps its tonal centre however long it evolves.
///
/// ```
/// use srr::audio::{in_scale, Pattern, Rng, MUTATED_STEPS, ROOT_NOTE};
///
/// let mut rng = Rng::new(7);
/// let original = Pattern::generate(&mut rng);
/// assert_eq!(original.steps()[0].note, ROOT_NOTE);
/// assert!(original.steps().iter().all(|step| in_scale(step.note)));
///
/// let mut mutated = original;
/// mutated.mutate(&mut rng);
/// assert_eq!(original.differences(&mutated), MUTATED_STEPS);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pattern {
    steps: [Step; STEPS],
}

impl Pattern {
    /// A new random pattern: about 75 % notes (more on the beats), 28 % accents, 20 % slides,
    /// root-heavy notes in the low octaves.
    pub fn generate(rng: &mut Rng) -> Self {
        let steps = std::array::from_fn(|index| {
            let gate = if index.is_multiple_of(4) { 0.9 } else { 0.7 };
            Step {
                note: random_note(rng),
                rest: !rng.chance(gate),
                accent: rng.chance(0.28),
                slide: rng.chance(0.2),
            }
        });
        let mut pattern = Pattern { steps };
        pattern.steps[0].note = ROOT_NOTE;
        pattern.steps[0].rest = false;
        pattern
    }

    /// The sixteen steps, in playing order.
    pub fn steps(&self) -> &[Step; STEPS] {
        &self.steps
    }

    /// Changes exactly [`MUTATED_STEPS`] distinct steps (never the first one). Each picked
    /// step gets one change that always alters it: another degree of the scale (40 %, still
    /// root-heavy), another octave (20 %), or its accent (20 %), slide (10 %) or rest (10 %)
    /// flipped. Notes stay in the scale.
    pub fn mutate(&mut self, rng: &mut Rng) {
        let mut candidates: [usize; STEPS - 1] = std::array::from_fn(|i| i + 1);
        for k in 0..MUTATED_STEPS {
            let pick = k + rng.below((candidates.len() - k) as u32) as usize;
            candidates.swap(k, pick);
            let step = &mut self.steps[candidates[k]];
            match rng.below(10) {
                0..=3 => {
                    let mut weights = DEGREE_WEIGHTS;
                    weights[step.degree()] = 0;
                    step.note = note(rng.weighted(&weights), step.octave());
                }
                4 | 5 => {
                    let octave = (step.octave() + 1 + rng.below(2) as u8) % OCTAVES;
                    step.note = note(step.degree(), octave);
                }
                6 | 7 => step.accent = !step.accent,
                8 => step.slide = !step.slide,
                _ => step.rest = !step.rest,
            }
        }
    }

    /// How many steps differ between `self` and `other`.
    pub fn differences(&self, other: &Pattern) -> usize {
        self.steps
            .iter()
            .zip(&other.steps)
            .filter(|(a, b)| a != b)
            .count()
    }
}
