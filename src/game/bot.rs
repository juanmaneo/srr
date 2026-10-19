//! The computer opponents: a [`Bot`] reads the match like a player looks at the screen and
//! answers with the [`Input`] of its fighter.
//!
//! Four rivals exist, listed by [`Rival`] (the menus cycle through it, and it spawns them):
//!
//! * the [`Dummy`], a training dummy that never presses anything;
//! * the [`Brawler`], the computer that fights back, described below — in three tunings:
//!   the standard one, [`BrawlerConfig::PATIENT`] (guards everything, picks its openings) and
//!   [`BrawlerConfig::AGGRESSIVE`] (rushes in, rarely guards).
//!
//! A new strategy is a new type implementing [`Bot`] and a new [`Rival`] naming it; a new
//! difficulty of the same strategy is a new [`BrawlerConfig`]. Bots are
//! deterministic, like the rest of the rules: the brawler's "randomness" is a xorshift
//! generator seeded by the caller, so the same seed against the same inputs plays the same
//! match. It plays fair: it only sees what the [`Match`] shows and presses buttons under the
//! same rules as a human (fresh presses for turns, techniques and grappling).
//!
//! Every tick, in this order, the brawler:
//!
//! * outside the fight (countdown, finished round), knocked out, or without anybody left to
//!   fight, it presses nothing;
//! * in a grapple, underneath: during a projection it tries the counter (the same technique's
//!   two buttons on one tick) now and then within the counter window; otherwise it mashes fresh
//!   presses at a human pace to escape (and with [`Glitch::Reversal`] it tries the reversal on
//!   the first tick of a clinch);
//! * in a grapple, on top: it advances to the next position as soon as it may, and strikes
//!   now and then meanwhile;
//! * with a strike that connected, it chains the next tier (low → medium → high), punch or
//!   kick at random (a kick in the air): the combo;
//! * busy (attacking, stunned...), it presses nothing;
//! * with its nearest living enemy behind it, it turns around (down + direction);
//! * in the air, it drifts towards the enemy, and kicks (at random strength) once the enemy
//!   is within a kick's reach;
//! * when the enemy's strike is coming, it often guards: it steps back to block, or crouches
//!   under a high attack thrown from the floor;
//! * out of reach, it walks in (jumping once in a while); out of the enemy's line, it
//!   sidesteps back onto it;
//! * in reach, every `decision_pause` ticks at most it picks something: a grab, a judo
//!   technique or a strike up close, a strike or a step in further away; between two
//!   decisions it holds its ground, walks in or backs off. How often and how patiently is
//!   tuned by its [`BrawlerConfig`], which a caller may adjust (the defaults are the
//!   standard computer).
//!
//! Whatever it does on the floor, the bot sidesteps towards the enemy's line while it is more
//! than half the `attack_depth` away from it, so a sidestep only shakes it off for a moment.
//!
//! ```
//! use srr::game::{Bot, Brawler, Match, MatchConfig, PlayerId, Rival, Roster};
//! use std::collections::HashMap;
//!
//! let mut m = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
//! let mut bot = Brawler::new(PlayerId(1), 42);
//! for _ in 0..600 {
//!     let input = bot.think(&m);
//!     m.step(&HashMap::from([(PlayerId(1), input)]));
//! }
//! assert!(m.fighter(PlayerId(1)).unwrap().x() < 800, "the bot walked in");
//!
//! let mut dummy = Rival::Dummy.spawn(PlayerId(1), 42);
//! assert_eq!(dummy.think(&m), srr::game::Input::NONE, "the dummy never moves");
//! ```
//!
//! [`Glitch::Reversal`]: super::Glitch::Reversal

use std::fmt;

use super::config::{Glitch, MatchConfig};
use super::fighter::{AttackPhase, Facing, Fighter, FighterState};
use super::grapple::{Grapple, GrapplePosition, GrappleRole};
use super::input::{Attack, Input, Limb, Strength};
use super::roster::PlayerId;
use super::simulation::{Match, Phase};
use super::technique::Technique;

