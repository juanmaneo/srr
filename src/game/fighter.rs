//! A single fighter: movement, jump physics, attack and grab timelines, hit reactions and its
//! side of a grapple.
//!
//! Fighters are created and driven by [`Match`](super::Match); outside this module they can
//! only be observed through the read-only accessors of [`Fighter`].

use super::config::{AttackData, Glitch, MatchConfig};
use super::grapple::{Grapple, GrapplePosition, GrappleRole, Hold, Link};
use super::input::{Attack, Input, Limb, Strength};
use super::roster::{PlayerId, Roster};
use super::technique::Technique;

/// The direction a fighter looks in; attacks reach in this direction.
///
/// Fighters never turn by themselves: walking never turns (walking away from the facing
/// direction is a step *back*), and only the player turns a free fighter, with
/// [`Input::down`] and a direction pressed together (one of the two freshly): down + left
/// faces left, down + right faces right. A fighter that jumped over its opponent keeps looking
/// the old way until its player turns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Facing {
    /// Towards smaller x.
    Left,
    /// Towards larger x.
    Right,
}

impl Facing {
    /// -1 for left, 1 for right.
    pub const fn sign(self) -> i32 {
        match self {
            Facing::Left => -1,
            Facing::Right => 1,
        }
    }

    /// The other direction.
    ///
    /// ```
    /// use srr::game::Facing;
    ///
    /// assert_eq!(Facing::Left.opposite(), Facing::Right);
    /// assert_eq!(Facing::Right.opposite(), Facing::Left);
    /// ```
    pub const fn opposite(self) -> Facing {
        match self {
            Facing::Left => Facing::Right,
            Facing::Right => Facing::Left,
        }
    }
}

/// The phase of an attack, a grab or a technique in progress; see
/// [`FighterState::Attacking`], [`FighterState::Grabbing`] and [`FighterState::Technique`].
///
/// An attack runs `Startup`, then `Active` (the only phase that can hit), then `Recovery`, for
/// the number of ticks given by its [`AttackData`]; a grab and a technique do the same with the
/// timeline of their [`GrappleData`](super::GrappleData) and
/// [`TechniqueData`](super::TechniqueData).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttackPhase {
    /// Winding up; the attack cannot hit yet.
    Startup,
    /// The attack can hit targets in reach.
    Active,
    /// Cooling down; the fighter is still committed (unless the
    /// [`Glitch::AnimationCancel`] glitch is enabled and jump is pressed).
    Recovery,
}

/// What a fighter visibly does after the last step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FighterState {
    /// Standing on the floor, doing nothing.
    Idle,
    /// Moving on the floor: walking forward, or sidestepping (in any direction but back).
    Walking,
    /// In the air (jumping or falling).
    Airborne,
    /// Stepping back on the floor, away from the direction it faces, with the guard up.
    ///
    /// There is no block button: a hit is blocked when the target, free and on the floor, moves
    /// in the direction the attack travels (away from the attacker) on the hit tick. Facing the
    /// opponent, that is exactly this state.
    Blocking,
    /// Crouching on the floor ([`Input::down`] held): the fighter does not walk, does not
    /// block, and high attacks (punch and kick) pass over its head.
    Crouching,
    /// In the middle of an attack, in the given phase.
    Attacking(AttackPhase),
    /// Attempting a grab, in the given phase; only the `Active` phase can connect.
    Grabbing(AttackPhase),
    /// Attempting a judo technique, in the given phase; only the `Active` phase can connect.
    Technique(Technique, AttackPhase),
    /// Locked in a grapple, on the given side, in the given position; see
    /// [`Fighter::grapple`].
    Grappling(GrappleRole, GrapplePosition),
    /// Stunned by a hit (or by an escape from its grapple) and unable to act.
    HitStun,
    /// Out of health for the rest of the round.
    Knockout,
}

/// The fighters an attack has already struck, one bit per position in the match, since an
/// attack hits each target at most once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HitTargets(u16);

// Every fighter of a match needs its own bit.
const _: () = assert!(Roster::MAX_PLAYERS <= u16::BITS as usize);

