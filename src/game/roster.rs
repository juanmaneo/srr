//! Who plays, and who is allied with whom.
//!
//! A [`Roster`] is a validated list of [`PlayerSlot`]s. It is built once, before a match
//! starts, and answers alliance questions while the match runs. Rosters are tiny (at most
//! [`Roster::MAX_PLAYERS`] players), so every query is a plain linear scan.
//!
//! ```
//! use srr::game::{PlayerId, Roster, TeamId};
//!
//! let roster = Roster::two_versus_one();
//! assert!(roster.are_allies(PlayerId(0), PlayerId(1)));
//! assert!(roster.are_enemies(PlayerId(0), PlayerId(2)));
//! assert_eq!(roster.members(TeamId(0)), vec![PlayerId(0), PlayerId(1)]);
//! ```

use std::fmt;

/// Identifies a player (and their fighter) within a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerId(pub u8);

/// Identifies a team; players sharing a team are allies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TeamId(pub u8);

/// One participant of a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerSlot {
    /// The participating player.
    pub player: PlayerId,
    /// The team the player fights for.
    pub team: TeamId,
}

/// A validated list of participants: 2 to 10 distinct players spread over at least two teams.
///
/// The order of the slots is the *roster order*, which the simulation uses to update fighters
/// and to place them in the arena.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roster {
    slots: Vec<PlayerSlot>,
}

/// Why a roster could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RosterError {
    /// Fewer than [`Roster::MIN_PLAYERS`] players were given.
    TooFewPlayers {
        /// The number of players that was given.
        count: usize,
    },
    /// More than [`Roster::MAX_PLAYERS`] players were given.
    TooManyPlayers {
        /// The number of players that was given.
        count: usize,
    },
    /// The same player id appears in more than one slot.
    DuplicatePlayer(PlayerId),
    /// A team size of zero was given to [`Roster::from_team_sizes`].
    EmptyTeam,
    /// Everybody is on the same team, so nobody could ever win a fight.
    NoOpposingTeams,
}

impl Roster {
    /// The smallest number of players a match accepts.
    pub const MIN_PLAYERS: usize = 2;
    /// The largest number of players a match accepts.
    pub const MAX_PLAYERS: usize = 10;

    /// Builds a roster from explicit slots, keeping their order.
    ///
    /// Errors are reported in this order: player count, duplicate players, missing opposition.
    ///
    /// # Errors
    ///
    /// Returns a [`RosterError`] when the slots do not form a valid roster.
    ///
    /// # Complexity
    ///
    /// O(n²) in the number of slots (duplicate detection); n is at most 10 for any valid
    /// roster, and oversized inputs are rejected before the scan.
    ///
    /// # Examples
    ///
    /// ```
    /// use srr::game::{PlayerId, PlayerSlot, Roster, RosterError, TeamId};
    ///
    /// let slot = |player, team| PlayerSlot { player: PlayerId(player), team: TeamId(team) };
    /// let roster = Roster::new(vec![slot(5, 7), slot(2, 3)]).unwrap();
    /// assert_eq!(roster.players(), vec![PlayerId(5), PlayerId(2)]);
    ///
    /// assert_eq!(
    ///     Roster::new(vec![slot(0, 1), slot(1, 1)]),
    ///     Err(RosterError::NoOpposingTeams)
    /// );
    /// ```
    pub fn new(slots: Vec<PlayerSlot>) -> Result<Self, RosterError> {
        check_player_count(slots.len())?;
        for (index, slot) in slots.iter().enumerate() {
            if slots[..index]
                .iter()
                .any(|other| other.player == slot.player)
            {
                return Err(RosterError::DuplicatePlayer(slot.player));
            }
        }
        let roster = Roster { slots };
        if roster.teams().len() < 2 {
            return Err(RosterError::NoOpposingTeams);
        }
        Ok(roster)
    }

    /// 1v1: player 0 (team 0) against player 1 (team 1).
    pub fn duel() -> Self {
        Self::from_valid_sizes(&[1, 1])
    }

    /// Players 0 and 1 (team 0) against player 2 (team 1).
    pub fn two_versus_one() -> Self {
        Self::from_valid_sizes(&[2, 1])
    }

    /// Every player on their own team; player `i` is on team `i`.
    ///
    /// # Errors
    ///
    /// Returns [`RosterError::TooFewPlayers`] or [`RosterError::TooManyPlayers`] when `count`
    /// is outside `2..=10`.
    ///
    /// # Examples
    ///
    /// ```
    /// use srr::game::{PlayerId, Roster};
    ///
    /// let brawl = Roster::free_for_all(4).unwrap();
    /// assert_eq!(brawl.player_count(), 4);
    /// assert!(brawl.are_enemies(PlayerId(0), PlayerId(3)));
    /// assert!(Roster::free_for_all(11).is_err());
    /// ```
    pub fn free_for_all(count: usize) -> Result<Self, RosterError> {
        check_player_count(count)?;
        Ok(Self::from_valid_sizes(&vec![1; count]))
    }

