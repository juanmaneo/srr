//! Facing: fighters never turn by themselves, the player turns them (down + a direction).
//!
//! * fighters spawn facing the arena centre (right when at or left of it);
//! * down and a direction held together, one of them freshly pressed, make a free fighter
//!   face that direction, on the floor and in the air, before it acts on the same step; in
//!   either order; holding them does nothing more, and down alone or a direction alone never
//!   turns;
//! * walking, jumping over the opponent, or the opponent walking around never turn a fighter;
//! * attacking, grabbing, stunned, grappling and knocked-out fighters ignore the combination,
//!   and it is not replayed once they are free again.

mod common;

use common::*;
use srr::game::{Facing, FighterState, Input, MatchConfig, Roster};

#[test]
fn fighters_spawn_facing_the_centre() {
    let m = duel_at(cfg(), 600, 601);
    assert_eq!(facing_of(&m, 0), Facing::Right, "at the centre");
    assert_eq!(facing_of(&m, 1), Facing::Left, "right of the centre");
}

#[test]
fn an_idle_fighter_does_not_turn_towards_an_enemy_behind_it() {
    // Both spawn in the left half, facing right; player 1 is behind player 0.
    let mut m = duel_at(cfg(), 500, 300);
    run(&mut m, 10, &no_inputs());
    assert_eq!(facing_of(&m, 0), Facing::Right);
    assert_eq!(facing_of(&m, 1), Facing::Right);
}

#[test]
fn down_and_a_direction_face_that_direction() {
    let mut m = duel_at(cfg(), 500, 300);
    m.step(&only(0, press_turn(Facing::Left)));
    assert_eq!(facing_of(&m, 0), Facing::Left);
    assert_eq!(
        state_of(&m, 0),
        FighterState::Crouching,
        "turned on the spot, crouching"
    );
    assert_eq!(x_of(&m, 0), 500, "no step");
    run(&mut m, 5, &only(0, press_turn(Facing::Left)));
    assert_eq!(facing_of(&m, 0), Facing::Left, "holding changes nothing");
    m.step(&only(0, press_turn(Facing::Left)));
    m.step(&only(0, press_turn(Facing::Right)));
    assert_eq!(facing_of(&m, 0), Facing::Right, "and back");
}

#[test]
fn the_combination_works_in_either_order_but_not_split() {
    // Down first, then the direction.
    let mut m = duel_at(cfg(), 500, 300);
    run(&mut m, 3, &only(0, press_down()));
    assert_eq!(facing_of(&m, 0), Facing::Right);
    m.step(&only(0, press_turn(Facing::Left)));
    assert_eq!(facing_of(&m, 0), Facing::Left);

    // The direction first (stepping back), then down.
    let mut m = duel_at(cfg(), 500, 300);
    run(&mut m, 3, &only(0, press_left()));
    assert_eq!(
        facing_of(&m, 0),
        Facing::Right,
        "a direction alone never turns"
    );
    m.step(&only(0, press_turn(Facing::Left)));
    assert_eq!(facing_of(&m, 0), Facing::Left);

    // Facing the direction already: nothing to do.
    let mut m = duel_at(cfg(), 500, 300);
    m.step(&only(0, press_turn(Facing::Right)));
    assert_eq!(facing_of(&m, 0), Facing::Right);

    // Both directions with down: no direction, no turn.
    let mut m = duel_at(cfg(), 500, 300);
    m.step(&only(
        0,
        Input {
            left: true,
            ..press_turn(Facing::Right)
        },
    ));
    assert_eq!(facing_of(&m, 0), Facing::Right);
}

#[test]
fn turning_changes_which_way_is_stepping_back() {
    let mut m = duel_at(cfg(), 500, 300);
    m.step(&only(0, press_left()));
    assert_eq!(state_of(&m, 0), FighterState::Blocking);
    m.step(&only(0, press_turn(Facing::Left)));
    assert_eq!(facing_of(&m, 0), Facing::Left);
    m.step(&only(0, press_left()));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Walking,
        "now walking forward"
    );
    assert_eq!(x_of(&m, 0), 480);
}

