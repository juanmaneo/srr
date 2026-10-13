//! The intentional glitches ("BERP"-style exploits from the README's inspiration).
//!
//! Each glitch is individually switchable through `MatchConfig::glitches`; with a glitch off the
//! corresponding behaviour must not happen.
//!
//! * `AnimationCancel`: a jump pressed on a step that starts in an attack's *recovery* cancels
//!   the rest of the recovery and takes off immediately. Start-up and active frames cannot be
//!   cancelled.
//! * `SuperJump`: a jump pressed on the step in which a fighter lands re-launches it at once with
//!   a boosted take-off velocity (so a jump held down keeps bouncing, higher than a normal jump).
//!
//! Timeline reminder for `cfg()` (the same for all six attacks): an attack started on step 1 is
//! in start-up after steps 1..=3, active after steps 4..=5 and in recovery after steps 6..=9.

mod common;

use common::*;
use srr::game::{
    Attack, AttackData, AttackPhase, FighterState, Glitch, GlitchSet, Input, MatchConfig, Strength,
};

fn only_glitch(glitch: Glitch) -> MatchConfig {
    MatchConfig {
        glitches: GlitchSet::none().with(glitch),
        ..cfg()
    }
}

/// Player 0 attacks on step 1, idles until `jump_step - 1`, and presses jump on `jump_step`.
/// Returns the match right after `jump_step`.
fn attack_then_jump_on(config: MatchConfig, jump_step: usize) -> srr::game::Match {
    let mut m = duel_at(config, 100, 1100);
    m.step(&only(0, jab()));
    run(&mut m, jump_step - 2, &no_inputs());
    m.step(&only(0, press_jump()));
    m
}

/// The state the attack timeline predicts after `step` when nothing interferes.
fn timeline_state(step: usize) -> FighterState {
    match step {
        1..=3 => FighterState::Attacking(AttackPhase::Startup),
        4..=5 => FighterState::Attacking(AttackPhase::Active),
        6..=9 => FighterState::Attacking(AttackPhase::Recovery),
        _ => FighterState::Idle,
    }
}

/// Highest point reached while player 0 holds jump for `steps` steps.
fn highest_point_holding_jump(config: MatchConfig, steps: usize) -> i32 {
    let mut m = duel_at(config, 100, 1100);
    let mut highest = 0;
    for _ in 0..steps {
        m.step(&only(0, press_jump()));
        highest = highest.max(y_of(&m, 0));
    }
    highest
}

// ---------------------------------------------------------------------------------------------
// AnimationCancel
// ---------------------------------------------------------------------------------------------

#[test]
fn recovery_can_be_cancelled_into_a_jump() {
    for jump_step in 7..=9 {
        let m = attack_then_jump_on(only_glitch(Glitch::AnimationCancel), jump_step);
        assert_eq!(
            state_of(&m, 0),
            FighterState::Airborne,
            "jump pressed on step {jump_step}"
        );
        assert_eq!(y_of(&m, 0), 20, "jump pressed on step {jump_step}");
    }
}

#[test]
fn a_cancelled_attack_does_not_resume_after_landing() {
    let mut m = attack_then_jump_on(only_glitch(Glitch::AnimationCancel), 7);
    run(&mut m, 8, &no_inputs());
    assert_eq!(y_of(&m, 0), 0);
    assert_eq!(state_of(&m, 0), FighterState::Idle);
}

#[test]
fn start_up_and_active_frames_cannot_be_cancelled() {
    for jump_step in 2..=6 {
        let m = attack_then_jump_on(only_glitch(Glitch::AnimationCancel), jump_step);
        assert_eq!(
            state_of(&m, 0),
            timeline_state(jump_step),
            "jump pressed on step {jump_step}"
        );
        assert_eq!(y_of(&m, 0), 0, "jump pressed on step {jump_step}");
    }
}