    /// Assigns consecutive player ids to consecutive teams: `[2, 1]` is a 2v1.
    ///
    /// # Errors
    ///
    /// Reports, in this order: a total player count outside `2..=10`, a team of size zero,
    /// and fewer than two teams.
    ///
    /// # Examples
    ///
    /// ```
    /// use srr::game::{PlayerId, Roster, TeamId};
    ///
    /// let roster = Roster::from_team_sizes(&[3, 2]).unwrap();
    /// assert_eq!(roster.team_of(PlayerId(2)), Some(TeamId(0)));
    /// assert_eq!(roster.team_of(PlayerId(3)), Some(TeamId(1)));
    /// ```
    pub fn from_team_sizes(sizes: &[usize]) -> Result<Self, RosterError> {
        let count = sizes
            .iter()
            .fold(0usize, |sum, &size| sum.saturating_add(size));
        check_player_count(count)?;
        if sizes.contains(&0) {
            return Err(RosterError::EmptyTeam);
        }
        if sizes.len() < 2 {
            return Err(RosterError::NoOpposingTeams);
        }
        Ok(Self::from_valid_sizes(sizes))
    }

    /// `sizes` must describe 2..=10 players, so the ids fit in a `u8`.
    fn from_valid_sizes(sizes: &[usize]) -> Self {
        let slots = sizes
            .iter()
            .enumerate()
            .flat_map(|(team, &size)| std::iter::repeat_n(TeamId(team as u8), size))
            .enumerate()
            .map(|(player, team)| PlayerSlot {
                player: PlayerId(player as u8),
                team,
            })
            .collect();
        Roster { slots }
    }

    /// The number of players in the roster.
    pub fn player_count(&self) -> usize {
        self.slots.len()
    }

    /// Players in roster order.
    ///
    /// Complexity: O(n) and allocates.
    pub fn players(&self) -> Vec<PlayerId> {
        self.slots.iter().map(|slot| slot.player).collect()
    }

    /// Distinct teams, sorted.
    ///
    /// Complexity: O(n log n) and allocates.
    pub fn teams(&self) -> Vec<TeamId> {
        let mut teams: Vec<TeamId> = self.slots.iter().map(|slot| slot.team).collect();
        teams.sort();
        teams.dedup();
        teams
    }

    /// Members of `team` in roster order (empty for an unknown team).
    ///
    /// Complexity: O(n) and allocates.
    pub fn members(&self, team: TeamId) -> Vec<PlayerId> {
        self.slots
            .iter()
            .filter(|slot| slot.team == team)
            .map(|slot| slot.player)
            .collect()
    }

    /// The team of `player`, or `None` when the player is not in the roster.
    ///
    /// Complexity: O(n) linear scan.
    pub fn team_of(&self, player: PlayerId) -> Option<TeamId> {
        self.slots
            .iter()
            .find(|slot| slot.player == player)
            .map(|slot| slot.team)
    }

    /// Two different players of the same team. Unknown players have no allies.
    ///
    /// Complexity: O(n).
    pub fn are_allies(&self, a: PlayerId, b: PlayerId) -> bool {
        a != b && matches!((self.team_of(a), self.team_of(b)), (Some(x), Some(y)) if x == y)
    }

    /// Two players of different teams. Unknown players have no enemies.
    ///
    /// Complexity: O(n).
    pub fn are_enemies(&self, a: PlayerId, b: PlayerId) -> bool {
        a != b && matches!((self.team_of(a), self.team_of(b)), (Some(x), Some(y)) if x != y)
    }
}

fn check_player_count(count: usize) -> Result<(), RosterError> {
    if count < Roster::MIN_PLAYERS {
        Err(RosterError::TooFewPlayers { count })
    } else if count > Roster::MAX_PLAYERS {
        Err(RosterError::TooManyPlayers { count })
    } else {
        Ok(())
    }
}

impl fmt::Display for RosterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RosterError::TooFewPlayers { count } => write!(
                f,
                "a match needs at least {} players, got {count}",
                Roster::MIN_PLAYERS
            ),
            RosterError::TooManyPlayers { count } => write!(
                f,
                "a match allows at most {} players, got {count}",
                Roster::MAX_PLAYERS
            ),
            RosterError::DuplicatePlayer(player) => {
                write!(f, "player {} appears more than once", player.0)
            }
            RosterError::EmptyTeam => write!(f, "a team has no players"),
            RosterError::NoOpposingTeams => write!(f, "all players are on the same team"),
        }
    }
}

impl std::error::Error for RosterError {}
