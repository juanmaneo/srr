//! Combos: a strike that connected chains into a stronger one, and the hits taken inside the
//! stun of the previous one are counted.
//!
//! The chain: once a strike hit a fighter that did not block it, holding a stronger attack
//! button (low → medium → high, punch or kick; a kick in the air) cancels the rest of the strike
//! into that attack at once. A whiffed or blocked strike runs its full course, and nothing
//! chains after a high strike. The chain comes right after the technique conversion in the
//! priorities, before the animation cancel glitch.
//!
//! The count (`Fighter::combo_hits`): an unblocked hit on a fighter still stunned by the
//! previous one adds to its count, whoever threw it; the count is 1 after a lone hit, back to 0
//! once the stun ends (a hit landing on the last stunned step starts a new combo), and a
//! knocked-out fighter keeps the count of the combo that finished it.
//!
//! Numbers of `common::cfg` (3 start-up, 2 active, 4 recovery steps, damage 20, 5 steps of hit
//! stun), with the knockback switched off so that the target stays in reach: a strike pressed
//! on step `s` lands on step `s + 3`, and its target is stunned on steps `s + 4` to `s + 8`.

mod common;

use common::*;
use srr::game::{
    Attack, AttackPhase, FighterState, Glitch, GlitchSet, Input, MatchConfig, Strength,
};

const MEDIUM_PUNCH: Attack = Attack::punch(Strength::Medium);
const HIGH_PUNCH: Attack = Attack::punch(Strength::High);
const LOW_KICK: Attack = Attack::kick(Strength::Low);
const MEDIUM_KICK: Attack = Attack::kick(Strength::Medium);
const HIGH_KICK: Attack = Attack::kick(Strength::High);
const STARTUP: FighterState = FighterState::Attacking(AttackPhase::Startup);
const ACTIVE: FighterState = FighterState::Attacking(AttackPhase::Active);

/// [`cfg`] without knockback: a chained strike reaches the target where the first one left it.
fn chain_cfg() -> MatchConfig {
    cfg_with(|d| d.knockback = 0)
}

fn combo_hits(m: &srr::game::Match, id: u8) -> u32 {
    fighter(m, id).combo_hits()
}

/// Player 0 jabs player 1 (80 units away): the jab lands on step 4.
fn landed_jab() -> srr::game::Match {
    let mut m = close_duel(chain_cfg());
    m.step(&only(0, jab()));
    run(&mut m, 3, &no_inputs());
    assert_eq!(health_of(&m, 1), 80);
    m
}

// ---------------------------------------------------------------------------------------------
// The chain
// ---------------------------------------------------------------------------------------------

#[test]
fn a_strike_that_connected_is_cancelled_into_a_stronger_one_up_to_the_high_tier() {
    let mut m = landed_jab();
    assert!(fighter(&m, 0).attack_connected());
    m.step(&only(0, press(MEDIUM_PUNCH))); // step 5: the chain
    assert_eq!(
        (attack_of(&m, 0), state_of(&m, 0)),
        (Some(MEDIUM_PUNCH), STARTUP)
    );
    assert!(!fighter(&m, 0).attack_connected(), "a fresh strike");
    run(&mut m, 3, &no_inputs()); // step 8: the medium punch lands
    assert_eq!(health_of(&m, 1), 60);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    m.step(&only(0, press(HIGH_KICK))); // step 9: a kick chains after a punch too
    assert_eq!(
        (attack_of(&m, 0), state_of(&m, 0)),
        (Some(HIGH_KICK), STARTUP)
    );
    run(&mut m, 3, &no_inputs()); // step 12
    assert_eq!(health_of(&m, 1), 40);
    assert_eq!(combo_hits(&m, 1), 3);
    m.step(&only(0, press(HIGH_PUNCH))); // step 13: nothing is stronger than high
    assert_eq!(
        (attack_of(&m, 0), state_of(&m, 0)),
        (Some(HIGH_KICK), ACTIVE)
    );
}

#[test]
fn the_chain_needs_a_strictly_stronger_strike() {
    for same_tier in [JAB, LOW_KICK] {
        let mut m = landed_jab();
        m.step(&only(0, press(same_tier))); // step 5: the second active step of the jab
        assert_eq!((attack_of(&m, 0), state_of(&m, 0)), (Some(JAB), ACTIVE));
    }
}

