//! Creating a match: validation, spawn points, initial fighter state.
//!
//! Default spawn rule: with `n` fighters in roster order, fighter `i` spawns at
//! `arena_width * (i + 1) / (n + 1)` (integer division), on the floor, at full health.
//! A fighter faces right unless it stands strictly right of the arena centre.

mod common;

use common::*;
use srr::game::{
    ConfigError, Facing, FighterState, Match, MatchConfig, Outcome, Phase, PlayerId, Roster,
    SetupError,
};

// ---------------------------------------------------------------------------------------------
// Default spawn points
// ---------------------------------------------------------------------------------------------

#[test]
fn a_duel_spawns_two_fighters_evenly_and_facing_each_other() {
    let m = Match::new(Roster::duel(), cfg()).unwrap();
    assert_eq!(x_of(&m, 0), 400);
    assert_eq!(x_of(&m, 1), 800);
    assert_eq!(facing_of(&m, 0), Facing::Right);
    assert_eq!(facing_of(&m, 1), Facing::Left);
}

#[test]
fn three_players_spawn_evenly_and_the_middle_one_faces_right() {
    let m = Match::new(Roster::free_for_all(3).unwrap(), cfg()).unwrap();
    assert_eq!(x_of(&m, 0), 300);
    assert_eq!(x_of(&m, 1), 600);
    assert_eq!(x_of(&m, 2), 900);
    assert_eq!(facing_of(&m, 0), Facing::Right);
    assert_eq!(
        facing_of(&m, 1),
        Facing::Right,
        "the centre counts as left half"
    );
    assert_eq!(facing_of(&m, 2), Facing::Left);
}

#[test]
fn every_fighter_starts_idle_on_the_floor_with_full_health() {
    let m = Match::new(Roster::two_versus_one(), cfg()).unwrap();
    for id in 0..3 {
        assert_eq!(y_of(&m, id), 0);
        assert_eq!(health_of(&m, id), 100);
        assert_eq!(state_of(&m, id), FighterState::Idle);
        assert!(!fighter(&m, id).is_knocked_out());
    }
}

#[test]
fn fighters_follow_roster_order() {
    let m = Match::new(Roster::two_versus_one(), cfg()).unwrap();
    let ids: Vec<PlayerId> = m.fighters().iter().map(|f| f.player()).collect();
    assert_eq!(ids, vec![PlayerId(0), PlayerId(1), PlayerId(2)]);
}

#[test]
fn ten_players_fit_in_the_arena_in_strictly_increasing_order() {
    let m = Match::new(Roster::free_for_all(10).unwrap(), cfg()).unwrap();
    assert_eq!(m.fighters().len(), 10);
    let xs: Vec<i32> = m.fighters().iter().map(|f| f.x()).collect();
    assert!(xs.windows(2).all(|pair| pair[0] < pair[1]), "{xs:?}");
    assert!(xs.iter().all(|x| (0..=1200).contains(x)), "{xs:?}");
}

#[test]
fn unknown_players_have_no_fighter() {
    let m = Match::new(Roster::duel(), cfg()).unwrap();
    assert!(m.fighter(PlayerId(9)).is_none());
}

#[test]
fn the_match_exposes_its_roster_and_config() {
    let m = Match::new(Roster::two_versus_one(), cfg()).unwrap();
    assert_eq!(m.roster(), &Roster::two_versus_one());
    assert_eq!(m.config(), &cfg());
}

// ---------------------------------------------------------------------------------------------
// Initial phase
// ---------------------------------------------------------------------------------------------

#[test]
fn without_countdown_the_match_starts_fighting() {
    let m = Match::new(Roster::duel(), cfg()).unwrap();
    assert_eq!(m.phase(), Phase::Fighting);
    assert_eq!(m.outcome(), None);
    assert_eq!(m.remaining_ticks(), cfg().round_time_ticks);
    assert!(!m.is_paused());
}

#[test]
fn with_a_countdown_the_match_starts_in_countdown() {
    let config = MatchConfig {
        countdown_ticks: 90,
        ..cfg()
    };
    let m = Match::new(Roster::duel(), config).unwrap();
    assert_eq!(m.phase(), Phase::Countdown { remaining: 90 });
    assert_eq!(m.outcome(), None);
}

#[test]
fn a_new_match_can_be_cloned_and_compared() {
    let m = Match::new(Roster::duel(), cfg()).unwrap();
    assert_eq!(m.clone(), m);
}

