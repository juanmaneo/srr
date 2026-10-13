//! Deterministic lock-step for a free-for-all match of 2 or more players.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use super::players::PlayerSet;
use super::protocol::ClientMessage;
use crate::game::{
    Input, Match, MatchConfig, Outcome, PlayerId, PlayerSlot, Roster, RosterError, SetupError,
    TeamId,
};

/// Ticks between reading a local input and simulating it. Every player's input must have
/// arrived by then; on a local network three ticks (50 ms) leave plenty of margin.
pub const INPUT_DELAY: u32 = 3;

/// Why a [`Session`] could not be created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// The local player is not one of the session's players.
    NotInSession(PlayerId),
    /// The players do not form a valid roster (e.g. fewer than two of them).
    InvalidRoster(RosterError),
    /// The match could not be set up with the given configuration.
    InvalidSetup(SetupError),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::NotInSession(player) => {
                write!(f, "player {} does not take part in the match", player.0)
            }
            SessionError::InvalidRoster(error) => write!(f, "{error}"),
            SessionError::InvalidSetup(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SessionError {}

/// One online match, everybody for themselves, as seen from one of the machines.
///
/// Local inputs are scheduled `delay` ticks ahead and sent to the other players; a tick is
/// simulated once the inputs of every player for it are known. The first `delay` ticks press
/// nothing for everybody, so the match starts at once. When somebody's input is late the session
/// simply waits, and it never schedules local inputs more than `delay` ticks ahead, so nobody
/// runs away.
///
/// A finished round stays on screen; the next one is a new session ([`Session::next`]), which
/// keeps counting the round wins.
///
/// ```
/// use srr::game::{MatchConfig, PlayerId};
/// use srr::net::{ClientMessage, PlayerSet, Session};
///
/// let players = PlayerSet::first(3);
/// let mut machines: Vec<Session> = players
///     .iter()
///     .map(|p| Session::new(p, players, 7, MatchConfig::default(), 2).unwrap())
///     .collect();
///
/// for _ in 0..10 {
///     let sent: Vec<(PlayerId, ClientMessage)> = machines
///         .iter_mut()
///         .map(|m| (m.local_player(), m.submit_local(Default::default()).unwrap()))
///         .collect();
///     for (from, message) in sent {
///         let ClientMessage::Input { session, tick, input } = message else { unreachable!() };
///         for machine in machines.iter_mut().filter(|m| m.local_player() != from) {
///             machine.receive_remote(session, from, tick, input);
///         }
///     }
///     machines.iter_mut().for_each(|m| { m.advance(); });
/// }
/// // The 2 implicit ticks plus the 10 exchanged ones, identical everywhere.
/// assert_eq!(machines[0].tick(), 12);
/// assert_eq!(machines[0].current(), machines[2].current());
/// ```
#[derive(Debug, Clone)]
pub struct Session {
    local: PlayerId,
    players: PlayerSet,
    id: u8,
    delay: u32,
    config: MatchConfig,
    current: Match,
    tick: u32,
    next_local: u32,
    /// Known inputs of the ticks still to simulate, by tick then player.
    inputs: BTreeMap<(u32, u8), Input>,
    wins: [u32; PlayerSet::CAPACITY],
}

impl Session {
    /// A new match numbered `id` between `players` (each on their own team), with `local`
    /// playing on this machine.
    ///
    /// # Errors
    ///
    /// In this order: [`SessionError::NotInSession`] when `local` is not one of `players`,
    /// [`SessionError::InvalidRoster`] when the players cannot form a match (fewer than two),
    /// and [`SessionError::InvalidSetup`] when the configuration does not validate.
    pub fn new(
        local: PlayerId,
        players: PlayerSet,
        id: u8,
        config: MatchConfig,
        delay: u32,
    ) -> Result<Self, SessionError> {
        if !players.contains(local) {
            return Err(SessionError::NotInSession(local));
        }
        let current = Self::fresh_match(players, &config)?;
        Ok(Session {
            local,
            players,
            id,
            delay,
            config,
            current,
            tick: 0,
            next_local: delay,
            inputs: BTreeMap::new(),
            wins: [0; PlayerSet::CAPACITY],
        })
    }

    fn fresh_match(players: PlayerSet, config: &MatchConfig) -> Result<Match, SessionError> {
        let slots = players
            .iter()
            .map(|player| PlayerSlot {
                player,
                team: TeamId(player.0),
            })
            .collect();
        let roster = Roster::new(slots).map_err(SessionError::InvalidRoster)?;
        Match::new(roster, config.clone()).map_err(SessionError::InvalidSetup)
    }

    /// The next round between the same players, numbered `id`: a fresh match, the round wins
    /// carried over.
    ///
    /// ```
    /// use srr::game::{MatchConfig, PlayerId};
    /// use srr::net::{PlayerSet, Session};
    ///
    /// let first = Session::new(PlayerId(0), PlayerSet::first(2), 1, MatchConfig::default(), 0)
    ///     .unwrap();
    /// let second = first.next(2);
    /// assert_eq!((second.id(), second.tick()), (2, 0));
    /// assert_eq!(second.players(), first.players());
    /// ```
    pub fn next(&self, id: u8) -> Session {
        Session {
            id,
            current: Self::fresh_match(self.players, &self.config)
                .expect("the players and the configuration were validated by Session::new"),
            tick: 0,
            next_local: self.delay,
            inputs: BTreeMap::new(),
            ..self.clone()
        }
    }

    /// Everybody fighting in this session.
    pub fn players(&self) -> PlayerSet {
        self.players
    }

    /// The player controlled on this machine.
    pub fn local_player(&self) -> PlayerId {
        self.local
    }

    /// The session number given by the server.
    pub fn id(&self) -> u8 {
        self.id
    }

    /// The round being played (or just finished).
    pub fn current(&self) -> &Match {
        &self.current
    }

    /// Number of ticks simulated so far.
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// Rounds won by `player` (0 for players outside the session).
    pub fn wins(&self, player: PlayerId) -> u32 {
        if self.players.contains(player) {
            self.wins[usize::from(player.0)]
        } else {
            0
        }
    }

    /// The other players whose input for the next tick to simulate is still missing.
    pub fn waiting_for(&self) -> PlayerSet {
        self.players
            .without(self.local)
            .iter()
            .filter(|&player| self.frame(self.tick, player).is_none())
            .collect()
    }

    /// Whether the next tick waits for the input of another player.
    pub fn awaiting_remote(&self) -> bool {
        !self.waiting_for().is_empty()
    }

    /// Schedules the local input for the next free tick and returns the message to send, or
    /// `None` when local inputs are already scheduled `delay` ticks ahead of the simulation
    /// (somebody is late; the input is dropped).
    pub fn submit_local(&mut self, input: Input) -> Option<ClientMessage> {
        if self.next_local > self.tick.saturating_add(self.delay) {
            return None;
        }
        let tick = self.next_local;
        self.inputs.insert((tick, self.local.0), input);
        self.next_local += 1;
        Some(ClientMessage::Input {
            session: self.id,
            tick,
            input,
        })
    }

    /// Records the input of another player for `tick`. Returns whether it was accepted: inputs
    /// of another session, of the local player or of a player outside the session, of already
    /// simulated or implicit ticks, and duplicates are ignored.
    pub fn receive_remote(
        &mut self,
        session: u8,
        player: PlayerId,
        tick: u32,
        input: Input,
    ) -> bool {
        if session != self.id
            || player == self.local
            || !self.players.contains(player)
            || tick < self.tick.max(self.delay)
        {
            return false;
        }
        match self.inputs.entry((tick, player.0)) {
            std::collections::btree_map::Entry::Occupied(_) => false,
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(input);
                true
            }
        }
    }

    /// Simulates every tick for which the inputs of all players are known; returns how many
    /// ran.
    pub fn advance(&mut self) -> u32 {
        let start = self.tick;
        loop {
            let inputs: Option<HashMap<PlayerId, Input>> = self
                .players
                .iter()
                .map(|player| self.frame(self.tick, player).map(|input| (player, input)))
                .collect();
            let Some(inputs) = inputs else {
                break;
            };
            for player in self.players.iter() {
                self.inputs.remove(&(self.tick, player.0));
            }
            self.step(&inputs);
            self.tick += 1;
        }
        self.tick - start
    }

    /// The input of `player` for `tick`; the first `delay` ticks press nothing.
    fn frame(&self, tick: u32, player: PlayerId) -> Option<Input> {
        if tick < self.delay {
            Some(Input::NONE)
        } else {
            self.inputs.get(&(tick, player.0)).copied()
        }
    }

    fn step(&mut self, inputs: &HashMap<PlayerId, Input>) {
        if self.current.outcome().is_some() {
            return;
        }
        self.current.step(inputs);
        if let Some(Outcome::Winner(team)) = self.current.outcome() {
            // Everybody fights for themselves: team i is player i.
            self.wins[usize::from(team.0)] += 1;
        }
    }
}
