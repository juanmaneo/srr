//! The characters: who a player fights as.
//!
//! Every fighter plays one [`Character`]. A character changes the fighter's own numbers (health,
//! speed, jump, the power and reach of its strikes, the reach of its grabs) and gives it one
//! power, its **special move** ([`SpecialData`], on [`Input::special`](super::Input::special)).
//! Everything shared by the fighters (the arena, the timer, the grappling ladder, blocking...)
//! stays in the [`MatchConfig`].
//!
//! | Character | Health | Speed | Jump | Power | Reach | Grab reach | Special move |
//! | --------- | -----: | ----: | ---: | ----: | ----: | ---------: | ------------ |
//! | Chalk     | 100 %  | 100 % | 100 % | 100 % | 100 % | 100 %     | **Shoulder Rush**: a lunging charge across the alley |
//! | Torch     | 90 %   | 95 %  | 100 % | 95 %  | 100 % | 100 %     | **Fireball**: a flaming shot that flies across the arena |
//! | Brick     | 130 %  | 75 %  | 85 %  | 115 % | 105 % | 125 %     | **Iron Palm**: a slow palm strike that shrugs off hits while it winds up |
//! | Zip       | 80 %   | 135 % | 112 % | 85 %  | 95 %  | 90 %      | **Slide Tackle**: a low slide that passes under high attacks |
//!
//! Chalk is the default character and changes nothing: a fighter playing Chalk follows the
//! [`MatchConfig`] exactly, special move included ([`MatchConfig::special`]). The other
//! characters scale the configuration by their percentages and bring their own special move.
//!
//! A new character is one more variant here and one more entry in [`Character::data`].
//!
//! ```
//! use srr::game::{Character, MatchConfig};
//!
//! let base = MatchConfig::default();
//! assert_eq!(Character::Chalk.rules(&base), base, "the default character changes nothing");
//!
//! let brick = Character::Brick.rules(&base);
//! assert!(brick.max_health > base.max_health && brick.walk_speed < base.walk_speed);
//! assert!(Character::Torch.rules(&base).special.projectile.is_some(), "Torch throws fireballs");
//! ```

use std::fmt;

use super::config::{AttackData, MatchConfig};

/// The shot thrown by a projectile special move (the [`Character::Torch`]'s fireball). It
/// leaves the thrower on the first active tick of the move and flies straight ahead, on the
/// thrower's line, until it hits somebody, leaves the arena or burns out.
///
/// It hits like a strike with the move's [`SpecialData::attack`] numbers (damage, knockback,
/// hit-stun): stepping back away from it blocks it, and it vanishes on its first hit, blocked
/// or not. It hits a fighter whose body (`size` units on both sides of the shot) it crosses
/// during a tick, within the match's `attack_height` and `attack_depth`, so it never tunnels
/// through anybody however fast it flies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectileData {
    /// Distance flown per tick. Must not be negative.
    pub speed: i32,
    /// Ticks the shot flies before it burns out. Must be at least one.
    pub lifetime: u32,
    /// Half the width of the shot: how close to a fighter it hits. Must not be negative.
    pub size: i32,
}

/// A character's special move: one power, on [`Input::special`](super::Input::special), from
/// the floor only.
///
/// It runs like a strike with the timeline of `attack` (start-up, active, recovery), and like
/// a strike it can be blocked by stepping back and is cancelled by a hit (unless `armored`).
/// What makes it special:
///
/// * `lunge`: the fighter rushes forward by this distance on every start-up and active tick;
/// * `projectile`: instead of hitting with its body, the fighter throws a shot on the first
///   active tick (see [`ProjectileData`]);
/// * `armored`: hits taken during the start-up still hurt, but neither stun nor push the
///   fighter, so the move goes on (a grab still beats it);
/// * `low_profile`: until its recovery, high attacks thrown from the floor pass over the
///   fighter, as if it crouched;
/// * `cooldown`: once started, the move cannot be started again for this many ticks.
///
/// A strike that connected can be cancelled into the special move, like into a stronger
/// strike (see [`Match::step`](super::Match::step)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpecialData {
    /// The timeline and the hit of the move (for a projectile: the hit of the shot, and the
    /// `range` is unused).
    pub attack: AttackData,
    /// Forward distance per start-up and active tick. Must not be negative.
    pub lunge: i32,
    /// The shot thrown on the first active tick, if the move throws one.
    pub projectile: Option<ProjectileData>,
    /// Whether hits taken during the start-up neither stun nor push the fighter.
    pub armored: bool,
    /// Whether high attacks thrown from the floor pass over the fighter until the recovery.
    pub low_profile: bool,
    /// Ticks, from its start, before the move can be started again.
    pub cooldown: u32,
}

