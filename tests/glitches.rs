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
    Attack, AttackData, AttackPhase, FighterState, Glitch, GlitchSet, Input, Match, MatchConfig,
    Roster, Strength,
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

// ---------------------------------------------------------------------------------------------
// EchoBarrage: a punch that connects unblocked rings lag copies around the target
// ---------------------------------------------------------------------------------------------

/// Player 0 jabs player 1 from close range, unblocked; returns the match right after the hit.
fn landed_jab(config: MatchConfig) -> Match {
    let mut m = close_duel(config);
    run(&mut m, 4, &only(0, jab()));
    assert_eq!(
        health_of(&m, 1),
        100 - DATA.damage,
        "the jab connected unblocked"
    );
    m
}

#[test]
fn an_unblocked_punch_rings_echoes_around_the_target() {
    let mut m = landed_jab(only_glitch(Glitch::EchoBarrage));
    m.step(&no_inputs());
    assert_eq!(
        health_of(&m, 1),
        100 - DATA.damage - cfg().echo_damage,
        "the first echo hit lands on the next tick"
    );
    run(&mut m, cfg().echo_punches as usize - 1, &no_inputs());
    assert_eq!(
        health_of(&m, 1),
        100 - DATA.damage - cfg().echo_punches as i32 * cfg().echo_damage,
        "one echo hit per tick, then the ring despawns"
    );
    run(&mut m, 5, &no_inputs());
    assert_eq!(
        health_of(&m, 1),
        100 - DATA.damage - cfg().echo_punches as i32 * cfg().echo_damage,
        "the ring despawns after its last hit"
    );
}

#[test]
fn the_echo_ring_cannot_be_blocked_by_stepping_back() {
    let mut m = landed_jab(only_glitch(Glitch::EchoBarrage));
    let before = health_of(&m, 1);
    run(&mut m, 1, &only(1, press_right()));
    assert_eq!(
        before - health_of(&m, 1),
        cfg().echo_damage,
        "the echoes hit from every direction, so guarding changes nothing"
    );
}

#[test]
fn without_the_glitch_a_punch_hurts_only_once() {
    let m = landed_jab(cfg());
    assert_eq!(health_of(&m, 1), 100 - DATA.damage);
}

#[test]
fn a_blocked_punch_rings_no_echoes() {
    let mut m = close_duel(only_glitch(Glitch::EchoBarrage));
    run(&mut m, 3, &only(0, jab()));
    m.step(&inputs_of(&[(0, jab()), (1, press_right())]));
    let blocked = 100 - DATA.damage * (100 - cfg().block_reduction_percent) / 100;
    assert_eq!(health_of(&m, 1), blocked, "the jab was blocked");
    run(&mut m, 5, &no_inputs());
    assert_eq!(health_of(&m, 1), blocked, "no echoes after a blocked punch");
}

#[test]
fn a_kick_connects_without_ringing_echoes() {
    let mut m = close_duel(only_glitch(Glitch::EchoBarrage));
    run(&mut m, 4, &only(0, press(Attack::kick(Strength::Low))));
    assert_eq!(health_of(&m, 1), 100 - DATA.damage, "the kick connected");
    run(&mut m, 5, &no_inputs());
    assert_eq!(
        health_of(&m, 1),
        100 - DATA.damage,
        "only a punch rings the echoes"
    );
}

// ---------------------------------------------------------------------------------------------
// StretchStrike: rubber arms, and the fist that keeps existing once broken off
// ---------------------------------------------------------------------------------------------

#[test]
fn punches_reach_further_than_their_frame_data_under_the_glitch() {
    let config = only_glitch(Glitch::StretchStrike);
    let beyond = DATA.range + cfg().stretch_reach;
    let mut m = duel_at(config, 100, 100 + beyond);
    run(&mut m, 5, &only(0, jab()));
    assert_eq!(
        health_of(&m, 1),
        100 - DATA.damage,
        "the stretched punch reaches its extended range"
    );
}

#[test]
fn kicks_keep_their_normal_range_under_the_glitch() {
    let config = only_glitch(Glitch::StretchStrike);
    let beyond = DATA.range + cfg().stretch_reach;
    let mut m = duel_at(config, 100, 100 + beyond);
    run(&mut m, 5, &only(0, press(Attack::kick(Strength::Low))));
    assert_eq!(health_of(&m, 1), 100, "the kick falls short");
}