/// How the computer is tuned: the chances behind every dice it rolls and how patiently it
/// waits between two decisions. [`BrawlerConfig::default`] is the standard computer;
/// [`Brawler::with_config`] builds a brawler with a custom one.
///
/// ```
/// use srr::game::BrawlerConfig;
///
/// let config = BrawlerConfig::default();
/// assert_eq!(config.guard_percent, 55);
/// assert_eq!(config.decision_pause, (8, 14));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BrawlerConfig {
    /// Chance, per tick, that a bottom mashes a fresh press (when it did not press on the
    /// tick before): about `100 * mash_percent / TICKS_PER_SECOND` presses a second.
    pub mash_percent: u32,
    /// Chance, per tick of the counter window, that the bottom of a projection counters it.
    pub counter_percent: u32,
    /// Chance that the bottom of a fresh clinch tries the reversal glitch.
    pub reversal_percent: u32,
    /// Chance, per tick, that the top strikes while waiting to advance.
    pub top_strike_percent: u32,
    /// Chance that the bot guards against one enemy strike.
    pub guard_percent: u32,
    /// Chance, per tick of walking in, of a jump.
    pub jump_percent: u32,
    /// Ticks between two decisions in reach, picked between the two bounds (in either order;
    /// at most: a step in halves the wait).
    pub decision_pause: (u32, u32),
}

impl Default for BrawlerConfig {
    /// The standard computer: about 12 mashes a second, a patient guard, a jump in fifty
    /// ticks of walking, and a decision every 8 to 14 ticks.
    fn default() -> Self {
        BrawlerConfig {
            mash_percent: 25,
            counter_percent: 8,
            reversal_percent: 30,
            top_strike_percent: 12,
            guard_percent: 55,
            jump_percent: 2,
            decision_pause: (8, 14),
        }
    }
}

impl BrawlerConfig {
    /// The patient computer: guards nearly everything, waits for its openings, escapes
    /// grapples with method and counters often, but jumps rarely and never rushes in.
    pub const PATIENT: BrawlerConfig = BrawlerConfig {
        mash_percent: 20,
        counter_percent: 15,
        reversal_percent: 20,
        top_strike_percent: 8,
        guard_percent: 85,
        jump_percent: 1,
        decision_pause: (16, 26),
    };

    /// The aggressive computer: rushes in, decides twice as often, presses hard in grapples
    /// and jumps at the enemy, but guards one strike in five.
    pub const AGGRESSIVE: BrawlerConfig = BrawlerConfig {
        mash_percent: 40,
        counter_percent: 5,
        reversal_percent: 40,
        top_strike_percent: 20,
        guard_percent: 20,
        jump_percent: 4,
        decision_pause: (4, 7),
    };
}

const GRAB: Input = Input {
    grab: true,
    ..Input::NONE
};

/// A computer-controlled player: see the [module documentation](self).
///
/// Implementations are [`Clone`] (so that the client holding one can be copied) and
/// deterministic: everything they decide comes from the [`Match`] they are shown and from
/// their own state.
pub trait Bot: fmt::Debug + CloneBot {
    /// The player the bot controls.
    fn player(&self) -> PlayerId;

    /// The input of the bot's fighter for the next step of `current`. Call it exactly once per
    /// step: a bot may remember what it pressed.
    fn think(&mut self, current: &Match) -> Input;
}

/// Copies a boxed [`Bot`] without knowing its type; implemented for every `Bot + Clone`.
pub trait CloneBot {
    /// A boxed copy of the bot.
    fn clone_box(&self) -> Box<dyn Bot>;
}

impl<B: Bot + Clone + 'static> CloneBot for B {
    fn clone_box(&self) -> Box<dyn Bot> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn Bot> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// The computer opponents a player alone can fight. Each one names a [`Bot`] and spawns it;
/// [`Rival::ALL`] is the order the menus go round them in: the three computers of increasing
/// recklessness, then the training dummy.
///
/// ```
/// use srr::game::Rival;
///
/// assert_eq!(
///     Rival::ALL,
///     [
///         Rival::Brawler,
///         Rival::Patient,
///         Rival::Aggressive,
///         Rival::Dummy
///     ]
/// );
/// assert_eq!(Rival::Dummy.next(), Rival::Brawler, "round and round");
/// assert_eq!(Rival::Brawler.next(), Rival::Patient);
/// assert_eq!(Rival::Aggressive.next(), Rival::Dummy);
/// assert_eq!(Rival::Brawler.previous(), Rival::Dummy);
/// assert_eq!(Rival::Dummy.name(), "the training dummy");
/// assert!(!Rival::Dummy.fights() && Rival::Brawler.fights());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rival {
    /// The training dummy ([`Dummy`]): it never moves.
    Dummy,
    /// The computer ([`Brawler`], standard tuning): it fights back.
    Brawler,
    /// The patient computer ([`Brawler`], [`BrawlerConfig::PATIENT`]): it guards and waits.
    Patient,
    /// The aggressive computer ([`Brawler`], [`BrawlerConfig::AGGRESSIVE`]): it rushes in.
    Aggressive,
}

