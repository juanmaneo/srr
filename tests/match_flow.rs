//! Match flow: countdown, round timer, pausing, win / draw rules.
//!
//! * `Countdown { remaining: n }` lasts exactly `n` steps (inputs are ignored, the round timer
//!   does not run); the match is `Fighting` after the n-th step.
//! * `Fighting` lasts until one team is left standing (that team wins), everybody is knocked out
//!   at once (draw), or `round_time_ticks` fighting steps have elapsed (the team with the highest
//!   total health of its fighters wins; a tie for the lead is a draw).
//! * `Finished` is final: stepping a finished match changes nothing.
//! * A paused match does not change at all when stepped.

mod common;

use common::*;
use srr::game::{Match, MatchConfig, Outcome, Phase, Roster, TeamId};

/// Every hit is a knock-out.
fn lethal() -> MatchConfig {
    MatchConfig {
        max_health: 20,
        ..cfg()
    }
}

fn match_with(roster: Roster, config: MatchConfig, spawns: &[i32]) -> Match {
    Match::with_spawns(roster, config, spawns).expect("valid setup")
}

// ---------------------------------------------------------------------------------------------
// Countdown
// ---------------------------------------------------------------------------------------------

fn countdown_match() -> Match {
    let config = MatchConfig {
        countdown_ticks: 3,
        ..cfg()
    };
    duel_at(config, 560, 640)
}

#[test]
fn the_countdown_lasts_exactly_the_configured_number_of_steps() {
    let mut m = countdown_match();
    assert_eq!(m.phase(), Phase::Countdown { remaining: 3 });
    m.step(&no_inputs());
    assert_eq!(m.phase(), Phase::Countdown { remaining: 2 });
    m.step(&no_inputs());
    assert_eq!(m.phase(), Phase::Countdown { remaining: 1 });
    m.step(&no_inputs());
    assert_eq!(m.phase(), Phase::Fighting);
}

#[test]
fn inputs_are_ignored_during_the_countdown() {
    let mut m = countdown_match();
    // Three countdown steps; the third one is still a countdown step.
    run(&mut m, 3, &only(0, press_right()));
    assert_eq!(x_of(&m, 0), 560);
    assert_eq!(m.phase(), Phase::Fighting);
    m.step(&only(0, press_right()));
    assert_eq!(x_of(&m, 0), 570);
}

#[test]
fn the_round_timer_starts_after_the_countdown() {
    let mut m = countdown_match();
    run(&mut m, 3, &no_inputs());
    assert_eq!(m.remaining_ticks(), cfg().round_time_ticks);
    m.step(&no_inputs());
    assert_eq!(m.remaining_ticks(), cfg().round_time_ticks - 1);
}

// ---------------------------------------------------------------------------------------------
// Round timer
// ---------------------------------------------------------------------------------------------

#[test]
fn the_round_timer_counts_down_with_every_fighting_step() {
    let mut m = close_duel(cfg());
    assert_eq!(m.remaining_ticks(), 600);
    m.step(&no_inputs());
    assert_eq!(m.remaining_ticks(), 599);
    m.step(&no_inputs());
    assert_eq!(m.remaining_ticks(), 598);
}

#[test]
fn the_round_ends_after_exactly_the_configured_number_of_steps() {
    let config = MatchConfig {
        round_time_ticks: 10,
        ..cfg()
    };
    let mut m = close_duel(config);
    run(&mut m, 9, &no_inputs());
    assert_eq!(m.phase(), Phase::Fighting);
    assert_eq!(m.remaining_ticks(), 1);
    m.step(&no_inputs());
    assert_eq!(m.remaining_ticks(), 0);
    assert!(matches!(m.phase(), Phase::Finished(_)));
}

#[test]
fn a_one_step_round_ends_on_its_first_step() {
    let config = MatchConfig {
        round_time_ticks: 1,
        ..cfg()
    };
    let mut m = close_duel(config);
    m.step(&no_inputs());
    assert!(matches!(m.phase(), Phase::Finished(_)));
}

