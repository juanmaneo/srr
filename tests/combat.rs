//! Attacks, hit detection, blocking, hit stun, knockback, knock-outs and alliances.
//!
//! Every attack of `cfg()` has the same frame data, so these tests use the jab (low punch);
//! `attacks.rs` covers what differs between the six attacks.
//!
//! Attack timeline (start-up S, active A, recovery R; `cfg()` has S=3, A=2, R=4):
//! the step in which the attack input is read is step 1 of the attack. The state *after* step k
//! is `Startup` for k in 1..=S, `Active` for S+1..=S+A, `Recovery` for S+A+1..=S+A+R, and the
//! fighter is free again on step S+A+R+1 (so after that step it is `Idle` without input, or
//! `Startup` again when an attack button is still held).
//!
//! Hit rules:
//! * an active attack hits every valid target, at most once per target per attack;
//! * the target must be in front of the attacker (or on the same x), at most the attack's
//!   `range` away horizontally and at most `attack_height` away vertically (bounds inclusive);
//!   fighters never turn by themselves, so a target behind the attacker is missed until the
//!   attacker's player turns it around (see `facing.rs`);
//! * valid targets are living enemies; allies only with `friendly_fire`;
//! * there is no block button: a target that stepped back on the hit tick (walked on the floor,
//!   free, in the direction the attack travels) blocks. It takes
//!   `damage * (100 - block_reduction_percent) / 100` (rounded down) and is neither stunned nor
//!   pushed; otherwise the target takes the full damage, is stunned for `hitstun` steps (the hit
//!   step itself excluded) and pushed `knockback` units in the attack's direction (clamped to
//!   the arena);
//! * a stunned fighter can be hit again (combos) and every hit restarts the stun;
//! * a fighter reduced to 0 health is knocked out for good.

mod common;

use common::*;
use srr::game::{
    AttackPhase, Facing, FighterState, Input, Match, MatchConfig, Outcome, Phase, Roster, TeamId,
};

const STARTUP: FighterState = FighterState::Attacking(AttackPhase::Startup);
const ACTIVE: FighterState = FighterState::Attacking(AttackPhase::Active);
const RECOVERY: FighterState = FighterState::Attacking(AttackPhase::Recovery);

/// Player 0 attacks once (and then idles) for a whole attack cycle; returns player 1's health.
fn health_after_one_attack(config: MatchConfig, attacker_x: i32, target_x: i32) -> i32 {
    let mut m = duel_at(config, attacker_x, target_x);
    m.step(&only(0, jab()));
    run(&mut m, 9, &no_inputs());
    health_of(&m, 1)
}

/// Steps 1..=4 of "player 0 attacks": the hit lands on the fourth step.
fn run_until_the_first_hit(m: &mut Match) {
    m.step(&only(0, jab()));
    run(m, 3, &no_inputs());
}

/// Player 0 jabs on steps 1..=4 (the hit lands on step 4) while player 1 does nothing on steps
/// 1..=3 and holds `target_input` on step 4.
fn hit_while_the_target_holds(config: MatchConfig, target_input: Input) -> Match {
    let mut m = close_duel(config);
    run(&mut m, 3, &only(0, jab()));
    m.step(&inputs_of(&[(0, jab()), (1, target_input)]));
    m
}

// ---------------------------------------------------------------------------------------------
// Attack timeline
// ---------------------------------------------------------------------------------------------

#[test]
fn an_attack_moves_through_startup_active_and_recovery() {
    let mut m = duel_at(cfg(), 100, 1100);
    m.step(&only(0, jab()));
    let mut states = vec![state_of(&m, 0)];
    for _ in 1..10 {
        m.step(&no_inputs());
        states.push(state_of(&m, 0));
    }
    assert_eq!(
        states,
        vec![
            STARTUP,
            STARTUP,
            STARTUP,
            ACTIVE,
            ACTIVE,
            RECOVERY,
            RECOVERY,
            RECOVERY,
            RECOVERY,
            FighterState::Idle
        ]
    );
}

#[test]
fn the_fighter_reports_its_attack_only_while_attacking() {
    let mut m = duel_at(cfg(), 100, 1100);
    assert_eq!(attack_of(&m, 0), None);
    m.step(&only(0, jab()));
    for step in 1..=9 {
        assert_eq!(attack_of(&m, 0), Some(JAB), "after step {step}");
        m.step(&no_inputs());
    }
    assert_eq!(attack_of(&m, 0), None, "after step 10");
}