#[test]
fn a_whiffed_punch_breaks_its_fist_off_at_full_stretch() {
    let config = only_glitch(Glitch::StretchStrike);
    let mut m = duel_at(config, 100, 1100);
    run(&mut m, DATA.total() as usize, &only(0, jab()));
    let fist = fighter(&m, 0).detached_fist().expect("the fist broke off");
    assert_eq!(fist.x(), 100 + DATA.range + cfg().stretch_reach);
    assert_eq!(
        (fist.y(), fist.z()),
        (0, 0),
        "it snapped off on the puncher's line"
    );
    assert_eq!(fist.remaining(), cfg().fist_lifetime - 1);
    assert_eq!(
        (fist.hit().damage, fist.hit().knockback, fist.hit().hitstun),
        (cfg().fist_damage, 0, 0),
        "the fist hurts, but neither pushes nor stuns"
    );
    run(&mut m, cfg().fist_lifetime as usize - 1, &no_inputs());
    assert!(
        fighter(&m, 0).detached_fist().is_none(),
        "the fist burns out"
    );
}

#[test]
fn a_connected_punch_keeps_its_fist() {
    let m = landed_jab(only_glitch(Glitch::StretchStrike));
    assert!(
        fighter(&m, 0).detached_fist().is_none(),
        "a punch that connected breaks nothing off"
    );
}

#[test]
fn a_broken_off_fist_still_hits_the_fighter_it_floats_on() {
    let config = only_glitch(Glitch::StretchStrike);
    let fist_x = 100 + DATA.range + cfg().stretch_reach;
    // Player 1 stands past the fist, out of its reach, then walks back onto it.
    let mut m = duel_at(config, 100, fist_x + cfg().attack_height + 1);
    run(&mut m, DATA.total() as usize, &only(0, jab()));
    assert_eq!(health_of(&m, 1), 100, "the punch whiffed");
    assert!(fighter(&m, 0).detached_fist().is_some());
    run(&mut m, 1, &only(1, press_left()));
    assert_eq!(
        health_of(&m, 1),
        100 - cfg().fist_damage,
        "the floating fist hits the fighter standing on it"
    );
    assert!(
        fighter(&m, 0).detached_fist().is_none(),
        "the fist spent itself on its hit"
    );
}

#[test]
fn without_the_glitch_no_fist_ever_breaks_off() {
    let mut m = duel_at(cfg(), 100, 1100);
    run(&mut m, DATA.total() as usize, &only(0, jab()));
    assert!(fighter(&m, 0).detached_fist().is_none());
}

// ---------------------------------------------------------------------------------------------
// QuickDraw: a punch pressed during a punch's recovery restarts the punch at once
// ---------------------------------------------------------------------------------------------

#[test]
fn pressing_a_punch_in_the_recovery_restarts_the_punch() {
    let config = only_glitch(Glitch::QuickDraw);
    let mut m = duel_at(config, 100, 1100);
    m.step(&only(0, jab()));
    run(&mut m, 5, &no_inputs());
    m.step(&only(0, press(Attack::punch(Strength::High))));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Startup),
        "the fresh punch starts over, the recovery thrown away"
    );
    assert_eq!(
        attack_of(&m, 0),
        Some(Attack::punch(Strength::High)),
        "whichever punch is held comes out"
    );
}

#[test]
fn held_down_the_button_keeps_the_fists_flying() {
    let config = only_glitch(Glitch::QuickDraw);
    let mut m = duel_at(config, 100, 1100);
    // DATA.total() is 9: after 20 held steps the jab has restarted twice over.
    run(&mut m, 20, &only(0, jab()));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Startup),
        "the flurry is in its third round (20 = 9 + 9 + 2)"
    );
}

#[test]
fn without_the_glitch_the_recovery_plays_out() {
    let mut m = duel_at(cfg(), 100, 1100);
    m.step(&only(0, jab()));
    run(&mut m, 5, &no_inputs());
    m.step(&only(0, jab()));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Recovery),
        "the press is ignored until the attack is over"
    );
}