#[test]
fn time_up_with_equal_health_is_a_draw() {
    let config = MatchConfig {
        round_time_ticks: 10,
        ..cfg()
    };
    let mut m = close_duel(config);
    run(&mut m, 10, &no_inputs());
    assert_eq!(m.phase(), Phase::Finished(Outcome::Draw));
    assert_eq!(m.outcome(), Some(Outcome::Draw));
}

/// One hit (on step 4), then the 20-step round runs out. Returns the final phase.
fn phase_after_one_hit_and_time_up(attacker: u8) -> Phase {
    let config = MatchConfig {
        round_time_ticks: 20,
        ..cfg()
    };
    let mut m = close_duel(config);
    m.step(&only(attacker, jab()));
    run(&mut m, 18, &no_inputs());
    assert_eq!(m.phase(), Phase::Fighting, "one step of the round is left");
    m.step(&no_inputs());
    m.phase()
}

#[test]
fn time_up_gives_the_win_to_the_healthier_fighter() {
    assert_eq!(
        phase_after_one_hit_and_time_up(0),
        Phase::Finished(Outcome::Winner(TeamId(0)))
    );
    assert_eq!(
        phase_after_one_hit_and_time_up(1),
        Phase::Finished(Outcome::Winner(TeamId(1)))
    );
}

#[test]
fn time_up_compares_the_total_health_of_each_team() {
    // Team 0 has two fighters (200 health) against one (100): the sum decides, not the best or
    // the average fighter (those would tie).
    let config = MatchConfig {
        round_time_ticks: 5,
        ..cfg()
    };
    let mut m = match_with(Roster::two_versus_one(), config, &[100, 200, 1100]);
    run(&mut m, 5, &no_inputs());
    assert_eq!(m.phase(), Phase::Finished(Outcome::Winner(TeamId(0))));
}

#[test]
fn time_up_with_tied_leaders_is_a_draw() {
    // Player 0 hurts player 2; players 0 and 1 both keep 100 health and tie for the lead.
    let config = MatchConfig {
        round_time_ticks: 20,
        ..cfg()
    };
    let mut m = match_with(Roster::free_for_all(3).unwrap(), config, &[560, 1100, 640]);
    m.step(&only(0, jab()));
    run(&mut m, 19, &no_inputs());
    assert_eq!(health_of(&m, 2), 80);
    assert_eq!(m.phase(), Phase::Finished(Outcome::Draw));
}

// ---------------------------------------------------------------------------------------------
// Knock-outs decide the round
// ---------------------------------------------------------------------------------------------

#[test]
fn a_knock_out_ends_the_duel_immediately() {
    let mut m = close_duel(lethal());
    run(&mut m, 3, &only(0, jab()));
    assert_eq!(m.phase(), Phase::Fighting);
    assert_eq!(m.outcome(), None);
    m.step(&only(0, jab()));
    assert_eq!(m.outcome(), Some(Outcome::Winner(TeamId(0))));
}

#[test]
fn simultaneous_knock_outs_are_a_draw() {
    let mut m = close_duel(lethal());
    run(&mut m, 4, &inputs_of(&[(0, jab()), (1, jab())]));
    assert_eq!(m.phase(), Phase::Finished(Outcome::Draw));
}

#[test]
fn two_versus_one_is_won_by_the_pair_when_the_lone_fighter_falls() {
    // Player 0 (560) knocks out the lone player 2 (640); player 1 is far away.
    let mut m = match_with(Roster::two_versus_one(), lethal(), &[560, 1100, 640]);
    run(&mut m, 4, &only(0, jab()));
    assert_eq!(m.phase(), Phase::Finished(Outcome::Winner(TeamId(0))));
}

