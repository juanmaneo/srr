//! Match configuration and the registry of intentional glitches.
//!
//! [`MatchConfig`] holds every tunable rule of a match, including the frame data of the six
//! attacks ([`AttackData`]), the grappling rules ([`GrappleData`]) and the judo techniques
//! ([`TechniqueData`]), the default special move ([`SpecialData`]) and the [`Character`] of
//! every player; [`MatchConfig::validate`] is called by the match constructors.
//! [`Glitch`] and [`GlitchSet`] describe the exploits that exist on purpose and can be switched
//! on and off individually.

use std::fmt;

use super::character::{Character, SHOULDER_RUSH, SpecialData};
use super::grapple::GrapplePosition;
use super::input::{Attack, Limb};
use super::roster::{PlayerId, Roster};
use super::technique::Technique;

/// The simulation advances by one `Match::step` per tick, at this fixed rate.
pub const TICKS_PER_SECOND: u32 = 60;

/// An exploit that exists on purpose ("BERP" style). Each one can be switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Glitch {
    /// A jump pressed during an attack's recovery cancels the rest of the recovery.
    AnimationCancel,
    /// A jump pressed on the landing step re-launches the fighter with a boosted velocity.
    SuperJump,
    /// A grabbed fighter that freshly presses grab on its first tick in the grapple (the tick
    /// right after the grab connected) swaps the roles: it takes the top of a fresh clinch.
    Reversal,
}

impl Glitch {
    /// Every glitch, in declaration order.
    pub const ALL: [Glitch; 3] = [Glitch::AnimationCancel, Glitch::SuperJump, Glitch::Reversal];

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// A set of enabled glitches.
///
/// Stored as a bit mask, so the set is `Copy` and every operation is O(1). The builder methods
/// take and return the set by value.
///
/// ```
/// use srr::game::{Glitch, GlitchSet};
///
/// let set = GlitchSet::none().with(Glitch::SuperJump);
/// assert!(set.contains(Glitch::SuperJump));
/// assert!(!set.contains(Glitch::AnimationCancel));
/// assert_eq!(set.without(Glitch::SuperJump), GlitchSet::none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlitchSet(u8);

impl GlitchSet {
    /// The set with every glitch disabled.
    pub const fn none() -> Self {
        GlitchSet(0)
    }

    /// The set with every glitch in [`Glitch::ALL`] enabled.
    ///
    /// Complexity: O(g) in the number of glitches (a fold over [`Glitch::ALL`]).
    pub fn all() -> Self {
        Glitch::ALL.into_iter().fold(Self::none(), Self::with)
    }

    /// This set plus `glitch`. Adding a glitch that is already present changes nothing.
    pub const fn with(self, glitch: Glitch) -> Self {
        GlitchSet(self.0 | glitch.bit())
    }

    /// This set minus `glitch`. Removing a glitch that is absent changes nothing.
    pub const fn without(self, glitch: Glitch) -> Self {
        GlitchSet(self.0 & !glitch.bit())
    }

    /// Whether `glitch` is enabled in this set.
    pub const fn contains(self, glitch: Glitch) -> bool {
        self.0 & glitch.bit() != 0
    }
}

/// The frame data of one attack. Distances are in arena units, durations in ticks.
///
/// An attack runs `startup` ticks (the tick that reads the button is the first one), then
/// `active` ticks during which it can hit, then `recovery` ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttackData {
    /// Ticks spent winding up. May be zero.
    pub startup: u32,
    /// Ticks the attack can hit. Must be at least one.
    pub active: u32,
    /// Ticks spent cooling down after the active phase. May be zero.
    pub recovery: u32,
    /// Horizontal reach in front of the attacker. Must not be negative.
    pub range: i32,
    /// Health lost by an unblocked target. Must not be negative.
    pub damage: i32,
    /// Push applied to an unblocked target, in the attack's direction. Must not be negative.
    pub knockback: i32,
    /// Ticks an unblocked target cannot act, the hit tick excluded.
    pub hitstun: u32,
}

impl AttackData {
    /// Total length of the attack: start-up + active + recovery (saturating).
    pub const fn total(&self) -> u32 {
        self.startup
            .saturating_add(self.active)
            .saturating_add(self.recovery)
    }
}

