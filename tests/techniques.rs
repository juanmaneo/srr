//! Judo techniques: two keys pressed together, a projection, then a hold on the ground.
//!
//! README: `5` + `9` is the flying juji-gatame foot sweep into hon-gesa-gatame, `8` + `9` the
//! ippon-seoi-nage into ude-hishigi-juji-gatame; only the exact same combination counters.
//!
//! Numbers from `common::cfg` ([`SWEEP`], [`THROW`]): the sweep winds up 2 steps and is active
//! on steps 3 and 4 (range 60), its projection lasts 4 steps (counter in the first 2) and lands
//! for 10; the throw winds up 3 steps, is active on step 4 (range 50), its projection lasts 5
//! steps (counter in the first 3) and lands for 15. The two presses may be 2 steps apart.

mod common;

use common::*;
use srr::game::{
    Attack, AttackPhase, Facing, FighterState, GlitchSet, Grapple, GrapplePosition, GrappleRole,
    Input, Match, MatchConfig, PlayerId, Roster, Strength, Technique,
};

const MEDIUM_PUNCH: Attack = Attack::punch(Strength::Medium);
const MEDIUM_KICK: Attack = Attack::kick(Strength::Medium);
const HIGH_KICK: Attack = Attack::kick(Strength::High);

/// `5` + `9`.
fn sweep_keys() -> Input {
    press(MEDIUM_PUNCH).with(HIGH_KICK)
}

/// `8` + `9`.
fn throw_keys() -> Input {
    press(MEDIUM_KICK).with(HIGH_KICK)
}

fn grapple_of(m: &Match, id: u8) -> Grapple {
    fighter(m, id)
        .grapple()
        .unwrap_or_else(|| panic!("player {id} is not grappling"))
}

fn position_of(m: &Match, id: u8) -> GrapplePosition {
    grapple_of(m, id).position
}

/// Player 0 at 560 facing right, player 1 at 600, 40 in front: in reach of both techniques.
fn duel() -> Match {
    duel_at(cfg(), 560, 600)
}

/// Player 0 sweeps player 1: caught on step 3, in the projection.
fn swept() -> Match {
    let mut m = duel();
    m.step(&only(0, sweep_keys()));
    run(&mut m, 2, &no_inputs());
    m
}

/// Player 0 throws player 1: caught on step 4, in the projection.
fn thrown() -> Match {
    let mut m = duel();
    m.step(&only(0, throw_keys()));
    run(&mut m, 3, &no_inputs());
    m
}

// ---------------------------------------------------------------------------------------------
// The techniques
// ---------------------------------------------------------------------------------------------

#[test]
fn each_technique_has_two_buttons_a_projection_and_a_finish() {
    assert_eq!(Technique::ALL, [Technique::Sweep, Technique::Throw]);
    assert_eq!(Technique::Sweep.index(), 0);
    assert_eq!(Technique::Throw.index(), 1);
    assert_eq!(Technique::Sweep.buttons(), [MEDIUM_PUNCH, HIGH_KICK]);
    assert_eq!(Technique::Throw.buttons(), [MEDIUM_KICK, HIGH_KICK]);
    for technique in Technique::ALL {
        let (projection, finish) = (technique.projection(), technique.finish());
        assert!(projection.is_projection());
        assert!(!finish.is_projection());
        assert_eq!(projection.next(), Some(finish));
        assert_eq!(finish.next(), None);
        assert_eq!(projection.technique(), Some(technique));
        assert_eq!(finish.technique(), Some(technique));
    }
    assert_eq!(Technique::Sweep.finish(), GrapplePosition::ScarfHold);
    assert_eq!(Technique::Throw.finish(), GrapplePosition::Armbar);
    assert_eq!(Technique::Sweep.to_string(), "juji-gatame sweep");
    assert_eq!(Technique::Throw.to_string(), "ippon-seoi-nage");
}