#[test]
fn the_chain_may_skip_a_tier() {
    let mut m = landed_jab();
    m.step(&only(0, press(HIGH_PUNCH)));
    assert_eq!(
        (attack_of(&m, 0), state_of(&m, 0)),
        (Some(HIGH_PUNCH), STARTUP)
    );
}

#[test]
fn a_stronger_button_held_during_the_strike_chains_right_after_the_hit() {
    let mut m = close_duel(chain_cfg());
    m.step(&only(0, jab()));
    run(&mut m, 3, &only(0, press(MEDIUM_PUNCH))); // steps 2..=4: the jab goes on and lands
    assert_eq!((attack_of(&m, 0), health_of(&m, 1)), (Some(JAB), 80));
    m.step(&only(0, press(MEDIUM_PUNCH))); // step 5
    assert_eq!(
        (attack_of(&m, 0), state_of(&m, 0)),
        (Some(MEDIUM_PUNCH), STARTUP)
    );
}

#[test]
fn a_whiffed_strike_cannot_be_cancelled() {
    let mut m = duel_at(chain_cfg(), 100, 1100);
    m.step(&only(0, jab()));
    run(&mut m, 3, &no_inputs());
    assert!(!fighter(&m, 0).attack_connected());
    m.step(&only(0, press(MEDIUM_PUNCH))); // step 5
    assert_eq!((attack_of(&m, 0), state_of(&m, 0)), (Some(JAB), ACTIVE));
}

#[test]
fn a_blocked_strike_cannot_be_cancelled_and_does_not_count() {
    let mut m = close_duel(chain_cfg());
    run(&mut m, 3, &only(0, jab()));
    m.step(&inputs_of(&[(0, jab()), (1, press_right())])); // step 4: blocked
    assert_eq!(health_of(&m, 1), 95);
    assert!(!fighter(&m, 0).attack_connected());
    assert_eq!(combo_hits(&m, 1), 0);
    m.step(&only(0, press(MEDIUM_PUNCH))); // step 5
    assert_eq!((attack_of(&m, 0), state_of(&m, 0)), (Some(JAB), ACTIVE));
}

#[test]
fn the_chain_comes_before_the_animation_cancel_glitch() {
    let config = MatchConfig {
        glitches: GlitchSet::none().with(Glitch::AnimationCancel),
        ..chain_cfg()
    };
    let mut m = close_duel(config);
    m.step(&only(0, jab()));
    run(&mut m, 4, &no_inputs()); // step 5: the last active step
    let jump_and_punch = Input {
        jump: true,
        ..press(MEDIUM_PUNCH)
    };
    m.step(&only(0, jump_and_punch)); // step 6: recovery, both are possible
    assert_eq!(
        (attack_of(&m, 0), state_of(&m, 0)),
        (Some(MEDIUM_PUNCH), STARTUP)
    );
    assert_eq!(y_of(&m, 0), 0);
}

#[test]
fn in_the_air_only_a_stronger_kick_chains() {
    let config = cfg_with(|d| {
        d.startup = 0;
        d.knockback = 0;
    });
    let mut m = close_duel(config);
    m.step(&inputs_of(&[(0, press_jump()), (1, press_jump())]));
    m.step(&only(0, press(LOW_KICK))); // step 2: active at once, both at height 35
    assert_eq!(health_of(&m, 1), 80);
    assert!(fighter(&m, 0).attack_connected());
    m.step(&only(0, press(HIGH_PUNCH))); // step 3: punches do not work in the air
    assert_eq!(
        (attack_of(&m, 0), state_of(&m, 0)),
        (Some(LOW_KICK), ACTIVE)
    );
    m.step(&only(0, press(MEDIUM_KICK))); // step 4: the chain, both at height 50
    assert_eq!(attack_of(&m, 0), Some(MEDIUM_KICK));
    assert_eq!(
        health_of(&m, 1),
        60,
        "active at once, the second kick lands too"
    );
    assert_eq!(combo_hits(&m, 1), 2);
    assert!(fighter(&m, 0).is_airborne() && fighter(&m, 1).is_airborne());
}

// ---------------------------------------------------------------------------------------------
// The count
// ---------------------------------------------------------------------------------------------