/// The grappling rules: the grab attempt, then the clinch / mount / submission ladder.
/// Distances are in arena units, durations in ticks, damage in health points.
///
/// A grab runs like an attack (`startup`, `active`, `recovery`, with the tick that reads the
/// button as the first one) but cannot be blocked and only catches fighters on the floor that
/// are neither stunned nor grappling. See [`Grapple`](super::Grapple) and
/// [`Match::step`](super::Match::step) for the rules.
///
/// ```
/// use srr::game::{GrappleData, MatchConfig};
///
/// let grapple = GrappleData { escape_presses: [2, 3, 4], ..GrappleData::default() };
/// let config = MatchConfig { grapple, ..MatchConfig::default() };
/// assert_eq!(config.validate(), Ok(()));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GrappleData {
    /// Ticks the grab winds up; a strike landing meanwhile interrupts it. May be zero.
    pub startup: u32,
    /// Ticks the grab can connect. Must be at least one.
    pub active: u32,
    /// Ticks spent recovering from a grab that connected with nobody. May be zero.
    pub recovery: u32,
    /// Horizontal reach of the grab in front of the grabber (vertically it uses
    /// [`MatchConfig::attack_height`]). Must not be negative.
    pub range: i32,
    /// Distance between the two fighters of a grapple. Must not be negative.
    pub grapple_distance: i32,
    /// Ticks the top must hold a position before a fresh grab press advances it.
    pub advance_ticks: u32,
    /// Ticks a position may be held before the referee stands both fighters up.
    pub max_hold_ticks: u32,
    /// Minimum ticks between two strikes of the top.
    pub strike_cooldown: u32,
    /// Damage of a strike of the top in the clinch. Must not be negative.
    pub clinch_strike_damage: i32,
    /// Damage of a strike of the top in the mount. Must not be negative.
    pub ground_strike_damage: i32,
    /// Damage of the takedown from the clinch into the mount. Must not be negative.
    pub takedown_damage: i32,
    /// Damage of the submission, dealt every `submission_interval` ticks. Must not be
    /// negative.
    pub submission_damage: i32,
    /// Ticks between two drains of the submission. Must be at least one.
    pub submission_interval: u32,
    /// Escape points the bottom needs to break free, indexed by [`GrapplePosition::index`]
    /// (clinch, mount, submission). Each must be at least one.
    pub escape_presses: [u32; 3],
    /// Distance between the two fighters once a grapple breaks. Must not be negative.
    pub break_distance: i32,
    /// Hit-stun ticks of the top when the bottom escapes.
    pub break_stun: u32,
}

impl GrappleData {
    /// Total length of a grab that connected with nobody: start-up + active + recovery
    /// (saturating).
    pub const fn total(&self) -> u32 {
        self.startup
            .saturating_add(self.active)
            .saturating_add(self.recovery)
    }
}

/// The rules of one judo [`Technique`]. Distances are in arena units, durations in ticks,
/// damage in health points.
///
/// The attempt runs like a grab (`startup`, `active`, `recovery`, the tick that completes the
/// combination being the first one), cannot be blocked, and catches the first fighter in
/// `range` that a grab could catch. The projection then lasts `projection_ticks`, the first
/// `counter_window` of which allow the counter; the landing deals `damage`, and the finish on
/// the ground drains health like the submission ([`GrappleData::submission_damage`] every
/// [`GrappleData::submission_interval`] ticks) until the bottom escapes with `escape_presses`
/// or the referee stands both up.
///
/// ```
/// use srr::game::{MatchConfig, Technique};
///
/// let config = MatchConfig::default();
/// let throw = config.technique(Technique::Throw);
/// assert!(throw.damage > config.technique(Technique::Sweep).damage);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TechniqueData {
    /// Ticks the technique winds up; a strike landing meanwhile interrupts it. May be zero.
    pub startup: u32,
    /// Ticks the technique can connect. Must be at least one.
    pub active: u32,
    /// Ticks spent recovering from a technique that caught nobody. May be zero.
    pub recovery: u32,
    /// Horizontal reach in front of the fighter (vertically it uses
    /// [`MatchConfig::attack_height`]). Must not be negative.
    pub range: i32,
    /// Ticks the projection lasts before the bottom hits the floor.
    pub projection_ticks: u32,
    /// First ticks of the projection during which the bottom can counter.
    pub counter_window: u32,
    /// Damage of the landing. Must not be negative.
    pub damage: i32,
    /// Escape points the bottom needs to break the finish. Must be at least one.
    pub escape_presses: u32,
}