#[test]
fn the_timeline_follows_the_configured_frame_data() {
    let config = cfg_with(|d| {
        d.startup = 1;
        d.active = 1;
        d.recovery = 1;
    });
    let mut m = duel_at(config, 100, 1100);
    m.step(&only(0, jab()));
    let mut states = vec![state_of(&m, 0)];
    for _ in 1..4 {
        m.step(&no_inputs());
        states.push(state_of(&m, 0));
    }
    assert_eq!(states, vec![STARTUP, ACTIVE, RECOVERY, FighterState::Idle]);
}

#[test]
fn an_attack_without_start_up_is_active_on_its_first_step() {
    let config = cfg_with(|d| {
        d.startup = 0;
        d.recovery = 0;
    });
    let mut m = close_duel(config);
    m.step(&only(0, jab()));
    assert_eq!(state_of(&m, 0), ACTIVE);
    assert_eq!(health_of(&m, 1), 80);
    m.step(&no_inputs());
    assert_eq!(state_of(&m, 0), ACTIVE);
    m.step(&no_inputs());
    assert_eq!(state_of(&m, 0), FighterState::Idle);
}

#[test]
fn holding_attack_chains_a_new_attack_as_soon_as_recovery_is_over() {
    let mut m = duel_at(cfg(), 100, 1100);
    run(&mut m, 9, &only(0, jab()));
    assert_eq!(state_of(&m, 0), RECOVERY);
    m.step(&only(0, jab()));
    assert_eq!(state_of(&m, 0), STARTUP);
}

#[test]
fn movement_input_is_ignored_during_an_attack() {
    let mut m = duel_at(cfg(), 100, 1100);
    m.step(&only(0, jab()));
    run(&mut m, 8, &only(0, press_right()));
    assert_eq!(x_of(&m, 0), 100);
}

#[test]
fn a_punch_is_ignored_while_airborne() {
    // Kicks work in the air: see `jump_kicks.rs`.
    let mut m = duel_at(cfg(), 100, 1100);
    m.step(&only(0, press_jump()));
    m.step(&only(0, jab()));
    assert_eq!(state_of(&m, 0), FighterState::Airborne);
    assert_eq!(attack_of(&m, 0), None);
}

#[test]
fn a_fighter_stepping_back_can_start_an_attack() {
    let mut m = duel_at(cfg(), 100, 1100);
    m.step(&only(0, press_left()));
    assert_eq!(state_of(&m, 0), FighterState::Blocking);
    m.step(&only(0, jab()));
    assert_eq!(state_of(&m, 0), STARTUP);
}

// ---------------------------------------------------------------------------------------------
// Hit detection
// ---------------------------------------------------------------------------------------------

#[test]
fn the_hit_connects_on_the_first_active_step() {
    let mut m = close_duel(cfg());
    m.step(&only(0, jab()));
    run(&mut m, 2, &no_inputs());
    assert_eq!(health_of(&m, 1), 100, "still in start-up");
    m.step(&no_inputs());
    assert_eq!(health_of(&m, 1), 80);
}

#[test]
fn damage_is_dealt_once_per_attack_even_when_active_for_several_steps() {
    let config = cfg_with(|d| {
        d.active = 3;
        d.knockback = 0;
    });
    assert_eq!(health_after_one_attack(config, 560, 640), 80);
}

#[test]
fn damage_is_configurable() {
    let config = cfg_with(|d| d.damage = 7);
    assert_eq!(health_after_one_attack(config, 560, 640), 93);
}

#[test]
fn a_target_exactly_at_range_is_hit() {
    assert_eq!(health_after_one_attack(cfg(), 500, 600), 80);
}

#[test]
fn a_target_one_unit_beyond_range_is_missed() {
    assert_eq!(health_after_one_attack(cfg(), 500, 601), 100);
}

#[test]
fn a_target_on_the_same_x_is_hit() {
    assert_eq!(health_after_one_attack(cfg(), 560, 560), 80);
}