#[test]
fn the_sweep_projects_then_pins_in_hon_gesa_gatame() {
    let mut m = duel();
    m.step(&only(0, sweep_keys()));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Technique(Technique::Sweep, AttackPhase::Startup)
    );
    assert_eq!(attack_of(&m, 0), None, "a technique is not a strike");
    m.step(&no_inputs());
    m.step(&no_inputs()); // step 3: active, catches player 1
    assert_eq!(
        state_of(&m, 0),
        FighterState::Grappling(GrappleRole::Top, GrapplePosition::Sweep)
    );
    assert_eq!(
        state_of(&m, 1),
        FighterState::Grappling(GrappleRole::Bottom, GrapplePosition::Sweep)
    );
    let grapple = grapple_of(&m, 1);
    assert_eq!(grapple.partner, PlayerId(0));
    assert_eq!(
        (grapple.escape, grapple.escape_needed, grapple.held_ticks),
        (0, 0, 0)
    );
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (560, 600));
    assert_eq!(
        (facing_of(&m, 0), facing_of(&m, 1)),
        (Facing::Right, Facing::Left)
    );

    run(&mut m, 3, &no_inputs());
    assert_eq!(
        position_of(&m, 1),
        GrapplePosition::Sweep,
        "still in the air"
    );
    assert_eq!(health_of(&m, 1), 100);
    m.step(&no_inputs()); // the 4th projection tick lands
    assert_eq!(position_of(&m, 1), GrapplePosition::ScarfHold);
    assert_eq!(health_of(&m, 1), 90, "the landing hurts");
    let grapple = grapple_of(&m, 1);
    assert_eq!((grapple.escape_needed, grapple.held_ticks), (3, 0));

    // The pin drains like the submission: 4 every 2 ticks.
    m.step(&no_inputs());
    assert_eq!(health_of(&m, 1), 90);
    m.step(&no_inputs());
    assert_eq!(health_of(&m, 1), 86);
}

#[test]
fn the_throw_projects_then_locks_ude_hishigi_juji_gatame() {
    let mut m = duel();
    m.step(&only(0, throw_keys()));
    run(&mut m, 2, &no_inputs());
    assert_eq!(
        state_of(&m, 0),
        FighterState::Technique(Technique::Throw, AttackPhase::Startup)
    );
    m.step(&no_inputs()); // step 4: active
    assert_eq!(
        state_of(&m, 1),
        FighterState::Grappling(GrappleRole::Bottom, GrapplePosition::Throw)
    );
    run(&mut m, 4, &no_inputs());
    assert_eq!(position_of(&m, 0), GrapplePosition::Throw);
    m.step(&no_inputs());
    assert_eq!(position_of(&m, 0), GrapplePosition::Armbar);
    assert_eq!(health_of(&m, 1), 85);
    assert_eq!(grapple_of(&m, 1).escape_needed, 4);
}

#[test]
fn a_technique_that_catches_nobody_recovers_and_cannot_be_cancelled() {
    let config = MatchConfig {
        glitches: GlitchSet::all(),
        ..cfg()
    };
    let mut m = duel_at(config, 560, 700);
    m.step(&only(0, sweep_keys()));
    run(&mut m, 3, &no_inputs());
    assert_eq!(
        state_of(&m, 0),
        FighterState::Technique(Technique::Sweep, AttackPhase::Active)
    );
    m.step(&only(0, press_jump()));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Technique(Technique::Sweep, AttackPhase::Recovery),
        "no animation cancel"
    );
    run(&mut m, 4, &no_inputs()); // step 9: the last recovery step
    assert!(matches!(state_of(&m, 0), FighterState::Technique(..)));
    m.step(&only(0, press_right()));
    assert_eq!(state_of(&m, 0), FighterState::Walking);
    assert_eq!(x_of(&m, 0), 570);
}

// ---------------------------------------------------------------------------------------------
// Simultaneous presses
// ---------------------------------------------------------------------------------------------

#[test]
fn the_second_press_converts_the_attack_the_first_one_started() {
    let mut m = duel();
    m.step(&only(0, press(MEDIUM_PUNCH)));
    assert_eq!(attack_of(&m, 0), Some(MEDIUM_PUNCH));
    m.step(&only(0, press(MEDIUM_PUNCH)));
    m.step(&only(0, sweep_keys())); // 2 steps later: still simultaneous
    assert_eq!(
        state_of(&m, 0),
        FighterState::Technique(Technique::Sweep, AttackPhase::Startup)
    );
    assert_eq!(attack_of(&m, 0), None);

    // The other order works too: the high kick first, then the medium kick.
    let mut m = duel();
    m.step(&only(0, press(HIGH_KICK)));
    m.step(&only(0, throw_keys()));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Technique(Technique::Throw, AttackPhase::Startup)
    );
}

#[test]
fn presses_further_apart_than_the_window_stay_separate_attacks() {
    let mut m = duel();
    m.step(&only(0, press(MEDIUM_PUNCH)));
    run(&mut m, 2, &only(0, press(MEDIUM_PUNCH)));
    m.step(&only(0, sweep_keys())); // 3 steps later
    assert_eq!(attack_of(&m, 0), Some(MEDIUM_PUNCH));
    assert_eq!(health_of(&m, 1), 80, "the punch landed on step 4");
}