impl TechniqueData {
    /// Total length of a technique that caught nobody: start-up + active + recovery
    /// (saturating).
    pub const fn total(&self) -> u32 {
        self.startup
            .saturating_add(self.active)
            .saturating_add(self.recovery)
    }
}

impl Default for GrappleData {
    /// The standard balance: a fast, short grab (4 + 2 ticks, range 55) with a long, punishable
    /// 20-tick whiff recovery; positions advance after 0.75 s and are stood up after 5 s;
    /// escaping needs 4 / 7 / 10 presses in the clinch / mount / submission.
    fn default() -> Self {
        GrappleData {
            startup: 4,
            active: 2,
            recovery: 20,
            range: 55,
            grapple_distance: 40,
            advance_ticks: 45,
            max_hold_ticks: 5 * TICKS_PER_SECOND,
            strike_cooldown: 15,
            clinch_strike_damage: 3,
            ground_strike_damage: 4,
            takedown_damage: 8,
            submission_damage: 2,
            submission_interval: 12,
            escape_presses: [4, 7, 10],
            break_distance: 90,
            break_stun: 20,
        }
    }
}

/// Every tunable rule of a match. Distances are in arena units, durations in ticks.
///
/// [`MatchConfig::default`] gives the game's standard balance; use struct update syntax to
/// change single values:
///
/// ```
/// use srr::game::MatchConfig;
///
/// let fast_rounds = MatchConfig { round_time_ticks: 600, ..MatchConfig::default() };
/// assert_eq!(fast_rounds.validate(), Ok(()));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchConfig {
    /// The arena spans `0..=arena_width`. Must be positive.
    pub arena_width: i32,
    /// Health every fighter starts with. Must be positive.
    pub max_health: i32,
    /// Horizontal distance per tick, on the ground and in the air.
    pub walk_speed: i32,
    /// Vertical velocity on the take-off tick.
    pub jump_velocity: i32,
    /// Vertical velocity lost per airborne tick. Must be positive, otherwise jumps never end.
    pub gravity: i32,
    /// Frame data of the low, medium and high punch, indexed by [`Strength::index`].
    ///
    /// [`Strength::index`]: super::Strength::index
    pub punches: [AttackData; 3],
    /// Frame data of the low, medium and high kick, indexed by [`Strength::index`].
    ///
    /// [`Strength::index`]: super::Strength::index
    pub kicks: [AttackData; 3],
    /// Vertical reach of every attack, above and below the attacker.
    pub attack_height: i32,
    /// Share of the damage absorbed by blocking, `0..=100`.
    pub block_reduction_percent: i32,
    /// Length of a round in fighting ticks (the countdown does not count). Must be positive.
    pub round_time_ticks: u32,
    /// Ticks before the fight starts; inputs are ignored meanwhile.
    pub countdown_ticks: u32,
    /// Whether allies can hurt each other.
    pub friendly_fire: bool,
    /// The glitches that are enabled in this match.
    pub glitches: GlitchSet,
    /// Extra take-off velocity of a [`Glitch::SuperJump`].
    pub super_jump_bonus: i32,
    /// The grab and the grappling ladder.
    pub grapple: GrappleData,
    /// Width of a fighter's push box: two fighters on the floor (or at nearly the same height)
    /// are never closer than this; zero lets fighters walk through each other. Must not be
    /// negative.
    pub body_width: i32,
    /// Height of a fighter's push box: a fighter that is at least this much higher than
    /// another passes over it (jumping over the opponent crosses sides). Must not be negative.
    pub body_height: i32,
    /// Most ticks between the two presses of a technique's combination (zero: the very same
    /// tick).
    pub combo_window: u32,
    /// The two judo techniques, indexed by [`Technique::index`].
    pub techniques: [TechniqueData; 2],
    /// How far fighters can sidestep: they stay within `-arena_depth..=arena_depth` across the
    /// arena (positive towards the wall), and spawn on the middle line, `0`. Zero keeps every
    /// fighter on that line. Must not be negative.
    pub arena_depth: i32,
    /// Sideways distance per tick of a sidestep (on the floor only).
    pub sidestep_speed: i32,
    /// Sideways reach of every attack, grab and technique, on both sides of the attacker: a
    /// target further across has sidestepped out of the line. Must not be negative.
    pub attack_depth: i32,
    /// Depth of a fighter's push box: fighters at least this far apart across the arena walk
    /// past each other. Must not be negative.
    pub body_depth: i32,
    /// The special move of the default character ([`Character::Chalk`]); every other
    /// character brings its own (see [`Character::rules`]).
    pub special: SpecialData,
    /// The character of each player, indexed by [`PlayerId`] (everybody plays
    /// [`Character::Chalk`] by default). Players outside the match are ignored.
    pub characters: [Character; Roster::MAX_PLAYERS],
}