impl HitTargets {
    const NONE: Self = HitTargets(0);

    fn contains(self, index: usize) -> bool {
        self.0 & (1u16 << index) != 0
    }

    fn insert(&mut self, index: usize) {
        self.0 |= 1u16 << index;
    }
}

/// A strike in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Strike {
    attack: Attack,
    /// The step of the attack just completed (1-based).
    frame: u32,
    /// The fighters the attack already struck.
    hit: HitTargets,
    /// Whether the attack hit a fighter that did not block it, which opens the chain: the rest
    /// of the strike can be cancelled into a stronger one.
    connected: bool,
}

impl Strike {
    /// The first step of `attack`.
    fn new(attack: Attack) -> Self {
        Strike {
            attack,
            frame: 1,
            hit: HitTargets::NONE,
            connected: false,
        }
    }

    /// The step after this one.
    fn next(self) -> Self {
        Strike {
            frame: self.frame + 1,
            ..self
        }
    }
}

/// What drives a fighter's next update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Reads its input.
    Free,
    Attacking(Strike),
    /// `frame` is the step of the grab attempt just completed (1-based).
    Grabbing {
        frame: u32,
    },
    /// `frame` is the step of the technique attempt just completed (1-based).
    Technique {
        technique: Technique,
        frame: u32,
    },
    /// Locked in a grapple; driven by the match, not by the fighter's own update.
    Grappling(Link),
    /// Steps of stun left (always at least one).
    Stunned {
        remaining: u32,
    },
    KnockedOut,
}

/// One fighter of a match: position (along the arena, across it and above the floor), health,
/// facing and current state.
///
/// Fighters cannot be constructed or mutated from outside the game module; obtain them with
/// [`Match::fighters`](super::Match::fighter) and read them through the accessors. All
/// accessors are O(1).
///
/// ```
/// use srr::game::{FighterState, Match, MatchConfig, PlayerId, Roster};
///
/// let m = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
/// let fighter = m.fighter(PlayerId(0)).unwrap();
/// assert_eq!(fighter.health(), MatchConfig::default().max_health);
/// assert_eq!(fighter.y(), 0);
/// assert_eq!(fighter.state(), FighterState::Idle);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fighter {
    player: PlayerId,
    x: i32,
    y: i32,
    z: i32,
    vertical_velocity: i32,
    airborne: bool,
    health: i32,
    facing: Facing,
    /// Horizontal direction walked on the floor during the last update, 0 when the fighter did
    /// not walk on the floor. Used to decide whether a hit is blocked.
    stride: i32,
    /// The input of the previous fighting step, to detect fresh presses.
    previous: Input,
    /// The buttons freshly pressed during the last update (held now, not on the step before).
    fresh: Input,
    /// For each attack button (punches, then kicks, low to high): the consecutive steps it has
    /// been held, 1 on the step it is pressed, 0 when released.
    held: [u32; 6],
    /// The technique whose combination the buttons completed during the last update.
    combo: Option<Technique>,
    /// Hits of the combo the fighter is caught in: unblocked hits taken in a row, each inside
    /// the stun of the previous one; back to zero when the stun ends.
    combo_hits: u32,
    action: Action,
    state: FighterState,
}

impl Fighter {
    /// A fighter on the floor, on the middle line, at full health, facing right unless it
    /// stands strictly right of the arena centre.
    pub(crate) fn spawn(player: PlayerId, x: i32, config: &MatchConfig) -> Self {
        let right_half = 2 * i64::from(x) > i64::from(config.arena_width);
        Fighter {
            player,
            x,
            y: 0,
            z: 0,
            vertical_velocity: 0,
            airborne: false,
            health: config.max_health,
            facing: if right_half {
                Facing::Left
            } else {
                Facing::Right
            },
            stride: 0,
            previous: Input::NONE,
            fresh: Input::NONE,
            held: [0; 6],
            combo: None,
            combo_hits: 0,
            action: Action::Free,
            state: FighterState::Idle,
        }
    }

    /// The player controlling this fighter.
    pub fn player(&self) -> PlayerId {
        self.player
    }