impl Rival {
    /// Every rival, in menu order: the standard computer first (a fight is what a player
    /// waiting for others most likely wants), then the patient and the aggressive ones, the
    /// training dummy last.
    pub const ALL: [Rival; 4] = [
        Rival::Brawler,
        Rival::Patient,
        Rival::Aggressive,
        Rival::Dummy,
    ];

    /// The rival after this one (the first after the last).
    pub fn next(self) -> Rival {
        Rival::ALL[(self.index() + 1) % Rival::ALL.len()]
    }

    /// The rival before this one (the last before the first).
    pub fn previous(self) -> Rival {
        Rival::ALL[(self.index() + Rival::ALL.len() - 1) % Rival::ALL.len()]
    }

    fn index(self) -> usize {
        Rival::ALL
            .iter()
            .position(|&rival| rival == self)
            .expect("every rival is listed")
    }

    /// How the menus call the rival.
    pub fn name(self) -> &'static str {
        match self {
            Rival::Dummy => "the training dummy",
            Rival::Brawler => "the computer",
            Rival::Patient => "the patient computer",
            Rival::Aggressive => "the aggressive computer",
        }
    }

    /// Whether the rival fights back: the HUD tags a rival that does `CPU`, and a training
    /// dummy by its player number like anybody else.
    pub fn fights(self) -> bool {
        self != Rival::Dummy
    }

    /// A fresh bot controlling `player`, seeded with `seed` (meaningless to a dummy). The
    /// three computers are the same [`Brawler`] with different [`BrawlerConfig`]s.
    pub fn spawn(self, player: PlayerId, seed: u64) -> Box<dyn Bot> {
        match self {
            Rival::Dummy => Box::new(Dummy::new(player)),
            Rival::Brawler => Box::new(Brawler::new(player, seed)),
            Rival::Patient => Box::new(Brawler::with_config(player, seed, BrawlerConfig::PATIENT)),
            Rival::Aggressive => Box::new(Brawler::with_config(
                player,
                seed,
                BrawlerConfig::AGGRESSIVE,
            )),
        }
    }
}

/// The training dummy: a [`Bot`] that never presses anything, to try moves on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dummy {
    player: PlayerId,
}

impl Dummy {
    /// A dummy standing in for `player`.
    pub fn new(player: PlayerId) -> Self {
        Dummy { player }
    }
}

impl Bot for Dummy {
    fn player(&self) -> PlayerId {
        self.player
    }

    fn think(&mut self, _current: &Match) -> Input {
        Input::NONE
    }
}

/// What the brawler does between two decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Plan {
    Hold,
    Approach,
    Retreat,
}

/// The computer that fights back; see the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Brawler {
    player: PlayerId,
    /// The xorshift state; never zero.
    state: u64,
    /// How the brawler is tuned.
    config: BrawlerConfig,
    /// The input of the previous tick, to make fresh presses.
    previous: Input,
    /// Ticks before the next decision in reach.
    wait: u32,
    plan: Plan,
    /// Whether the bot guards against the enemy strike in progress, once decided.
    guard: Option<bool>,
    /// Which button the next mash presses.
    mash_kick: bool,
}

impl Bot for Brawler {
    fn player(&self) -> PlayerId {
        self.player
    }

    fn think(&mut self, current: &Match) -> Input {
        let input = self.decide(current);
        self.previous = input;
        input
    }
}

impl Brawler {
    /// A brawler controlling `player`; two brawlers with the same seed play the same way.
    /// It is tuned by [`BrawlerConfig::default`]; see [`Brawler::with_config`] for another
    /// tuning.
    pub fn new(player: PlayerId, seed: u64) -> Self {
        Self::with_config(player, seed, BrawlerConfig::default())
    }

