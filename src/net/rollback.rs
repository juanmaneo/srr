//! Rollback for a free-for-all match of 2 to [`MAX_PLAYERS`](super::MAX_PLAYERS) players: the
//! server decides the inputs of every tick, each machine predicts the ticks it has not heard
//! about yet and corrects itself when the server's frames disagree.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::ops::Range;

use super::players::PlayerSet;
use super::protocol::{Characters, FrameInputs};
use crate::game::{
    Input, Match, MatchConfig, Outcome, PlayerId, PlayerSlot, Roster, RosterError, SetupError,
    TICKS_PER_SECOND, TeamId,
};

/// Ticks between reading a local input and the tick it is played on. Two ticks (33 ms) give
/// the input a head start towards the server, so the prediction is wrong less often, without
/// making the controls feel slow.
pub const INPUT_DELAY: u32 = 2;

/// The most ticks the round shown runs ahead of the last frame of the server (half a second).
/// Beyond that the connection is too far behind: the match waits for the server rather than
/// guessing on, and the frames that arrive then may rewrite at most that much. It is also the
/// most ticks the round shown trails the frames before jumping to them (after the browser
/// paused the page, say).
pub const MAX_ROLLBACK: u32 = TICKS_PER_SECOND / 2;

/// Ticks a player counts as lagging after one of their inputs reached the server too late
/// (the server repeated their previous input instead).
pub const LAG_MEMORY: u32 = TICKS_PER_SECOND;

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
/// Every tick the machine schedules its local input [`INPUT_DELAY`] ticks ahead and sends it to
/// the server ([`Session::submit_local`]). The server answers with one frame per tick holding
/// the inputs of everybody ([`Session::receive_frame`]); a slow player never holds the others
/// up, because the server repeats their previous input when theirs comes too late.
///
/// So the machine keeps two matches: the *confirmed* one, played with the frames of the server
/// only, and the one on screen ([`Session::current`]), which [`Session::advance`] moves one tick
/// further every tick. When the frames are ahead of it (a fast network), it simply plays them.
/// Otherwise it guesses that everybody whose input is not known yet still holds what they held
/// in the last frame. When a frame proves a guess wrong, the shown match is played again from
/// the confirmed one: a correction of a few ticks, the way fighting games roll back. It never
/// runs more than [`MAX_ROLLBACK`] ticks ahead of the confirmed match: past that it waits for
/// the server.
///
/// A finished round stays on screen; the next one is a new session ([`Session::next`]), which
/// keeps counting the round wins.
///
/// ```
/// use std::collections::BTreeMap;
/// use srr::game::{Input, MatchConfig};
/// use srr::net::{PlayerSet, Session};
///
/// let players = PlayerSet::first(3);
/// let mut machines: Vec<Session> = players
///     .iter()
///     .map(|p| Session::new(p, players, 7, MatchConfig::default(), 2).unwrap())
///     .collect();
///
/// // Each machine plays on, predicting; the server gathers the inputs, tick by tick.
/// let mut frames = BTreeMap::new();
/// for _ in 0..10 {
///     for machine in machines.iter_mut() {
///         let player = usize::from(machine.local_player().0);
///         for tick in machine.submit_local(Input::NONE) {
///             frames.entry(tick).or_insert([Input::NONE; PlayerSet::CAPACITY])[player] =
///                 Input::NONE;
///         }
///         machine.advance();
///     }
/// }
/// assert_eq!((machines[0].tick(), machines[0].confirmed_tick()), (10, 0));
///
/// // Every input of ticks 0 to 11 (two ahead) is known: the server sends the frames.
/// for (&tick, inputs) in &frames {
///     for machine in machines.iter_mut() {
///         assert!(machine.receive_frame(7, tick, inputs, PlayerSet::EMPTY));
///     }
/// }
/// machines.iter_mut().for_each(|m| { m.advance(); });
/// assert_eq!((machines[0].tick(), machines[0].confirmed_tick()), (11, 12));
/// assert_eq!(machines[0].current(), machines[2].current());
/// ```
#[derive(Debug, Clone)]
pub struct Session {
    local: PlayerId,
    players: PlayerSet,
    id: u8,
    delay: u32,
    config: MatchConfig,
    /// The match after `confirmed_tick` ticks, played with the frames of the server only.
    confirmed: Match,
    confirmed_tick: u32,
    /// The match shown: `confirmed` plus the predicted ticks, up to `tick`.
    current: Match,
    tick: u32,
    /// The first tick without a local input yet.
    next_local: u32,
    /// The local inputs of the ticks the server has not confirmed yet.
    local_inputs: BTreeMap<u32, Input>,
    /// The inputs `current` was predicted with, for the ticks `confirmed_tick..tick` (when
    /// the round shown is ahead of the frames).
    predicted: VecDeque<FrameInputs>,
    /// The frames of the ticks `tick..confirmed_tick`, still to play on screen (when the round
    /// shown trails the frames).
    unplayed: VecDeque<FrameInputs>,
    /// A frame differed from its prediction: `current` must be played again.
    mispredicted: bool,
    /// The inputs of the last frame: the guess for every later tick.
    latest: FrameInputs,
    /// For each player, the confirmed tick from which they no longer count as lagging.
    lagging_until: [u32; PlayerSet::CAPACITY],
    /// Rounds won in the earlier sessions between the same players.
    carried: [u32; PlayerSet::CAPACITY],
}