/// Shorthand for the default frame data table.
const fn frames(
    startup: u32,
    active: u32,
    recovery: u32,
    range: i32,
    damage: i32,
    knockback: i32,
    hitstun: u32,
) -> AttackData {
    AttackData {
        startup,
        active,
        recovery,
        range,
        damage,
        knockback,
        hitstun,
    }
}

/// Shorthand for the default technique table.
const fn technique(
    (startup, active, recovery): (u32, u32, u32),
    range: i32,
    (projection_ticks, counter_window): (u32, u32),
    damage: i32,
    escape_presses: u32,
) -> TechniqueData {
    TechniqueData {
        startup,
        active,
        recovery,
        range,
        projection_ticks,
        counter_window,
        damage,
        escape_presses,
    }
}

impl Default for MatchConfig {
    /// The standard balance: a 1200-wide arena, 300 deep, 100 health, 99-second rounds after a
    /// 2-second countdown, no friendly fire, every glitch enabled, the default
    /// [`GrappleData`], 50 × 90 × 40 push boxes and a 3-tick window for the techniques'
    /// combinations.
    ///
    /// Stronger attacks deal more damage but start and recover more slowly; kicks reach
    /// further and hit a bit harder than punches of the same strength, but are slower. The
    /// sweep is the faster technique, the throw hurts more; both are very punishable when they
    /// whiff. A sidestep leaves the line of an attack in nine ticks: too slow to dodge a jab on
    /// reaction, fast enough for a high kick seen coming.
    fn default() -> Self {
        MatchConfig {
            arena_width: 1200,
            max_health: 100,
            walk_speed: 7,
            jump_velocity: 22,
            gravity: 2,
            //             startup active recovery range damage knockback hitstun
            punches: [
                frames(3, 2, 6, 80, 5, 12, 10),
                frames(5, 3, 10, 85, 9, 20, 14),
                frames(8, 3, 16, 90, 14, 32, 18),
            ],
            kicks: [
                frames(5, 3, 9, 105, 7, 16, 12),
                frames(7, 3, 13, 115, 11, 26, 16),
                frames(11, 4, 20, 125, 17, 40, 22),
            ],
            attack_height: 60,
            block_reduction_percent: 75,
            round_time_ticks: 99 * TICKS_PER_SECOND,
            countdown_ticks: 2 * TICKS_PER_SECOND,
            friendly_fire: false,
            glitches: GlitchSet::all(),
            super_jump_bonus: 10,
            grapple: GrappleData::default(),
            body_width: 50,
            body_height: 90,
            combo_window: 3,
            //                    timeline     range projection damage escape
            techniques: [
                technique((6, 3, 24), 75, (24, 12), 10, 9),
                technique((8, 2, 28), 65, (30, 12), 14, 11),
            ],
            arena_depth: 150,
            sidestep_speed: 5,
            attack_depth: 40,
            body_depth: 40,
            special: SHOULDER_RUSH,
            characters: [Character::Chalk; Roster::MAX_PLAYERS],
        }
    }
}

