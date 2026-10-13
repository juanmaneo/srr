//! Walking, stepping back, jumping and input priority.
//!
//! Jump physics, per step (`v` is the vertical velocity, `g` = gravity):
//! on the take-off step `v = jump_velocity`; every airborne step does `y += v; v -= g`; the step
//! in which `y` reaches 0 or below puts the fighter on the floor and ends the jump.
//! With `jump_velocity = 20`, `gravity = 5` the heights after steps 1..=9 are
//! `20, 35, 45, 50, 50, 45, 35, 20, 0`.
//!
//! Walking never turns a fighter (only the turn button does, see `facing.rs`): walking towards
//! the facing direction is `Walking`, walking away from it on the floor is stepping back,
//! reported as `Blocking`.
//!
//! All tests keep the fighters far apart (100 and 1100) so that no attack can connect: player 0
//! faces right and player 1 faces left.

mod common;

use common::*;
use srr::game::{
    Attack, AttackPhase, Facing, FighterState, Input, MatchConfig, Strength, Technique,
};

fn far_apart(config: MatchConfig) -> srr::game::Match {
    duel_at(config, 100, 1100)
}

// ---------------------------------------------------------------------------------------------
// Walking
// ---------------------------------------------------------------------------------------------

#[test]
fn walking_right_moves_by_the_walk_speed_each_step() {
    let mut m = far_apart(cfg());
    m.step(&only(0, press_right()));
    assert_eq!(x_of(&m, 0), 110);
    m.step(&only(0, press_right()));
    assert_eq!(x_of(&m, 0), 120);
    assert_eq!(state_of(&m, 0), FighterState::Walking);
    assert_eq!(facing_of(&m, 0), Facing::Right);
}

#[test]
fn walking_left_moves_by_the_walk_speed_each_step() {
    let mut m = far_apart(cfg());
    m.step(&only(1, press_left()));
    assert_eq!(x_of(&m, 1), 1090);
    assert_eq!(state_of(&m, 1), FighterState::Walking);
    assert_eq!(facing_of(&m, 1), Facing::Left);
}

#[test]
fn walk_speed_is_configurable() {
    let config = MatchConfig {
        walk_speed: 3,
        ..cfg()
    };
    let mut m = far_apart(config);
    run(&mut m, 4, &only(0, press_right()));
    assert_eq!(x_of(&m, 0), 112);
}

#[test]
fn releasing_the_input_stops_the_fighter() {
    let mut m = far_apart(cfg());
    m.step(&only(0, press_right()));
    m.step(&no_inputs());
    assert_eq!(x_of(&m, 0), 110);
    assert_eq!(state_of(&m, 0), FighterState::Idle);
}

#[test]
fn opposite_directions_cancel_out() {
    let mut m = far_apart(cfg());
    let both = Input {
        left: true,
        right: true,
        ..Input::NONE
    };
    m.step(&only(0, both));
    assert_eq!(x_of(&m, 0), 100);
    assert_eq!(state_of(&m, 0), FighterState::Idle);
    assert_eq!(facing_of(&m, 0), Facing::Right, "facing is unchanged");
}

#[test]
fn changing_direction_does_not_turn_the_fighter() {
    let mut m = far_apart(cfg());
    m.step(&only(0, press_left()));
    assert_eq!(facing_of(&m, 0), Facing::Right, "still facing the enemy");
    assert_eq!(x_of(&m, 0), 90);
    assert_eq!(state_of(&m, 0), FighterState::Blocking);
    m.step(&only(0, press_right()));
    assert_eq!(facing_of(&m, 0), Facing::Right);
    assert_eq!(x_of(&m, 0), 100);
    assert_eq!(state_of(&m, 0), FighterState::Walking);
}

#[test]
fn the_left_wall_stops_a_walking_fighter() {
    let mut m = duel_at(cfg(), 5, 1100);
    m.step(&only(0, press_left()));
    assert_eq!(x_of(&m, 0), 0, "clamped, not -5");
    m.step(&only(0, press_left()));
    assert_eq!(x_of(&m, 0), 0);
    assert_eq!(facing_of(&m, 0), Facing::Right);
}