#[test]
fn a_kick_pressed_in_a_punchs_recovery_does_not_restart() {
    let config = only_glitch(Glitch::QuickDraw);
    let mut m = duel_at(config, 100, 1100);
    m.step(&only(0, jab()));
    run(&mut m, 5, &no_inputs());
    m.step(&only(0, press(Attack::kick(Strength::High))));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Recovery),
        "only a punch restarts a punch"
    );
}

// ---------------------------------------------------------------------------------------------
// CraterSlam: a landed judo throw quakes the ground under everybody standing near
// ---------------------------------------------------------------------------------------------

#[test]
fn a_landed_throw_quakes_the_fighters_standing_nearby() {
    let quaked = quake_match(only_glitch(Glitch::CraterSlam), 600 + cfg().slam_radius);
    assert_eq!(health_of(&quaked, 2), 100 - cfg().slam_damage);
}

#[test]
fn a_bystander_beyond_the_radius_is_unhurt() {
    let quaked = quake_match(only_glitch(Glitch::CraterSlam), 601 + cfg().slam_radius);
    assert_eq!(health_of(&quaked, 2), 100);
}

#[test]
fn without_the_glitch_the_throw_hurts_only_the_thrown() {
    let quaked = quake_match(cfg(), 600 + cfg().slam_radius);
    assert_eq!(health_of(&quaked, 2), 100);
}

#[test]
fn an_aerial_bystander_shakes_it_off() {
    let config = only_glitch(Glitch::CraterSlam);
    let roster = Roster::free_for_all(3).expect("three fighters");
    let mut m = Match::with_spawns(roster, config, &[560, 600, 700]).expect("valid spawns");
    let keys = press(Attack::kick(Strength::Medium)).with(Attack::kick(Strength::High));
    m.step(&only(2, press_jump()));
    run(&mut m, 9, &inputs_of(&[(0, keys), (2, press_jump())]));
    assert_eq!(
        health_of(&m, 2),
        100,
        "a fighter in the air feels no earthquake"
    );
}

/// A three-way brawl: player 0 throws player 1, player 2 stands at `bystander_x`. Returns the
/// match right after the projection lands.
fn quake_match(config: MatchConfig, bystander_x: i32) -> Match {
    let roster = Roster::free_for_all(3).expect("three fighters");
    let mut m = Match::with_spawns(roster, config, &[560, 600, bystander_x]).expect("valid spawns");
    let keys = press(Attack::kick(Strength::Medium)).with(Attack::kick(Strength::High));
    run(&mut m, 9, &only(0, keys));
    assert_eq!(health_of(&m, 1), 100 - THROW.damage, "the throw landed");
    m
}

// ---------------------------------------------------------------------------------------------
// PhantomWarp: an unblocked hit taken in the air snaps the victim along the hit
// ---------------------------------------------------------------------------------------------

#[test]
fn an_unblocked_hit_taken_in_the_air_snaps_the_victim_along() {
    let config = only_glitch(Glitch::PhantomWarp);
    // Player 1 jumps straight up inside player 0's reach; player 0 jabs it out of the air.
    let mut m = duel_at(config, 560, 640);
    run(&mut m, 4, &only(1, press_jump()));
    let airborne = y_of(&m, 1);
    assert!(airborne > 0);
    run(&mut m, 4, &only(0, jab()));
    assert_eq!(health_of(&m, 1), 100 - DATA.damage, "the jab connected");
    // The knockback pushed player 1 right by DATA.knockback; the warp snaps it further.
    assert_eq!(
        x_of(&m, 1),
        640 + DATA.knockback + cfg().warp_distance,
        "the desync snaps the victim beyond the knockback"
    );
}

#[test]
fn without_the_glitch_the_knockback_alone_carries_the_victim() {
    let config = cfg();
    let mut m = duel_at(config, 560, 640);
    run(&mut m, 4, &only(1, press_jump()));
    run(&mut m, 4, &only(0, jab()));
    assert_eq!(x_of(&m, 1), 640 + DATA.knockback);
}

#[test]
fn a_hit_taken_on_the_floor_does_not_warp() {
    let config = only_glitch(Glitch::PhantomWarp);
    let m = landed_jab(config);
    assert_eq!(
        x_of(&m, 1),
        640 + DATA.knockback,
        "only an airborne victim desyncs"
    );
}