/// Why a configuration cannot run a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// `arena_width` is zero or negative.
    NonPositiveArenaWidth,
    /// `max_health` is zero or negative.
    NonPositiveMaxHealth,
    /// `gravity` is zero or negative.
    NonPositiveGravity,
    /// The `active` frames of the given attack are zero, so it could never hit.
    ZeroActiveFrames(Attack),
    /// `round_time_ticks` is zero.
    ZeroRoundTime,
    /// `block_reduction_percent` is outside `0..=100`; carries the offending value.
    InvalidBlockReduction(i32),
    /// A distance, speed or damage that only makes sense when non-negative.
    NegativeValue {
        /// The name of the offending [`MatchConfig`] field.
        field: &'static str,
        /// The negative value it holds.
        value: i32,
    },
    /// A distance or damage of the given attack's [`AttackData`] is negative.
    NegativeAttackValue {
        /// The attack whose frame data is invalid.
        attack: Attack,
        /// The name of the offending [`AttackData`] field.
        field: &'static str,
        /// The negative value it holds.
        value: i32,
    },
    /// The grab's `active` frames are zero, so it could never connect.
    ZeroGrabActiveFrames,
    /// The given technique's `active` frames are zero, so it could never connect.
    ZeroTechniqueActiveFrames(Technique),
    /// The submission's `submission_interval` is zero.
    ZeroSubmissionInterval,
    /// The escape from the given position (a rung of the ladder or a technique's finish) needs
    /// zero presses.
    ZeroEscapePresses(GrapplePosition),
    /// The special move's `active` frames are zero, so it could never hit.
    ZeroSpecialActiveFrames,
    /// The special move's projectile has a zero `lifetime`, so it could never hit.
    ZeroProjectileLifetime,
}

impl MatchConfig {
    /// The frame data of `attack`.
    ///
    /// ```
    /// use srr::game::{Attack, MatchConfig, Strength};
    ///
    /// let config = MatchConfig::default();
    /// let low = config.attack(Attack::punch(Strength::Low));
    /// let high = config.attack(Attack::punch(Strength::High));
    /// assert!(high.damage > low.damage);
    /// ```
    pub fn attack(&self, attack: Attack) -> &AttackData {
        let table = match attack.limb {
            Limb::Punch => &self.punches,
            Limb::Kick => &self.kicks,
        };
        &table[attack.strength.index()]
    }

    /// The rules of `technique`.
    pub fn technique(&self, technique: Technique) -> &TechniqueData {
        &self.techniques[technique.index()]
    }

    /// The character `player` plays ([`Character::Chalk`] for an id beyond the table).
    pub fn character(&self, player: PlayerId) -> Character {
        self.characters
            .get(usize::from(player.0))
            .copied()
            .unwrap_or_default()
    }

    /// This configuration with `player` playing `character` (no change for an id beyond the
    /// table).
    ///
    /// ```
    /// use srr::game::{Character, MatchConfig, PlayerId};
    ///
    /// let config = MatchConfig::default().with_character(PlayerId(1), Character::Torch);
    /// assert_eq!(config.character(PlayerId(1)), Character::Torch);
    /// assert_eq!(config.character(PlayerId(0)), Character::Chalk);
    /// ```
    pub fn with_character(mut self, player: PlayerId, character: Character) -> Self {
        if let Some(slot) = self.characters.get_mut(usize::from(player.0)) {
            *slot = character;
        }
        self
    }

    /// The rules the fighter of `player` follows: these, adjusted by its character (see
    /// [`Character::rules`]).
    pub fn rules_of(&self, player: PlayerId) -> MatchConfig {
        self.character(player).rules(self)
    }