    /// Horizontal position, within `0..=arena_width`.
    pub fn x(&self) -> i32 {
        self.x
    }

    /// Height above the floor.
    pub fn y(&self) -> i32 {
        self.y
    }

    /// Position across the arena, within `-arena_depth..=arena_depth`: `0` is the middle line
    /// everybody spawns on, positive towards the wall, negative towards the street. Only
    /// sidesteps change it (and a grab or a technique, which pulls its target onto its line).
    pub fn z(&self) -> i32 {
        self.z
    }

    /// Remaining health, within `0..=max_health`.
    pub fn health(&self) -> i32 {
        self.health
    }

    /// What the fighter is doing after the last step.
    pub fn state(&self) -> FighterState {
        self.state
    }

    /// The direction the fighter looks in.
    pub fn facing(&self) -> Facing {
        self.facing
    }

    /// The attack in progress, if the fighter is attacking.
    pub fn attack(&self) -> Option<Attack> {
        match self.action {
            Action::Attacking(strike) => Some(strike.attack),
            _ => None,
        }
    }

    /// Whether the attack in progress hit a fighter that did not block it: from the next tick
    /// on, the rest of the strike can be cancelled into a stronger strike (a combo; see
    /// [`Match::step`](super::Match::step)).
    ///
    /// ```
    /// use srr::game::{Match, MatchConfig, PlayerId, Roster};
    ///
    /// let m = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
    /// assert!(!m.fighter(PlayerId(0)).unwrap().attack_connected());
    /// ```
    pub fn attack_connected(&self) -> bool {
        matches!(self.action, Action::Attacking(strike) if strike.connected)
    }

    /// Whether the fighter has run out of health. A knocked-out fighter stays knocked out.
    pub fn is_knocked_out(&self) -> bool {
        self.action == Action::KnockedOut
    }

    /// Whether the fighter is in the air (jumping or falling), whatever it does there:
    /// [`FighterState::Airborne`] is only the free flight; an aerial kick or a hit taken in the
    /// air keep the fighter airborne too.
    ///
    /// ```
    /// use srr::game::{Match, MatchConfig, PlayerId, Roster};
    ///
    /// let m = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
    /// assert!(!m.fighter(PlayerId(0)).unwrap().is_airborne());
    /// ```
    pub fn is_airborne(&self) -> bool {
        self.airborne
    }

    /// The hits of the combo the fighter is caught in: a hit that lands while the fighter is
    /// still stunned by the previous one adds to the count. A blocked hit does not count, and
    /// the count drops back to zero once the stun ends (a knocked-out fighter keeps the count
    /// of the combo that finished it).
    ///
    /// ```
    /// use srr::game::{Match, MatchConfig, PlayerId, Roster};
    ///
    /// let m = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
    /// assert_eq!(m.fighter(PlayerId(0)).unwrap().combo_hits(), 0);
    /// ```
    pub fn combo_hits(&self) -> u32 {
        self.combo_hits
    }

    /// The grapple the fighter is locked in, if any, seen from its side.
    ///
    /// ```
    /// use srr::game::{Match, MatchConfig, PlayerId, Roster};
    ///
    /// let m = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
    /// assert_eq!(m.fighter(PlayerId(0)).unwrap().grapple(), None);
    /// ```
    pub fn grapple(&self) -> Option<Grapple> {
        self.link().map(Link::view)
    }

    /// Advances the fighter by one step.
    ///
    /// Every update also remembers which buttons were freshly pressed and which technique
    /// combination they completed, for the grapple.
    pub(crate) fn update(&mut self, input: Input, config: &MatchConfig) {
        self.stride = 0;
        self.fresh = input.pressed_since(self.previous);
        self.previous = input;
        for (held, attack) in self.held.iter_mut().zip(Attack::ALL) {
            *held = if input.is_pressed(attack) {
                held.saturating_add(1)
            } else {
                0
            };
        }
        self.combo = self.completed_combo(config);
        match self.action {
            // Knocked-out fighters ignore input but still drop to the floor.
            Action::KnockedOut => {
                self.fall(config);
            }
            Action::Stunned { remaining } => self.update_stunned(remaining, config),
            Action::Attacking(strike) => self.update_attacking(strike, input, config),
            Action::Grabbing { frame } => self.update_grabbing(frame, input, config),
            Action::Technique { technique, frame } => {
                self.update_technique(technique, frame, input, config)
            }
            // Grapplers stay put on the floor; the match plays the grapple.
            Action::Grappling(_) => {}
            Action::Free => self.update_free(input, config),
        }
    }