#[test]
fn an_attacker_does_not_turn_by_itself_to_hit_a_target_behind_it() {
    // Player 0 spawns at 560 facing right; player 1 is at 480, behind it.
    let mut m = duel_at(cfg(), 560, 480);
    assert_eq!(facing_of(&m, 0), Facing::Right);
    run_until_the_first_hit(&mut m);
    assert_eq!(facing_of(&m, 0), Facing::Right);
    assert_eq!(health_of(&m, 1), 100, "the jab went the other way");
}

#[test]
fn an_attacker_turns_around_to_hit_a_target_behind_it() {
    // Turning and attacking on the same step: the turn comes first, so the hit lands and
    // pushes player 1 further left.
    let mut m = duel_at(cfg(), 560, 480);
    m.step(&only(0, turning(jab(), Facing::Left)));
    assert_eq!(facing_of(&m, 0), Facing::Left);
    run(&mut m, 3, &no_inputs());
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(x_of(&m, 1), 450);
}

#[test]
fn a_target_that_crosses_behind_the_attacker_during_start_up_is_missed() {
    // Player 1 (580) walks left through player 0 (560), which is committed to a right-facing
    // attack: after step 4 it stands at 540, behind the attacker, for both active steps.
    let mut m = duel_at(cfg(), 560, 580);
    m.step(&inputs_of(&[(0, jab()), (1, press_left())]));
    run(&mut m, 4, &only(1, press_left()));
    assert_eq!(x_of(&m, 1), 530);
    assert_eq!(facing_of(&m, 0), Facing::Right, "attacks never turn");
    run(&mut m, 5, &no_inputs());
    assert_eq!(health_of(&m, 1), 100);
}

#[test]
fn an_enemy_behind_the_attacker_is_missed_when_another_one_is_nearer_in_front() {
    // Free-for-all: player 0 (560) spawns facing right, towards player 1 (620); player 2 (460)
    // is within range distance but behind.
    let mut m = brawl_at(cfg(), &[560, 620, 460]);
    run_until_the_first_hit(&mut m);
    assert_eq!(facing_of(&m, 0), Facing::Right);
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(health_of(&m, 2), 100);
}

#[test]
fn a_left_facing_attacker_hits_targets_on_its_left() {
    // Player 0 stands at 700 (right half) and faces left.
    assert_eq!(health_after_one_attack(cfg(), 700, 640), 80);
}

#[test]
fn a_left_facing_attacker_respects_the_range_too() {
    assert_eq!(health_after_one_attack(cfg(), 700, 600), 80);
    assert_eq!(health_after_one_attack(cfg(), 700, 599), 100);
}

#[test]
fn jumping_over_an_attack_avoids_it() {
    let mut m = close_duel(cfg());
    m.step(&inputs_of(&[(0, jab()), (1, press_jump())]));
    run(&mut m, 9, &no_inputs());
    // The target is at height 50 on both active steps (4 and 5), above the reach of 40.
    assert_eq!(health_of(&m, 1), 100);
}

fn health_after_attacking_a_jumper(attack_height: i32) -> i32 {
    let config = MatchConfig {
        attack_height,
        ..cfg_with(|d| {
            d.startup = 1;
            d.active = 1;
            d.recovery = 1;
        })
    };
    let mut m = close_duel(config);
    // Step 1: the attack starts and the target takes off (height 20). Step 2: the attack is
    // active and the target is at height 35.
    m.step(&inputs_of(&[(0, jab()), (1, press_jump())]));
    m.step(&no_inputs());
    health_of(&m, 1)
}

#[test]
fn the_vertical_reach_is_inclusive() {
    assert_eq!(health_after_attacking_a_jumper(35), 80);
}

#[test]
fn a_target_one_unit_above_the_vertical_reach_is_missed() {
    assert_eq!(health_after_attacking_a_jumper(34), 100);
}

#[test]
fn the_attacker_is_not_affected_by_its_own_hit() {
    let mut m = close_duel(cfg());
    run_until_the_first_hit(&mut m);
    assert_eq!(health_of(&m, 0), 100);
    assert_eq!(x_of(&m, 0), 560);
}

// ---------------------------------------------------------------------------------------------
// Hit effects
// ---------------------------------------------------------------------------------------------