#[test]
fn the_right_wall_stops_a_walking_fighter() {
    let mut m = duel_at(cfg(), 100, 1195);
    m.step(&only(1, press_right()));
    assert_eq!(x_of(&m, 1), 1200, "clamped, not 1205");
    m.step(&only(1, press_right()));
    assert_eq!(x_of(&m, 1), 1200);
    assert_eq!(facing_of(&m, 1), Facing::Left);
}

#[test]
fn inputs_only_move_the_fighter_they_belong_to() {
    let mut m = far_apart(cfg());
    m.step(&only(0, press_right()));
    assert_eq!(x_of(&m, 1), 1100);
    assert_eq!(state_of(&m, 1), FighterState::Idle);
}

#[test]
fn both_players_can_move_in_the_same_step() {
    let mut m = far_apart(cfg());
    m.step(&inputs_of(&[(0, press_right()), (1, press_left())]));
    assert_eq!(x_of(&m, 0), 110);
    assert_eq!(x_of(&m, 1), 1090);
}

#[test]
fn inputs_of_unknown_players_are_ignored() {
    let mut m = far_apart(cfg());
    let before = m.clone();
    m.step(&only(9, press_right()));
    assert_eq!(m.fighters(), before.fighters());
}

// ---------------------------------------------------------------------------------------------
// Stepping back
// ---------------------------------------------------------------------------------------------

#[test]
fn stepping_back_puts_a_grounded_fighter_in_the_blocking_state() {
    let mut m = far_apart(cfg());
    m.step(&only(0, press_left()));
    assert_eq!(state_of(&m, 0), FighterState::Blocking);
    m.step(&no_inputs());
    assert_eq!(state_of(&m, 0), FighterState::Idle);
}

#[test]
fn stepping_back_moves_at_walk_speed() {
    let mut m = far_apart(cfg());
    run(&mut m, 3, &only(0, press_left()));
    assert_eq!(x_of(&m, 0), 70);
    assert_eq!(state_of(&m, 0), FighterState::Blocking);
}

#[test]
fn a_left_facing_fighter_steps_back_by_walking_right() {
    let mut m = far_apart(cfg());
    m.step(&only(1, press_right()));
    assert_eq!(x_of(&m, 1), 1110);
    assert_eq!(state_of(&m, 1), FighterState::Blocking);
    assert_eq!(facing_of(&m, 1), Facing::Left);
}

#[test]
fn stepping_back_in_the_air_is_just_airborne() {
    let mut m = far_apart(cfg());
    m.step(&only(0, press_jump()));
    m.step(&only(0, press_left()));
    assert_eq!(state_of(&m, 0), FighterState::Airborne);
    assert_eq!(x_of(&m, 0), 90, "horizontal control still works");
}

#[test]
fn there_is_no_block_without_movement() {
    let mut m = far_apart(cfg());
    m.step(&no_inputs());
    assert_eq!(state_of(&m, 0), FighterState::Idle);
}

// ---------------------------------------------------------------------------------------------
// Jumping
// ---------------------------------------------------------------------------------------------

#[test]
fn a_jump_follows_the_gravity_arc() {
    let mut m = far_apart(cfg());
    m.step(&only(0, press_jump()));
    let mut heights = vec![y_of(&m, 0)];
    for _ in 0..8 {
        m.step(&no_inputs());
        heights.push(y_of(&m, 0));
    }
    assert_eq!(heights, vec![20, 35, 45, 50, 50, 45, 35, 20, 0]);
}

#[test]
fn the_arc_depends_on_the_configured_jump_velocity_and_gravity() {
    let config = MatchConfig {
        jump_velocity: 10,
        gravity: 10,
        ..cfg()
    };
    let mut m = far_apart(config);
    m.step(&only(0, press_jump()));
    let mut heights = vec![y_of(&m, 0)];
    for _ in 0..2 {
        m.step(&no_inputs());
        heights.push(y_of(&m, 0));
    }
    assert_eq!(heights, vec![10, 10, 0]);
}

#[test]
fn a_fighter_is_airborne_until_the_landing_step_and_idle_afterwards() {
    let mut m = far_apart(cfg());
    m.step(&only(0, press_jump()));
    for step in 1..=8 {
        assert_eq!(state_of(&m, 0), FighterState::Airborne, "after step {step}");
        m.step(&no_inputs());
    }
    assert_eq!(state_of(&m, 0), FighterState::Idle, "after step 9");
    assert_eq!(y_of(&m, 0), 0);
}

