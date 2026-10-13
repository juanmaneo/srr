//! The binary messages exchanged between the browsers and the relay server.
//!
//! Every message is one WebSocket binary frame: a tag byte followed by fixed-size fields,
//! integers in little-endian order. A set of players ([`PlayerSet`]) is one byte, bit `i` for
//! player `i`.
//!
//! | tag | message                           | direction | fields                                       |
//! | --- | --------------------------------- | --------- | -------------------------------------------- |
//! | 1   | [`ServerMessage::Welcome`]        | to client | player `u8`                                  |
//! | 2   | [`ServerMessage::Start`]          | to client | session `u8`, players `u8`                   |
//! | 3   | [`ClientMessage::Input`]          | to server | session `u8`, tick `u32`, input `u16`        |
//! | 3   | [`ServerMessage::Input`]          | to client | session `u8`, player `u8`, tick `u32`, input `u16` |
//! | 4   | [`ServerMessage::PlayerLeft`]     | to client | player `u8`                                  |
//! | 5   | [`ServerMessage::RoomFull`]       | to client | —                                            |
//! | 6   | [`ServerMessage::Lobby`]          | to client | seated `u8`, ready `u8`                      |
//! | 7   | [`ClientMessage::Ready`]          | to server | —                                            |

use std::fmt;

use super::players::PlayerSet;
use crate::game::{Input, PlayerId};

const TAG_WELCOME: u8 = 1;
const TAG_START: u8 = 2;
const TAG_INPUT: u8 = 3;
const TAG_PLAYER_LEFT: u8 = 4;
const TAG_ROOM_FULL: u8 = 5;
const TAG_LOBBY: u8 = 6;
const TAG_READY: u8 = 7;

/// Length of an input sent by a client: tag, session, tick and input bits.
const CLIENT_INPUT_LENGTH: usize = 1 + 1 + 4 + 2;
/// Length of an input relayed by the server: the client's fields plus the player.
const SERVER_INPUT_LENGTH: usize = CLIENT_INPUT_LENGTH + 1;

/// Why bytes could not be decoded into a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The message has no bytes at all.
    Empty,
    /// The first byte is not the tag of a message this side can receive.
    UnknownTag(u8),
    /// The message has the wrong size for its tag.
    WrongLength {
        /// The tag of the message.
        tag: u8,
        /// The size the tag requires.
        expected: usize,
        /// The size that was received.
        actual: usize,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Empty => write!(f, "empty message"),
            DecodeError::UnknownTag(tag) => write!(f, "unknown message tag {tag}"),
            DecodeError::WrongLength {
                tag,
                expected,
                actual,
            } => write!(
                f,
                "message with tag {tag} must be {expected} bytes long, got {actual}"
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

/// What a browser sends to the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientMessage {
    /// The sender's input for `tick` of session `session`.
    Input {
        /// The session the input belongs to (see [`ServerMessage::Start`]).
        session: u8,
        /// The tick the input is for.
        tick: u32,
        /// What the sender holds on that tick.
        input: Input,
    },
    /// The sender is ready for the next match (in the lobby, or once a round is over). The
    /// server starts one when every seated player is ready.
    Ready,
}

impl ClientMessage {
    /// The bytes of one WebSocket binary frame.
    ///
    /// ```
    /// use srr::game::Input;
    /// use srr::net::ClientMessage;
    ///
    /// let message = ClientMessage::Input { session: 1, tick: 42, input: Input::NONE };
    /// assert_eq!(ClientMessage::decode(&message.encode()), Ok(message));
    /// assert_eq!(ClientMessage::Ready.encode(), vec![7]);
    /// ```
    pub fn encode(&self) -> Vec<u8> {
        match *self {
            ClientMessage::Input {
                session,
                tick,
                input,
            } => {
                let mut bytes = vec![TAG_INPUT, session];
                push_tick_input(&mut bytes, tick, input);
                bytes
            }
            ClientMessage::Ready => vec![TAG_READY],
        }
    }

    /// Parses one frame sent by a browser.
    ///
    /// # Errors
    ///
    /// Returns a [`DecodeError`] when the bytes are not a valid client message.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        match tag_of(bytes)? {
            TAG_INPUT => {
                let [_, session, t0, t1, t2, t3, i0, i1] = fixed::<CLIENT_INPUT_LENGTH>(bytes)?;
                let (tick, input) = tick_input([t0, t1, t2, t3], [i0, i1]);
                Ok(ClientMessage::Input {
                    session,
                    tick,
                    input,
                })
            }
            TAG_READY => fixed::<1>(bytes).map(|_| ClientMessage::Ready),
            tag => Err(DecodeError::UnknownTag(tag)),
        }
    }
}

