//! The browser side of an online match as a pure state machine.
//!
//! The glue feeds it the frames received from the server and calls [`Client::tick`] once per
//! fixed tick; it returns the frames to send. [`Client::ready`] tells the server the player
//! wants to fight (in the lobby, or once a round is over).
//!
//! A local client plays without any server, as player 1 against player 2, a [`Bot`] chosen
//! among the [`Rival`]s: the idle training dummy ([`Client::practice`]) or the computer
//! ([`Client::versus`]), which can take over from each other in the middle of a round
//! ([`Client::set_rival`]).

use std::cmp::Ordering;
use std::collections::VecDeque;
use std::fmt;

use super::players::PlayerSet;
use super::protocol::{ClientMessage, DecodeError, ServerMessage};
use super::rollback::{MAX_ROLLBACK, Session, SessionError};
use crate::game::{Bot, Input, MatchConfig, PlayerId, Rival, TICKS_PER_SECOND};

/// The fighter played by the bot of a local client.
const RIVAL: PlayerId = PlayerId(1);

/// The margin, in ticks, an online client keeps between its inputs reaching the server and
/// the server giving up on them (see [`ServerMessage::Timing`]). The server reports the worst
/// margin of a while, so jitter is already accounted for.
pub const TARGET_LEAD: i32 = 2;
/// Extra margin tolerated above [`TARGET_LEAD`] before slowing down: more margin is harmless,
/// it only makes the predictions longer.
const LEAD_SLACK: i32 = 4;
/// The most ticks an online client delays its inputs (half a second), on the worst
/// connections.
pub const MAX_INPUT_DELAY: u32 = TICKS_PER_SECOND / 2;
/// How many ticks the round shown may run ahead of the server's frames before a client that
/// has to send its inputs earlier delays them instead of running further ahead: past that the
/// others' moves would be guessed for too long, and corrected too visibly.
const PREDICTION_BUDGET: u32 = MAX_ROLLBACK / 2;

/// Where the connection is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Waiting for the server to say which seat we have.
    Connecting,
    /// Seated in the room, between matches: alone, waiting for the others to get ready, or
    /// after a player left the match (which stays visible).
    Lobby,
    /// A match is running (or its round just finished).
    Playing,
    /// Every seat was already taken.
    RoomFull,
    /// The connection to the server was lost.
    Disconnected,
    /// The server sent something this client cannot handle; the connection is useless.
    Failed(ClientError),
}

/// A protocol violation seen by the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientError {
    /// A frame could not be decoded.
    Decode(DecodeError),
    /// The server started a match before telling which seat we have.
    StartBeforeWelcome,
    /// The match could not be created.
    Session(SessionError),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Decode(error) => write!(f, "bad message from the server: {error}"),
            ClientError::StartBeforeWelcome => {
                write!(f, "the server started a match before assigning a seat")
            }
            ClientError::Session(error) => write!(f, "cannot start the match: {error}"),
        }
    }
}

impl std::error::Error for ClientError {}

#[derive(Debug, Clone)]
enum Mode {
    Online,
    /// Alone: `bot` plays [`RIVAL`]; `rival` says which one it is, to spawn a fresh one for
    /// the next round.
    Local {
        rival: Rival,
        bot: Box<dyn Bot>,
    },
}

/// One browser's view of the online match.
///
/// ```
/// use srr::game::{Input, MatchConfig, PlayerId};
/// use srr::net::{Client, PlayerSet, ServerMessage, Status, INPUT_DELAY};
///
/// let mut client = Client::online(MatchConfig::default(), INPUT_DELAY);
/// client.receive(&ServerMessage::Welcome { player: PlayerId(2) }.encode());
/// assert_eq!(client.status(), Status::Lobby);
/// assert!(client.ready().is_some(), "Enter in the lobby says we are ready");
///
/// client.receive(&ServerMessage::Start { session: 1, players: PlayerSet::first(3) }.encode());
/// assert_eq!(client.status(), Status::Playing);
/// let to_send = client.tick(Input::NONE);
/// assert_eq!(to_send.len(), 1 + INPUT_DELAY as usize, "the first inputs go to the server");
/// assert_eq!(client.session().unwrap().tick(), 1, "played ahead, without waiting");
/// ```
#[derive(Debug, Clone)]
pub struct Client {
    config: MatchConfig,
    delay: u32,
    mode: Mode,
    player: Option<PlayerId>,
    session: Option<Session>,
    status: Status,
    stalled: u32,
    /// Ticks to run twice (positive) or to skip (negative) to keep in time with the server.
    pace: i32,
    /// Ticks of margin gained (or given back) towards the server since the match started, by
    /// running ticks twice, skipping them, or changing the input delay.
    gained: i32,
    /// When `gained` changed: the first input tick sent with each value, oldest first. A
    /// timing report about an older input does not reflect the later changes yet.
    gains: VecDeque<(u32, i32)>,
    seated: PlayerSet,
    ready: PlayerSet,
    left: Option<PlayerId>,
    departed: PlayerSet,
}