#[test]
fn fighters_do_not_turn_when_they_walk_past_each_other() {
    let mut m = duel_at(cfg(), 580, 620);
    run(
        &mut m,
        6,
        &inputs_of(&[(0, press_right()), (1, press_left())]),
    );
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (640, 560));
    assert_eq!(facing_of(&m, 0), Facing::Right);
    assert_eq!(facing_of(&m, 1), Facing::Left);
    assert_eq!(state_of(&m, 0), FighterState::Walking);
    assert_eq!(state_of(&m, 1), FighterState::Walking);
}

#[test]
fn jumping_over_the_opponent_keeps_the_old_facing_until_the_player_turns() {
    // Player 0 (560) jumps right over player 1 (640): nine steps of 10 units, landing at 650.
    let jump_right = Input {
        right: true,
        ..press_jump()
    };
    let mut m = duel_at(cfg(), 560, 640);
    m.step(&only(0, jump_right));
    run(&mut m, 8, &only(0, press_right()));
    assert_eq!((x_of(&m, 0), y_of(&m, 0)), (650, 0));
    assert_eq!(state_of(&m, 0), FighterState::Walking, "landed");
    assert_eq!(facing_of(&m, 0), Facing::Right, "still looking the old way");
    run(&mut m, 10, &no_inputs());
    assert_eq!(facing_of(&m, 0), Facing::Right, "never by itself");
    m.step(&only(0, press_turn(Facing::Left)));
    assert_eq!(facing_of(&m, 0), Facing::Left);
}

#[test]
fn a_fighter_can_turn_in_the_air_and_keeps_drifting() {
    let mut m = duel_at(cfg(), 560, 640);
    m.step(&only(0, press_jump()));
    m.step(&only(0, press_turn(Facing::Left)));
    assert_eq!(state_of(&m, 0), FighterState::Airborne);
    assert_eq!(facing_of(&m, 0), Facing::Left);
    assert_eq!(x_of(&m, 0), 550, "down does not stop the air control");
}

#[test]
fn a_turn_pressed_during_an_attack_is_ignored_and_not_replayed() {
    let mut m = close_duel(cfg());
    m.step(&only(0, jab()));
    m.step(&only(0, press_turn(Facing::Left)));
    assert_eq!(facing_of(&m, 0), Facing::Right);
    run(&mut m, 10, &only(0, press_turn(Facing::Left)));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Crouching,
        "the attack is over"
    );
    assert_eq!(
        facing_of(&m, 0),
        Facing::Right,
        "the combination was not fresh any more"
    );
}

#[test]
fn stunned_and_knocked_out_fighters_do_not_turn() {
    let config = MatchConfig {
        max_health: 20,
        ..cfg_with(|d| d.hitstun = 20)
    };
    // Player 1 (640) is hit on step 4 for 20: knocked out.
    let mut m = close_duel(config.clone());
    m.step(&only(0, jab()));
    run(&mut m, 3, &no_inputs());
    assert!(fighter(&m, 1).is_knocked_out());
    m.step(&only(1, press_turn(Facing::Right)));
    assert_eq!(facing_of(&m, 1), Facing::Left);

    let mut m = close_duel(MatchConfig {
        max_health: 100,
        ..config
    });
    m.step(&only(0, jab()));
    run(&mut m, 3, &no_inputs());
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    m.step(&only(1, press_turn(Facing::Right)));
    assert_eq!(facing_of(&m, 1), Facing::Left);
}

#[test]
fn allies_and_enemies_alike_are_ignored() {
    // 2v1: player 0 (700) spawns facing left, towards its ally player 1 (680); the only enemy,
    // player 2, stands at 800 behind it. Nobody turns.
    let mut m =
        srr::game::Match::with_spawns(Roster::two_versus_one(), cfg(), &[700, 680, 800]).unwrap();
    run(&mut m, 3, &no_inputs());
    assert_eq!(facing_of(&m, 0), Facing::Left);
    assert_eq!(facing_of(&m, 1), Facing::Left);
    assert_eq!(facing_of(&m, 2), Facing::Left);
}
