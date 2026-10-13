//! A match: setup, the fixed-step simulation, and the round outcome.

use std::collections::HashMap;
use std::fmt;

use super::config::{AttackData, ConfigError, MatchConfig};
use super::fighter::{AttackPhase, Facing, Fighter};
use super::grapple::{GrappleRole, Hold, Turn, separate, snap};
use super::input::Input;
use super::roster::{PlayerId, Roster, TeamId};

/// How a finished round ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// The given team won: it was the last one standing, or it had the highest total health
    /// when time ran out.
    Winner(TeamId),
    /// Nobody won: everybody was knocked out at once, or the best teams tied at time-up.
    Draw,
}

/// Where a match is in its life cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Steps left before the fight starts.
    Countdown {
        /// Countdown steps still to run; the match is `Fighting` after the last one.
        remaining: u32,
    },
    /// The round is running.
    Fighting,
    /// Final: stepping a finished match changes nothing.
    Finished(Outcome),
}

/// Why a match could not be created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupError {
    /// The configuration failed [`MatchConfig::validate`].
    InvalidConfig(ConfigError),
    /// The number of spawn points differs from the number of players.
    SpawnCountMismatch {
        /// The number of players in the roster.
        expected: usize,
        /// The number of spawn points given.
        actual: usize,
    },
    /// A spawn point lies outside `0..=arena_width`.
    SpawnOutOfBounds {
        /// The player whose spawn point is invalid.
        player: PlayerId,
        /// The offending x coordinate.
        x: i32,
    },
}

impl From<ConfigError> for SetupError {
    fn from(error: ConfigError) -> Self {
        SetupError::InvalidConfig(error)
    }
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetupError::InvalidConfig(error) => write!(f, "invalid match configuration: {error}"),
            SetupError::SpawnCountMismatch { expected, actual } => {
                write!(f, "expected {expected} spawn points, got {actual}")
            }
            SetupError::SpawnOutOfBounds { player, x } => {
                write!(f, "player {} spawns outside the arena at x = {x}", player.0)
            }
        }
    }
}

impl std::error::Error for SetupError {}

/// One hit found during attack resolution, applied once every hit of the step is known.
/// `attacker` and `target` are positions in `Match::fighters`.
struct Hit {
    attacker: usize,
    target: usize,
    data: AttackData,
    direction: Facing,
    blocked: bool,
}

/// A whole match. Cloning gives an independent snapshot that continues identically.
///
/// The simulation is deterministic: the state after [`Match::step`] depends only on the state
/// before it and on the inputs of that tick, which makes snapshots usable for rollback.
///
/// With `n` fighters (at most 10) a tick costs O(n²) in the worst case, because every active
/// attack and grab is tested against every other fighter.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// use srr::game::{Input, Match, MatchConfig, Phase, PlayerId, Roster};
///
/// let config = MatchConfig { countdown_ticks: 0, ..MatchConfig::default() };
/// let mut m = Match::new(Roster::duel(), config).unwrap();
/// assert_eq!(m.phase(), Phase::Fighting);
///
/// let mut inputs = HashMap::new();
/// inputs.insert(PlayerId(0), Input { right: true, ..Input::NONE });
///
/// let before = m.fighter(PlayerId(0)).unwrap().x();
/// m.step(&inputs);
/// let after = m.fighter(PlayerId(0)).unwrap().x();
/// assert_eq!(after, before + m.config().walk_speed);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    roster: Roster,
    config: MatchConfig,
    fighters: Vec<Fighter>,
    phase: Phase,
    remaining_ticks: u32,
    paused: bool,
}

impl Match {
    /// Spawns fighter `i` of `n` at `arena_width * (i + 1) / (n + 1)` (integer division), in
    /// roster order. Fighters at or left of the arena centre face right, the others left.
    ///
    /// The match starts in [`Phase::Countdown`], or directly in [`Phase::Fighting`] when
    /// `countdown_ticks` is zero.
    ///
    /// # Errors
    ///
    /// Returns [`SetupError::InvalidConfig`] when `config` does not validate.
    ///
    /// Complexity: O(n) in the number of players.
    ///
    /// ```
    /// use srr::game::{Match, MatchConfig, PlayerId, Roster};
    ///
    /// let m = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
    /// assert_eq!(m.fighter(PlayerId(0)).unwrap().x(), 400);
    /// assert_eq!(m.fighter(PlayerId(1)).unwrap().x(), 800);
    /// ```
    pub fn new(roster: Roster, config: MatchConfig) -> Result<Self, SetupError> {
        let count = roster.player_count() as i64;
        let width = i64::from(config.arena_width);
        // The quotient lies between 0 and the width, so it fits in an i32.
        let spawns: Vec<i32> = (0..count)
            .map(|index| (width * (index + 1) / (count + 1)) as i32)
            .collect();
        Self::with_spawns(roster, config, &spawns)
    }

