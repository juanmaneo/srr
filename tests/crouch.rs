//! Crouching: holding down (the down arrow by default) on the floor.
//!
//! * a crouching fighter stays put: it neither walks nor steps back, so it never blocks;
//! * high attacks (punch and kick) pass over a crouching target; low and medium ones hit it
//!   with full damage, stun and knockback;
//! * on the floor the priorities are technique > attack > grab > jump > crouch > walk, so down
//!   never prevents acting, and a fighter landing with down held lands crouching;
//! * down + a direction turns around (see `facing.rs`), then keeps crouching.

mod common;

use common::*;
use srr::game::{Attack, AttackPhase, Facing, FighterState, Input, Strength, Technique};

#[test]
fn holding_down_crouches_on_the_spot() {
    let mut m = close_duel(cfg());
    m.step(&only(0, press_down()));
    assert_eq!(state_of(&m, 0), FighterState::Crouching);
    run(&mut m, 5, &only(0, press_down()));
    assert_eq!(state_of(&m, 0), FighterState::Crouching);
    assert_eq!(x_of(&m, 0), 560);
    m.step(&no_inputs());
    assert_eq!(state_of(&m, 0), FighterState::Idle, "standing up again");
}

#[test]
fn a_crouching_fighter_does_not_walk_or_step_back() {
    let mut m = close_duel(cfg());
    run(&mut m, 3, &only(0, press_down()));
    // Down then right (forward): no turn needed, no step either.
    run(
        &mut m,
        3,
        &only(
            0,
            Input {
                right: true,
                ..press_down()
            },
        ),
    );
    assert_eq!(x_of(&m, 0), 560);
    assert_eq!(state_of(&m, 0), FighterState::Crouching);
    // Releasing down walks again.
    m.step(&only(0, press_right()));
    assert_eq!(x_of(&m, 0), 570);
    assert_eq!(state_of(&m, 0), FighterState::Walking);
}

#[test]
fn turning_around_keeps_crouching() {
    let mut m = close_duel(cfg());
    run(&mut m, 3, &only(0, press_turn(Facing::Left)));
    assert_eq!(facing_of(&m, 0), Facing::Left);
    assert_eq!(state_of(&m, 0), FighterState::Crouching);
    assert_eq!(x_of(&m, 0), 560);
}

/// Player 0 throws `attack` at player 1 (80 away), which holds `target` for the whole attack.
fn hit_by(attack: Attack, target: Input) -> (i32, FighterState, i32) {
    let mut m = close_duel(cfg());
    m.step(&inputs_of(&[(0, Input::pressing(attack)), (1, target)]));
    run(&mut m, 3, &only(1, target)); // active on step 4
    (health_of(&m, 1), state_of(&m, 1), x_of(&m, 1))
}

#[test]
fn high_attacks_pass_over_a_crouching_target() {
    for attack in [Attack::punch(Strength::High), Attack::kick(Strength::High)] {
        let (health, state, x) = hit_by(attack, press_down());
        assert_eq!(
            (health, state, x),
            (100, FighterState::Crouching, 640),
            "{attack}"
        );
        assert_eq!(
            hit_by(attack, Input::NONE).0,
            80,
            "{attack} hits a standing target"
        );
    }
}

#[test]
fn a_high_attack_still_active_hits_a_target_that_stands_up() {
    // Still active on step 5: standing up then gets hit.
    let mut m = close_duel(cfg());
    let high = Input::pressing(Attack::kick(Strength::High));
    m.step(&inputs_of(&[(0, high), (1, press_down())]));
    run(&mut m, 3, &only(1, press_down()));
    assert_eq!(health_of(&m, 1), 100);
    m.step(&no_inputs());
    assert_eq!(health_of(&m, 1), 80, "the attack is still active on step 5");
}

#[test]
fn low_and_medium_attacks_hit_a_crouching_target_without_a_block() {
    for strength in [Strength::Low, Strength::Medium] {
        for attack in [Attack::punch(strength), Attack::kick(strength)] {
            let (health, state, x) = hit_by(attack, press_down());
            assert_eq!(
                (health, state, x),
                (80, FighterState::HitStun, 670),
                "{attack}: full damage, stunned and pushed"
            );
        }
    }
}

#[test]
fn holding_back_and_down_does_not_block() {
    // Player 1 faces left: right + down is a crouch, not a step back.
    let back_and_down = Input {
        right: true,
        ..press_down()
    };
    let (health, state, _) = hit_by(Attack::punch(Strength::Low), back_and_down);
    assert_eq!((health, state), (80, FighterState::HitStun));

    // Standing, the same keys without down step back and block (starting 40 away, so the
    // target is still in range after stepping back).
    let mut m = duel_at(cfg(), 570, 610);
    m.step(&inputs_of(&[(0, jab()), (1, press_right())]));
    run(&mut m, 3, &only(1, press_right()));
    assert_eq!(
        (health_of(&m, 1), state_of(&m, 1)),
        (95, FighterState::Blocking),
        "standing blocks"
    );
}

#[test]
fn acting_takes_priority_over_crouching() {
    let cases = [
        (jab(), FighterState::Attacking(AttackPhase::Startup)),
        (press_grab(), FighterState::Grabbing(AttackPhase::Startup)),
        (press_jump(), FighterState::Airborne),
    ];
    for (input, expected) in cases {
        let mut m = close_duel(cfg());
        run(&mut m, 2, &only(0, press_down()));
        m.step(&only(
            0,
            Input {
                down: true,
                ..input
            },
        ));
        assert_eq!(state_of(&m, 0), expected, "{input:?}");
    }

    let [first, second] = Technique::Sweep.buttons();
    let mut m = close_duel(cfg());
    m.step(&only(
        0,
        Input {
            down: true,
            ..Input::pressing(first).with(second)
        },
    ));
    assert!(
        matches!(
            state_of(&m, 0),
            FighterState::Technique(Technique::Sweep, _)
        ),
        "{:?}",
        state_of(&m, 0)
    );
}

#[test]
fn a_crouching_attacker_returns_to_crouching_after_the_attack() {
    let mut m = close_duel(cfg());
    m.step(&only(
        0,
        Input {
            down: true,
            ..jab()
        },
    ));
    run(&mut m, 9, &only(0, press_down()));
    assert_eq!(state_of(&m, 0), FighterState::Crouching);
}

#[test]
fn down_in_the_air_does_nothing_until_the_landing_which_crouches() {
    let mut m = close_duel(cfg());
    m.step(&only(0, press_jump()));
    run(&mut m, 7, &only(0, press_down()));
    assert_eq!(state_of(&m, 0), FighterState::Airborne);
    m.step(&only(0, press_down())); // the ninth step lands
    assert_eq!(fighter(&m, 0).y(), 0);
    assert_eq!(state_of(&m, 0), FighterState::Crouching);
}
