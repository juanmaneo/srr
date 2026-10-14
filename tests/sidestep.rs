//! Sidesteps: moving across the arena, Tekken style, to leave the line of an attack.
//!
//! Numbers from `common::cfg`: a sidestep moves 5 per step, within -150..=150 across; strikes,
//! grabs and techniques reach 40 across (so 9 steps of sidestep dodge them), and push boxes are
//! 40 deep. Everybody spawns on the middle line, 0.

mod common;

use common::*;
use srr::game::{
    Attack, AttackPhase, FighterState, GrapplePosition, GrappleRole, Input, Match, MatchConfig,
    Strength,
};

fn sidestep_in() -> Input {
    Input {
        sidestep_in: true,
        ..Input::NONE
    }
}

fn sidestep_out() -> Input {
    Input {
        sidestep_out: true,
        ..Input::NONE
    }
}

fn z_of(m: &Match, id: u8) -> i32 {
    fighter(m, id).z()
}

/// `config` once player 1 sidestepped `steps` times towards the wall (`1`) or the street (`-1`)
/// while player 0 waited, at `p0_x` and `p1_x`.
fn dodged(config: MatchConfig, p0_x: i32, p1_x: i32, direction: i32, steps: usize) -> Match {
    let mut m = duel_at(config, p0_x, p1_x);
    let input = if direction > 0 {
        sidestep_in()
    } else {
        sidestep_out()
    };
    run(&mut m, steps, &only(1, input));
    assert_eq!(z_of(&m, 1), direction * 5 * steps as i32);
    m
}

// ---------------------------------------------------------------------------------------------
// Moving across
// ---------------------------------------------------------------------------------------------

#[test]
fn everybody_spawns_on_the_middle_line() {
    let m = brawl_at(cfg(), &[100, 400, 700, 1000]);
    assert!(m.fighters().iter().all(|f| f.z() == 0));
}

#[test]
fn a_sidestep_moves_across_by_its_speed_and_stops_at_the_depth_of_the_arena() {
    let mut m = close_duel(cfg());
    m.step(&only(0, sidestep_in()));
    assert_eq!((x_of(&m, 0), z_of(&m, 0)), (560, 5), "only across");
    assert_eq!(state_of(&m, 0), FighterState::Walking);
    run(&mut m, 40, &only(0, sidestep_in()));
    assert_eq!(z_of(&m, 0), 150, "against the wall");
    run(&mut m, 100, &only(0, sidestep_out()));
    assert_eq!(z_of(&m, 0), -150, "at the kerb");
    m.step(&no_inputs());
    assert_eq!(state_of(&m, 0), FighterState::Idle);
    assert_eq!(z_of(&m, 1), 0, "the other one stayed on its line");
}

#[test]
fn an_arena_without_depth_keeps_everybody_on_the_line() {
    let mut m = close_duel(MatchConfig {
        arena_depth: 0,
        ..cfg()
    });
    run(&mut m, 5, &only(0, sidestep_in()));
    assert_eq!(z_of(&m, 0), 0);
}

#[test]
fn both_sidestep_keys_cancel_out() {
    let mut m = close_duel(cfg());
    let both = Input {
        sidestep_in: true,
        sidestep_out: true,
        ..Input::NONE
    };
    m.step(&only(0, both));
    assert_eq!(z_of(&m, 0), 0);
    assert_eq!(state_of(&m, 0), FighterState::Idle);
}

#[test]
fn walking_and_sidestepping_go_together_and_stepping_back_still_blocks() {
    let mut m = close_duel(cfg());
    let forward = Input {
        sidestep_in: true,
        ..press_right()
    };
    m.step(&only(0, forward));
    assert_eq!((x_of(&m, 0), z_of(&m, 0)), (570, 5));
    assert_eq!(state_of(&m, 0), FighterState::Walking);

    let back = Input {
        sidestep_out: true,
        ..press_left()
    };
    m.step(&only(0, back));
    assert_eq!((x_of(&m, 0), z_of(&m, 0)), (560, 0));
    assert_eq!(state_of(&m, 0), FighterState::Blocking);
}