impl Client {
    /// A client that has just opened its connection to the server.
    pub fn online(config: MatchConfig, delay: u32) -> Self {
        Client {
            config,
            delay,
            mode: Mode::Online,
            player: None,
            session: None,
            status: Status::Connecting,
            stalled: 0,
            pace: 0,
            gained: 0,
            gains: VecDeque::new(),
            seated: PlayerSet::EMPTY,
            ready: PlayerSet::EMPTY,
            left: None,
            departed: PlayerSet::EMPTY,
        }
    }

    /// A client playing alone as player 1 against a training dummy that never moves
    /// ([`Rival::Dummy`]). Nothing is ever sent; [`Client::ready`] starts the next round once
    /// one is over.
    pub fn practice(config: MatchConfig) -> Self {
        Client::versus(config, Rival::Dummy)
    }

    /// A client playing alone as player 1 against `rival`, player 2.
    pub fn versus(config: MatchConfig, rival: Rival) -> Self {
        let mut client = Client {
            mode: Mode::Local {
                rival,
                bot: rival.spawn(RIVAL, 0),
            },
            ..Client::online(config, 0)
        };
        client.handle(ServerMessage::Welcome {
            player: PlayerId(0),
        });
        client.handle(ServerMessage::Start {
            session: 0,
            players: PlayerSet::first(2),
        });
        client
    }

    /// Where the connection is.
    pub fn status(&self) -> Status {
        self.status
    }

    /// The rules of every match this client plays.
    pub fn config(&self) -> &MatchConfig {
        &self.config
    }

    /// Whether this client plays alone, without a server.
    pub fn is_local(&self) -> bool {
        matches!(self.mode, Mode::Local { .. })
    }

    /// Who a local client plays against; `None` online.
    pub fn rival(&self) -> Option<Rival> {
        match &self.mode {
            Mode::Online => None,
            Mode::Local { rival, .. } => Some(*rival),
        }
    }

    /// Changes who a local client plays against: the new bot takes over the rival's fighter
    /// at once, in the middle of the round. Nothing changes online, or when `rival` already
    /// plays.
    ///
    /// ```
    /// use srr::game::{MatchConfig, Rival};
    /// use srr::net::Client;
    ///
    /// let mut client = Client::practice(MatchConfig::default());
    /// client.set_rival(Rival::Brawler);
    /// assert_eq!(client.rival(), Some(Rival::Brawler));
    /// assert!(client.bot_player().is_some(), "the computer is in");
    /// ```
    pub fn set_rival(&mut self, wanted: Rival) {
        let seed = self
            .session
            .as_ref()
            .map_or(0, |session| u64::from(session.id()));
        if let Mode::Local { rival, bot } = &mut self.mode
            && *rival != wanted
        {
            *rival = wanted;
            *bot = wanted.spawn(RIVAL, seed);
        }
    }

    /// The player controlled by the computer, if any: the rival of a local client, unless it
    /// is a training dummy (see [`Rival::fights`]).
    pub fn bot_player(&self) -> Option<PlayerId> {
        match &self.mode {
            Mode::Local { rival, bot } if rival.fights() => Some(bot.player()),
            _ => None,
        }
    }

    /// The player controlled here, once the server has said so.
    pub fn player(&self) -> Option<PlayerId> {
        self.player
    }

    /// The current match, if one was started (it stays readable after it ended).
    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// The occupied seats of the room, as last reported by the server.
    pub fn seated(&self) -> PlayerSet {
        self.seated
    }

    /// The seated players waiting for the next match, as last reported by the server.
    pub fn ready_players(&self) -> PlayerSet {
        self.ready
    }