    /// A brawler controlling `player`, tuned by `config`; two brawlers with the same seed and
    /// the same config play the same way.
    pub fn with_config(player: PlayerId, seed: u64, config: BrawlerConfig) -> Self {
        // splitmix64, so that close seeds give unrelated sequences.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Brawler {
            player,
            state: (z ^ (z >> 31)) | 1,
            config,
            previous: Input::NONE,
            wait: 0,
            plan: Plan::Hold,
            guard: None,
            mash_kick: false,
        }
    }

    fn decide(&mut self, current: &Match) -> Input {
        let me = match current.fighter(self.player) {
            Some(me) if current.phase() == Phase::Fighting && !me.is_knocked_out() => me,
            _ => return Input::NONE,
        };
        let config = current.config();
        if let Some(grapple) = me.grapple() {
            return self.grapple(&grapple, config);
        }
        if let Some(chain) = self.chain(me) {
            return chain;
        }
        // Busy (attacking, stunned...) or, in theory, nobody left to fight: nothing to do.
        let target = match (is_free(me.state()), nearest_enemy(current, me)) {
            (true, Some(target)) => target,
            _ => return Input::NONE,
        };
        let dx = target.x() - me.x();
        let dz = i64::from(target.z()) - i64::from(me.z());
        let lane = if dz.abs() > i64::from(config.attack_depth / 2) {
            dz.signum() as i32
        } else {
            0
        };
        // Straight above or below the target: keep facing the same way.
        let toward = [Facing::Left, me.facing(), Facing::Right][(dx.signum() + 1) as usize];
        if me.facing() != toward {
            let turn = Input {
                down: true,
                ..walking(toward)
            };
            // Turning needs a fresh press of the combination.
            return if self.previous == turn {
                Input::NONE
            } else {
                turn
            };
        }
        if me.state() == FighterState::Airborne {
            return self.fly(dx, dz, toward, config);
        }
        match threat(target, me, config) {
            Some(attack) if self.guards() => {
                return if attack.strength == Strength::High && !target.is_airborne() {
                    Input {
                        down: true,
                        ..Input::NONE
                    }
                } else {
                    walking(toward.opposite())
                };
            }
            Some(_) => {}
            None => self.guard = None,
        }
        let distance = dx.abs();
        let reach = Attack::ALL
            .into_iter()
            .map(|attack| config.attack(attack).range)
            .fold(config.grapple.range, i32::max);
        if distance > reach {
            self.plan = Plan::Approach;
            let jump = self.chance(self.config.jump_percent);
            return Input {
                jump,
                ..sidestepping(walking(toward), lane)
            };
        }
        if dz.abs() > i64::from(config.attack_depth) {
            // In reach but out of the line: nothing would connect.
            return sidestepping(Input::NONE, lane);
        }
        if self.wait > 0 {
            self.wait -= 1;
            let walk = match self.plan {
                Plan::Hold => Input::NONE,
                Plan::Approach => walking(toward),
                Plan::Retreat => walking(toward.opposite()),
            };
            return sidestepping(walk, lane);
        }
        self.wait = self.between(self.config.decision_pause.0, self.config.decision_pause.1);
        self.offense(distance, toward, config)
    }

    /// The strike that chains after the one in progress, once it connected: the next tier,
    /// with the limb drawn at random (a kick in the air). Nothing after a high strike.
    fn chain(&mut self, me: &Fighter) -> Option<Input> {
        let attack = me.attack().filter(|_| me.attack_connected())?;
        let strength = *Strength::ALL.get(attack.strength.index() + 1)?;
        let limb = if me.is_airborne() || self.chance(50) {
            Limb::Kick
        } else {
            Limb::Punch
        };
        Some(Input::pressing(Attack { limb, strength }))
    }

    /// In the air: drifting towards the enemy (`dx` along, `dz` across), with a kick of a random
    /// strength once the enemy is within the reach of the kicks.
    fn fly(&mut self, dx: i32, dz: i64, toward: Facing, config: &MatchConfig) -> Input {
        let reach = Strength::ALL
            .into_iter()
            .map(|strength| config.attack(Attack::kick(strength)).range)
            .fold(0, i32::max);
        let steer = walking(toward);
        if dx.abs() > reach || dz.abs() > i64::from(config.attack_depth) {
            return steer;
        }
        let strength = Strength::ALL[self.between(0, 2) as usize];
        steer.with(Attack::kick(strength))
    }

    /// Whether the bot guards against the strike in progress (decided once per strike).
    fn guards(&mut self) -> bool {
        match self.guard {
            Some(guard) => guard,
            None => {
                let guard = self.chance(self.config.guard_percent);
                self.guard = Some(guard);
                guard
            }
        }
    }

    /// Picks the next move in reach (`distance` is at most the longest reach).
    fn offense(&mut self, distance: i32, toward: Facing, config: &MatchConfig) -> Input {
        let roll = self.between(0, 99);
        let reaching: Vec<Attack> = Attack::ALL
            .into_iter()
            .filter(|&attack| config.attack(attack).range >= distance)
            .collect();
        if distance <= config.grapple.range {
            self.plan = Plan::Hold;
            match roll {
                0..=24 => return GRAB,
                25..=44 => return technique_input(Technique::Sweep),
                45..=59 => return technique_input(Technique::Throw),
                _ => {}
            }
        } else if roll < 35 {
            self.plan = Plan::Approach;
            self.wait /= 2;
            return walking(toward);
        } else {
            self.plan = if roll < 55 { Plan::Retreat } else { Plan::Hold };
        }
        // Up close nothing may reach (with very short strikes): grab instead.
        let index = self.between(0, reaching.len().max(1) as u32 - 1) as usize;
        reaching.get(index).copied().map_or(GRAB, Input::pressing)
    }

    fn grapple(&mut self, grapple: &Grapple, config: &MatchConfig) -> Input {
        if self.previous != Input::NONE {
            // Every move in a grapple needs a fresh press: release first.
            return Input::NONE;
        }
        let position = grapple.position;
        match (grapple.role, position.technique()) {
            (GrappleRole::Bottom, Some(technique)) if position.is_projection() => {
                let window = config.technique(technique).counter_window;
                if grapple.held_ticks < window && self.chance(self.config.counter_percent) {
                    technique_input(technique)
                } else {
                    Input::NONE
                }
            }
            (GrappleRole::Bottom, _)
                if position == GrapplePosition::Clinch
                    && grapple.held_ticks == 0
                    && config.glitches.contains(Glitch::Reversal)
                    && self.chance(self.config.reversal_percent) =>
            {
                GRAB
            }
            (GrappleRole::Bottom, _) if self.chance(self.config.mash_percent) => {
                self.mash_kick = !self.mash_kick;
                let limb = if self.mash_kick {
                    Limb::Kick
                } else {
                    Limb::Punch
                };
                Input::pressing(Attack {
                    limb,
                    strength: Strength::Low,
                })
            }
            (GrappleRole::Bottom, _) => Input::NONE,
            (GrappleRole::Top, _)
                if !position.is_projection()
                    && position.next().is_some()
                    && grapple.held_ticks + 1 >= config.grapple.advance_ticks =>
            {
                GRAB
            }
            (GrappleRole::Top, _) if self.chance(self.config.top_strike_percent) => {
                Input::pressing(Attack::punch(Strength::Low))
            }
            (GrappleRole::Top, _) => Input::NONE,
        }
    }

    /// xorshift64.
    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    fn chance(&mut self, percent: u32) -> bool {
        self.next() % 100 < u64::from(percent)
    }

    /// A number between `low` and `high`, both included, in either order: a hand-written
    /// [`BrawlerConfig::decision_pause`] may be reversed. The span is counted in `u64` so that
    /// `(0, u32::MAX)` cannot overflow.
    fn between(&mut self, low: u32, high: u32) -> u32 {
        let (low, high) = (low.min(high), low.max(high));
        let span = u64::from(high - low) + 1;
        // The remainder is at most `high - low`, so it fits in a u32 and the sum stays <= high.
        low + (self.next() % span) as u32
    }
}

