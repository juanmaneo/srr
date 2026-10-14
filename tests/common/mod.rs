//! Shared helpers for the `srr` integration tests.
//!
//! Every gameplay test builds its match from [`cfg`], an explicit configuration, so no test
//! depends on the numbers chosen for `MatchConfig::default()`.
//!
//! Numbers worth remembering when reading the tests (all derived from [`cfg`]):
//!
//! * the arena is 1200 wide (centre at 600); fighters at or left of the centre spawn facing right
//! * walking moves 10 units per step; a jump launches at 20 with gravity 5
//!   (apex 50, nine steps from take-off to landing)
//! * all six attacks share the same frame data ([`DATA`]): 3 start-up + 2 active + 4 recovery
//!   steps, range 100, damage 20, knockback 30, and a hit stuns for 5 steps; the vertical reach
//!   is 40 and blocking absorbs 75 % of the damage
//! * `close_duel` places the fighters at 560 and 640: facing each other, 80 units apart; player
//!   0 blocks by pressing left, player 1 by pressing right (stepping back)
//! * the grab ([`GRAPPLE`]) has 2 start-up + 1 active + 6 recovery steps and range 50; a
//!   grapple holds the fighters 40 apart and breaks them 100 apart (see [`GRAPPLE`] for the
//!   ladder numbers)
//! * the techniques ([`SWEEP`], [`THROW`]) accept their two presses up to 2 steps apart
//! * push boxes are off (`body_width` 0, `body_height` 100) so fighters may share an x; the
//!   collision tests switch them on
//! * everybody spawns on the middle line (`z` 0); a sidestep moves 5 across per step, within
//!   -150..=150; strikes, grabs and techniques reach 40 across, push boxes are 40 deep
//!
//! Simulation contract the tests rely on (one `Match::step` per tick, fixed timestep):
//!
//! 1. every living fighter reads its input, in roster order, against the state left by the
//!    previous step; a free fighter faces the direction pressed together with down (never by
//!    itself), then, on the floor, acts with the priority technique > attack > grab > jump >
//!    crouch > walk and sidestep. Attacking,
//!    grabbing and jumping need the ground, horizontal control also works in the air, and
//!    walking never turns the fighter: walking away from the facing direction is stepping back;
//! 2. overlapping push boxes are separated;
//! 3. every attack that is active after that resolves against the state produced by the
//!    update, so simultaneous attacks trade hits, high attacks pass over a crouching target,
//!    and a target that stepped back (walked on the floor in the direction the attack travels)
//!    on the hit tick blocks it;
//! 4. every grapple plays one tick; 5. every active technique, then every active grab, connects;
//! 6. the round timer runs and the outcome is evaluated.
#![allow(dead_code)]

use std::collections::HashMap;

use srr::game::{
    Attack, AttackData, Facing, Fighter, FighterState, GlitchSet, GrappleData, Input, Match,
    MatchConfig, PlayerId, Roster, Strength, TechniqueData,
};

pub type Inputs = HashMap<PlayerId, Input>;

/// The frame data [`cfg`] gives to every one of the six attacks.
pub const DATA: AttackData = AttackData {
    startup: 3,
    active: 2,
    recovery: 4,
    range: 100,
    damage: 20,
    knockback: 30,
    hitstun: 5,
};

/// The grappling rules [`cfg`] uses: small round numbers, so the tests can count ticks.
pub const GRAPPLE: GrappleData = GrappleData {
    startup: 2,
    active: 1,
    recovery: 6,
    range: 50,
    grapple_distance: 40,
    advance_ticks: 5,
    max_hold_ticks: 100,
    strike_cooldown: 3,
    clinch_strike_damage: 2,
    ground_strike_damage: 3,
    takedown_damage: 7,
    submission_damage: 4,
    submission_interval: 2,
    escape_presses: [2, 3, 4],
    break_distance: 100,
    break_stun: 6,
};

/// The sweep (`5` + `9`) [`cfg`] uses: 2 start-up + 2 active + 5 recovery steps, range 60, a
/// 4-step projection countered in its first 2 steps, landing for 10, escaped with 3 presses.
pub const SWEEP: TechniqueData = TechniqueData {
    startup: 2,
    active: 2,
    recovery: 5,
    range: 60,
    projection_ticks: 4,
    counter_window: 2,
    damage: 10,
    escape_presses: 3,
};

/// The throw (`8` + `9`) [`cfg`] uses: 3 start-up + 1 active + 6 recovery steps, range 50, a
/// 5-step projection countered in its first 3 steps, landing for 15, escaped with 4 presses.
pub const THROW: TechniqueData = TechniqueData {
    startup: 3,
    active: 1,
    recovery: 6,
    range: 50,
    projection_ticks: 5,
    counter_window: 3,
    damage: 15,
    escape_presses: 4,
};

/// The weakest, fastest attack; with [`cfg`] it behaves like every other attack.
pub const JAB: Attack = Attack::punch(Strength::Low);

