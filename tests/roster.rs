//! Roster: who plays, and who is allied with whom.
//!
//! README: 1v1 (2 players), 1v1v1 (3 players), 2v1 (3 players), up to 10 players with "any kind
//! of alliance or as full brawler".

use srr::game::{PlayerId, PlayerSlot, Roster, RosterError, TeamId};

fn slot(player: u8, team: u8) -> PlayerSlot {
    PlayerSlot {
        player: PlayerId(player),
        team: TeamId(team),
    }
}

/// One slot per entry; the entry is the team of the player whose id is its index.
fn slots(teams: &[u8]) -> Vec<PlayerSlot> {
    teams
        .iter()
        .enumerate()
        .map(|(index, team)| slot(index as u8, *team))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Presets from the README
// ---------------------------------------------------------------------------------------------

#[test]
fn player_limits_match_the_game_design() {
    assert_eq!(Roster::MIN_PLAYERS, 2);
    assert_eq!(Roster::MAX_PLAYERS, 10);
}

#[test]
fn ids_are_ordered_and_copyable() {
    assert!(PlayerId(1) < PlayerId(2));
    assert!(TeamId(0) < TeamId(1));
    let id = PlayerId(3);
    let copy = id;
    assert_eq!(id, copy);
}

#[test]
fn duel_is_a_one_versus_one() {
    let roster = Roster::duel();
    assert_eq!(roster.player_count(), 2);
    assert_eq!(roster.players(), vec![PlayerId(0), PlayerId(1)]);
    assert_eq!(roster.teams(), vec![TeamId(0), TeamId(1)]);
    assert!(roster.are_enemies(PlayerId(0), PlayerId(1)));
    assert!(!roster.are_allies(PlayerId(0), PlayerId(1)));
}

#[test]
fn two_versus_one_allies_the_first_two_players() {
    let roster = Roster::two_versus_one();
    assert_eq!(roster.player_count(), 3);
    assert_eq!(
        roster.players(),
        vec![PlayerId(0), PlayerId(1), PlayerId(2)]
    );
    assert_eq!(roster.teams(), vec![TeamId(0), TeamId(1)]);
    assert_eq!(roster.members(TeamId(0)), vec![PlayerId(0), PlayerId(1)]);
    assert_eq!(roster.members(TeamId(1)), vec![PlayerId(2)]);
    assert!(roster.are_allies(PlayerId(0), PlayerId(1)));
    assert!(roster.are_allies(PlayerId(1), PlayerId(0)));
    assert!(roster.are_enemies(PlayerId(0), PlayerId(2)));
    assert!(roster.are_enemies(PlayerId(2), PlayerId(1)));
    assert!(!roster.are_allies(PlayerId(1), PlayerId(2)));
}

#[test]
fn free_for_all_puts_every_player_on_their_own_team() {
    let roster = Roster::free_for_all(3).unwrap();
    assert_eq!(roster.player_count(), 3);
    assert_eq!(roster.teams().len(), 3);
    for a in roster.players() {
        for b in roster.players() {
            if a != b {
                assert!(roster.are_enemies(a, b), "{a:?} vs {b:?}");
                assert!(!roster.are_allies(a, b), "{a:?} vs {b:?}");
            }
        }
    }
}

#[test]
fn free_for_all_supports_two_to_ten_players() {
    for count in 2..=10 {
        let roster = Roster::free_for_all(count).unwrap();
        assert_eq!(roster.player_count(), count);
        assert_eq!(roster.teams().len(), count);
    }
}

#[test]
fn free_for_all_of_two_is_a_duel() {
    assert_eq!(Roster::free_for_all(2).unwrap(), Roster::duel());
}

#[test]
fn free_for_all_rejects_too_few_or_too_many_players() {
    assert_eq!(
        Roster::free_for_all(0),
        Err(RosterError::TooFewPlayers { count: 0 })
    );
    assert_eq!(
        Roster::free_for_all(1),
        Err(RosterError::TooFewPlayers { count: 1 })
    );
    assert_eq!(
        Roster::free_for_all(11),
        Err(RosterError::TooManyPlayers { count: 11 })
    );
}

// ---------------------------------------------------------------------------------------------
// Team sizes
// ---------------------------------------------------------------------------------------------

#[test]
fn team_sizes_assign_consecutive_players_to_consecutive_teams() {
    assert_eq!(
        Roster::from_team_sizes(&[2, 1]).unwrap(),
        Roster::two_versus_one()
    );
    assert_eq!(Roster::from_team_sizes(&[1, 1]).unwrap(), Roster::duel());
}

#[test]
fn team_sizes_support_arbitrary_alliances_up_to_ten_players() {
    let roster = Roster::from_team_sizes(&[3, 3, 2, 2]).unwrap();
    assert_eq!(roster.player_count(), 10);
    assert_eq!(roster.teams().len(), 4);
    assert_eq!(
        roster.members(TeamId(0)),
        vec![PlayerId(0), PlayerId(1), PlayerId(2)]
    );
    assert_eq!(
        roster.members(TeamId(1)),
        vec![PlayerId(3), PlayerId(4), PlayerId(5)]
    );
    assert_eq!(roster.members(TeamId(2)), vec![PlayerId(6), PlayerId(7)]);
    assert_eq!(roster.members(TeamId(3)), vec![PlayerId(8), PlayerId(9)]);
    assert!(roster.are_allies(PlayerId(0), PlayerId(2)));
    assert!(roster.are_enemies(PlayerId(2), PlayerId(3)));
    assert!(roster.are_enemies(PlayerId(7), PlayerId(8)));
}

#[test]
fn team_sizes_accept_exactly_ten_players() {
    assert!(Roster::from_team_sizes(&[5, 5]).is_ok());
}

#[test]
fn team_sizes_reject_invalid_player_counts() {
    assert_eq!(
        Roster::from_team_sizes(&[]),
        Err(RosterError::TooFewPlayers { count: 0 })
    );
    assert_eq!(
        Roster::from_team_sizes(&[1]),
        Err(RosterError::TooFewPlayers { count: 1 })
    );
    assert_eq!(
        Roster::from_team_sizes(&[5, 6]),
        Err(RosterError::TooManyPlayers { count: 11 })
    );
}

#[test]
fn team_sizes_reject_empty_teams() {
    assert_eq!(
        Roster::from_team_sizes(&[2, 0, 1]),
        Err(RosterError::EmptyTeam)
    );
}

#[test]
fn team_sizes_reject_a_single_team() {
    assert_eq!(
        Roster::from_team_sizes(&[3]),
        Err(RosterError::NoOpposingTeams)
    );
}

// ---------------------------------------------------------------------------------------------
// Explicit slots
// ---------------------------------------------------------------------------------------------

#[test]
fn new_keeps_slot_order_and_team_ids() {
    let roster = Roster::new(vec![slot(5, 7), slot(2, 3), slot(9, 7)]).unwrap();
    assert_eq!(
        roster.players(),
        vec![PlayerId(5), PlayerId(2), PlayerId(9)]
    );
    assert_eq!(roster.teams(), vec![TeamId(3), TeamId(7)]);
    assert_eq!(roster.team_of(PlayerId(5)), Some(TeamId(7)));
    assert_eq!(roster.team_of(PlayerId(2)), Some(TeamId(3)));
    assert_eq!(
        roster.members(TeamId(7)),
        vec![PlayerId(5), PlayerId(9)],
        "members are listed in slot order"
    );
}

#[test]
fn new_rejects_fewer_than_two_players() {
    assert_eq!(
        Roster::new(vec![]),
        Err(RosterError::TooFewPlayers { count: 0 })
    );
    assert_eq!(
        Roster::new(vec![slot(0, 0)]),
        Err(RosterError::TooFewPlayers { count: 1 })
    );
}

#[test]
fn new_rejects_more_than_ten_players() {
    let eleven: Vec<PlayerSlot> = (0..11).map(|i| slot(i, i)).collect();
    assert_eq!(
        Roster::new(eleven),
        Err(RosterError::TooManyPlayers { count: 11 })
    );
    let ten: Vec<PlayerSlot> = (0..10).map(|i| slot(i, i)).collect();
    assert!(Roster::new(ten).is_ok());
}

#[test]
fn new_rejects_duplicate_players() {
    assert_eq!(
        Roster::new(vec![slot(0, 0), slot(1, 1), slot(1, 2)]),
        Err(RosterError::DuplicatePlayer(PlayerId(1)))
    );
}

#[test]
fn new_rejects_rosters_without_opposition() {
    assert_eq!(
        Roster::new(slots(&[4, 4])),
        Err(RosterError::NoOpposingTeams)
    );
    assert_eq!(
        Roster::new(slots(&[4, 4, 4])),
        Err(RosterError::NoOpposingTeams)
    );
}

#[test]
fn validation_order_is_count_then_duplicates_then_opposition() {
    // Eleven players on one team: the player count is reported first.
    assert_eq!(
        Roster::new(slots(&[0; 11])),
        Err(RosterError::TooManyPlayers { count: 11 })
    );
    // Same player twice on one team: the duplicate is reported before the missing opposition.
    assert_eq!(
        Roster::new(vec![slot(0, 0), slot(0, 0)]),
        Err(RosterError::DuplicatePlayer(PlayerId(0)))
    );
}

// ---------------------------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------------------------

#[test]
fn unknown_players_have_no_team_and_no_relationships() {
    let roster = Roster::duel();
    let stranger = PlayerId(9);
    assert_eq!(roster.team_of(stranger), None);
    assert!(!roster.are_allies(stranger, PlayerId(0)));
    assert!(!roster.are_enemies(stranger, PlayerId(0)));
    assert!(!roster.are_allies(PlayerId(0), stranger));
    assert!(!roster.are_enemies(PlayerId(0), stranger));
    assert!(!roster.are_enemies(stranger, stranger));
}

#[test]
fn a_player_is_neither_ally_nor_enemy_of_themselves() {
    let roster = Roster::two_versus_one();
    assert!(!roster.are_allies(PlayerId(0), PlayerId(0)));
    assert!(!roster.are_enemies(PlayerId(0), PlayerId(0)));
}

#[test]
fn teams_are_sorted_and_deduplicated() {
    let roster = Roster::new(slots(&[5, 2, 5, 9])).unwrap();
    assert_eq!(roster.teams(), vec![TeamId(2), TeamId(5), TeamId(9)]);
}

#[test]
fn members_of_an_unknown_team_is_empty() {
    assert!(Roster::duel().members(TeamId(42)).is_empty());
}

#[test]
fn team_of_reports_the_team_of_each_player() {
    let roster = Roster::two_versus_one();
    assert_eq!(roster.team_of(PlayerId(0)), Some(TeamId(0)));
    assert_eq!(roster.team_of(PlayerId(1)), Some(TeamId(0)));
    assert_eq!(roster.team_of(PlayerId(2)), Some(TeamId(1)));
}

#[test]
fn rosters_can_be_cloned_and_compared() {
    let roster = Roster::two_versus_one();
    assert_eq!(roster.clone(), roster);
    assert_ne!(Roster::duel(), Roster::two_versus_one());
}

// ---------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------

#[test]
fn errors_have_human_readable_messages() {
    assert!(RosterError::TooFewPlayers { count: 1 }
        .to_string()
        .contains('1'));
    assert!(RosterError::TooManyPlayers { count: 11 }
        .to_string()
        .contains("11"));
    assert!(RosterError::DuplicatePlayer(PlayerId(4))
        .to_string()
        .contains('4'));
    assert!(!RosterError::NoOpposingTeams.to_string().is_empty());
    assert!(!RosterError::EmptyTeam.to_string().is_empty());
}

#[test]
fn roster_error_is_a_std_error() {
    fn assert_error<E: std::error::Error>() {}
    assert_error::<RosterError>();
}
