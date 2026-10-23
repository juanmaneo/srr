//! Shots in flight: the fireballs thrown by projectile special moves.
//!
//! A [`Projectile`] belongs to the [`Match`](super::Match), not to its thrower: it keeps flying
//! when the thrower is hit, and every tick the match moves it and checks what it crosses (see
//! [`Match::step`](super::Match::step)). Outside the game module projectiles are read-only.

use super::character::ProjectileData;
use super::config::AttackData;
use super::fighter::Facing;
use super::roster::PlayerId;

/// One shot in flight. All accessors are O(1).
///
/// ```
/// use std::collections::HashMap;
/// use srr::game::{Character, Input, Match, MatchConfig, PlayerId, Roster};
///
/// let config = MatchConfig { countdown_ticks: 0, ..MatchConfig::default() }
///     .with_character(PlayerId(0), Character::Torch);
/// let mut m = Match::new(Roster::duel(), config).unwrap();
/// let fireball = HashMap::from([(PlayerId(0), Input { special: true, ..Input::NONE })]);
/// for _ in 0..13 {
///     m.step(&fireball);
/// }
/// let shot = &m.projectiles()[0];
/// assert_eq!(shot.owner(), PlayerId(0));
/// assert!(shot.x() > m.fighter(PlayerId(0)).unwrap().x(), "it flies towards the rival");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Projectile {
    pub(crate) owner: PlayerId,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
    pub(crate) facing: Facing,
    /// Ticks of flight left.
    pub(crate) remaining: u32,
    pub(crate) flight: ProjectileData,
    pub(crate) hit: AttackData,
}

impl Projectile {
    /// The player who threw the shot.
    pub fn owner(&self) -> PlayerId {
        self.owner
    }

    /// Position along the arena.
    pub fn x(&self) -> i32 {
        self.x
    }

    /// Height above the floor it was thrown from (the thrower's feet).
    pub fn y(&self) -> i32 {
        self.y
    }

    /// Position across the arena: the thrower's line.
    pub fn z(&self) -> i32 {
        self.z
    }

    /// The direction it flies in.
    pub fn facing(&self) -> Facing {
        self.facing
    }

    /// Ticks of flight left before it burns out.
    pub fn remaining(&self) -> u32 {
        self.remaining
    }

    /// Half its width.
    pub fn size(&self) -> i32 {
        self.flight.size
    }

    /// Where it will be after this tick's flight.
    pub(crate) fn next_x(&self) -> i32 {
        self.x
            .saturating_add(self.facing.sign().saturating_mul(self.flight.speed))
    }

    /// The span along the arena it sweeps this tick, widened by its size: `(low, high)`.
    pub(crate) fn swept(&self) -> (i64, i64) {
        let (from, to) = (i64::from(self.x), i64::from(self.next_x()));
        let size = i64::from(self.flight.size);
        (from.min(to) - size, from.max(to) + size)
    }
}