    /// The first technique, in [`Technique::ALL`] order, whose combination the buttons
    /// complete on this step.
    fn completed_combo(&self, config: &MatchConfig) -> Option<Technique> {
        Technique::ALL
            .into_iter()
            .find(|&technique| self.completes(technique, config))
    }

    /// Whether both buttons of `technique` are held, one of them pressed on this step and the
    /// other at most `combo_window` steps earlier.
    fn completes(&self, technique: Technique, config: &MatchConfig) -> bool {
        let window = config.combo_window.saturating_add(1);
        let [first, second] = technique.buttons().map(|attack| self.held[slot(attack)]);
        first.min(second) == 1 && first.max(second) <= window
    }

    fn update_stunned(&mut self, remaining: u32, config: &MatchConfig) {
        self.fall(config);
        let remaining = remaining - 1;
        if remaining == 0 {
            self.release();
        } else {
            self.action = Action::Stunned { remaining };
        }
    }

    /// On the floor, an attack still winding up turns into the first technique whose
    /// combination, completed now, includes the attack's own button (the first of the two
    /// presses started the attack), and an attack that connected is cancelled into a stronger
    /// strike as soon as its button is held. In the air, see [`Fighter::update_flying_kick`].
    fn update_attacking(&mut self, strike: Strike, input: Input, config: &MatchConfig) {
        if self.airborne {
            self.update_flying_kick(strike, input, config);
            return;
        }
        let data = config.attack(strike.attack);
        let active_end = data.startup.saturating_add(data.active);
        let converted = Technique::ALL.into_iter().find(|&technique| {
            strike.frame <= data.startup
                && technique.buttons().contains(&strike.attack)
                && self.completes(technique, config)
        });

        if let Some(technique) = converted {
            self.start_technique(technique, 1, config);
        } else if let Some(next) = self.chained(strike, input) {
            self.set_strike(Strike::new(next), config);
        } else if strike.frame > active_end
            && input.jump
            && config.glitches.contains(Glitch::AnimationCancel)
        {
            // Glitch: the jump throws away the remaining recovery frames.
            self.action = Action::Free;
            self.take_off(input, config);
        } else if strike.frame >= data.total() {
            self.action = Action::Free;
            self.update_free(input, config);
        } else {
            self.set_strike(strike.next(), config);
        }
    }

    /// An aerial kick flies on with the jump: the fighter keeps steering, and the landing ends
    /// the kick whatever its phase (a kick started too late never comes out). Like on the
    /// floor, a kick that connected is cancelled into a stronger kick as soon as its button is
    /// held.
    fn update_flying_kick(&mut self, strike: Strike, input: Input, config: &MatchConfig) {
        if self.glide(input, config) {
            self.action = Action::Free;
            self.state = self.moving_state(input);
        } else if let Some(next) = self.chained(strike, input) {
            self.set_strike(Strike::new(next), config);
        } else if strike.frame >= config.attack(strike.attack).total() {
            self.action = Action::Free;
            self.fly(input, config);
        } else {
            self.set_strike(strike.next(), config);
        }
    }

    /// The strike `strike` is cancelled into, if it connected and `input` requests a stronger
    /// attack (a stronger kick, in the air).
    fn chained(&self, strike: Strike, input: Input) -> Option<Attack> {
        if !strike.connected {
            return None;
        }
        let next = if self.airborne {
            input.kick()
        } else {
            input.attack()
        };
        next.filter(|next| next.strength > strike.attack.strength)
    }

    /// Puts the fighter at the step of `strike`.
    fn set_strike(&mut self, strike: Strike, config: &MatchConfig) {
        let data = config.attack(strike.attack);
        self.action = Action::Attacking(strike);
        self.state = FighterState::Attacking(phase_at(strike.frame, data.startup, data.active));
    }