impl SpecialData {
    /// Total length of the move: start-up + active + recovery (saturating).
    pub const fn total(&self) -> u32 {
        self.attack.total()
    }
}

/// Who a player fights as. See the [module documentation](self) for the roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Character {
    /// The all-rounder, the default: the standard numbers and a lunging shoulder charge.
    #[default]
    Chalk,
    /// The zoner: a little frail and slow, but throws fireballs across the arena.
    Torch,
    /// The heavy grappler: tough, hard-hitting and long-armed, but slow; an armored palm.
    Brick,
    /// The rushdown: fast and jumpy but frail and light-handed; a slide under high attacks.
    Zip,
}

/// What a character changes: its stats as percentages of the [`MatchConfig`], and its special
/// move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CharacterData {
    /// The character's name, as shown on screen.
    pub name: &'static str,
    /// The name of its special move.
    pub special_name: &'static str,
    /// One line about how it plays.
    pub tagline: &'static str,
    /// Share of `max_health`.
    pub health_percent: i32,
    /// Share of `walk_speed` and `sidestep_speed`.
    pub speed_percent: i32,
    /// Share of `jump_velocity`.
    pub jump_percent: i32,
    /// Share of the damage of the six strikes.
    pub power_percent: i32,
    /// Share of the range of the six strikes.
    pub reach_percent: i32,
    /// Share of the range of the grab and of the two judo techniques.
    pub grab_reach_percent: i32,
    /// The special move; `None` keeps [`MatchConfig::special`] (the default character's).
    pub special: Option<SpecialData>,
}

/// The default special move, the Chalk's Shoulder Rush: [`MatchConfig::special`] by default.
pub const SHOULDER_RUSH: SpecialData = SpecialData {
    attack: AttackData {
        startup: 8,
        active: 6,
        recovery: 18,
        range: 60,
        damage: 12,
        knockback: 48,
        hitstun: 20,
    },
    lunge: 12,
    projectile: None,
    armored: false,
    low_profile: false,
    cooldown: 0,
};

const FIREBALL: SpecialData = SpecialData {
    attack: AttackData {
        startup: 12,
        active: 1,
        recovery: 22,
        range: 0,
        damage: 10,
        knockback: 24,
        hitstun: 18,
    },
    lunge: 0,
    projectile: Some(ProjectileData {
        speed: 11,
        lifetime: 70,
        size: 20,
    }),
    armored: false,
    low_profile: false,
    cooldown: 75,
};

const IRON_PALM: SpecialData = SpecialData {
    attack: AttackData {
        startup: 14,
        active: 4,
        recovery: 26,
        range: 95,
        damage: 20,
        knockback: 64,
        hitstun: 26,
    },
    lunge: 0,
    projectile: None,
    armored: true,
    low_profile: false,
    cooldown: 0,
};

const SLIDE_TACKLE: SpecialData = SpecialData {
    attack: AttackData {
        startup: 4,
        active: 8,
        recovery: 18,
        range: 70,
        damage: 9,
        knockback: 30,
        hitstun: 28,
    },
    lunge: 15,
    projectile: None,
    armored: false,
    low_profile: true,
    cooldown: 0,
};

impl Character {
    /// Every character, in the order of the menus.
    pub const ALL: [Character; 4] = [
        Character::Chalk,
        Character::Torch,
        Character::Brick,
        Character::Zip,
    ];

    /// Position in [`Character::ALL`]: the byte that stands for the character on the wire.
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// The character at `index` in [`Character::ALL`], if any.
    ///
    /// ```
    /// use srr::game::Character;
    ///
    /// for character in Character::ALL {
    ///     assert_eq!(Character::from_index(character.index()), Some(character));
    /// }
    /// assert_eq!(Character::from_index(200), None);
    /// ```
    pub fn from_index(index: u8) -> Option<Character> {
        Character::ALL.get(usize::from(index)).copied()
    }