#[test]
fn landing_while_holding_a_direction_shows_the_matching_ground_state() {
    let mut m = far_apart(cfg());
    m.step(&only(0, press_jump()));
    run(&mut m, 8, &only(0, press_left()));
    assert_eq!(y_of(&m, 0), 0);
    assert_eq!(state_of(&m, 0), FighterState::Blocking);

    let mut m = far_apart(cfg());
    m.step(&only(0, press_jump()));
    run(&mut m, 8, &only(0, press_right()));
    assert_eq!(state_of(&m, 0), FighterState::Walking);
}

#[test]
fn holding_jump_does_not_allow_a_double_jump() {
    let mut m = far_apart(cfg());
    let mut heights = Vec::new();
    for _ in 0..5 {
        m.step(&only(0, press_jump()));
        heights.push(y_of(&m, 0));
    }
    assert_eq!(heights, vec![20, 35, 45, 50, 50]);
}

#[test]
fn a_fighter_can_jump_again_once_landed() {
    let mut m = far_apart(cfg());
    m.step(&only(0, press_jump()));
    run(&mut m, 8, &no_inputs());
    assert_eq!(y_of(&m, 0), 0);
    m.step(&only(0, press_jump()));
    assert_eq!(y_of(&m, 0), 20);
}

#[test]
fn airborne_fighters_keep_horizontal_control() {
    let mut m = far_apart(cfg());
    let jump_right = Input {
        jump: true,
        right: true,
        ..Input::NONE
    };
    run(&mut m, 3, &only(0, jump_right));
    assert_eq!(x_of(&m, 0), 130);
    assert_eq!(y_of(&m, 0), 45);
}

#[test]
fn airborne_fighters_cannot_leave_the_arena() {
    let mut m = duel_at(cfg(), 100, 1195);
    let jump_right = Input {
        jump: true,
        right: true,
        ..Input::NONE
    };
    run(&mut m, 4, &only(1, jump_right));
    assert_eq!(x_of(&m, 1), 1200);
}

// ---------------------------------------------------------------------------------------------
// Priorities: attack > jump > walk
// ---------------------------------------------------------------------------------------------

#[test]
fn jump_wins_over_walking_and_carries_the_horizontal_movement() {
    let mut m = far_apart(cfg());
    let both = Input {
        jump: true,
        left: true,
        ..Input::NONE
    };
    m.step(&only(0, both));
    assert_eq!(state_of(&m, 0), FighterState::Airborne);
    assert_eq!(y_of(&m, 0), 20);
    assert_eq!(x_of(&m, 0), 90);
}

#[test]
fn a_technique_wins_over_everything_else() {
    let mut m = far_apart(cfg());
    let everything = Input::from_bits(u16::MAX);
    m.step(&only(0, everything));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Technique(Technique::Sweep, AttackPhase::Startup),
        "both combinations are complete: the first technique wins"
    );
    assert_eq!(attack_of(&m, 0), None);
    assert_eq!(x_of(&m, 0), 100);
    assert_eq!(y_of(&m, 0), 0);
}

#[test]
fn attack_wins_over_everything_else_but_a_technique() {
    let mut m = far_apart(cfg());
    let everything_but_the_high_kick = Input {
        kick_high: false,
        ..Input::from_bits(u16::MAX)
    };
    m.step(&only(0, everything_but_the_high_kick));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Startup)
    );
    assert_eq!(attack_of(&m, 0), Some(Attack::punch(Strength::High)));
    assert_eq!(x_of(&m, 0), 100);
    assert_eq!(y_of(&m, 0), 0);
}

#[test]
fn attack_wins_over_walking() {
    let mut m = far_apart(cfg());
    let walk_and_attack = Input {
        right: true,
        ..jab()
    };
    m.step(&only(0, walk_and_attack));
    assert_eq!(x_of(&m, 0), 100, "attacking fighters stand still");
}

#[test]
fn attack_wins_over_jumping() {
    let mut m = far_apart(cfg());
    let jump_and_attack = Input {
        jump: true,
        ..jab()
    };
    m.step(&only(0, jump_and_attack));
    assert_eq!(y_of(&m, 0), 0);
    assert_eq!(attack_of(&m, 0), Some(JAB));
}