    /// Unlike an attack, a grab's recovery cannot be cancelled: a whiffed grab is punishable.
    fn update_grabbing(&mut self, frame: u32, input: Input, config: &MatchConfig) {
        let data = &config.grapple;
        if frame >= data.total() {
            self.action = Action::Free;
            self.update_free(input, config);
        } else {
            self.start_grab(frame + 1, config);
        }
    }

    fn start_grab(&mut self, frame: u32, config: &MatchConfig) {
        let data = &config.grapple;
        self.action = Action::Grabbing { frame };
        self.state = FighterState::Grabbing(phase_at(frame, data.startup, data.active));
    }

    /// Like a grab, a technique's recovery cannot be cancelled.
    fn update_technique(
        &mut self,
        technique: Technique,
        frame: u32,
        input: Input,
        config: &MatchConfig,
    ) {
        if frame >= config.technique(technique).total() {
            self.action = Action::Free;
            self.update_free(input, config);
        } else {
            self.start_technique(technique, frame + 1, config);
        }
    }

    fn start_technique(&mut self, technique: Technique, frame: u32, config: &MatchConfig) {
        let data = config.technique(technique);
        self.action = Action::Technique { technique, frame };
        self.state = FighterState::Technique(technique, phase_at(frame, data.startup, data.active));
    }

    /// Down + a direction turns the fighter first (on the floor and in the air). Then, on the
    /// floor, the priorities are: technique > attack > grab > jump > crouch > walk (forward,
    /// or back with the guard up) and sidestep, which go together. In the air the fighter
    /// steers and falls, then a kick starts an aerial kick (punches, grabs and techniques need
    /// the floor).
    fn update_free(&mut self, input: Input, config: &MatchConfig) {
        self.turn(input);
        if self.airborne {
            if self.glide(input, config) {
                self.state = self.moving_state(input);
            } else {
                self.fly(input, config);
            }
            return;
        }
        if let Some(technique) = self.combo {
            self.start_technique(technique, 1, config);
        } else if let Some(attack) = input.attack() {
            self.set_strike(Strike::new(attack), config);
        } else if input.grab {
            self.start_grab(1, config);
        } else if input.jump {
            self.take_off(input, config);
        } else if input.down {
            self.state = FighterState::Crouching;
        } else {
            self.stride = input.horizontal();
            self.walk(self.stride, config);
            self.sidestep(input.sideways(), config);
            self.state = self.moving_state(input);
        }
    }

    /// Faces the direction held together with down, when the combination is completed on this
    /// step (down or the direction freshly pressed): holding them does nothing more, and a
    /// press made while the fighter could not turn (attacking, stunned...) is not replayed.
    fn turn(&mut self, input: Input) {
        let (toward, fresh) = match input.horizontal() {
            -1 => (Facing::Left, self.fresh.left),
            1 => (Facing::Right, self.fresh.right),
            _ => return,
        };
        if input.down && (fresh || self.fresh.down) {
            self.facing = toward;
        }
    }

    fn take_off(&mut self, input: Input, config: &MatchConfig) {
        self.walk(input.horizontal(), config);
        self.launch(config.jump_velocity, config);
        self.state = self.moving_state(input);
    }

    /// One airborne step: steering, the arc, and on the landing step the super jump glitch.
    /// Returns true when the fighter lands in this step (even if the glitch relaunches it at
    /// once).
    fn glide(&mut self, input: Input, config: &MatchConfig) -> bool {
        self.walk(input.horizontal(), config);
        let landed = self.fall(config);
        if landed && input.jump && config.glitches.contains(Glitch::SuperJump) {
            // Glitch: jumping on the landing step bounces higher than a normal jump.
            self.launch(
                config.jump_velocity.saturating_add(config.super_jump_bonus),
                config,
            );
        }
        landed
    }

    /// Still in the air after the arc of the step: a kick button starts an aerial kick,
    /// otherwise the fighter just flies.
    fn fly(&mut self, input: Input, config: &MatchConfig) {
        match input.kick() {
            Some(kick) => self.set_strike(Strike::new(kick), config),
            None => self.state = FighterState::Airborne,
        }
    }