#[test]
fn only_an_attack_still_winding_up_converts() {
    let config = MatchConfig {
        combo_window: 10,
        ..cfg()
    };
    let mut m = duel_at(config, 560, 700);
    m.step(&only(0, press(MEDIUM_PUNCH)));
    run(&mut m, 3, &only(0, press(MEDIUM_PUNCH)));
    m.step(&only(0, sweep_keys())); // step 5: the punch is active
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Active)
    );
    assert_eq!(attack_of(&m, 0), Some(MEDIUM_PUNCH));
}

#[test]
fn an_attack_converts_only_into_a_technique_that_uses_its_button() {
    // A medium kick, then 5 + 9 with the 8 released: the sweep does not use the medium kick.
    let mut m = duel_at(cfg(), 560, 700);
    m.step(&only(0, press(MEDIUM_KICK)));
    m.step(&only(0, sweep_keys()));
    assert_eq!(attack_of(&m, 0), Some(MEDIUM_KICK));

    // With the 8 still held, 9 completes both combinations: the medium kick turns into the
    // throw, the one that uses it, although the sweep comes first otherwise.
    let mut m = duel_at(cfg(), 560, 700);
    m.step(&only(0, press(MEDIUM_KICK)));
    m.step(&only(
        0,
        press(MEDIUM_KICK).with(MEDIUM_PUNCH).with(HIGH_KICK),
    ));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Technique(Technique::Throw, AttackPhase::Startup)
    );
}

#[test]
fn other_pairs_of_buttons_are_no_technique() {
    let mut m = duel_at(cfg(), 560, 700);
    m.step(&only(0, press(MEDIUM_PUNCH).with(MEDIUM_KICK)));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Startup)
    );
    assert_eq!(attack_of(&m, 0), Some(MEDIUM_KICK));
}

#[test]
fn holding_both_buttons_triggers_once() {
    let mut m = duel_at(cfg(), 560, 700);
    run(&mut m, 9, &only(0, sweep_keys())); // the whiff is over after step 9
    m.step(&only(0, sweep_keys()));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Startup),
        "held buttons repeat the high kick, not the technique"
    );
}

#[test]
fn techniques_need_the_floor() {
    let mut m = duel_at(cfg(), 560, 700);
    m.step(&only(0, press_jump()));
    m.step(&only(0, sweep_keys()));
    assert_eq!(state_of(&m, 0), FighterState::Airborne);
}

// ---------------------------------------------------------------------------------------------
// Who can be caught
// ---------------------------------------------------------------------------------------------

#[test]
fn techniques_cannot_be_blocked_by_stepping_back() {
    // Player 1 at 605 faces left: pressing right steps back, to 635 on step 3, still within a
    // longer sweep's range.
    let mut config = cfg();
    config.techniques[Technique::Sweep.index()].range = 100;
    let mut m = duel_at(config, 560, 605);
    m.step(&inputs_of(&[(0, sweep_keys()), (1, press_right())]));
    m.step(&only(1, press_right()));
    assert_eq!(state_of(&m, 1), FighterState::Blocking);
    m.step(&only(1, press_right()));
    assert_eq!(position_of(&m, 1), GrapplePosition::Sweep);
}

#[test]
fn a_strike_interrupts_a_technique_winding_up() {
    // Player 1's jab (pressed on step 1) hits on step 4; player 0's throw (pressed on step 2)
    // would only be active on step 5.
    let mut m = duel();
    m.step(&only(1, turning(jab(), Facing::Left)));
    m.step(&only(0, throw_keys()));
    run(&mut m, 2, &no_inputs());
    assert_eq!(state_of(&m, 0), FighterState::HitStun);
    run(&mut m, 2, &no_inputs());
    assert!(!matches!(state_of(&m, 1), FighterState::Grappling(..)));
}