#[test]
fn two_versus_one_continues_while_one_ally_is_still_standing() {
    // The lone player 2 (500) knocks out player 0 (560); player 1 is still up.
    let mut m = match_with(Roster::two_versus_one(), lethal(), &[560, 1100, 500]);
    run(&mut m, 4, &only(2, jab()));
    assert!(fighter(&m, 0).is_knocked_out());
    assert_eq!(m.phase(), Phase::Fighting);
}

#[test]
fn the_lone_fighter_can_win_a_two_versus_one() {
    // Player 2 (640, facing left) knocks out both players 0 (600) and 1 (580).
    let mut m = match_with(Roster::two_versus_one(), lethal(), &[600, 580, 640]);
    run(&mut m, 4, &only(2, jab()));
    assert_eq!(m.phase(), Phase::Finished(Outcome::Winner(TeamId(1))));
}

#[test]
fn a_free_for_all_continues_until_one_fighter_is_left() {
    let mut m = match_with(
        Roster::free_for_all(3).unwrap(),
        lethal(),
        &[560, 640, 1100],
    );
    run(&mut m, 4, &only(0, jab()));
    assert!(fighter(&m, 1).is_knocked_out());
    assert_eq!(m.phase(), Phase::Fighting, "players 0 and 2 are still up");
}

#[test]
fn the_winner_of_a_free_for_all_is_the_survivors_own_team() {
    // Player 2 (640, facing left) knocks out players 0 (600) and 1 (580) at once.
    let mut m = match_with(Roster::free_for_all(3).unwrap(), lethal(), &[600, 580, 640]);
    run(&mut m, 4, &only(2, jab()));
    assert_eq!(m.phase(), Phase::Finished(Outcome::Winner(TeamId(2))));
}

// ---------------------------------------------------------------------------------------------
// Finished matches
// ---------------------------------------------------------------------------------------------

#[test]
fn a_finished_match_ignores_further_steps() {
    let mut m = close_duel(lethal());
    run(&mut m, 4, &only(0, jab()));
    assert!(matches!(m.phase(), Phase::Finished(_)));

    let frozen = m.clone();
    run(&mut m, 20, &inputs_of(&[(0, press_right()), (1, jab())]));
    assert_eq!(m, frozen);
}

// ---------------------------------------------------------------------------------------------
// Pause
// ---------------------------------------------------------------------------------------------

#[test]
fn a_paused_match_does_not_change() {
    let mut m = close_duel(cfg());
    m.step(&only(0, press_right()));
    m.pause();
    assert!(m.is_paused());

    let frozen = m.clone();
    run(&mut m, 5, &only(0, press_right()));
    assert_eq!(m, frozen, "positions, timer and phase are all frozen");
}

#[test]
fn a_resumed_match_continues_where_it_stopped() {
    let mut m = close_duel(cfg());
    m.pause();
    run(&mut m, 5, &only(0, press_right()));
    m.resume();
    assert!(!m.is_paused());
    m.step(&only(0, press_right()));
    assert_eq!(x_of(&m, 0), 570);
    assert_eq!(m.remaining_ticks(), 599);
}

#[test]
fn the_countdown_does_not_advance_while_paused() {
    let mut m = countdown_match();
    m.pause();
    run(&mut m, 10, &no_inputs());
    assert_eq!(m.phase(), Phase::Countdown { remaining: 3 });
    m.resume();
    m.step(&no_inputs());
    assert_eq!(m.phase(), Phase::Countdown { remaining: 2 });
}

#[test]
fn pausing_twice_or_resuming_a_running_match_is_harmless() {
    let mut m = close_duel(cfg());
    m.resume();
    assert!(!m.is_paused());
    m.step(&only(0, press_right()));
    assert_eq!(x_of(&m, 0), 570);

    m.pause();
    m.pause();
    assert!(m.is_paused());
    m.resume();
    assert!(!m.is_paused(), "one resume undoes any number of pauses");
}