    /// The take-off step already moves the fighter up.
    fn launch(&mut self, velocity: i32, config: &MatchConfig) {
        self.airborne = true;
        self.vertical_velocity = velocity;
        self.fall(config);
    }

    /// One airborne step of the jump arc; returns true when the fighter lands in this step.
    fn fall(&mut self, config: &MatchConfig) -> bool {
        if !self.airborne {
            return false;
        }
        self.y = self.y.saturating_add(self.vertical_velocity);
        self.vertical_velocity = self.vertical_velocity.saturating_sub(config.gravity);
        if self.y > 0 {
            return false;
        }
        self.y = 0;
        self.vertical_velocity = 0;
        self.airborne = false;
        true
    }

    /// Moves without turning: only down + a direction changes the facing.
    fn walk(&mut self, direction: i32, config: &MatchConfig) {
        self.x = self
            .x
            .saturating_add(direction * config.walk_speed)
            .clamp(0, config.arena_width);
    }

    /// Moves across the arena, towards the wall for a positive `direction`.
    fn sidestep(&mut self, direction: i32, config: &MatchConfig) {
        self.z = self
            .z
            .saturating_add(direction * config.sidestep_speed)
            .clamp(-config.arena_depth, config.arena_depth);
    }

    fn moving_state(&self, input: Input) -> FighterState {
        let direction = input.horizontal();
        if self.airborne {
            FighterState::Airborne
        } else if input.down {
            // Landing with down held.
            FighterState::Crouching
        } else if direction == 0 && input.sideways() == 0 {
            FighterState::Idle
        } else if direction == -self.facing.sign() {
            FighterState::Blocking
        } else {
            FighterState::Walking
        }
    }

    /// Back to reading input; the combo the fighter was caught in, if any, is over.
    fn release(&mut self) {
        self.combo_hits = 0;
        self.action = Action::Free;
        self.state = self.moving_state(Input::NONE);
    }

    // ----------------------------------------------------------------------------------------
    // Combat
    // ----------------------------------------------------------------------------------------

    pub(crate) fn is_attack_active(&self) -> bool {
        self.state == FighterState::Attacking(AttackPhase::Active)
    }

    pub(crate) fn is_grab_active(&self) -> bool {
        self.state == FighterState::Grabbing(AttackPhase::Active)
    }

    /// Whether `target` is inside the box of an active grab: in front (or on the same x),
    /// within the grab's range horizontally, within `attack_height` vertically and within
    /// `attack_depth` across, all bounds inclusive.
    pub(crate) fn grab_reaches(&self, target: &Fighter, config: &MatchConfig) -> bool {
        self.is_grab_active() && self.in_box(target, config.grapple.range, config)
    }

    /// Whether a grab or a technique can catch the fighter: on the floor, not knocked out, not
    /// stunned (throw invulnerability) and not already grappling.
    pub(crate) fn is_grabbable(&self) -> bool {
        !self.airborne
            && matches!(
                self.action,
                Action::Free
                    | Action::Attacking { .. }
                    | Action::Grabbing { .. }
                    | Action::Technique { .. }
            )
    }

    /// The technique being attempted and its phase, if any.
    pub(crate) fn attempt(&self) -> Option<(Technique, AttackPhase)> {
        match self.state {
            FighterState::Technique(technique, phase) => Some((technique, phase)),
            _ => None,
        }
    }

    /// Whether `target` is inside the box of an active technique: like a grab, with the
    /// technique's range.
    pub(crate) fn technique_reaches(&self, target: &Fighter, config: &MatchConfig) -> bool {
        self.attempt().is_some_and(|(technique, phase)| {
            phase == AttackPhase::Active
                && self.in_box(target, config.technique(technique).range, config)
        })
    }

    /// The technique whose combination the buttons completed during the last update.
    pub(crate) fn combo(&self) -> Option<Technique> {
        self.combo
    }