#[test]
fn airborne_stunned_and_out_of_reach_fighters_are_not_caught() {
    let mut m = duel();
    m.step(&inputs_of(&[(0, sweep_keys()), (1, press_jump())]));
    run(&mut m, 3, &no_inputs());
    assert_eq!(state_of(&m, 1), FighterState::Airborne);
    assert!(fighter(&m, 0).grapple().is_none());

    let mut m = duel_at(cfg(), 560, 621);
    m.step(&only(0, sweep_keys()));
    run(&mut m, 3, &no_inputs());
    assert!(fighter(&m, 0).grapple().is_none(), "61 away, range 60");

    // Behind the fighter does not count either.
    let mut m = duel_at(cfg(), 560, 540);
    m.step(&only(0, sweep_keys()));
    run(&mut m, 3, &no_inputs());
    assert!(fighter(&m, 0).grapple().is_none());

    let config = cfg_with(|d| d.hitstun = 20);
    let mut m = duel_at(config, 560, 600);
    m.step(&only(0, jab()));
    run(&mut m, 9, &no_inputs()); // player 1 stunned from step 4 to step 24
    m.step(&only(0, sweep_keys()));
    run(&mut m, 3, &no_inputs());
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert!(fighter(&m, 0).grapple().is_none());
}

#[test]
fn allies_are_not_caught_without_friendly_fire() {
    let mut m = Match::with_spawns(Roster::two_versus_one(), cfg(), &[560, 600, 1000]).unwrap();
    m.step(&only(0, sweep_keys()));
    run(&mut m, 3, &no_inputs());
    assert!(fighter(&m, 0).grapple().is_none());
}

#[test]
fn a_technique_catches_a_grabbing_fighter_before_its_grab_connects() {
    // Both active on step 3: techniques connect before grabs.
    let mut m = duel_at(cfg(), 560, 600);
    m.step(&inputs_of(&[(0, sweep_keys()), (1, turn_and_grab())]));
    run(&mut m, 2, &no_inputs());
    assert_eq!(grapple_of(&m, 0).role, GrappleRole::Top);
    assert_eq!(position_of(&m, 0), GrapplePosition::Sweep);
}

fn turn_and_grab() -> Input {
    turning(press_grab(), Facing::Left)
}

// ---------------------------------------------------------------------------------------------
// Counters
// ---------------------------------------------------------------------------------------------

#[test]
fn the_same_combination_in_the_air_counters_the_sweep() {
    let mut m = swept();
    m.step(&only(1, sweep_keys())); // first projection tick
    assert_eq!(grapple_of(&m, 1).role, GrappleRole::Top);
    assert_eq!(grapple_of(&m, 0).role, GrappleRole::Bottom);
    assert_eq!(position_of(&m, 1), GrapplePosition::Sweep);
    assert_eq!(grapple_of(&m, 1).held_ticks, 0, "a fresh projection");

    // A counter cannot be countered.
    m.step(&inputs_of(&[(0, sweep_keys()), (1, sweep_keys())]));
    assert_eq!(grapple_of(&m, 1).role, GrappleRole::Top);
    run(&mut m, 3, &no_inputs());
    assert_eq!(position_of(&m, 1), GrapplePosition::ScarfHold);
    assert_eq!((health_of(&m, 0), health_of(&m, 1)), (90, 100));
}

#[test]
fn the_same_combination_counters_the_throw_until_the_window_closes() {
    let mut m = thrown();
    run(&mut m, 2, &no_inputs());
    m.step(&only(1, throw_keys())); // the 3rd projection tick: the last one of the window
    assert_eq!(grapple_of(&m, 1).role, GrappleRole::Top);

    let mut m = thrown();
    run(&mut m, 3, &no_inputs());
    m.step(&only(1, throw_keys())); // the 4th: too late
    assert_eq!(grapple_of(&m, 1).role, GrappleRole::Bottom);
    m.step(&no_inputs());
    assert_eq!(position_of(&m, 1), GrapplePosition::Armbar);
}

#[test]
fn another_combination_grab_or_mashing_does_not_counter_or_escape() {
    let config = MatchConfig {
        glitches: GlitchSet::all(),
        ..cfg()
    };
    let mut m = duel_at(config, 560, 600);
    m.step(&only(0, sweep_keys()));
    run(&mut m, 2, &no_inputs());
    m.step(&only(1, press_grab())); // no reversal glitch in a projection
    m.step(&only(1, throw_keys()));
    m.step(&only(1, press_left()));
    let grapple = grapple_of(&m, 1);
    assert_eq!(grapple.role, GrappleRole::Bottom);
    assert_eq!(
        (grapple.escape, grapple.position),
        (0, GrapplePosition::Sweep)
    );
}