    /// The player whose departure ended the last match, until the next one starts.
    pub fn left_player(&self) -> Option<PlayerId> {
        self.left
    }

    /// The players of the current match who left it; their fighters stand still.
    pub fn departed(&self) -> PlayerSet {
        self.departed
    }

    /// Consecutive [`Client::tick`] calls, while playing, that could not move the match on
    /// because nothing came from the server for too long (see
    /// [`MAX_ROLLBACK`](super::MAX_ROLLBACK)).
    pub fn stalled_ticks(&self) -> u32 {
        self.stalled
    }

    /// The ticks still to run twice (positive) or to skip (negative) to keep in time with the
    /// server, as asked by its last [`ServerMessage::Timing`].
    pub fn pace(&self) -> i32 {
        self.pace
    }

    /// Handles one binary frame from the server. Nothing changes once the client failed or
    /// was disconnected, even when the frame is garbage.
    pub fn receive(&mut self, bytes: &[u8]) {
        match ServerMessage::decode(bytes) {
            Ok(message) => self.handle(message),
            Err(error) => self.fail(ClientError::Decode(error)),
        }
    }

    /// Handles one decoded message from the server. Nothing changes once the client failed
    /// or was disconnected.
    pub fn handle(&mut self, message: ServerMessage) {
        if self.is_over() {
            return;
        }
        match message {
            ServerMessage::Welcome { player } => {
                self.player = Some(player);
                self.status = Status::Lobby;
            }
            ServerMessage::Start { session, players } => self.start(session, players),
            ServerMessage::Frame {
                session,
                tick,
                players,
                late,
                inputs,
            } => {
                if let Some(current) = self.session.as_mut().filter(|s| s.players() == players) {
                    current.receive_frame(session, tick, &inputs, late);
                }
            }
            ServerMessage::PlayerLeft { player } => self.player_left(player),
            ServerMessage::RoomFull => self.status = Status::RoomFull,
            ServerMessage::Lobby { seated, ready } => {
                self.seated = seated;
                self.ready = ready;
            }
            ServerMessage::Timing { tick, lead } => {
                if self.status == Status::Playing {
                    self.pace = pace_for(i32::from(lead) + self.gained_since(tick));
                }
            }
        }
    }