    /// Whether the fighter has a push box: everybody but knocked-out and grappling fighters.
    pub(crate) fn is_solid(&self) -> bool {
        !matches!(self.action, Action::KnockedOut | Action::Grappling(_))
    }

    /// Moves the fighter to `x` (already inside the arena) to resolve a collision.
    pub(crate) fn push_to(&mut self, x: i32) {
        self.x = x;
    }

    /// Moves the fighter onto the line `z` (already inside the arena): a grab or a technique
    /// pulls its target onto its own line.
    pub(crate) fn pull_to(&mut self, z: i32) {
        self.z = z;
    }

    /// Whether `target` is inside the box of the current attack: in front (or on the same x),
    /// within the attack's range horizontally, within `attack_height` vertically and within
    /// `attack_depth` across, all bounds inclusive; high attacks pass over a crouching target,
    /// unless they come down from the air. Always false when the fighter is not attacking.
    pub(crate) fn reaches(&self, target: &Fighter, config: &MatchConfig) -> bool {
        self.attack().is_some_and(|attack| {
            let over_the_head = attack.strength == Strength::High
                && !self.airborne
                && target.state == FighterState::Crouching;
            !over_the_head && self.in_box(target, config.attack(attack).range, config)
        })
    }

    fn in_box(&self, target: &Fighter, range: i32, config: &MatchConfig) -> bool {
        let ahead = (target.x - self.x) * self.facing.sign();
        (0..=range).contains(&ahead)
            && (target.y - self.y).abs() <= config.attack_height
            && (i64::from(target.z) - i64::from(self.z)).abs() <= i64::from(config.attack_depth)
    }

    /// Whether a hit travelling in `direction` is blocked: the fighter is free, on the floor,
    /// and walked that way (away from the attacker) during this tick.
    pub(crate) fn guards_against(&self, direction: Facing) -> bool {
        self.stride == direction.sign()
    }

    /// Whether the current attack already struck the fighter at position `target` in the match.
    pub(crate) fn has_hit(&self, target: usize) -> bool {
        matches!(self.action, Action::Attacking(strike) if strike.hit.contains(target))
    }

    /// Remembers that the current attack struck the fighter at position `target` in the match,
    /// and whether that fighter blocked it (an unblocked hit opens the chain).
    pub(crate) fn record_hit(&mut self, target: usize, blocked: bool) {
        if let Action::Attacking(strike) = &mut self.action {
            strike.hit.insert(target);
            strike.connected |= !blocked;
        }
    }

    /// Applies one hit of an attack with frame data `data`, travelling in `direction`.
    pub(crate) fn take_hit(
        &mut self,
        data: &AttackData,
        direction: Facing,
        blocked: bool,
        config: &MatchConfig,
    ) {
        let damage = if blocked {
            // Both factors are validated non-negative and the percentage is at most 100, so the
            // result fits back into an i32.
            (i64::from(data.damage) * i64::from(100 - config.block_reduction_percent) / 100) as i32
        } else {
            data.damage
        };
        self.health = self.health.saturating_sub(damage).max(0);

        if !blocked {
            self.x = self
                .x
                .saturating_add(direction.sign() * data.knockback)
                .clamp(0, config.arena_width);
            self.combo_hits = self.combo_hits.saturating_add(1);
        }

        if self.health == 0 {
            self.knock_out();
        } else if !blocked {
            // Being hit cancels whatever the fighter was doing, including its own attack, grab
            // or grapple.
            self.stun(data.hitstun);
        }
    }

    fn knock_out(&mut self) {
        self.action = Action::KnockedOut;
        self.state = FighterState::Knockout;
    }

    /// Stunned for `ticks` steps after this one; with zero the fighter is simply free.
    fn stun(&mut self, ticks: u32) {
        if ticks > 0 {
            self.action = Action::Stunned { remaining: ticks };
            self.state = FighterState::HitStun;
        } else {
            self.release();
        }
    }

    // ----------------------------------------------------------------------------------------
    // Grappling
    // ----------------------------------------------------------------------------------------

    /// The fighter's side of its grapple, if it is grappling.
    pub(crate) fn link(&self) -> Option<&Link> {
        match &self.action {
            Action::Grappling(link) => Some(link),
            _ => None,
        }
    }