#[test]
fn a_hit_stuns_and_pushes_the_target_away_from_a_right_facing_attacker() {
    let mut m = close_duel(cfg());
    run_until_the_first_hit(&mut m);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(x_of(&m, 1), 670);
}

#[test]
fn a_hit_pushes_the_target_away_from_a_left_facing_attacker() {
    let mut m = duel_at(cfg(), 700, 640);
    run_until_the_first_hit(&mut m);
    assert_eq!(x_of(&m, 1), 610);
}

#[test]
fn knockback_cannot_push_a_fighter_through_the_left_wall() {
    let config = cfg_with(|d| d.range = 700);
    let mut m = duel_at(config, 700, 10);
    run_until_the_first_hit(&mut m);
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(x_of(&m, 1), 0);
}

#[test]
fn knockback_cannot_push_a_fighter_through_the_right_wall() {
    let config = cfg_with(|d| d.range = 700);
    let mut m = duel_at(config, 500, 1195);
    run_until_the_first_hit(&mut m);
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(x_of(&m, 1), 1200);
}

#[test]
fn a_stunned_fighter_cannot_act_for_the_configured_number_of_steps() {
    let mut m = close_duel(cfg());
    run_until_the_first_hit(&mut m); // step 4: hit, stun for steps 5..=9
    let held_left = only(1, press_left());

    for step in 5..=8 {
        m.step(&held_left);
        assert_eq!(state_of(&m, 1), FighterState::HitStun, "after step {step}");
        assert_eq!(x_of(&m, 1), 670, "after step {step}");
    }
    m.step(&held_left); // step 9: last stunned step, the input is still ignored
    assert_ne!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(x_of(&m, 1), 670);

    m.step(&held_left); // step 10: free again, walks towards player 0
    assert_eq!(x_of(&m, 1), 660);
    assert_eq!(state_of(&m, 1), FighterState::Walking);
    assert_eq!(health_of(&m, 1), 80, "no further damage");
}

#[test]
fn without_hit_stun_the_target_can_act_on_the_next_step() {
    let config = cfg_with(|d| d.hitstun = 0);
    let mut m = close_duel(config);
    run_until_the_first_hit(&mut m);
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(x_of(&m, 1), 670, "still pushed back");
    assert_eq!(state_of(&m, 1), FighterState::Idle);
    m.step(&only(1, press_left()));
    assert_eq!(x_of(&m, 1), 660);
}

#[test]
fn a_fighter_hit_in_the_air_falls_while_stunned() {
    let config = cfg_with(|d| d.hitstun = 20);
    let mut m = close_duel(config);
    m.step(&only(0, jab()));
    m.step(&no_inputs());
    m.step(&only(1, press_jump())); // step 3: take-off, height 20
    m.step(&no_inputs()); // step 4: height 35, within reach
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(y_of(&m, 1), 35);
    run(&mut m, 7, &no_inputs()); // the rest of the arc: 45, 50, 50, 45, 35, 20, 0
    assert_eq!(y_of(&m, 1), 0);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
}

#[test]
fn a_stun_that_ends_in_the_air_leaves_the_fighter_airborne() {
    let config = cfg_with(|d| d.hitstun = 1);
    let mut m = close_duel(config);
    m.step(&only(0, jab()));
    m.step(&no_inputs());
    m.step(&only(1, press_jump()));
    m.step(&no_inputs()); // step 4: hit at height 35
    m.step(&no_inputs()); // step 5: the only stunned step
    assert_eq!(state_of(&m, 1), FighterState::Airborne);
    assert_eq!(y_of(&m, 1), 45);
}

#[test]
fn being_hit_cancels_the_victims_own_attack() {
    let mut m = close_duel(cfg());
    m.step(&only(0, jab())); // step 1: player 0 starts, active on step 4
    m.step(&only(1, jab())); // step 2: player 1 starts, would be active on step 5
    run(&mut m, 2, &no_inputs()); // steps 3, 4: player 0 hits player 1 on step 4
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(attack_of(&m, 1), None);
    run(&mut m, 20, &no_inputs());
    assert_eq!(health_of(&m, 0), 100, "the interrupted attack never lands");
}

