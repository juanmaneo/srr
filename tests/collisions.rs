//! Push boxes: fighters cannot walk through each other, but can jump over each other.
//!
//! With `body_width` 40 and `body_height` 30 (the rest from `common::cfg`): two solid fighters
//! (neither knocked out nor grappling) less than 40 apart and less than 30 apart vertically are
//! pushed apart after the movement of the step, each by half the overlap, the pair shifted
//! back inside the arena at a wall.

mod common;

use common::*;
use srr::game::{FighterState, GrapplePosition, Input, MatchConfig};

fn solid() -> MatchConfig {
    MatchConfig {
        body_width: 40,
        body_height: 30,
        ..cfg()
    }
}

#[test]
fn walking_into_a_standing_fighter_pushes_it() {
    let mut m = duel_at(solid(), 500, 545);
    m.step(&only(0, press_right()));
    // 510 and 545 overlap by 5: both move half of it, around their middle (527).
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (507, 547));
    run(&mut m, 10, &only(0, press_right()));
    assert_eq!(x_of(&m, 1) - x_of(&m, 0), 40, "always one body apart");
    assert!(x_of(&m, 1) > 547, "pushed along");
}

#[test]
fn fighters_walking_into_each_other_stop_one_body_apart() {
    let mut m = duel_at(solid(), 500, 560);
    run(
        &mut m,
        10,
        &inputs_of(&[(0, press_right()), (1, press_left())]),
    );
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (510, 550));
    assert_eq!(state_of(&m, 0), FighterState::Walking);
}

#[test]
fn fighters_on_the_same_x_are_separated_first_in_roster_order_to_the_left() {
    let mut m = duel_at(solid(), 600, 600);
    m.step(&no_inputs());
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (580, 620));
}

#[test]
fn a_fighter_against_a_wall_cannot_be_pushed_through_it() {
    let mut m = duel_at(solid(), 1180, 1200);
    m.step(&no_inputs());
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (1160, 1200));
    run(&mut m, 5, &only(0, press_right()));
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (1160, 1200));

    let mut m = duel_at(solid(), 10, 0);
    m.step(&no_inputs());
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (40, 0));
}

#[test]
fn in_an_arena_narrower_than_a_body_both_stay_inside() {
    let config = MatchConfig {
        arena_width: 30,
        ..solid()
    };
    let mut m = duel_at(config, 10, 20);
    m.step(&no_inputs());
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (0, 30));
}

#[test]
fn jumping_over_the_opponent_crosses_sides() {
    // Player 0 (540) jumps right over player 1 (600): 30 or more above it from step 2 to step
    // 7, so the bodies pass; landing on step 8 (y 20) at 620 pushes it to the right side.
    let jump_right = Input {
        right: true,
        ..press_jump()
    };
    let mut m = duel_at(solid(), 540, 600);
    m.step(&only(0, jump_right));
    run(&mut m, 5, &only(0, press_right()));
    assert_eq!(
        (x_of(&m, 0), y_of(&m, 0)),
        (600, 45),
        "right above player 1"
    );
    assert_eq!(x_of(&m, 1), 600);
    run(&mut m, 2, &only(0, press_right()));
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (630, 590));
    m.step(&only(0, press_right()));
    assert_eq!((x_of(&m, 0), y_of(&m, 0)), (640, 0));
    assert!(x_of(&m, 0) > x_of(&m, 1), "crossed over");
}

#[test]
fn a_low_jump_bumps_into_the_opponent() {
    let config = MatchConfig {
        body_height: 100,
        ..solid()
    };
    let jump_right = Input {
        right: true,
        ..press_jump()
    };
    let mut m = duel_at(config, 540, 600);
    m.step(&only(0, jump_right));
    run(&mut m, 8, &only(0, press_right()));
    assert!(x_of(&m, 0) < x_of(&m, 1), "never got past");
    assert!(x_of(&m, 1) - x_of(&m, 0) >= 40);
}

#[test]
fn grapplers_keep_their_own_distance() {
    // A grapple holds the fighters 40 apart, closer than these 45-wide bodies.
    let config = MatchConfig {
        body_width: 45,
        ..solid()
    };
    let mut m = duel_at(config, 560, 605);
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    assert_eq!(
        state_of(&m, 0),
        FighterState::Grappling(srr::game::GrappleRole::Top, GrapplePosition::Clinch)
    );
    run(&mut m, 3, &no_inputs());
    assert_eq!(x_of(&m, 1) - x_of(&m, 0), 40);
}

#[test]
fn knocked_out_fighters_can_be_walked_over() {
    let config = MatchConfig {
        max_health: 20,
        ..cfg_with(|d| d.knockback = 0)
    };
    let config = MatchConfig {
        body_width: 40,
        body_height: 30,
        ..config
    };
    // A third fighter far away keeps the round going.
    let mut m = brawl_at(config, &[560, 600, 1100]);
    m.step(&only(0, jab()));
    run(&mut m, 8, &no_inputs());
    assert!(fighter(&m, 1).is_knocked_out());
    run(&mut m, 6, &only(0, press_right()));
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (620, 600));
}