#[test]
fn the_cancel_window_follows_the_frame_data_of_the_attack_in_progress() {
    // The high kick is slower than the jab: start-up 6, active 2, so it is in recovery after
    // step 9 and step 10 is the first step that can be cancelled.
    let mut config = only_glitch(Glitch::AnimationCancel);
    config.kicks[2] = AttackData { startup: 6, ..DATA };
    let high_kick = Attack::kick(Strength::High);

    let mut m = duel_at(config, 100, 1100);
    m.step(&only(0, press(high_kick)));
    run(&mut m, 6, &no_inputs());
    m.step(&only(0, press_jump())); // step 8: starts in the active phase
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Active)
    );
    m.step(&only(0, press_jump())); // step 9: still starts in the active phase
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Recovery)
    );
    assert_eq!(y_of(&m, 0), 0);

    let jump_right = Input {
        jump: true,
        right: true,
        ..Input::NONE
    };
    m.step(&only(0, jump_right)); // step 10: first step that starts in recovery
    assert_eq!(state_of(&m, 0), FighterState::Airborne);
    assert_eq!(
        (x_of(&m, 0), y_of(&m, 0)),
        (110, 20),
        "the take-off carries the direction"
    );
    assert_eq!(attack_of(&m, 0), None);
}

#[test]
fn without_the_glitch_recovery_cannot_be_cancelled() {
    for jump_step in 7..=9 {
        let m = attack_then_jump_on(cfg(), jump_step);
        assert_eq!(
            state_of(&m, 0),
            timeline_state(jump_step),
            "jump pressed on step {jump_step}"
        );
        assert_eq!(y_of(&m, 0), 0, "jump pressed on step {jump_step}");
    }
}

#[test]
fn super_jump_alone_does_not_allow_animation_cancels() {
    let m = attack_then_jump_on(only_glitch(Glitch::SuperJump), 8);
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Recovery)
    );
    assert_eq!(y_of(&m, 0), 0);
}

// ---------------------------------------------------------------------------------------------
// SuperJump
// ---------------------------------------------------------------------------------------------

#[test]
fn a_normal_jump_never_exceeds_its_apex_even_when_jump_is_held() {
    assert_eq!(highest_point_holding_jump(cfg(), 40), 50);
}

#[test]
fn super_jump_goes_higher_than_a_normal_jump() {
    let boosted = highest_point_holding_jump(only_glitch(Glitch::SuperJump), 40);
    let normal = highest_point_holding_jump(cfg(), 40);
    assert!(boosted > normal, "{boosted} should exceed {normal}");
}

#[test]
fn landing_with_jump_held_keeps_the_fighter_airborne_under_the_glitch() {
    let mut m = duel_at(only_glitch(Glitch::SuperJump), 100, 1100);
    run(&mut m, 9, &only(0, press_jump()));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Airborne,
        "re-launched on landing"
    );
}

#[test]
fn landing_with_jump_held_just_lands_without_the_glitch() {
    let mut m = duel_at(cfg(), 100, 1100);
    run(&mut m, 9, &only(0, press_jump()));
    assert_eq!(state_of(&m, 0), FighterState::Idle);
    assert_eq!(y_of(&m, 0), 0);
}

#[test]
fn pressing_jump_only_on_the_landing_step_is_enough() {
    let mut m = duel_at(only_glitch(Glitch::SuperJump), 100, 1100);
    m.step(&only(0, press_jump()));
    run(&mut m, 7, &no_inputs()); // steps 2..=8, still in the air
    assert_eq!(state_of(&m, 0), FighterState::Airborne);
    m.step(&only(0, press_jump())); // step 9: landing step
    assert_eq!(state_of(&m, 0), FighterState::Airborne);
}

#[test]
fn without_a_jump_press_on_the_landing_step_the_fighter_lands_normally() {
    let mut m = duel_at(only_glitch(Glitch::SuperJump), 100, 1100);
    m.step(&only(0, press_jump()));
    run(&mut m, 8, &no_inputs());
    assert_eq!(state_of(&m, 0), FighterState::Idle);
    assert_eq!(y_of(&m, 0), 0);
}

#[test]
fn animation_cancel_alone_does_not_boost_jumps() {
    assert_eq!(
        highest_point_holding_jump(only_glitch(Glitch::AnimationCancel), 40),
        50
    );
}

#[test]
fn all_glitches_can_be_active_together() {
    let config = MatchConfig {
        glitches: GlitchSet::all(),
        ..cfg()
    };
    let m = attack_then_jump_on(config.clone(), 8);
    assert_eq!(state_of(&m, 0), FighterState::Airborne);
    assert!(highest_point_holding_jump(config, 40) > 50);
}