#[test]
fn simultaneous_active_attacks_trade_hits() {
    let mut m = close_duel(cfg());
    run(&mut m, 4, &inputs_of(&[(0, jab()), (1, jab())]));
    assert_eq!(health_of(&m, 0), 80);
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(state_of(&m, 0), FighterState::HitStun);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(x_of(&m, 0), 530);
    assert_eq!(x_of(&m, 1), 670);
}

#[test]
fn stunned_fighters_can_be_hit_again_and_every_hit_restarts_the_stun() {
    let config = cfg_with(|d| {
        d.knockback = 0;
        d.hitstun = 20;
    });
    let mut m = close_duel(config);
    // Player 0 holds attack until step 10 and so starts a second attack on step 10.
    run(&mut m, 10, &only(0, jab()));
    assert_eq!(health_of(&m, 1), 80, "only the first attack has hit so far");

    run(&mut m, 3, &no_inputs()); // steps 11..=13: the second attack hits on step 13
    assert_eq!(health_of(&m, 1), 60);

    // The first stun alone would have ended after step 24; the second one lasts until step 33.
    run(&mut m, 12, &no_inputs());
    assert_eq!(state_of(&m, 1), FighterState::HitStun, "after step 25");
    run(&mut m, 8, &no_inputs());
    assert_ne!(state_of(&m, 1), FighterState::HitStun, "after step 33");
}

#[test]
fn two_attackers_hitting_the_same_target_on_the_same_step_both_count() {
    // Players 0 (560) and 2 (720) both jab player 1 (640) from either side.
    let mut m = brawl_at(cfg_with(|d| d.knockback = 0), &[560, 640, 720]);
    run(&mut m, 4, &inputs_of(&[(0, jab()), (2, jab())]));
    assert_eq!(health_of(&m, 1), 60);
    assert_eq!(x_of(&m, 1), 640);
}

// ---------------------------------------------------------------------------------------------
// Blocking (stepping back)
// ---------------------------------------------------------------------------------------------

#[test]
fn stepping_back_on_the_hit_step_blocks() {
    // Player 1 faces left, so pressing right steps back (to 650, still in range).
    let mut m = hit_while_the_target_holds(cfg(), press_right());
    assert_eq!(health_of(&m, 1), 95, "20 damage, 75 % blocked");
    assert_eq!(state_of(&m, 1), FighterState::Blocking);
    assert_eq!(x_of(&m, 1), 650, "no knockback");
    // Not stunned: the target acts on the very next step.
    m.step(&only(1, press_left()));
    assert_eq!(x_of(&m, 1), 640);
    assert_eq!(state_of(&m, 1), FighterState::Walking);
}

#[test]
fn stepping_back_during_the_whole_start_up_blocks_too() {
    let mut m = duel_at(cfg(), 560, 610);
    run(&mut m, 4, &inputs_of(&[(0, jab()), (1, press_right())]));
    assert_eq!(x_of(&m, 1), 650);
    assert_eq!(health_of(&m, 1), 95);
    assert_eq!(state_of(&m, 1), FighterState::Blocking);
}

#[test]
fn a_right_facing_fighter_blocks_by_stepping_left() {
    let mut m = close_duel(cfg());
    run(&mut m, 3, &only(1, jab()));
    m.step(&inputs_of(&[(0, press_left()), (1, jab())]));
    assert_eq!(x_of(&m, 0), 550);
    assert_eq!(health_of(&m, 0), 95);
    assert_eq!(state_of(&m, 0), FighterState::Blocking);
}

#[test]
fn a_step_back_released_on_the_hit_step_does_not_block() {
    let mut m = duel_at(cfg(), 560, 610);
    run(&mut m, 3, &inputs_of(&[(0, jab()), (1, press_right())]));
    assert_eq!(state_of(&m, 1), FighterState::Blocking);
    m.step(&only(0, jab()));
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(x_of(&m, 1), 670);
}

#[test]
fn standing_still_does_not_block() {
    let m = hit_while_the_target_holds(cfg(), Input::NONE);
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
}

#[test]
fn walking_forward_does_not_block() {
    let m = hit_while_the_target_holds(cfg(), press_left());
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(x_of(&m, 1), 660, "walked to 630, then pushed 30");
}

#[test]
fn attacking_while_holding_back_does_not_block() {
    // The attack wins over the walk, so the fighter does not step back.
    let held = Input {
        right: true,
        ..jab()
    };
    let m = hit_while_the_target_holds(cfg(), held);
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(x_of(&m, 1), 670);
}

