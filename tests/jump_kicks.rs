//! Aerial kicks: a kick pressed in the air is thrown from the air.
//!
//! Punches, grabs and the judo techniques still need the floor. An aerial kick follows the frame
//! data of the kick on the floor, flies on with the jump (the fighter keeps steering) and ends
//! on the landing step whatever its phase. It reaches like every strike (`attack_height` above
//! and below the kicker), so it comes down on a standing target during the last steps of the
//! arc; coming from above, a high kick hits a crouching target too. Stepping back blocks it.
//!
//! Numbers of `common::cfg`: the jump launches at 20 with gravity 5, so the heights after the
//! take-off step are 20, 35, 45, 50, 50, 45, 35, 20, then the ninth step lands; every strike has
//! 3 start-up, 2 active and 4 recovery steps, reaches 100 along and 40 up or down, and deals 20.

mod common;

use common::*;
use srr::game::{
    Attack, AttackPhase, FighterState, Glitch, GlitchSet, Input, MatchConfig, Strength,
};

const LOW_KICK: Attack = Attack::kick(Strength::Low);
const HIGH_KICK: Attack = Attack::kick(Strength::High);
const STARTUP: FighterState = FighterState::Attacking(AttackPhase::Startup);
const ACTIVE: FighterState = FighterState::Attacking(AttackPhase::Active);
const RECOVERY: FighterState = FighterState::Attacking(AttackPhase::Recovery);

/// [`cfg`] where every strike is active from its first step.
fn instant() -> MatchConfig {
    cfg_with(|d| d.startup = 0)
}

fn only_glitch(glitch: Glitch) -> MatchConfig {
    MatchConfig {
        glitches: GlitchSet::none().with(glitch),
        ..cfg()
    }
}

// ---------------------------------------------------------------------------------------------
// Starting a kick in the air
// ---------------------------------------------------------------------------------------------

#[test]
fn a_kick_pressed_in_the_air_starts_an_aerial_kick() {
    for strength in Strength::ALL {
        let kick = Attack::kick(strength);
        let mut m = duel_at(cfg(), 100, 1100);
        m.step(&only(0, press_jump()));
        m.step(&only(0, press(kick)));
        assert_eq!(state_of(&m, 0), STARTUP, "{kick}");
        assert_eq!(attack_of(&m, 0), Some(kick));
        assert!(fighter(&m, 0).is_airborne());
        assert_eq!(y_of(&m, 0), 35, "the jump goes on");
    }
}

#[test]
fn in_the_air_the_strongest_kick_wins_whatever_punch_is_held() {
    let mut m = duel_at(cfg(), 100, 1100);
    m.step(&only(0, press_jump()));
    let mash = press(Attack::punch(Strength::High))
        .with(LOW_KICK)
        .with(Attack::kick(Strength::Medium));
    m.step(&only(0, mash));
    assert_eq!(attack_of(&m, 0), Some(Attack::kick(Strength::Medium)));
}

#[test]
fn an_aerial_kick_follows_the_frame_data_while_the_jump_goes_on_and_ends_on_landing() {
    let mut m = duel_at(cfg(), 100, 1100);
    m.step(&only(0, press_jump()));
    m.step(&only(0, press(LOW_KICK))); // step 2
    let mut timeline = vec![(state_of(&m, 0), y_of(&m, 0))];
    for _ in 3..=9 {
        m.step(&no_inputs());
        timeline.push((state_of(&m, 0), y_of(&m, 0)));
    }
    assert_eq!(
        timeline,
        [
            (STARTUP, 35),
            (STARTUP, 45),
            (STARTUP, 50),
            (ACTIVE, 50),
            (ACTIVE, 45),
            (RECOVERY, 35),
            (RECOVERY, 20),
            (FighterState::Idle, 0),
        ]
    );
    assert_eq!(
        attack_of(&m, 0),
        None,
        "the landing threw away the last two recovery steps"
    );
}

#[test]
fn the_fighter_keeps_steering_during_an_aerial_kick() {
    let mut m = duel_at(cfg(), 100, 1100);
    m.step(&only(0, press_jump()));
    let kick_right = Input {
        right: true,
        ..press(LOW_KICK)
    };
    m.step(&only(0, kick_right)); // step 2
    assert_eq!(x_of(&m, 0), 110);
    run(&mut m, 2, &only(0, kick_right)); // steps 3 and 4
    assert_eq!(x_of(&m, 0), 130);
    assert_eq!(
        (state_of(&m, 0), attack_of(&m, 0)),
        (STARTUP, Some(LOW_KICK)),
        "the held button does not restart the kick"
    );
}