#[test]
fn the_hits_taken_inside_the_stun_are_counted_until_the_stun_ends() {
    let mut m = close_duel(chain_cfg());
    assert_eq!(combo_hits(&m, 1), 0);
    m.step(&only(0, jab()));
    run(&mut m, 3, &no_inputs()); // step 4
    assert_eq!(combo_hits(&m, 1), 1, "a lone hit");
    m.step(&only(0, press(MEDIUM_PUNCH))); // step 5
    run(&mut m, 3, &no_inputs()); // step 8: the second hit, stunned on steps 9..=13
    assert_eq!(combo_hits(&m, 1), 2);
    run(&mut m, 4, &no_inputs()); // steps 9..=12
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(combo_hits(&m, 1), 2, "the count holds through the stun");
    m.step(&no_inputs()); // step 13: the last stunned step frees the fighter
    assert_eq!(state_of(&m, 1), FighterState::Idle);
    assert_eq!(combo_hits(&m, 1), 0);
    m.step(&only(0, jab())); // step 14: player 0 is free again too
    run(&mut m, 3, &no_inputs()); // step 17
    assert_eq!(
        (health_of(&m, 1), combo_hits(&m, 1)),
        (40, 1),
        "a new combo"
    );
}

/// Player 0 (560) jabs player 1 (640) on step 1; player 2 (740, facing left) jabs it later so
/// that its jab lands on step `lands_on`. The count after step 12.
fn hits_taken_from_both_sides(lands_on: usize) -> u32 {
    let mut m = brawl_at(chain_cfg(), &[560, 640, 740]);
    m.step(&only(0, jab()));
    for step in 2..=12 {
        let inputs = if step == lands_on - 3 {
            only(2, jab())
        } else {
            no_inputs()
        };
        m.step(&inputs);
    }
    assert_eq!(health_of(&m, 1), 60, "both jabs landed");
    combo_hits(&m, 1)
}

#[test]
fn a_hit_from_another_fighter_continues_the_combo() {
    assert_eq!(hits_taken_from_both_sides(8), 2);
}

#[test]
fn a_hit_landing_on_the_last_stunned_step_starts_a_new_combo() {
    assert_eq!(hits_taken_from_both_sides(9), 1);
}

#[test]
fn without_hit_stun_there_is_no_combo() {
    let config = cfg_with(|d| {
        d.hitstun = 0;
        d.knockback = 0;
    });
    let mut m = close_duel(config);
    m.step(&only(0, jab()));
    run(&mut m, 3, &no_inputs());
    assert_eq!((health_of(&m, 1), combo_hits(&m, 1)), (80, 0));
}

#[test]
fn a_knocked_out_fighter_keeps_the_count_of_the_combo_that_finished_it() {
    let config = MatchConfig {
        max_health: 40,
        ..chain_cfg()
    };
    let mut m = close_duel(config);
    m.step(&only(0, jab()));
    run(&mut m, 3, &no_inputs()); // step 4: 20 left
    m.step(&only(0, press(MEDIUM_PUNCH)));
    run(&mut m, 3, &no_inputs()); // step 8: the knockout
    assert!(fighter(&m, 1).is_knocked_out());
    assert_eq!(combo_hits(&m, 1), 2);
    run(&mut m, 30, &no_inputs());
    assert_eq!(combo_hits(&m, 1), 2);
}

// ---------------------------------------------------------------------------------------------
// From the air to the floor
// ---------------------------------------------------------------------------------------------

#[test]
fn a_jump_kick_opens_a_combo_that_goes_on_from_the_landing() {
    let config = cfg_with(|d| {
        d.knockback = 0;
        d.hitstun = 8;
    });
    let mut m = close_duel(config);
    m.step(&only(0, press_jump()));
    run(&mut m, 2, &no_inputs());
    m.step(&only(0, press(LOW_KICK))); // step 4: pressed at the apex
    run(&mut m, 3, &no_inputs()); // step 7: comes down on player 1
    assert_eq!((health_of(&m, 1), combo_hits(&m, 1)), (80, 1));
    run(&mut m, 2, &no_inputs()); // step 9: the landing ends the kick
    assert_eq!(state_of(&m, 0), FighterState::Idle);
    m.step(&only(0, jab())); // step 10: free at once
    run(&mut m, 3, &no_inputs()); // step 13: inside the stun of the kick
    assert_eq!((health_of(&m, 1), combo_hits(&m, 1)), (60, 2));
    m.step(&only(0, press(MEDIUM_PUNCH))); // step 14: the chain
    run(&mut m, 3, &no_inputs()); // step 17
    assert_eq!((health_of(&m, 1), combo_hits(&m, 1)), (40, 3));
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
}
