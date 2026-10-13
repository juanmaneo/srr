//! A tiny deterministic pseudo-random generator (SplitMix64).

/// A deterministic pseudo-random generator (Sebastiano Vigna's SplitMix64).
///
/// Fast, tiny, statistically good enough for music, and the same sequence on every platform,
/// so a seed always produces the same tune. Not for cryptography.
///
/// ```
/// use srr::audio::Rng;
///
/// let (mut a, mut b) = (Rng::new(42), Rng::new(42));
/// assert_eq!(a.next_u64(), b.next_u64());
/// assert!(a.below(7) < 7);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator whose whole sequence is determined by `seed` (every seed is valid).
    pub const fn new(seed: u64) -> Self {
        Rng { state: seed }
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..bound` (`0` when `bound` is `0`), by fixed-point multiplication of 32
    /// random bits (bias below 2^-32 per value, irrelevant here).
    pub fn below(&mut self, bound: u32) -> u32 {
        let bits = self.next_u64() >> 32;
        ((bits * u64::from(bound)) >> 32) as u32
    }

    /// A number uniformly distributed in `0.0..1.0` (24 bits of precision).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u32 << 24) as f32
    }

    /// `true` with probability `p` (never for `p <= 0`, always for `p >= 1`).
    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }

    /// An index into `weights`, each picked with a probability proportional to its weight.
    /// Returns the last index when every weight is zero, and `0` for an empty slice.
    pub fn weighted(&mut self, weights: &[u32]) -> usize {
        let total = weights.iter().sum();
        let mut pick = self.below(total);
        let mut index = 0;
        while index + 1 < weights.len() && pick >= weights[index] {
            pick -= weights[index];
            index += 1;
        }
        index
    }
}