/// Explicit, glitch-free, countdown-free configuration used by almost every test.
pub fn cfg() -> MatchConfig {
    MatchConfig {
        arena_width: 1200,
        max_health: 100,
        walk_speed: 10,
        jump_velocity: 20,
        gravity: 5,
        punches: [DATA; 3],
        kicks: [DATA; 3],
        attack_height: 40,
        block_reduction_percent: 75,
        round_time_ticks: 600,
        countdown_ticks: 0,
        friendly_fire: false,
        glitches: GlitchSet::none(),
        super_jump_bonus: 10,
        grapple: GRAPPLE,
        body_width: 0,
        body_height: 100,
        combo_window: 2,
        techniques: [SWEEP, THROW],
        arena_depth: 150,
        sidestep_speed: 5,
        attack_depth: 40,
        body_depth: 40,
    }
}

/// `config` with `change` applied to the frame data of all six attacks.
pub fn with_every_attack(mut config: MatchConfig, change: impl Fn(&mut AttackData)) -> MatchConfig {
    config
        .punches
        .iter_mut()
        .chain(config.kicks.iter_mut())
        .for_each(change);
    config
}

/// [`cfg`] with `change` applied to the frame data of all six attacks.
pub fn cfg_with(change: impl Fn(&mut AttackData)) -> MatchConfig {
    with_every_attack(cfg(), change)
}

// ---------------------------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------------------------

pub fn no_inputs() -> Inputs {
    Inputs::new()
}

pub fn inputs_of(pairs: &[(u8, Input)]) -> Inputs {
    pairs
        .iter()
        .map(|(id, input)| (PlayerId(*id), *input))
        .collect()
}

pub fn only(player: u8, input: Input) -> Inputs {
    inputs_of(&[(player, input)])
}

pub fn press_left() -> Input {
    Input {
        left: true,
        ..Input::NONE
    }
}

pub fn press_right() -> Input {
    Input {
        right: true,
        ..Input::NONE
    }
}

pub fn press_jump() -> Input {
    Input {
        jump: true,
        ..Input::NONE
    }
}

pub fn press_grab() -> Input {
    Input {
        grab: true,
        ..Input::NONE
    }
}

pub fn press_down() -> Input {
    Input {
        down: true,
        ..Input::NONE
    }
}

/// `input` with down and the direction of `toward` held too: turns a free fighter that way.
pub fn turning(input: Input, toward: Facing) -> Input {
    Input {
        down: true,
        left: toward == Facing::Left,
        right: toward == Facing::Right,
        ..input
    }
}

/// Down + the direction of `toward`, nothing else.
pub fn press_turn(toward: Facing) -> Input {
    turning(Input::NONE, toward)
}

/// Only the button of `attack` is held.
pub fn press(attack: Attack) -> Input {
    Input::pressing(attack)
}

/// Only the low punch ([`JAB`]) is held.
pub fn jab() -> Input {
    press(JAB)
}

// ---------------------------------------------------------------------------------------------
// Match construction
// ---------------------------------------------------------------------------------------------

/// A duel with player 0 at `p0_x` and player 1 at `p1_x`.
pub fn duel_at(config: MatchConfig, p0_x: i32, p1_x: i32) -> Match {
    Match::with_spawns(Roster::duel(), config, &[p0_x, p1_x]).expect("valid duel setup")
}

/// A duel where both fighters face each other, 80 units apart (inside the attack range of
/// [`cfg`]).
pub fn close_duel(config: MatchConfig) -> Match {
    duel_at(config, 560, 640)
}

/// A free-for-all with one player per spawn point.
pub fn brawl_at(config: MatchConfig, spawns: &[i32]) -> Match {
    let roster = Roster::free_for_all(spawns.len()).expect("valid free-for-all size");
    Match::with_spawns(roster, config, spawns).expect("valid free-for-all setup")
}

// ---------------------------------------------------------------------------------------------
// Observation
// ---------------------------------------------------------------------------------------------

pub fn fighter(m: &Match, id: u8) -> &Fighter {
    m.fighter(PlayerId(id))
        .unwrap_or_else(|| panic!("no fighter with id {id}"))
}

pub fn x_of(m: &Match, id: u8) -> i32 {
    fighter(m, id).x()
}

pub fn y_of(m: &Match, id: u8) -> i32 {
    fighter(m, id).y()
}

pub fn health_of(m: &Match, id: u8) -> i32 {
    fighter(m, id).health()
}

pub fn state_of(m: &Match, id: u8) -> FighterState {
    fighter(m, id).state()
}

pub fn facing_of(m: &Match, id: u8) -> Facing {
    fighter(m, id).facing()
}

pub fn attack_of(m: &Match, id: u8) -> Option<Attack> {
    fighter(m, id).attack()
}

/// Steps the match `steps` times with the same inputs every time.
pub fn run(m: &mut Match, steps: usize, inputs: &Inputs) {
    for _ in 0..steps {
        m.step(inputs);
    }
}