#[test]
fn holding_a_kick_in_the_air_kicks_again_once_the_first_one_is_over() {
    let short = cfg_with(|d| {
        d.startup = 0;
        d.active = 1;
        d.recovery = 1;
    });
    let states = |held: bool| {
        let mut m = duel_at(short.clone(), 100, 1100);
        m.step(&only(0, press_jump()));
        (2..=5)
            .map(|step| {
                let input = if step == 2 || held {
                    press(LOW_KICK)
                } else {
                    Input::NONE
                };
                m.step(&only(0, input));
                state_of(&m, 0)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(states(true), [ACTIVE, RECOVERY, ACTIVE, RECOVERY]);
    assert_eq!(
        states(false),
        [
            ACTIVE,
            RECOVERY,
            FighterState::Airborne,
            FighterState::Airborne
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// Landing
// ---------------------------------------------------------------------------------------------

#[test]
fn a_kick_started_too_late_never_comes_out_and_the_held_button_kicks_from_the_floor() {
    let mut m = close_duel(cfg());
    m.step(&only(0, press_jump()));
    run(&mut m, 6, &no_inputs()); // steps 2..=7
    m.step(&only(0, press(LOW_KICK))); // step 8: height 20, one start-up step
    assert_eq!((state_of(&m, 0), y_of(&m, 0)), (STARTUP, 20));
    m.step(&only(0, press(LOW_KICK))); // step 9: the landing ends the kick
    assert_eq!(
        (y_of(&m, 0), state_of(&m, 0), attack_of(&m, 0)),
        (0, FighterState::Idle, None)
    );
    assert_eq!(health_of(&m, 1), 100);
    m.step(&only(0, press(LOW_KICK))); // step 10: the kick from the floor
    assert_eq!(
        (state_of(&m, 0), attack_of(&m, 0)),
        (STARTUP, Some(LOW_KICK))
    );
    assert!(!fighter(&m, 0).is_airborne());
}

#[test]
fn landing_during_an_aerial_kick_with_down_held_lands_crouching() {
    let mut m = close_duel(cfg());
    m.step(&only(0, press_jump()));
    run(&mut m, 6, &no_inputs());
    m.step(&only(0, press(LOW_KICK))); // step 8
    let crouch_kick = Input {
        down: true,
        ..press(LOW_KICK)
    };
    m.step(&only(0, crouch_kick)); // step 9
    assert_eq!(state_of(&m, 0), FighterState::Crouching);
}

#[test]
fn the_animation_cancel_glitch_cannot_double_jump_out_of_an_aerial_kick() {
    let mut m = duel_at(only_glitch(Glitch::AnimationCancel), 100, 1100);
    m.step(&only(0, press_jump()));
    m.step(&only(0, press(LOW_KICK))); // step 2
    run(&mut m, 4, &no_inputs()); // steps 3..=6: the kick is active on 5 and 6
    m.step(&only(0, press_jump())); // step 7: recovery
    assert_eq!((state_of(&m, 0), y_of(&m, 0)), (RECOVERY, 35));
    m.step(&only(0, press_jump())); // step 8
    assert_eq!((state_of(&m, 0), y_of(&m, 0)), (RECOVERY, 20));
    m.step(&only(0, press_jump())); // step 9: lands (no super jump glitch here)
    assert_eq!((state_of(&m, 0), y_of(&m, 0)), (FighterState::Idle, 0));
}

#[test]
fn the_super_jump_glitch_relaunches_a_landing_from_an_aerial_kick() {
    let mut m = duel_at(only_glitch(Glitch::SuperJump), 100, 1100);
    m.step(&only(0, press_jump()));
    m.step(&only(0, press(LOW_KICK)));
    run(&mut m, 6, &no_inputs()); // steps 3..=8
    m.step(&only(0, press_jump())); // step 9: lands and bounces, the kick is over
    assert_eq!(
        (state_of(&m, 0), y_of(&m, 0), attack_of(&m, 0)),
        (FighterState::Airborne, 30, None)
    );
}

// ---------------------------------------------------------------------------------------------
// Hitting from the air
// ---------------------------------------------------------------------------------------------

#[test]
fn a_kick_pressed_at_the_apex_comes_down_active_on_a_standing_target() {
    let mut m = close_duel(cfg());
    m.step(&only(0, press_jump()));
    run(&mut m, 2, &no_inputs()); // steps 2 and 3
    m.step(&only(0, press(LOW_KICK))); // step 4: height 50
    run(&mut m, 2, &no_inputs()); // steps 5 and 6: still winding up
    assert_eq!(health_of(&m, 1), 100);
    m.step(&no_inputs()); // step 7: active at height 35, within the reach of 40
    assert_eq!((state_of(&m, 0), y_of(&m, 0)), (ACTIVE, 35));
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(x_of(&m, 1), 670, "pushed back like by any strike");
}

#[test]
fn a_kick_active_too_high_passes_over_a_standing_target() {
    let mut m = close_duel(cfg());
    m.step(&only(0, press_jump()));
    m.step(&only(0, press(LOW_KICK))); // active on steps 5 and 6, at heights 50 and 45
    run(&mut m, 7, &no_inputs());
    assert_eq!(y_of(&m, 0), 0);
    assert_eq!(health_of(&m, 1), 100);
}

#[test]
fn an_aerial_high_kick_hits_a_crouching_target_unlike_one_from_the_floor() {
    let mut m = close_duel(instant());
    m.step(&inputs_of(&[(0, press_jump()), (1, press_down())]));
    m.step(&inputs_of(&[(0, press(HIGH_KICK)), (1, press_down())]));
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);

    let mut m = close_duel(instant());
    m.step(&inputs_of(&[(0, press(HIGH_KICK)), (1, press_down())]));
    assert_eq!(health_of(&m, 1), 100, "from the floor it passes over");
}

#[test]
fn stepping_back_blocks_an_aerial_kick() {
    let mut m = close_duel(instant());
    m.step(&only(0, press_jump()));
    // Player 1 faces left: right is back.
    m.step(&inputs_of(&[(0, press(LOW_KICK)), (1, press_right())]));
    assert_eq!(health_of(&m, 1), 95);
    assert_eq!(state_of(&m, 1), FighterState::Blocking);
    assert_eq!(x_of(&m, 1), 650, "walked back, not pushed");
}

#[test]
fn an_aerial_kick_hits_a_jumping_target() {
    let mut m = close_duel(instant());
    m.step(&inputs_of(&[(0, press_jump()), (1, press_jump())]));
    m.step(&only(0, press(LOW_KICK))); // both at height 35
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(y_of(&m, 1), 35);
}