#[test]
fn jumping_back_does_not_block() {
    let held = Input {
        right: true,
        jump: true,
        ..Input::NONE
    };
    let m = hit_while_the_target_holds(cfg(), held);
    assert_eq!(y_of(&m, 1), 20, "took off, still within reach");
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(x_of(&m, 1), 680, "moved to 650, then pushed 30");
}

#[test]
fn drifting_back_in_the_air_does_not_block() {
    let mut m = close_duel(cfg());
    run(&mut m, 2, &only(0, jab()));
    m.step(&inputs_of(&[(0, jab()), (1, press_jump())])); // step 3: take-off, height 20
    m.step(&inputs_of(&[(0, jab()), (1, press_right())])); // step 4: height 35, x 650
    assert_eq!(y_of(&m, 1), 35);
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
}

#[test]
fn blocking_depends_on_the_attack_direction_not_on_the_facing() {
    // Player 1 (560) spawns facing right, towards player 2 (600), and walks towards it: that is
    // a step away from player 0 (500), whose attack travels right, so it is blocked.
    let mut m = brawl_at(cfg(), &[500, 560, 600]);
    run(&mut m, 3, &only(0, jab()));
    m.step(&inputs_of(&[(0, jab()), (1, press_right())]));
    assert_eq!(facing_of(&m, 1), Facing::Right);
    assert_eq!(health_of(&m, 1), 95);
    assert_eq!(state_of(&m, 1), FighterState::Walking);
}

#[test]
fn a_full_block_absorbs_all_damage() {
    let config = MatchConfig {
        block_reduction_percent: 100,
        ..cfg()
    };
    let m = hit_while_the_target_holds(config, press_right());
    assert_eq!(health_of(&m, 1), 100);
}

#[test]
fn a_block_without_reduction_still_prevents_stun_and_knockback() {
    let config = MatchConfig {
        block_reduction_percent: 0,
        ..cfg()
    };
    let m = hit_while_the_target_holds(config, press_right());
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(state_of(&m, 1), FighterState::Blocking);
    assert_eq!(x_of(&m, 1), 650);
}

#[test]
fn blocked_damage_is_rounded_down() {
    let config = MatchConfig {
        block_reduction_percent: 50,
        ..cfg_with(|d| d.damage = 33)
    };
    let m = hit_while_the_target_holds(config, press_right());
    assert_eq!(health_of(&m, 1), 84, "33 * 50 / 100 = 16 damage");
}

#[test]
fn blocked_damage_can_knock_out() {
    let config = MatchConfig {
        max_health: 5,
        ..cfg()
    };
    let m = hit_while_the_target_holds(config, press_right());
    assert_eq!(health_of(&m, 1), 0, "20 * 25 / 100 = 5 damage");
    assert_eq!(state_of(&m, 1), FighterState::Knockout);
    assert_eq!(m.outcome(), Some(Outcome::Winner(TeamId(0))));
}

// ---------------------------------------------------------------------------------------------
// Knock-out
// ---------------------------------------------------------------------------------------------

#[test]
fn health_is_clamped_at_zero_and_the_fighter_is_knocked_out() {
    let config = MatchConfig {
        max_health: 10,
        ..cfg()
    };
    let mut m = close_duel(config);
    run_until_the_first_hit(&mut m);
    assert_eq!(health_of(&m, 1), 0, "20 damage on 10 health");
    assert_eq!(state_of(&m, 1), FighterState::Knockout);
    assert!(fighter(&m, 1).is_knocked_out());
}

#[test]
fn a_knocked_out_fighter_ignores_input_and_cannot_be_hurt_further() {
    let config = MatchConfig {
        max_health: 10,
        ..cfg()
    };
    // Three-way brawl so the match keeps running after the first knock-out.
    let mut m = brawl_at(config, &[560, 640, 1100]);
    run_until_the_first_hit(&mut m);
    assert!(fighter(&m, 1).is_knocked_out());
    assert_eq!(m.phase(), Phase::Fighting);

    let x_after_ko = x_of(&m, 1);
    // Player 1 tries to walk while player 0 keeps attacking it.
    run(&mut m, 30, &inputs_of(&[(0, jab()), (1, press_right())]));
    assert_eq!(x_of(&m, 1), x_after_ko);
    assert_eq!(health_of(&m, 1), 0);
    assert_eq!(state_of(&m, 1), FighterState::Knockout);
}