    /// The next character of the menus, wrapping around.
    ///
    /// ```
    /// use srr::game::Character;
    ///
    /// assert_eq!(Character::Chalk.next(), Character::Torch);
    /// assert_eq!(Character::Zip.next(), Character::Chalk);
    /// assert_eq!(Character::Chalk.previous(), Character::Zip);
    /// ```
    pub fn next(self) -> Character {
        Character::ALL[(usize::from(self.index()) + 1) % Character::ALL.len()]
    }

    /// The previous character of the menus, wrapping around.
    pub fn previous(self) -> Character {
        let count = Character::ALL.len();
        Character::ALL[(usize::from(self.index()) + count - 1) % count]
    }

    /// The character's stats and special move.
    pub const fn data(self) -> CharacterData {
        match self {
            Character::Chalk => CharacterData {
                name: "Chalk",
                special_name: "Shoulder Rush",
                tagline: "The all-rounder: standard numbers, a lunging charge",
                health_percent: 100,
                speed_percent: 100,
                jump_percent: 100,
                power_percent: 100,
                reach_percent: 100,
                grab_reach_percent: 100,
                special: None,
            },
            Character::Torch => CharacterData {
                name: "Torch",
                special_name: "Fireball",
                tagline: "The zoner: keeps you away with fireballs",
                health_percent: 90,
                speed_percent: 95,
                jump_percent: 100,
                power_percent: 95,
                reach_percent: 100,
                grab_reach_percent: 100,
                special: Some(FIREBALL),
            },
            Character::Brick => CharacterData {
                name: "Brick",
                special_name: "Iron Palm",
                tagline: "The heavy: tough, slow, hits through your strikes",
                health_percent: 130,
                speed_percent: 75,
                jump_percent: 85,
                power_percent: 115,
                reach_percent: 105,
                grab_reach_percent: 125,
                special: Some(IRON_PALM),
            },
            Character::Zip => CharacterData {
                name: "Zip",
                special_name: "Slide Tackle",
                tagline: "The rushdown: fast and frail, slides under high attacks",
                health_percent: 80,
                speed_percent: 135,
                jump_percent: 112,
                power_percent: 85,
                reach_percent: 95,
                grab_reach_percent: 90,
                special: Some(SLIDE_TACKLE),
            },
        }
    }

    /// The character's name, as shown on screen.
    pub const fn name(self) -> &'static str {
        self.data().name
    }

    /// The rules a fighter playing this character follows: `base` with the character's
    /// percentages applied and its special move. Health stays at least 1, so the result of a
    /// valid `base` is valid too; Chalk returns `base` unchanged.
    ///
    /// Complexity: O(1) (one copy of the configuration).
    pub fn rules(self, base: &MatchConfig) -> MatchConfig {
        let data = self.data();
        let mut rules = base.clone();
        rules.max_health = scale(base.max_health, data.health_percent).max(1);
        rules.walk_speed = scale(base.walk_speed, data.speed_percent);
        rules.sidestep_speed = scale(base.sidestep_speed, data.speed_percent);
        rules.jump_velocity = scale(base.jump_velocity, data.jump_percent);
        for strike in rules.punches.iter_mut().chain(rules.kicks.iter_mut()) {
            strike.damage = scale(strike.damage, data.power_percent);
            strike.range = scale(strike.range, data.reach_percent);
        }
        rules.grapple.range = scale(base.grapple.range, data.grab_reach_percent);
        for technique in &mut rules.techniques {
            technique.range = scale(technique.range, data.grab_reach_percent);
        }
        if let Some(special) = data.special {
            rules.special = special;
        }
        rules
    }
}

impl fmt::Display for Character {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// `value` scaled by `percent` (rounded towards zero; saturating).
fn scale(value: i32, percent: i32) -> i32 {
    let scaled = i64::from(value) * i64::from(percent) / 100;
    scaled.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_saturates_instead_of_overflowing() {
        assert_eq!(scale(i32::MAX, 200), i32::MAX);
        assert_eq!(scale(i32::MIN, 200), i32::MIN);
        assert_eq!(scale(7, 100), 7);
    }
}