    /// Checks that the configuration can run a match.
    ///
    /// Returns the first problem found, checking in this order: arena width, max health,
    /// gravity, round time, block reduction, the frame data of every attack in
    /// [`Attack::ALL`] order (active frames, then `range`, `damage`, `knockback`), the fields
    /// that must not be negative (`walk_speed`, `attack_height`, `super_jump_bonus`,
    /// `body_width`, `body_height`, `arena_depth`, `sidestep_speed`, `attack_depth`,
    /// `body_depth`), then the grappling rules: the grab's active frames, the
    /// submission interval, the escape presses in [`GrapplePosition::LADDER`] order, and the
    /// grappling values that must not be negative (reported as `NegativeValue` with the field
    /// names `grapple.range`, `grapple.grapple_distance`, `grapple.break_distance`,
    /// `grapple.clinch_strike_damage`, `grapple.ground_strike_damage`,
    /// `grapple.takedown_damage`, `grapple.submission_damage`), and finally every technique in
    /// [`Technique::ALL`] order: active frames, escape presses (reported for its finish), then
    /// `range` and `damage` (field names `sweep.range`, `sweep.damage`, `throw.range`,
    /// `throw.damage`), then the special move: active frames, the projectile's lifetime, then
    /// the values that must not be negative (`special.range`, `special.damage`,
    /// `special.knockback`, `special.lunge`, `special.projectile.speed`,
    /// `special.projectile.size`).
    ///
    /// # Errors
    ///
    /// Returns the [`ConfigError`] describing the first invalid value.
    ///
    /// Complexity: O(1).
    ///
    /// # Examples
    ///
    /// ```
    /// use srr::game::{ConfigError, MatchConfig};
    ///
    /// assert_eq!(MatchConfig::default().validate(), Ok(()));
    ///
    /// let broken = MatchConfig { gravity: 0, ..MatchConfig::default() };
    /// assert_eq!(broken.validate(), Err(ConfigError::NonPositiveGravity));
    /// ```
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.arena_width <= 0 {
            return Err(ConfigError::NonPositiveArenaWidth);
        }
        if self.max_health <= 0 {
            return Err(ConfigError::NonPositiveMaxHealth);
        }
        if self.gravity <= 0 {
            return Err(ConfigError::NonPositiveGravity);
        }
        if self.round_time_ticks == 0 {
            return Err(ConfigError::ZeroRoundTime);
        }
        if !(0..=100).contains(&self.block_reduction_percent) {
            return Err(ConfigError::InvalidBlockReduction(
                self.block_reduction_percent,
            ));
        }
        for attack in Attack::ALL {
            validate_attack(attack, self.attack(attack))?;
        }
        non_negative(&[
            ("walk_speed", self.walk_speed),
            ("attack_height", self.attack_height),
            ("super_jump_bonus", self.super_jump_bonus),
            ("body_width", self.body_width),
            ("body_height", self.body_height),
            ("arena_depth", self.arena_depth),
            ("sidestep_speed", self.sidestep_speed),
            ("attack_depth", self.attack_depth),
            ("body_depth", self.body_depth),
        ])?;
        validate_grapple(&self.grapple)?;
        for technique in Technique::ALL {
            validate_technique(technique, self.technique(technique))?;
        }
        validate_special(&self.special)
    }
}

fn validate_special(data: &SpecialData) -> Result<(), ConfigError> {
    if data.attack.active == 0 {
        return Err(ConfigError::ZeroSpecialActiveFrames);
    }
    let projectile = data.projectile.unwrap_or(super::character::ProjectileData {
        speed: 0,
        lifetime: 1,
        size: 0,
    });
    if projectile.lifetime == 0 {
        return Err(ConfigError::ZeroProjectileLifetime);
    }
    non_negative(&[
        ("special.range", data.attack.range),
        ("special.damage", data.attack.damage),
        ("special.knockback", data.attack.knockback),
        ("special.lunge", data.lunge),
        ("special.projectile.speed", projectile.speed),
        ("special.projectile.size", projectile.size),
    ])
}