impl Session {
    /// A new match numbered `id` between `players` (each on their own team), with `local`
    /// playing on this machine and its inputs scheduled `delay` ticks ahead.
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
        let slots = players
            .iter()
            .map(|player| PlayerSlot {
                player,
                team: TeamId(player.0),
            })
            .collect();
        let roster = Roster::new(slots).map_err(SessionError::InvalidRoster)?;
        let fresh = Match::new(roster, config.clone()).map_err(SessionError::InvalidSetup)?;
        Ok(Session {
            local,
            players,
            id,
            delay,
            config,
            confirmed: fresh.clone(),
            confirmed_tick: 0,
            current: fresh,
            tick: 0,
            next_local: 0,
            local_inputs: BTreeMap::new(),
            predicted: VecDeque::new(),
            unplayed: VecDeque::new(),
            mispredicted: false,
            latest: [Input::NONE; PlayerSet::CAPACITY],
            lagging_until: [0; PlayerSet::CAPACITY],
            carried: [0; PlayerSet::CAPACITY],
        })
    }

    /// The next round between the same players, numbered `id`: a fresh match, the round wins
    /// carried over (as confirmed by the server).
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
        self.next_with(id, self.config.characters)
    }

    /// Like [`Session::next`], with everybody playing the given characters (they may have
    /// changed between the rounds; the round wins are carried over all the same).
    ///
    /// ```
    /// use srr::game::{Character, MatchConfig, PlayerId};
    /// use srr::net::{PlayerSet, Session};
    ///
    /// let first = Session::new(PlayerId(0), PlayerSet::first(2), 1, MatchConfig::default(), 0)
    ///     .unwrap();
    /// let mut characters = [Character::Chalk; 10];
    /// characters[1] = Character::Torch;
    /// let second = first.next_with(2, characters);
    /// assert_eq!(second.current().config().character(PlayerId(1)), Character::Torch);
    /// ```
    pub fn next_with(&self, id: u8, characters: Characters) -> Session {
        let mut carried = self.carried;
        if let Some(winner) = winner(&self.confirmed) {
            carried[usize::from(winner.0)] += 1;
        }
        let config = MatchConfig {
            characters,
            ..self.config.clone()
        };
        let fresh = Session::new(self.local, self.players, id, config, self.delay)
            .expect("the players and the configuration were validated by Session::new");
        Session { carried, ..fresh }
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

    /// How many ticks ahead of the round shown the local inputs are scheduled.
    pub fn delay(&self) -> u32 {
        self.delay
    }

    /// Schedules the next local inputs `delay` ticks ahead of the round shown. A longer delay
    /// sends the inputs of the extra ticks at the next [`Session::submit_local`]; a shorter one
    /// sends nothing until the round shown catches up with the inputs already scheduled.
    pub fn set_delay(&mut self, delay: u32) {
        self.delay = delay;
    }

    /// The round shown: confirmed by the server up to [`Session::confirmed_tick`], predicted
    /// after that.
    pub fn current(&self) -> &Match {
        &self.current
    }

    /// The round as confirmed by the server, [`Session::confirmed_tick`] ticks in.
    pub fn confirmed(&self) -> &Match {
        &self.confirmed
    }

    /// Number of ticks of the round shown.
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// Number of ticks whose frame came from the server.
    pub fn confirmed_tick(&self) -> u32 {
        self.confirmed_tick
    }

    /// Whether the round is over for sure (the server's frames say so). The round shown may
    /// look over a little earlier, or, after a wrong guess, for a moment only.
    pub fn is_finished(&self) -> bool {
        self.confirmed.outcome().is_some()
    }

    /// Rounds won by `player`, the round shown included (0 for players outside the session).
    pub fn wins(&self, player: PlayerId) -> u32 {
        if !self.players.contains(player) {
            return 0;
        }
        let shown = u32::from(winner(&self.current) == Some(player));
        self.carried[usize::from(player.0)] + shown
    }

    /// The players whose inputs reached the server too late within the last [`LAG_MEMORY`]
    /// confirmed ticks: their fighters repeat their previous moves for a moment.
    pub fn lagging(&self) -> PlayerSet {
        self.players
            .iter()
            .filter(|&player| self.lagging_until[usize::from(player.0)] > self.confirmed_tick)
            .collect()
    }

    /// Schedules the local input for every tick up to `delay` ticks after the round shown that
    /// does not have one yet, and returns those ticks: the inputs to send to the server.
    /// Usually one tick; several at the start of the round or after catching up; none while
    /// the session waits for the server (the input is dropped).
    pub fn submit_local(&mut self, input: Input) -> Range<u32> {
        let start = self.next_local.max(self.confirmed_tick);
        let end = start.max(self.tick.saturating_add(self.delay).saturating_add(1));
        for tick in start..end {
            self.local_inputs.insert(tick, input);
        }
        self.next_local = end;
        start..end
    }

    /// Records the server's frame for `tick`: the input of each player, by id, and who was
    /// late. Returns whether it was accepted: frames of another session, and any frame but the
    /// next one (the server sends them in order), are ignored. The round shown catches up at
    /// the next [`Session::advance`].
    pub fn receive_frame(
        &mut self,
        session: u8,
        tick: u32,
        inputs: &FrameInputs,
        late: PlayerSet,
    ) -> bool {
        if session != self.id || tick != self.confirmed_tick {
            return false;
        }
        let mut known = [Input::NONE; PlayerSet::CAPACITY];
        for player in self.players.iter() {
            let index = usize::from(player.0);
            known[index] = inputs[index];
            if late.contains(player) {
                self.lagging_until[index] = tick + 1 + LAG_MEMORY;
            }
        }
        if tick == self.tick && self.mispredicted {
            // The frames reach the round shown: the confirmed match is what it should be.
            self.current.clone_from(&self.confirmed);
            self.predicted.clear();
            self.mispredicted = false;
        }
        play(&mut self.confirmed, self.players, &known);
        self.confirmed_tick += 1;
        self.local_inputs.remove(&tick);
        if tick < self.tick {
            if self.predicted.pop_front() != Some(known) {
                self.mispredicted = true;
            }
        } else {
            self.unplayed.push_back(known);
        }
        self.latest = known;
        true
    }

    /// Moves the round shown one tick on: with the server's frame of that tick when it came
    /// already (on a fast network, or in a local match where every input is known at once),
    /// otherwise by guessing, after correcting the guesses the frames received since the last
    /// call proved wrong. Returns whether it moved: it waits once it is [`MAX_ROLLBACK`] ticks
    /// ahead of the server. When it trails the frames by more than that (the page was paused),
    /// it jumps to them at once.
    pub fn advance(&mut self) -> bool {
        if self.tick < self.confirmed_tick {
            if self.confirmed_tick - self.tick > MAX_ROLLBACK {
                self.current.clone_from(&self.confirmed);
                self.tick = self.confirmed_tick;
                self.unplayed.clear();
            } else {
                let known = self
                    .unplayed
                    .pop_front()
                    .expect("one frame per tick between the round shown and the confirmed one");
                play(&mut self.current, self.players, &known);
                self.tick += 1;
            }
            return true;
        }
        if self.mispredicted {
            self.replay();
        }
        if self.tick - self.confirmed_tick >= MAX_ROLLBACK {
            return false;
        }
        let guess = self.guess(self.tick);
        play(&mut self.current, self.players, &guess);
        self.predicted.push_back(guess);
        self.tick += 1;
        true
    }

    /// Plays the predicted ticks again from the confirmed match, with the latest guesses.
    fn replay(&mut self) {
        self.current.clone_from(&self.confirmed);
        self.predicted.clear();
        for tick in self.confirmed_tick..self.tick {
            let guess = self.guess(tick);
            play(&mut self.current, self.players, &guess);
            self.predicted.push_back(guess);
        }
        self.mispredicted = false;
    }

    /// The inputs expected for `tick`: the local one when scheduled, otherwise what everybody
    /// held in the last frame.
    fn guess(&self, tick: u32) -> FrameInputs {
        let mut inputs = self.latest;
        if let Some(&input) = self.local_inputs.get(&tick) {
            inputs[usize::from(self.local.0)] = input;
        }
        inputs
    }
}

/// One tick of `game` with the inputs of `players`.
fn play(game: &mut Match, players: PlayerSet, inputs: &FrameInputs) {
    let inputs: HashMap<PlayerId, Input> = players
        .iter()
        .map(|player| (player, inputs[usize::from(player.0)]))
        .collect();
    game.step(&inputs);
}

/// The player who won `game`, if it is over with a winner (team `i` is player `i`).
fn winner(game: &Match) -> Option<PlayerId> {
    match game.outcome() {
        Some(Outcome::Winner(team)) => Some(PlayerId(team.0)),
        _ => None,
    }
}