    /// Spawns the fighters at explicit positions, in roster order.
    ///
    /// The configuration is validated first, then the number of spawn points, then each
    /// position.
    ///
    /// # Errors
    ///
    /// * [`SetupError::InvalidConfig`] when `config` does not validate;
    /// * [`SetupError::SpawnCountMismatch`] when `spawns` does not have one entry per player;
    /// * [`SetupError::SpawnOutOfBounds`] when a position is outside `0..=arena_width`.
    ///
    /// Complexity: O(n) in the number of players.
    ///
    /// ```
    /// use srr::game::{Match, MatchConfig, Roster, SetupError};
    ///
    /// let ok = Match::with_spawns(Roster::duel(), MatchConfig::default(), &[0, 1200]);
    /// assert!(ok.is_ok());
    ///
    /// let wrong = Match::with_spawns(Roster::duel(), MatchConfig::default(), &[10]);
    /// assert_eq!(
    ///     wrong.err(),
    ///     Some(SetupError::SpawnCountMismatch { expected: 2, actual: 1 })
    /// );
    /// ```
    pub fn with_spawns(
        roster: Roster,
        config: MatchConfig,
        spawns: &[i32],
    ) -> Result<Self, SetupError> {
        config.validate()?;
        let players = roster.players();
        if spawns.len() != players.len() {
            return Err(SetupError::SpawnCountMismatch {
                expected: players.len(),
                actual: spawns.len(),
            });
        }
        let fighters = players
            .into_iter()
            .zip(spawns)
            .map(|(player, &x)| {
                if (0..=config.arena_width).contains(&x) {
                    Ok(Fighter::spawn(player, x, &config))
                } else {
                    Err(SetupError::SpawnOutOfBounds { player, x })
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let phase = match config.countdown_ticks {
            0 => Phase::Fighting,
            remaining => Phase::Countdown { remaining },
        };
        Ok(Match {
            remaining_ticks: config.round_time_ticks,
            roster,
            config,
            fighters,
            phase,
            paused: false,
        })
    }

    /// The participants and their alliances.
    pub fn roster(&self) -> &Roster {
        &self.roster
    }

    /// The configuration the match was created with.
    pub fn config(&self) -> &MatchConfig {
        &self.config
    }

    /// Fighters in roster order.
    pub fn fighters(&self) -> &[Fighter] {
        &self.fighters
    }

    /// The fighter of `player`, or `None` when the player is not in the match.
    ///
    /// Complexity: O(n) linear scan.
    pub fn fighter(&self, player: PlayerId) -> Option<&Fighter> {
        self.fighters.iter().find(|f| f.player() == player)
    }

    /// The current phase of the match.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The result of the round, or `None` while it is still running.
    pub fn outcome(&self) -> Option<Outcome> {
        match self.phase {
            Phase::Finished(outcome) => Some(outcome),
            _ => None,
        }
    }

    /// Fighting steps left in the round.
    pub fn remaining_ticks(&self) -> u32 {
        self.remaining_ticks
    }

    /// Whether the match is paused (stepping a paused match does nothing).
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Freezes the match: [`Match::step`] changes nothing until [`Match::resume`].
    /// Pausing an already paused match is harmless.
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Undoes [`Match::pause`]; a single call resumes the match however often it was paused.
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Advances the match by one tick. Players without an entry in `inputs` press nothing;
    /// entries for players outside the match are ignored.
    ///
    /// A paused or finished match is left untouched. During the countdown the inputs are
    /// ignored and only the countdown advances. While fighting, one tick:
    ///
    /// 1. updates every fighter, in roster order, from its input and previous state; free
    ///    fighters turn to face the direction pressed together with down (nobody ever turns
    ///    by itself), then, on the floor, act with the priority technique > attack > grab >
    ///    jump > crouch > walk; an attack
    ///    still winding up turns into a technique whose combination includes its button;
    ///    grappling fighters stay put. Every fighter remembers its fresh presses (held now, not
    ///    on the previous tick) and the technique combination they completed;
    /// 2. pushes apart, in roster order of the pairs, every two solid fighters (neither knocked
    ///    out nor grappling) that are less than `body_width` apart and less than `body_height`
    ///    apart vertically: both move by half the overlap, the pair shifted back inside the
    ///    arena at a wall (on the same x, the first in roster order goes left);
    /// 3. resolves every attack (strike) that is active after that; high attacks pass over a
    ///    crouching target, and a target that walked in the direction the attack travels
    ///    (stepped back) during the tick blocks it. A
    ///    grappler that is hit or knocked out ends its grapple: its partner is free;
    /// 4. plays one tick of every grapple that existed before this tick's grabs, in roster
    ///    order of the top fighter: a projection counters or lands (see
    ///    [`Technique`](super::Technique)); otherwise reversal glitch, escape, referee
    ///    stand-up, then submission drain, advance or strike (see
    ///    [`GrappleData`](super::GrappleData)). A tap-out is a knockout and frees the top;
    /// 5. resolves every technique that is active, in roster order, then every active grab:
    ///    each catches the first fighter in roster order that is in reach, on the floor,
    ///    neither knocked out, stunned nor grappling, and that it may hurt; neither can be
    ///    blocked. Both fighters are snapped `grapple_distance` apart, facing each other, in a
    ///    clinch for a grab (its first grapple tick is the next one) or in the projection for
    ///    a technique. A technique that catches a fighter winding up the same technique is
    ///    countered (the roles are reversed, for good); two fighters whose same technique is
    ///    active at once clash and are pushed `break_distance` apart;
    /// 6. runs the round timer and evaluates the outcome.
    ///
    /// Complexity: O(n²) in the number of fighters in the worst case (every fighter attacking
    /// or grabbing every other), O(n) otherwise; allocation is limited to the lists of hits and
    /// grapples of the tick.
    pub fn step(&mut self, inputs: &HashMap<PlayerId, Input>) {
        if self.paused {
            return;
        }
        match self.phase {
            Phase::Countdown { remaining } => {
                self.phase = if remaining > 1 {
                    Phase::Countdown {
                        remaining: remaining - 1,
                    }
                } else {
                    Phase::Fighting
                };
            }
            Phase::Fighting => self.fight(inputs),
            Phase::Finished(_) => {}
        }
    }

    fn fight(&mut self, inputs: &HashMap<PlayerId, Input>) {
        for fighter in &mut self.fighters {
            let input = inputs.get(&fighter.player()).copied().unwrap_or_default();
            fighter.update(input, &self.config);
        }
        self.resolve_collisions();
        self.resolve_attacks();
        self.release_orphans();
        self.play_grapples();
        self.release_orphans();
        self.resolve_techniques();
        self.resolve_grabs();
        self.remaining_ticks = self.remaining_ticks.saturating_sub(1);
        self.phase = self.evaluate().map_or(Phase::Fighting, Phase::Finished);
    }

    /// Pushes overlapping solid fighters apart, one pass over the pairs in roster order; see
    /// [`Match::step`].
    fn resolve_collisions(&mut self) {
        let (width, height) = (self.config.body_width, self.config.body_height);
        for a in 0..self.fighters.len() {
            for b in a + 1..self.fighters.len() {
                let (first, second) = (&self.fighters[a], &self.fighters[b]);
                let gap = (i64::from(second.x()) - i64::from(first.x())).abs();
                let level = (i64::from(second.y()) - i64::from(first.y())).abs();
                if !first.is_solid()
                    || !second.is_solid()
                    || gap >= i64::from(width)
                    || level >= i64::from(height)
                {
                    continue;
                }
                let (left, right) = if first.x() <= second.x() {
                    (a, b)
                } else {
                    (b, a)
                };
                let (left_x, right_x) = separate(
                    self.fighters[left].x(),
                    self.fighters[right].x(),
                    Facing::Right,
                    width,
                    self.config.arena_width,
                );
                self.fighters[left].push_to(left_x);
                self.fighters[right].push_to(right_x);
            }
        }
    }

    /// Finds every hit against the post-update state first, then applies them, so that
    /// simultaneous attacks trade hits.
    fn resolve_attacks(&mut self) {
        for hit in self.collect_hits() {
            self.fighters[hit.attacker].record_hit(hit.target);
            // An earlier hit of this step may already have knocked the target out.
            let target = &mut self.fighters[hit.target];
            if !target.is_knocked_out() {
                target.take_hit(&hit.data, hit.direction, hit.blocked, &self.config);
            }
        }
    }

    fn collect_hits(&self) -> Vec<Hit> {
        let mut hits = Vec::new();
        for (a, attacker) in self.fighters.iter().enumerate() {
            let Some(attack) = attacker.attack().filter(|_| attacker.is_attack_active()) else {
                continue;
            };
            let direction = attacker.facing();
            for (t, target) in self.fighters.iter().enumerate() {
                // Cheapest checks first; the roster lookup comes last.
                if a != t
                    && !target.is_knocked_out()
                    && !attacker.has_hit(t)
                    && attacker.reaches(target, &self.config)
                    && self.may_hurt(attacker.player(), target.player())
                {
                    hits.push(Hit {
                        attacker: a,
                        target: t,
                        data: *self.config.attack(attack),
                        direction,
                        blocked: target.guards_against(direction),
                    });
                }
            }
        }
        hits
    }

    fn may_hurt(&self, attacker: PlayerId, target: PlayerId) -> bool {
        self.config.friendly_fire || self.roster.are_enemies(attacker, target)
    }

    /// Frees every grappler whose partner is no longer grappling with it (hit or knocked out).
    fn release_orphans(&mut self) {
        for index in 0..self.fighters.len() {
            let orphan = self.fighters[index].link().is_some_and(|link| {
                self.fighters[link.partner]
                    .link()
                    .is_none_or(|back| back.partner != index)
            });
            if orphan {
                self.fighters[index].release_grapple();
            }
        }
    }

    /// Plays one tick of every grapple, each pair once, in roster order of the top fighter.
    /// The pairs are listed first so that a reversal cannot play the same grapple twice.
    fn play_grapples(&mut self) {
        let pairs: Vec<(usize, usize, Hold)> = self
            .fighters
            .iter()
            .enumerate()
            .filter_map(|(index, fighter)| {
                fighter
                    .link()
                    .filter(|link| link.role == GrappleRole::Top)
                    .map(|link| (index, link.partner, link.hold))
            })
            .collect();
        for (top, bottom, hold) in pairs {
            self.play_grapple(top, bottom, hold);
        }
    }

    /// Plays one tick of the grapple `hold` between the fighters at positions `top` and
    /// `bottom`.
    fn play_grapple(&mut self, top: usize, bottom: usize, mut hold: Hold) {
        let turn = hold.tick(
            self.fighters[top].fresh(),
            self.fighters[bottom].fresh(),
            self.fighters[bottom].combo(),
            &self.config,
        );
        let data = &self.config.grapple;
        match turn {
            Turn::Continue { damage } => {
                self.fighters[top].set_hold(GrappleRole::Top, hold);
                self.fighters[bottom].set_hold(GrappleRole::Bottom, hold);
                self.fighters[bottom].suffer(damage);
            }
            Turn::Reversal => {
                self.fighters[top].set_hold(GrappleRole::Bottom, hold);
                self.fighters[bottom].set_hold(GrappleRole::Top, hold);
            }
            Turn::Escape | Turn::StandUp => {
                let (top_x, bottom_x) = separate(
                    self.fighters[top].x(),
                    self.fighters[bottom].x(),
                    self.fighters[top].facing(),
                    data.break_distance,
                    self.config.arena_width,
                );
                let stun = if turn == Turn::Escape {
                    data.break_stun
                } else {
                    0
                };
                self.fighters[top].leave_grapple(top_x, stun);
                self.fighters[bottom].leave_grapple(bottom_x, 0);
            }
        }
    }

    /// Connects every active technique, in roster order; see [`Match::step`].
    fn resolve_techniques(&mut self) {
        for attacker in 0..self.fighters.len() {
            // A fighter caught earlier in this loop is grappling, so no longer attempting.
            let Some((technique, AttackPhase::Active)) = self.fighters[attacker].attempt() else {
                continue;
            };
            let me = &self.fighters[attacker];
            let target = (0..self.fighters.len()).find(|&index| {
                let target = &self.fighters[index];
                index != attacker
                    && target.is_grabbable()
                    && me.technique_reaches(target, &self.config)
                    && self.may_hurt(me.player(), target.player())
            });
            let Some(target) = target else {
                continue;
            };
            match self.fighters[target].attempt() {
                Some((same, phase)) if same == technique && phase == AttackPhase::Active => {
                    self.clash(attacker, target);
                }
                Some((same, phase)) if same == technique && phase == AttackPhase::Startup => {
                    let hold = Hold::projecting(technique, true, &self.config);
                    self.lock(attacker, target, hold, GrappleRole::Bottom);
                }
                _ => {
                    let hold = Hold::projecting(technique, false, &self.config);
                    self.lock(attacker, target, hold, GrappleRole::Top);
                }
            }
        }
    }

    /// The same technique, active on both sides at once: both fighters bounce off each other,
    /// `break_distance` apart, free.
    fn clash(&mut self, attacker: usize, target: usize) {
        let (attacker_x, target_x) = separate(
            self.fighters[attacker].x(),
            self.fighters[target].x(),
            self.fighters[attacker].facing(),
            self.config.grapple.break_distance,
            self.config.arena_width,
        );
        self.fighters[attacker].leave_grapple(attacker_x, 0);
        self.fighters[target].leave_grapple(target_x, 0);
    }

    /// Connects every active grab, in roster order; see [`Match::step`].
    fn resolve_grabs(&mut self) {
        for grabber in 0..self.fighters.len() {
            // A grabber caught earlier in this loop is grappling, so no longer grabbing.
            if !self.fighters[grabber].is_grab_active() {
                continue;
            }
            let me = &self.fighters[grabber];
            let target = (0..self.fighters.len()).find(|&index| {
                let target = &self.fighters[index];
                index != grabber
                    && target.is_grabbable()
                    && me.grab_reaches(target, &self.config)
                    && self.may_hurt(me.player(), target.player())
            });
            if let Some(target) = target {
                let hold = Hold::connected(&self.config);
                self.lock(grabber, target, hold, GrappleRole::Top);
            }
        }
    }

    /// Locks the fighter at `catcher` and the one it caught in the grapple `hold`, the catcher
    /// on `role`: the caught fighter is snapped `grapple_distance` in front of the catcher and
    /// both face each other.
    fn lock(&mut self, catcher: usize, caught: usize, hold: Hold, role: GrappleRole) {
        let facing = self.fighters[catcher].facing();
        let (catcher_x, caught_x) = snap(
            self.fighters[catcher].x(),
            facing,
            self.config.grapple.grapple_distance,
            self.config.arena_width,
        );
        let catcher_id = self.fighters[catcher].player();
        let caught_id = self.fighters[caught].player();
        self.fighters[catcher].enter_grapple(caught, caught_id, role, hold, catcher_x, facing);
        self.fighters[caught].enter_grapple(
            catcher,
            catcher_id,
            role.opposite(),
            hold,
            caught_x,
            facing.opposite(),
        );
    }

    /// The last team standing wins; nobody standing is a draw; at time-up the highest total
    /// team health wins and a tie for the lead is a draw.
    fn evaluate(&self) -> Option<Outcome> {
        let mut standing = self
            .fighters
            .iter()
            .filter(|f| !f.is_knocked_out())
            .filter_map(|f| self.roster.team_of(f.player()));
        let Some(first) = standing.next() else {
            return Some(Outcome::Draw);
        };
        if standing.all(|team| team == first) {
            Some(Outcome::Winner(first))
        } else if self.remaining_ticks == 0 {
            Some(self.time_up_outcome())
        } else {
            None
        }
    }

    /// The team with the highest total health wins; a tie for the lead is a draw.
    fn time_up_outcome(&self) -> Outcome {
        let mut outcome = Outcome::Draw;
        let mut best = i64::MIN;
        for team in self.roster.teams() {
            let health = self.team_health(team);
            if health > best {
                best = health;
                outcome = Outcome::Winner(team);
            } else if health == best {
                outcome = Outcome::Draw;
            }
        }
        outcome
    }

    fn team_health(&self, team: TeamId) -> i64 {
        self.fighters
            .iter()
            .filter(|f| self.roster.team_of(f.player()) == Some(team))
            .map(|f| i64::from(f.health()))
            .sum()
    }
}