    /// A player of the match left: their fighter stands still from now on, and the match goes
    /// on unless fewer than two players remain.
    fn player_left(&mut self, player: PlayerId) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if self.status != Status::Playing || !session.players().contains(player) {
            return;
        }
        self.departed = self.departed.with(player);
        if session.players().len() - self.departed.len() < 2 {
            self.status = Status::Lobby;
            self.left = Some(player);
        }
    }

    /// The margin gained since the input for `tick` was sent; forgets the changes no later
    /// report can be about (the reports come in tick order).
    fn gained_since(&mut self, tick: u32) -> i32 {
        while self.gains.get(1).is_some_and(|&(start, _)| start <= tick) {
            self.gains.pop_front();
        }
        let then = match self.gains.front() {
            Some(&(start, gained)) if start <= tick => gained,
            _ => 0,
        };
        self.gained - then
    }

    /// The connection was closed or broke.
    pub fn disconnected(&mut self) {
        if !matches!(self.status, Status::Failed(_) | Status::RoomFull) {
            self.status = Status::Disconnected;
        }
    }

    /// One fixed tick: schedules the local `input` and moves the match one tick on (see
    /// [`Session::advance`]). Returns the frames to send to the server, in order. Does nothing
    /// unless playing.
    ///
    /// To keep in time with the server ([`Client::pace`]), an online client sometimes plays two
    /// ticks at once (with the same input), or none. Far from the server, where running ahead
    /// would mean guessing the others for too long, it delays its inputs one more tick instead
    /// (up to [`MAX_INPUT_DELAY`]); once the connection gets better, it gives that delay back
    /// before skipping ticks.
    pub fn tick(&mut self, input: Input) -> Vec<Vec<u8>> {
        let (Status::Playing, Some(session)) = (self.status, self.session.as_mut()) else {
            return Vec::new();
        };
        let ahead = session.tick().saturating_sub(session.confirmed_tick());
        let delay = session.delay();
        let runs = match self.pace.cmp(&0) {
            Ordering::Greater if ahead >= PREDICTION_BUDGET && delay < MAX_INPUT_DELAY => {
                session.set_delay(delay + 1);
                1
            }
            Ordering::Greater => 2,
            Ordering::Less if delay > self.delay => {
                session.set_delay(delay - 1);
                1
            }
            Ordering::Less => 0,
            Ordering::Equal => 1,
        };
        self.gained += self.pace.signum();
        self.pace -= self.pace.signum();
        let id = session.id();
        let mut outgoing = Vec::new();
        for _ in 0..runs {
            let ticks = session.submit_local(input);
            let recorded = self.gains.back().map_or(0, |&(_, gained)| gained);
            if !ticks.is_empty() && recorded != self.gained {
                self.gains.push_back((ticks.start, self.gained));
            }
            match &mut self.mode {
                Mode::Online => outgoing.extend(ticks.map(|tick| {
                    ClientMessage::Input {
                        session: id,
                        tick,
                        input,
                    }
                    .encode()
                })),
                Mode::Local { bot, .. } => {
                    // Everybody's input is known at once: the client is its own server.
                    for tick in ticks {
                        let mut inputs = [Input::NONE; PlayerSet::CAPACITY];
                        inputs[usize::from(session.local_player().0)] = input;
                        inputs[usize::from(RIVAL.0)] = bot.think(session.current());
                        session.receive_frame(id, tick, &inputs, PlayerSet::EMPTY);
                    }
                }
            }
            if session.advance() {
                self.stalled = 0;
            } else {
                // Nothing is sent while waiting: the inputs lose a tick of margin.
                self.stalled = self.stalled.saturating_add(1);
                self.gained -= 1;
            }
        }
        outgoing
    }

    /// The player wants to fight (Enter). Online, in the lobby or once the round is over, this
    /// returns the frame telling the server; the server starts the next match when everybody
    /// seated is ready. A local client starts its next round at once, once the round is over,
    /// with a fresh bot of the same [`Rival`] (seeded by the round).
    pub fn ready(&mut self) -> Option<Vec<u8>> {
        let finished = self.session.as_ref().is_some_and(Session::is_finished);
        match &mut self.mode {
            Mode::Local { rival, bot } => {
                if let Some(session) = self.session.as_mut().filter(|_| finished) {
                    *session = session.next(session.id().wrapping_add(1));
                    *bot = rival.spawn(RIVAL, u64::from(session.id()));
                }
                None
            }
            Mode::Online => match self.status {
                Status::Lobby => Some(ClientMessage::Ready.encode()),
                Status::Playing if finished => Some(ClientMessage::Ready.encode()),
                _ => None,
            },
        }
    }

    fn start(&mut self, id: u8, players: PlayerSet) {
        let Some(player) = self.player else {
            self.fail(ClientError::StartBeforeWelcome);
            return;
        };
        let rematch = self
            .session
            .as_ref()
            .filter(|old| old.players() == players && old.local_player() == player)
            .map(|old| old.next(id));
        let created = match rematch {
            Some(session) => Ok(session),
            None => Session::new(player, players, id, self.config.clone(), self.delay),
        };
        match created {
            Ok(session) => {
                self.session = Some(session);
                self.status = Status::Playing;
                self.stalled = 0;
                self.pace = 0;
                self.gained = 0;
                self.gains.clear();
                self.left = None;
                self.departed = PlayerSet::EMPTY;
                // Starting a match uses up everybody's readiness, on the server too.
                self.ready = PlayerSet::EMPTY;
            }
            Err(error) => self.fail(ClientError::Session(error)),
        }
    }

    /// Whether the connection is unusable: later frames change nothing.
    fn is_over(&self) -> bool {
        matches!(self.status, Status::Failed(_) | Status::Disconnected)
    }

    /// Records the first protocol violation; a client that is already over stays as it is.
    fn fail(&mut self, error: ClientError) {
        if !self.is_over() {
            self.status = Status::Failed(error);
        }
    }
}

/// The ticks to run twice (positive) or to skip (negative) when the inputs sent now should
/// arrive `lead` ticks before the server gives up on them: half the distance to
/// [`TARGET_LEAD`] (the network may have changed since the report), nothing within the
/// tolerated margin.
fn pace_for(lead: i32) -> i32 {
    if lead < TARGET_LEAD {
        (TARGET_LEAD - lead + 1) / 2
    } else if lead > TARGET_LEAD + LEAD_SLACK {
        -((lead - TARGET_LEAD) / 2)
    } else {
        0
    }
}