/// Whether a fighter in `state` reads its input (and can therefore move or start something).
fn is_free(state: FighterState) -> bool {
    matches!(
        state,
        FighterState::Idle
            | FighterState::Walking
            | FighterState::Airborne
            | FighterState::Blocking
            | FighterState::Crouching
    )
}

/// The closest enemy still standing, along and across the arena (the lowest player id among
/// equals).
fn nearest_enemy<'a>(current: &'a Match, me: &Fighter) -> Option<&'a Fighter> {
    current
        .fighters()
        .iter()
        .filter(|f| !f.is_knocked_out() && current.roster().are_enemies(me.player(), f.player()))
        .min_by_key(|f| {
            let along = (i64::from(f.x()) - i64::from(me.x())).abs();
            let across = (i64::from(f.z()) - i64::from(me.z())).abs();
            (along + across, f.player())
        })
}

/// The strike of `target` that is about to reach `me`, if any.
fn threat(target: &Fighter, me: &Fighter, config: &MatchConfig) -> Option<Attack> {
    let attack = target.attack()?;
    let ahead = (me.x() - target.x()) * target.facing().sign();
    let across = (i64::from(me.z()) - i64::from(target.z())).abs();
    let coming = matches!(
        target.state(),
        FighterState::Attacking(AttackPhase::Startup | AttackPhase::Active)
    );
    (coming
        && across <= i64::from(config.attack_depth)
        && (0..=config.attack(attack).range + config.walk_speed).contains(&ahead))
    .then_some(attack)
}

fn walking(toward: Facing) -> Input {
    Input {
        left: toward == Facing::Left,
        right: toward == Facing::Right,
        ..Input::NONE
    }
}

/// `input` with a sidestep towards the wall (`lane` 1) or the street (`lane` -1).
fn sidestepping(input: Input, lane: i32) -> Input {
    Input {
        sidestep_in: lane > 0,
        sidestep_out: lane < 0,
        ..input
    }
}

fn technique_input(technique: Technique) -> Input {
    let [first, second] = technique.buttons();
    Input::pressing(first).with(second)
}
