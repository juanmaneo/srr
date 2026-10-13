//! The browser side of an online match as a pure state machine.
//!
//! The glue feeds it the frames received from the server and calls [`Client::tick`] once per
//! fixed tick; it returns the frames to send. [`Client::ready`] tells the server the player
//! wants to fight (in the lobby, or once a round is over).
//!
//! A local client plays without any server, as player 1 against player 2: the idle training
//! dummy ([`Client::practice`]) or the computer ([`Client::versus_bot`]).

use std::fmt;

use super::lockstep::{Session, SessionError};
use super::players::PlayerSet;
use super::protocol::{ClientMessage, DecodeError, ServerMessage};
use crate::game::{Bot, Input, MatchConfig, PlayerId};

/// The fighter played by the dummy or the bot of a local client.
const RIVAL: PlayerId = PlayerId(1);

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

/// Who a local client plays against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rival {
    /// The training dummy: it never moves.
    Dummy,
    /// The computer ([`Bot`]).
    Bot,
}

#[derive(Debug, Clone)]
enum Mode {
    Online,
    /// `None` is the training dummy.
    Local {
        bot: Option<Bot>,
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
/// assert!(to_send.is_some(), "the local input goes to the other players");
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
    seated: PlayerSet,
    ready: PlayerSet,
    left: Option<PlayerId>,
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
            seated: PlayerSet::EMPTY,
            ready: PlayerSet::EMPTY,
            left: None,
        }
    }

    /// A client playing alone as player 1 against a training dummy that never moves. Nothing
    /// is ever sent; [`Client::ready`] starts the next round once one is over.
    pub fn practice(config: MatchConfig) -> Self {
        Client::local(config, None)
    }

    /// A client playing alone as player 1 against the computer (player 2, see [`Bot`]).
    pub fn versus_bot(config: MatchConfig) -> Self {
        Client::local(config, Some(Bot::new(RIVAL, 0)))
    }

    fn local(config: MatchConfig, bot: Option<Bot>) -> Self {
        let mut client = Client {
            mode: Mode::Local { bot },
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
            Mode::Local { bot: None } => Some(Rival::Dummy),
            Mode::Local { bot: Some(_) } => Some(Rival::Bot),
        }
    }

    /// The player controlled by the computer, if any.
    pub fn bot_player(&self) -> Option<PlayerId> {
        match &self.mode {
            Mode::Local { bot: Some(bot) } => Some(bot.player()),
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

    /// Consecutive [`Client::tick`] calls, while playing, that could not simulate anything
    /// because the input of another player was missing.
    pub fn stalled_ticks(&self) -> u32 {
        self.stalled
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
            ServerMessage::Input {
                session,
                player,
                tick,
                input,
            } => {
                if let Some(current) = self.session.as_mut() {
                    current.receive_remote(session, player, tick, input);
                }
            }
            ServerMessage::PlayerLeft { player } => {
                let in_match = self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.players().contains(player));
                if self.status == Status::Playing && in_match {
                    self.status = Status::Lobby;
                    self.left = Some(player);
                }
            }
            ServerMessage::RoomFull => self.status = Status::RoomFull,
            ServerMessage::Lobby { seated, ready } => {
                self.seated = seated;
                self.ready = ready;
            }
        }
    }

    /// The connection was closed or broke.
    pub fn disconnected(&mut self) {
        if !matches!(self.status, Status::Failed(_) | Status::RoomFull) {
            self.status = Status::Disconnected;
        }
    }

    /// One fixed tick: schedules the local `input` and simulates what can be simulated.
    /// Returns the frame to send to the server, if any. Does nothing unless playing.
    pub fn tick(&mut self, input: Input) -> Option<Vec<u8>> {
        if self.status != Status::Playing {
            return None;
        }
        let session = self.session.as_mut()?;
        let message = session.submit_local(input);
        if let (
            Mode::Local { bot },
            Some(ClientMessage::Input {
                session: id, tick, ..
            }),
        ) = (&mut self.mode, message)
        {
            let rival = bot
                .as_mut()
                .map_or(Input::NONE, |bot| bot.think(session.current()));
            session.receive_remote(id, RIVAL, tick, rival);
        }
        if session.advance() == 0 {
            self.stalled = self.stalled.saturating_add(1);
        } else {
            self.stalled = 0;
        }
        message
            .filter(|_| !self.is_local())
            .map(|message| message.encode())
    }

    /// The player wants to fight (Enter). Online, in the lobby or once the round is over, this
    /// returns the frame telling the server; the server starts the next match when everybody
    /// seated is ready. A local client starts its next round at once, once the round is over.
    pub fn ready(&mut self) -> Option<Vec<u8>> {
        let finished = self
            .session
            .as_ref()
            .is_some_and(|session| session.current().outcome().is_some());
        match &mut self.mode {
            Mode::Local { bot } => {
                if let Some(session) = self.session.as_mut().filter(|_| finished) {
                    *session = session.next(session.id().wrapping_add(1));
                    if let Some(bot) = bot {
                        *bot = Bot::new(RIVAL, u64::from(session.id()));
                    }
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
                self.left = None;
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