#[test]
fn a_fighter_knocked_out_in_the_air_falls_to_the_floor() {
    let config = MatchConfig {
        max_health: 10,
        ..cfg()
    };
    let mut m = brawl_at(config, &[560, 640, 1100]);
    m.step(&only(0, jab()));
    m.step(&no_inputs());
    m.step(&only(1, press_jump()));
    m.step(&only(1, press_jump())); // step 4: knocked out at height 35
    assert!(fighter(&m, 1).is_knocked_out());
    assert_eq!(y_of(&m, 1), 35);
    run(&mut m, 7, &only(1, press_jump()));
    assert_eq!(y_of(&m, 1), 0);
    run(&mut m, 5, &only(1, press_jump()));
    assert_eq!(y_of(&m, 1), 0, "no jumping once knocked out");
    assert_eq!(state_of(&m, 1), FighterState::Knockout);
}

#[test]
fn a_knock_out_ends_a_duel_in_favour_of_the_survivor() {
    let config = MatchConfig {
        max_health: 20,
        ..cfg()
    };
    let mut m = close_duel(config);
    run_until_the_first_hit(&mut m);
    assert_eq!(m.phase(), Phase::Finished(Outcome::Winner(TeamId(0))));
}

#[test]
fn a_target_knocked_out_by_one_attacker_ignores_the_other_hits_of_the_step() {
    let config = MatchConfig {
        max_health: 10,
        ..cfg()
    };
    let mut m = brawl_at(config, &[560, 640, 720]);
    run(&mut m, 4, &inputs_of(&[(0, jab()), (2, jab())]));
    assert!(fighter(&m, 1).is_knocked_out());
    assert_eq!(health_of(&m, 1), 0);
    assert_eq!(x_of(&m, 1), 670, "pushed once, by the first attacker only");
}

// ---------------------------------------------------------------------------------------------
// Alliances
// ---------------------------------------------------------------------------------------------

fn two_versus_one(config: MatchConfig, spawns: &[i32]) -> Match {
    Match::with_spawns(Roster::two_versus_one(), config, spawns).unwrap()
}

#[test]
fn allies_cannot_hurt_each_other_without_friendly_fire() {
    // Player 0 (560) attacks; its ally player 1 (640) is in range.
    let mut m = two_versus_one(cfg(), &[560, 640, 1100]);
    run_until_the_first_hit(&mut m);
    assert_eq!(health_of(&m, 1), 100);
    assert_ne!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(x_of(&m, 1), 640);
}

#[test]
fn allies_hurt_each_other_with_friendly_fire() {
    let config = MatchConfig {
        friendly_fire: true,
        ..cfg()
    };
    let mut m = two_versus_one(config, &[560, 640, 1100]);
    run_until_the_first_hit(&mut m);
    assert_eq!(health_of(&m, 1), 80);
}

#[test]
fn an_ally_in_range_does_not_shield_an_enemy_in_range() {
    // Player 1 (ally) stands at 600, player 2 (enemy) at 640; both are in front of player 0.
    let mut m = two_versus_one(cfg(), &[560, 600, 640]);
    run_until_the_first_hit(&mut m);
    assert_eq!(health_of(&m, 1), 100);
    assert_eq!(health_of(&m, 2), 80);
}

#[test]
fn in_a_free_for_all_one_attack_hits_every_enemy_in_range() {
    let mut m = brawl_at(cfg(), &[560, 640, 650]);
    run_until_the_first_hit(&mut m);
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(health_of(&m, 2), 80);
}

#[test]
fn enemies_out_of_range_are_not_hit_by_a_multi_target_attack() {
    let mut m = brawl_at(cfg(), &[560, 640, 700]);
    run_until_the_first_hit(&mut m);
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(health_of(&m, 2), 100, "140 units away, range is 100");
}

#[test]
fn input_type_is_plain_data() {
    // Guards the `Input` contract used all over this file: copyable and comparable.
    let a = jab();
    let b = a;
    assert_eq!(a, b);
    assert_ne!(a, Input::NONE);
}