// ---------------------------------------------------------------------------------------------
// Explicit spawn points
// ---------------------------------------------------------------------------------------------

#[test]
fn explicit_spawn_points_are_used_in_roster_order() {
    let m = Match::with_spawns(Roster::two_versus_one(), cfg(), &[10, 1000, 500]).unwrap();
    assert_eq!(x_of(&m, 0), 10);
    assert_eq!(x_of(&m, 1), 1000);
    assert_eq!(x_of(&m, 2), 500);
    assert_eq!(y_of(&m, 0), 0);
}

#[test]
fn spawn_facing_depends_on_the_side_of_the_arena_centre() {
    let m = Match::with_spawns(Roster::free_for_all(3).unwrap(), cfg(), &[600, 601, 0]).unwrap();
    assert_eq!(facing_of(&m, 0), Facing::Right, "exactly at the centre");
    assert_eq!(
        facing_of(&m, 1),
        Facing::Left,
        "one unit right of the centre"
    );
    assert_eq!(facing_of(&m, 2), Facing::Right, "left wall");

    let m = Match::with_spawns(Roster::duel(), cfg(), &[1200, 599]).unwrap();
    assert_eq!(facing_of(&m, 0), Facing::Left, "right wall");
    assert_eq!(
        facing_of(&m, 1),
        Facing::Right,
        "one unit left of the centre"
    );
}

#[test]
fn spawn_points_on_the_walls_are_valid() {
    assert!(Match::with_spawns(Roster::duel(), cfg(), &[0, 1200]).is_ok());
}

#[test]
fn spawn_points_outside_the_arena_are_rejected() {
    assert_eq!(
        Match::with_spawns(Roster::duel(), cfg(), &[-1, 500]).err(),
        Some(SetupError::SpawnOutOfBounds {
            player: PlayerId(0),
            x: -1
        })
    );
    assert_eq!(
        Match::with_spawns(Roster::duel(), cfg(), &[500, 1201]).err(),
        Some(SetupError::SpawnOutOfBounds {
            player: PlayerId(1),
            x: 1201
        })
    );
}

#[test]
fn spawn_count_must_match_the_roster() {
    assert_eq!(
        Match::with_spawns(Roster::duel(), cfg(), &[500]).err(),
        Some(SetupError::SpawnCountMismatch {
            expected: 2,
            actual: 1
        })
    );
    assert_eq!(
        Match::with_spawns(Roster::duel(), cfg(), &[1, 2, 3]).err(),
        Some(SetupError::SpawnCountMismatch {
            expected: 2,
            actual: 3
        })
    );
}

// ---------------------------------------------------------------------------------------------
// Invalid configuration
// ---------------------------------------------------------------------------------------------

#[test]
fn an_invalid_config_is_rejected_by_new() {
    let config = MatchConfig {
        round_time_ticks: 0,
        ..cfg()
    };
    assert_eq!(
        Match::new(Roster::duel(), config).err(),
        Some(SetupError::InvalidConfig(ConfigError::ZeroRoundTime))
    );
}

#[test]
fn an_invalid_config_is_rejected_by_with_spawns_before_the_spawns_are_checked() {
    let config = MatchConfig {
        max_health: 0,
        ..cfg()
    };
    assert_eq!(
        Match::with_spawns(Roster::duel(), config, &[1, 2, 3]).err(),
        Some(SetupError::InvalidConfig(ConfigError::NonPositiveMaxHealth))
    );
}

#[test]
fn setup_errors_have_human_readable_messages() {
    let errors = [
        SetupError::InvalidConfig(ConfigError::ZeroRoundTime),
        SetupError::SpawnCountMismatch {
            expected: 2,
            actual: 5,
        },
        SetupError::SpawnOutOfBounds {
            player: PlayerId(3),
            x: -77,
        },
    ];
    for error in &errors {
        assert!(!error.to_string().is_empty(), "{error:?}");
    }
    assert!(errors[1].to_string().contains('5'));
    assert!(errors[2].to_string().contains("-77"));
}

#[test]
fn setup_error_is_a_std_error() {
    fn assert_error<E: std::error::Error>() {}
    assert_error::<SetupError>();
}

#[test]
fn outcome_is_not_decided_at_the_start() {
    let m = Match::new(Roster::duel(), cfg()).unwrap();
    assert!(!matches!(m.phase(), Phase::Finished(Outcome::Winner(_))));
    assert!(!matches!(m.phase(), Phase::Finished(Outcome::Draw)));
}