/// What the server sends to a browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerMessage {
    /// Sent once on connection: the receiver sits in seat `player` and controls that fighter.
    Welcome {
        /// The fighter the receiver controls.
        player: PlayerId,
    },
    /// Every seated player is ready: start a new match, numbered `session`, between `players`.
    Start {
        /// Identifies the match; inputs of other sessions must be ignored.
        session: u8,
        /// Who fights, everybody for themselves.
        players: PlayerSet,
    },
    /// The input of `player` for `tick` of session `session`.
    Input {
        /// The session the input belongs to.
        session: u8,
        /// Whose input it is.
        player: PlayerId,
        /// The tick the input is for.
        tick: u32,
        /// What the player holds on that tick.
        input: Input,
    },
    /// `player` disconnected; a match they played in cannot go on.
    PlayerLeft {
        /// The player who left.
        player: PlayerId,
    },
    /// Every seat is taken; the server closes the connection.
    RoomFull,
    /// Who sits in the room and who is ready, sent to everybody whenever it changes.
    Lobby {
        /// The occupied seats.
        seated: PlayerSet,
        /// The seated players waiting for the next match.
        ready: PlayerSet,
    },
}

impl ServerMessage {
    /// The bytes of one WebSocket binary frame.
    ///
    /// ```
    /// use srr::game::PlayerId;
    /// use srr::net::ServerMessage;
    ///
    /// let message = ServerMessage::Welcome { player: PlayerId(1) };
    /// assert_eq!(message.encode(), vec![1, 1]);
    /// assert_eq!(ServerMessage::decode(&[1, 1]), Ok(message));
    /// ```
    pub fn encode(&self) -> Vec<u8> {
        match *self {
            ServerMessage::Welcome { player } => vec![TAG_WELCOME, player.0],
            ServerMessage::Start { session, players } => {
                vec![TAG_START, session, players.bits()]
            }
            ServerMessage::Input {
                session,
                player,
                tick,
                input,
            } => {
                let mut bytes = vec![TAG_INPUT, session, player.0];
                push_tick_input(&mut bytes, tick, input);
                bytes
            }
            ServerMessage::PlayerLeft { player } => vec![TAG_PLAYER_LEFT, player.0],
            ServerMessage::RoomFull => vec![TAG_ROOM_FULL],
            ServerMessage::Lobby { seated, ready } => vec![TAG_LOBBY, seated.bits(), ready.bits()],
        }
    }

    /// Parses one frame sent by the server.
    ///
    /// # Errors
    ///
    /// Returns a [`DecodeError`] when the bytes are not a valid server message.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        match tag_of(bytes)? {
            TAG_WELCOME => {
                let [_, player] = fixed::<2>(bytes)?;
                Ok(ServerMessage::Welcome {
                    player: PlayerId(player),
                })
            }
            TAG_START => {
                let [_, session, players] = fixed::<3>(bytes)?;
                Ok(ServerMessage::Start {
                    session,
                    players: PlayerSet::from_bits(players),
                })
            }
            TAG_INPUT => {
                let [_, session, player, t0, t1, t2, t3, i0, i1] =
                    fixed::<SERVER_INPUT_LENGTH>(bytes)?;
                let (tick, input) = tick_input([t0, t1, t2, t3], [i0, i1]);
                Ok(ServerMessage::Input {
                    session,
                    player: PlayerId(player),
                    tick,
                    input,
                })
            }
            TAG_PLAYER_LEFT => {
                let [_, player] = fixed::<2>(bytes)?;
                Ok(ServerMessage::PlayerLeft {
                    player: PlayerId(player),
                })
            }
            TAG_ROOM_FULL => fixed::<1>(bytes).map(|_| ServerMessage::RoomFull),
            TAG_LOBBY => {
                let [_, seated, ready] = fixed::<3>(bytes)?;
                Ok(ServerMessage::Lobby {
                    seated: PlayerSet::from_bits(seated),
                    ready: PlayerSet::from_bits(ready),
                })
            }
            tag => Err(DecodeError::UnknownTag(tag)),
        }
    }
}

fn tag_of(bytes: &[u8]) -> Result<u8, DecodeError> {
    bytes.first().copied().ok_or(DecodeError::Empty)
}

/// The bytes as an array of exactly `N` bytes; `bytes` is never empty here.
fn fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], DecodeError> {
    bytes.try_into().map_err(|_| DecodeError::WrongLength {
        tag: bytes[0],
        expected: N,
        actual: bytes.len(),
    })
}

fn push_tick_input(bytes: &mut Vec<u8>, tick: u32, input: Input) {
    bytes.extend_from_slice(&tick.to_le_bytes());
    bytes.extend_from_slice(&input.to_bits().to_le_bytes());
}

fn tick_input(tick: [u8; 4], input: [u8; 2]) -> (u32, Input) {
    (
        u32::from_le_bytes(tick),
        Input::from_bits(u16::from_le_bytes(input)),
    )
}