/// The first negative value of `fields`, as a [`ConfigError::NegativeValue`].
fn non_negative(fields: &[(&'static str, i32)]) -> Result<(), ConfigError> {
    match fields.iter().find(|(_, value)| *value < 0) {
        Some(&(field, value)) => Err(ConfigError::NegativeValue { field, value }),
        None => Ok(()),
    }
}

fn validate_grapple(data: &GrappleData) -> Result<(), ConfigError> {
    if data.active == 0 {
        return Err(ConfigError::ZeroGrabActiveFrames);
    }
    if data.submission_interval == 0 {
        return Err(ConfigError::ZeroSubmissionInterval);
    }
    if let Some(position) = GrapplePosition::LADDER
        .into_iter()
        .find(|position| data.escape_presses[position.index()] == 0)
    {
        return Err(ConfigError::ZeroEscapePresses(position));
    }
    non_negative(&[
        ("grapple.range", data.range),
        ("grapple.grapple_distance", data.grapple_distance),
        ("grapple.break_distance", data.break_distance),
        ("grapple.clinch_strike_damage", data.clinch_strike_damage),
        ("grapple.ground_strike_damage", data.ground_strike_damage),
        ("grapple.takedown_damage", data.takedown_damage),
        ("grapple.submission_damage", data.submission_damage),
    ])
}

fn validate_technique(technique: Technique, data: &TechniqueData) -> Result<(), ConfigError> {
    if data.active == 0 {
        return Err(ConfigError::ZeroTechniqueActiveFrames(technique));
    }
    if data.escape_presses == 0 {
        return Err(ConfigError::ZeroEscapePresses(technique.finish()));
    }
    let fields = match technique {
        Technique::Sweep => [("sweep.range", data.range), ("sweep.damage", data.damage)],
        Technique::Throw => [("throw.range", data.range), ("throw.damage", data.damage)],
    };
    non_negative(&fields)
}

fn validate_attack(attack: Attack, data: &AttackData) -> Result<(), ConfigError> {
    if data.active == 0 {
        return Err(ConfigError::ZeroActiveFrames(attack));
    }
    let non_negative = [
        ("range", data.range),
        ("damage", data.damage),
        ("knockback", data.knockback),
    ];
    match non_negative.into_iter().find(|(_, value)| *value < 0) {
        Some((field, value)) => Err(ConfigError::NegativeAttackValue {
            attack,
            field,
            value,
        }),
        None => Ok(()),
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::NonPositiveArenaWidth => write!(f, "the arena width must be positive"),
            ConfigError::NonPositiveMaxHealth => write!(f, "the maximum health must be positive"),
            ConfigError::NonPositiveGravity => {
                write!(f, "gravity must be positive, otherwise jumps never end")
            }
            ConfigError::ZeroActiveFrames(attack) => {
                write!(f, "the {attack} needs at least one active tick")
            }
            ConfigError::ZeroRoundTime => write!(f, "the round time must be positive"),
            ConfigError::InvalidBlockReduction(percent) => {
                write!(f, "block reduction must be within 0..=100 %, got {percent}")
            }
            ConfigError::NegativeValue { field, value } => {
                write!(f, "`{field}` must not be negative, got {value}")
            }
            ConfigError::NegativeAttackValue {
                attack,
                field,
                value,
            } => write!(
                f,
                "the {field} of the {attack} must not be negative, got {value}"
            ),
            ConfigError::ZeroGrabActiveFrames => {
                write!(f, "the grab needs at least one active tick")
            }
            ConfigError::ZeroTechniqueActiveFrames(technique) => {
                write!(f, "the {technique} needs at least one active tick")
            }
            ConfigError::ZeroSubmissionInterval => {
                write!(f, "the submission interval must be at least one tick")
            }
            ConfigError::ZeroEscapePresses(position) => {
                write!(f, "escaping the {position} needs at least one press")
            }
            ConfigError::ZeroSpecialActiveFrames => {
                write!(f, "the special move needs at least one active tick")
            }
            ConfigError::ZeroProjectileLifetime => {
                write!(f, "the projectile needs a lifetime of at least one tick")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_distances_speeds_and_damage_are_rejected() {
        let config = MatchConfig {
            walk_speed: -1,
            ..MatchConfig::default()
        };
        let error = config.validate().unwrap_err();
        assert_eq!(
            error,
            ConfigError::NegativeValue {
                field: "walk_speed",
                value: -1
            }
        );
        assert!(error.to_string().contains("walk_speed"));
    }

    #[test]
    fn negative_attack_data_is_rejected_with_the_attack_and_field() {
        let mut config = MatchConfig::default();
        config.kicks[2].knockback = -3;
        let error = config.validate().unwrap_err();
        let attack = Attack::kick(super::super::Strength::High);
        assert_eq!(
            error,
            ConfigError::NegativeAttackValue {
                attack,
                field: "knockback",
                value: -3
            }
        );
        assert_eq!(
            error.to_string(),
            "the knockback of the high kick must not be negative, got -3"
        );
    }
}