    /// The buttons freshly pressed during the last update.
    pub(crate) fn fresh(&self) -> Input {
        self.fresh
    }

    /// Locks the fighter in a grapple with the fighter at position `partner` in the match, at
    /// `x`, looking in `facing`. Whatever it was doing is cancelled.
    pub(crate) fn enter_grapple(
        &mut self,
        partner: usize,
        partner_id: PlayerId,
        role: GrappleRole,
        hold: Hold,
        x: i32,
        facing: Facing,
    ) {
        self.x = x;
        self.facing = facing;
        self.stride = 0;
        self.action = Action::Grappling(Link {
            partner,
            partner_id,
            role,
            hold,
        });
        self.set_hold(role, hold);
    }

    /// Updates the side and the shared hold of a grappling fighter (no-op otherwise).
    pub(crate) fn set_hold(&mut self, role: GrappleRole, hold: Hold) {
        if let Action::Grappling(link) = &mut self.action {
            link.role = role;
            link.hold = hold;
            self.state = FighterState::Grappling(role, hold.position());
        }
    }

    /// Leaves the grapple at `x`, stunned for `stun` steps (free at once when zero).
    pub(crate) fn leave_grapple(&mut self, x: i32, stun: u32) {
        self.x = x;
        self.stun(stun);
    }

    /// Lets go of a grapple whose partner is gone (hit or knocked out): free, where it stands.
    pub(crate) fn release_grapple(&mut self) {
        self.release();
    }

    /// Loses `damage` health to the grapple; reaching zero is a knockout (a tap-out).
    pub(crate) fn suffer(&mut self, damage: i32) {
        self.health = self.health.saturating_sub(damage).max(0);
        if self.health == 0 {
            self.knock_out();
        }
    }
}

/// The index of `attack`'s button in `Fighter::held`: [`Attack::ALL`] order.
fn slot(attack: Attack) -> usize {
    let limb = match attack.limb {
        Limb::Punch => 0,
        Limb::Kick => 3,
    };
    limb + attack.strength.index()
}

/// The phase of frame `frame` (1-based) of a timeline with the given start-up and active ticks.
fn phase_at(frame: u32, startup: u32, active: u32) -> AttackPhase {
    if frame <= startup {
        AttackPhase::Startup
    } else if frame <= startup.saturating_add(active) {
        AttackPhase::Active
    } else {
        AttackPhase::Recovery
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_hit_stun_a_hit_fighter_can_act_on_the_next_step() {
        let config = MatchConfig::default();
        let data = AttackData {
            hitstun: 0,
            ..*config.attack(Attack::punch(Strength::Low))
        };
        let mut fighter = Fighter::spawn(PlayerId(0), 100, &config);
        fighter.take_hit(&data, Facing::Right, false, &config);
        assert_eq!(fighter.state(), FighterState::Idle);
        assert_eq!(fighter.action, Action::Free);
        assert_eq!(fighter.health(), config.max_health - data.damage);
    }

    #[test]
    fn only_a_grappling_fighter_takes_a_new_hold() {
        let config = MatchConfig::default();
        let hold = Hold::connected(&config);
        let mut fighter = Fighter::spawn(PlayerId(0), 100, &config);
        let before = fighter.clone();
        fighter.set_hold(GrappleRole::Top, hold);
        assert_eq!(fighter, before, "a free fighter ignores it");

        fighter.enter_grapple(1, PlayerId(1), GrappleRole::Top, hold, 100, Facing::Right);
        fighter.set_hold(GrappleRole::Bottom, hold);
        assert_eq!(
            fighter.state(),
            FighterState::Grappling(GrappleRole::Bottom, hold.position()),
            "a grappling fighter switches sides"
        );
    }

    #[test]
    fn a_fighter_that_is_not_attacking_reaches_nobody() {
        let config = MatchConfig::default();
        let idle = Fighter::spawn(PlayerId(0), 100, &config);
        let target = Fighter::spawn(PlayerId(1), 100, &config);
        assert!(!idle.reaches(&target, &config));
    }
}