#[test]
fn jumping_crouching_and_attacking_win_over_the_sidestep() {
    for (input, state) in [
        (press_down(), FighterState::Crouching),
        (jab(), FighterState::Attacking(AttackPhase::Startup)),
        (press_jump(), FighterState::Airborne),
    ] {
        let mut m = close_duel(cfg());
        m.step(&only(
            0,
            Input {
                sidestep_in: true,
                ..input
            },
        ));
        assert_eq!(state_of(&m, 0), state);
        assert_eq!(z_of(&m, 0), 0, "{state:?}");
    }
}

#[test]
fn there_is_no_sidestep_in_the_air() {
    let mut m = close_duel(cfg());
    m.step(&only(0, press_jump()));
    run(&mut m, 5, &only(0, sidestep_in()));
    assert!(y_of(&m, 0) > 0, "still in the air");
    assert_eq!(z_of(&m, 0), 0);
}

// ---------------------------------------------------------------------------------------------
// Dodging
// ---------------------------------------------------------------------------------------------

#[test]
fn a_strike_misses_a_fighter_off_its_line_on_either_side() {
    for direction in [1, -1] {
        // 8 sidesteps: 40 across, still on the line.
        let mut m = dodged(cfg(), 560, 640, direction, 8);
        m.step(&only(0, jab()));
        run(&mut m, 5, &no_inputs());
        assert_eq!(health_of(&m, 1), 80, "hit at 40 across");

        // 9 sidesteps: 45 across, out of the line.
        let mut m = dodged(cfg(), 560, 640, direction, 9);
        m.step(&only(0, jab()));
        run(&mut m, 5, &no_inputs());
        assert_eq!(health_of(&m, 1), 100, "dodged at 45 across");
    }
}

#[test]
fn a_grab_misses_off_the_line_and_pulls_its_catch_onto_it() {
    let mut m = dodged(cfg(), 580, 620, 1, 9);
    m.step(&only(0, press_grab()));
    run(&mut m, 4, &no_inputs());
    assert!(!matches!(state_of(&m, 1), FighterState::Grappling(..)));

    let mut m = dodged(cfg(), 580, 620, 1, 8);
    m.step(&only(0, press_grab()));
    run(&mut m, 4, &no_inputs());
    assert_eq!(
        state_of(&m, 1),
        FighterState::Grappling(GrappleRole::Bottom, GrapplePosition::Clinch)
    );
    assert_eq!(z_of(&m, 1), 0, "pulled onto the catcher's line");
}

#[test]
fn a_technique_misses_off_the_line_and_pulls_its_catch_onto_it() {
    let sweep = press(Attack::punch(Strength::Medium)).with(Attack::kick(Strength::High));

    let mut m = dodged(cfg(), 580, 620, -1, 9);
    m.step(&only(0, sweep));
    run(&mut m, 3, &no_inputs());
    assert!(!matches!(state_of(&m, 1), FighterState::Grappling(..)));

    let mut m = dodged(cfg(), 580, 620, -1, 8);
    m.step(&only(0, sweep));
    run(&mut m, 3, &no_inputs());
    assert_eq!(
        state_of(&m, 1),
        FighterState::Grappling(GrappleRole::Bottom, GrapplePosition::Sweep)
    );
    assert_eq!(z_of(&m, 1), 0, "pulled onto the catcher's line");
}

// ---------------------------------------------------------------------------------------------
// Push boxes
// ---------------------------------------------------------------------------------------------

#[test]
fn fighters_a_body_depth_apart_walk_past_each_other() {
    let solid = MatchConfig {
        body_width: 40,
        body_height: 30,
        ..cfg()
    };
    // 35 across: the push boxes still meet.
    let mut m = dodged(solid.clone(), 500, 545, 1, 7);
    run(&mut m, 10, &only(0, press_right()));
    assert_eq!(x_of(&m, 1) - x_of(&m, 0), 40, "pushed along");

    // 40 across: they pass.
    let mut m = dodged(solid, 500, 545, 1, 8);
    run(&mut m, 10, &only(0, press_right()));
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (600, 545));
}
