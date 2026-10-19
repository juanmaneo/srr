//! A replay: a match setup and the inputs of every tick, recorded and played again.
//!
//! A [`Replay`] is a pure value: the roster, the configuration and the spawn points decide the
//! initial state, and the inputs of every tick decide the rest, because the simulation is
//! deterministic (see [`Match`]). Recording a match is therefore just remembering what every
//! player pressed, and playing a replay is stepping a fresh [`Match`] with the recorded
//! inputs, one tick at a time.
//!
//! ```
//! use srr::game::{Input, Match, MatchConfig, PlayerId, Replay, Roster};
//! use std::collections::HashMap;
//!
//! // Record: a duel where player 1 walks in for three ticks.
//! let mut replay = Replay::new(Roster::duel(), MatchConfig::default());
//! let mut game = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
//! let inputs = HashMap::from([(PlayerId(1), Input { right: true, ..Input::NONE })]);
//! for _ in 0..3 {
//!     game.step(&inputs);
//!     replay.record(&inputs);
//! }
//!
//! // Play it again, from the start.
//! let again = replay.play().unwrap();
//! assert_eq!(again, game, "the same inputs, the same match");
//! ```

use std::collections::HashMap;
use std::fmt;

use super::config::MatchConfig;
use super::input::Input;
use super::roster::{PlayerId, Roster};
use super::simulation::{Match, SetupError};

/// Why a replay could not be played.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayError {
    /// The replay's setup is invalid; [`Replay::play`] gives it back.
    Setup(SetupError),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplayError::Setup(error) => write!(f, "invalid replay setup: {error}"),
        }
    }
}

impl std::error::Error for ReplayError {}

/// A match to record or to play again: the roster, the configuration and the spawn points,
/// plus the inputs of every tick played so far.
///
/// While recording, [`Replay::record`] remembers the inputs of one tick; while playing,
/// [`Replay::play`] rebuilds the whole match, or [`Replay::frame`] hands the inputs of one
/// tick to [`Match::step`]. Both are plain data: cloning a replay copies it, and the same
/// replay always plays the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    roster: Roster,
    config: MatchConfig,
    spawns: Vec<i32>,
    frames: Vec<HashMap<PlayerId, Input>>,
}

impl Replay {
    /// An empty replay of a match of `roster` and `config` with the standard spawn points (see
    /// [`Match::new`]); ticks are recorded with [`Replay::record`].
    ///
    /// ```
    /// use srr::game::{MatchConfig, Replay, Roster};
    ///
    /// let replay = Replay::new(Roster::duel(), MatchConfig::default());
    /// assert_eq!(replay.len(), 0);
    /// ```
    pub fn new(roster: Roster, config: MatchConfig) -> Self {
        let count = roster.player_count() as i64;
        let width = i64::from(config.arena_width);
        // The quotient lies between 0 and the width, so it fits in an i32.
        let spawns: Vec<i32> = (0..count)
            .map(|index| (width * (index + 1) / (count + 1)) as i32)
            .collect();
        Self::with_spawns(roster, config, spawns)
    }

    /// An empty replay of a match of `roster` and `config` with the given spawn points (see
    /// [`Match::with_spawns`]).
    ///
    /// ```
    /// use srr::game::{MatchConfig, Replay, Roster};
    ///
    /// let replay = Replay::with_spawns(Roster::duel(), MatchConfig::default(), vec![100, 1100]);
    /// assert_eq!(replay.len(), 0);
    /// ```
    pub fn with_spawns(roster: Roster, config: MatchConfig, spawns: Vec<i32>) -> Self {
        Replay {
            roster,
            config,
            spawns,
            frames: Vec::new(),
        }
    }

    /// How many ticks are recorded.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether no tick is recorded yet.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The inputs recorded for `tick`, or `None` when `tick` is beyond the recording.
    ///
    /// ```
    /// use srr::game::{Input, MatchConfig, PlayerId, Replay, Roster};
    /// use std::collections::HashMap;
    ///
    /// let mut replay = Replay::new(Roster::duel(), MatchConfig::default());
    /// let input = Input { left: true, ..Input::NONE };
    /// replay.record(&HashMap::from([(PlayerId(0), input)]));
    /// assert_eq!(replay.frame(0).unwrap()[&PlayerId(0)], input);
    /// assert!(replay.frame(1).is_none());
    /// ```
    pub fn frame(&self, tick: u32) -> Option<&HashMap<PlayerId, Input>> {
        self.frames.get(tick as usize)
    }

    /// Remembers the inputs of one tick, as given to [`Match::step`]. Call it once per tick,
    /// right after stepping, in the order the ticks run. Inputs of players outside the roster
    /// are dropped.
    ///
    /// ```
    /// use srr::game::{Input, MatchConfig, PlayerId, Replay, Roster};
    /// use std::collections::HashMap;
    ///
    /// let mut replay = Replay::new(Roster::duel(), MatchConfig::default());
    /// replay.record(&HashMap::from([(PlayerId(0), Input::NONE)]));
    /// assert_eq!(replay.len(), 1);
    /// assert_eq!(replay.frame(0).unwrap()[&PlayerId(1)], Input::NONE);
    /// ```
    pub fn record(&mut self, inputs: &HashMap<PlayerId, Input>) {
        let frame = self
            .roster
            .players()
            .into_iter()
            .map(|player| (player, inputs.get(&player).copied().unwrap_or(Input::NONE)))
            .collect();
        self.frames.push(frame);
    }

    /// Plays the whole replay from the start: a fresh match, stepped tick by tick with the
    /// recorded inputs.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayError::Setup`] when the replay's setup is invalid.
    ///
    /// ```
    /// use srr::game::{Input, Match, MatchConfig, PlayerId, Replay, Roster};
    /// use std::collections::HashMap;
    ///
    /// let mut replay = Replay::new(Roster::duel(), MatchConfig::default());
    /// let mut game = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
    /// let inputs = HashMap::from([(PlayerId(1), Input { right: true, ..Input::NONE })]);
    /// for _ in 0..10 {
    ///     game.step(&inputs);
    ///     replay.record(&inputs);
    /// }
    /// assert_eq!(replay.play().unwrap(), game);
    /// ```
    pub fn play(&self) -> Result<Match, ReplayError> {
        let mut game = Match::with_spawns(self.roster.clone(), self.config.clone(), &self.spawns)
            .map_err(ReplayError::Setup)?;
        for inputs in &self.frames {
            game.step(inputs);
        }
        Ok(game)
    }
}