#[test]
fn a_technique_catching_the_same_technique_winding_up_is_countered() {
    // Player 0's sweep is active on step 3; player 1's, pressed on step 2, is still winding up.
    let mut m = duel();
    m.step(&only(0, sweep_keys()));
    m.step(&only(1, turning(sweep_keys(), Facing::Left)));
    m.step(&no_inputs());
    assert_eq!(grapple_of(&m, 1).role, GrappleRole::Top);
    assert_eq!(position_of(&m, 1), GrapplePosition::Sweep);
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (560, 600));
    assert_eq!(
        (facing_of(&m, 0), facing_of(&m, 1)),
        (Facing::Right, Facing::Left)
    );
    m.step(&only(0, sweep_keys()));
    assert_eq!(grapple_of(&m, 1).role, GrappleRole::Top, "for good");
}

#[test]
fn a_different_technique_winding_up_is_simply_caught() {
    let mut m = duel();
    m.step(&inputs_of(&[
        (0, sweep_keys()),
        (1, turning(throw_keys(), Facing::Left)),
    ]));
    run(&mut m, 2, &no_inputs());
    assert_eq!(grapple_of(&m, 0).role, GrappleRole::Top);
}

#[test]
fn the_same_technique_active_on_both_sides_clashes() {
    // 50 apart, facing each other, both sweeps active on step 3.
    let mut m = duel_at(cfg(), 570, 620);
    m.step(&inputs_of(&[(0, sweep_keys()), (1, sweep_keys())]));
    run(&mut m, 2, &no_inputs());
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (545, 645));
    assert_eq!(state_of(&m, 0), FighterState::Idle);
    assert_eq!(state_of(&m, 1), FighterState::Idle);
    assert!(fighter(&m, 0).grapple().is_none());
}

// ---------------------------------------------------------------------------------------------
// The finish on the ground
// ---------------------------------------------------------------------------------------------

#[test]
fn the_finish_is_escaped_by_mashing() {
    let mut m = swept();
    run(&mut m, 4, &no_inputs()); // landed in hon-gesa-gatame
    for _ in 0..2 {
        m.step(&only(1, press_left()));
        m.step(&no_inputs());
    }
    assert_eq!(grapple_of(&m, 1).escape, 2);
    m.step(&only(1, press_left()));
    assert!(fighter(&m, 1).grapple().is_none());
    assert_eq!(state_of(&m, 0), FighterState::HitStun);
    assert_eq!(x_of(&m, 1) - x_of(&m, 0), 100);
}

#[test]
fn the_top_cannot_advance_or_strike_from_the_finish() {
    let mut m = thrown();
    run(&mut m, 5, &no_inputs()); // in the armbar
    let health = health_of(&m, 1);
    m.step(&only(0, press_grab()));
    m.step(&only(0, jab()));
    assert_eq!(position_of(&m, 0), GrapplePosition::Armbar);
    assert_eq!(health_of(&m, 1), health - 4, "only the drain");
}

#[test]
fn a_finish_can_end_in_a_tap_out() {
    let config = MatchConfig {
        max_health: 20,
        ..cfg()
    };
    let mut m = duel_at(config, 560, 600);
    m.step(&only(0, throw_keys()));
    run(&mut m, 8, &no_inputs()); // landed for 15
    assert_eq!(health_of(&m, 1), 5);
    run(&mut m, 4, &no_inputs());
    assert!(fighter(&m, 1).is_knocked_out());
    assert_eq!(state_of(&m, 0), FighterState::Idle);
}

#[test]
fn the_referee_stands_the_finish_up() {
    let mut config = cfg();
    config.grapple.max_hold_ticks = 3;
    config.grapple.submission_damage = 0;
    let mut m = duel_at(config, 560, 600);
    m.step(&only(0, sweep_keys()));
    run(&mut m, 6, &no_inputs()); // landed
    run(&mut m, 3, &no_inputs());
    assert!(fighter(&m, 1).grapple().is_none());
    assert_eq!(state_of(&m, 0), FighterState::Idle);
}

#[test]
fn a_strike_from_a_third_fighter_breaks_the_projection() {
    let config = MatchConfig {
        friendly_fire: true,
        ..cfg()
    };
    let mut m = brawl_at(config, &[560, 600, 680]);
    m.step(&inputs_of(&[(0, sweep_keys()), (2, jab())]));
    run(&mut m, 2, &no_inputs()); // step 3: player 0 sweeps player 1
    assert_eq!(position_of(&m, 0), GrapplePosition::Sweep);
    m.step(&no_inputs()); // step 4: player 2's jab hits player 1 (at 600, 80 away)
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert!(fighter(&m, 0).grapple().is_none());
}
