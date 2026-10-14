//! The binary messages exchanged between the browsers and the relay server.
//!
//! Every message is one WebSocket binary frame: a tag byte followed by its fields, integers in
//! little-endian order. A set of players ([`PlayerSet`]) is two bytes, bit `i` for player `i`.
//!
//! | tag | message                       | direction | fields                                                   |
//! | --- | ----------------------------- | --------- | -------------------------------------------------------- |
//! | 1   | [`ServerMessage::Welcome`]    | to client | player `u8`                                              |
//! | 2   | [`ServerMessage::Start`]      | to client | session `u8`, players `u16`                              |
//! | 3   | [`ClientMessage::Input`]      | to server | session `u8`, tick `u32`, input `u16`                    |
//! | 4   | [`ServerMessage::PlayerLeft`] | to client | player `u8`                                              |
//! | 5   | [`ServerMessage::RoomFull`]   | to client | —                                                        |
//! | 6   | [`ServerMessage::Lobby`]      | to client | seated `u16`, ready `u16`                                |
//! | 7   | [`ClientMessage::Ready`]      | to server | —                                                        |
//! | 8   | [`ServerMessage::Frame`]      | to client | session `u8`, tick `u32`, players `u16`, late `u16`, then one input `u16` per player, by increasing id |
//! | 9   | [`ServerMessage::Timing`]     | to client | tick `u32`, lead `i8`                                    |

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
const TAG_FRAME: u8 = 8;
const TAG_TIMING: u8 = 9;

/// Length of an input sent by a client: tag, session, tick and input bits.
const INPUT_LENGTH: usize = 1 + 1 + 4 + 2;
/// Length of a frame before its inputs: tag, session, tick, players and late players.
const FRAME_HEADER_LENGTH: usize = 1 + 1 + 4 + 2 + 2;

/// The inputs of one tick, indexed by player id (see [`ServerMessage::Frame`]).
pub type FrameInputs = [Input; PlayerSet::CAPACITY];

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
                bytes.extend_from_slice(&tick.to_le_bytes());
                bytes.extend_from_slice(&input.to_bits().to_le_bytes());
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
                let [_, session, t0, t1, t2, t3, i0, i1] = fixed::<INPUT_LENGTH>(bytes)?;
                Ok(ClientMessage::Input {
                    session,
                    tick: u32::from_le_bytes([t0, t1, t2, t3]),
                    input: Input::from_bits(u16::from_le_bytes([i0, i1])),
                })
            }
            TAG_READY => fixed::<1>(bytes).map(|_| ClientMessage::Ready),
            tag => Err(DecodeError::UnknownTag(tag)),
        }
    }
}

/// What the server sends to a browser.
#[allow(
    clippy::large_enum_variant,
    reason = "a frame of inputs is a few hundred bytes, decoded and handled at once: boxing it would allocate 60 times per second and lose `Copy`"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerMessage {
    /// Sent once on connection: the receiver sits in seat `player` and controls that fighter.
    Welcome {
        /// The fighter the receiver controls.
        player: PlayerId,
    },
    /// Every seated player is ready: start a new match, numbered `session`, between `players`.
    Start {
        /// Identifies the match; frames of other sessions must be ignored.
        session: u8,
        /// Who fights, everybody for themselves.
        players: PlayerSet,
    },
    /// The inputs of every player of session `session` for `tick`, as decided by the server:
    /// sent, in tick order, as soon as every input arrived, or once the server gave up waiting.
    Frame {
        /// The session the frame belongs to.
        session: u8,
        /// The tick the inputs are for.
        tick: u32,
        /// The players of the session: whose inputs the frame carries.
        players: PlayerSet,
        /// The players whose input was late: the server repeated their previous one instead.
        late: PlayerSet,
        /// The input of each player of `players`, by id; the others are not sent (and decode
        /// as [`Input::NONE`]).
        inputs: FrameInputs,
    },
    /// `player` disconnected; their fighter stands still for the rest of the match.
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
    /// How early the receiver's inputs reached the server lately, at worst: the input for
    /// `tick` arrived `lead` ticks before the server would have given up on it (negative: too
    /// late, it was replaced). The client speeds up or slows down to keep a small margin; the
    /// tick tells it which of its corrections the report already reflects.
    Timing {
        /// The tick of the input with the smallest margin since the previous report.
        tick: u32,
        /// Its margin, in ticks.
        lead: i8,
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
                let mut bytes = vec![TAG_START, session];
                push_set(&mut bytes, players);
                bytes
            }
            ServerMessage::Frame {
                session,
                tick,
                players,
                late,
                inputs,
            } => {
                let mut bytes = Vec::with_capacity(FRAME_HEADER_LENGTH + 2 * players.len());
                bytes.extend_from_slice(&[TAG_FRAME, session]);
                bytes.extend_from_slice(&tick.to_le_bytes());
                push_set(&mut bytes, players);
                push_set(&mut bytes, late);
                for player in players.iter() {
                    let input = inputs[usize::from(player.0)];
                    bytes.extend_from_slice(&input.to_bits().to_le_bytes());
                }
                bytes
            }
            ServerMessage::PlayerLeft { player } => vec![TAG_PLAYER_LEFT, player.0],
            ServerMessage::RoomFull => vec![TAG_ROOM_FULL],
            ServerMessage::Lobby { seated, ready } => {
                let mut bytes = vec![TAG_LOBBY];
                push_set(&mut bytes, seated);
                push_set(&mut bytes, ready);
                bytes
            }
            ServerMessage::Timing { tick, lead } => {
                let mut bytes = vec![TAG_TIMING];
                bytes.extend_from_slice(&tick.to_le_bytes());
                bytes.push(lead.to_le_bytes()[0]);
                bytes
            }
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
                let [_, session, p0, p1] = fixed::<4>(bytes)?;
                Ok(ServerMessage::Start {
                    session,
                    players: set([p0, p1]),
                })
            }
            TAG_FRAME => decode_frame(bytes),
            TAG_PLAYER_LEFT => {
                let [_, player] = fixed::<2>(bytes)?;
                Ok(ServerMessage::PlayerLeft {
                    player: PlayerId(player),
                })
            }
            TAG_ROOM_FULL => fixed::<1>(bytes).map(|_| ServerMessage::RoomFull),
            TAG_LOBBY => {
                let [_, s0, s1, r0, r1] = fixed::<5>(bytes)?;
                Ok(ServerMessage::Lobby {
                    seated: set([s0, s1]),
                    ready: set([r0, r1]),
                })
            }
            TAG_TIMING => {
                let [_, t0, t1, t2, t3, lead] = fixed::<6>(bytes)?;
                Ok(ServerMessage::Timing {
                    tick: u32::from_le_bytes([t0, t1, t2, t3]),
                    lead: i8::from_le_bytes([lead]),
                })
            }
            tag => Err(DecodeError::UnknownTag(tag)),
        }
    }
}

/// A frame: the fixed header, then as many inputs as the header has players.
fn decode_frame(bytes: &[u8]) -> Result<ServerMessage, DecodeError> {
    let wrong_length = |expected| DecodeError::WrongLength {
        tag: TAG_FRAME,
        expected,
        actual: bytes.len(),
    };
    let Some((header, rest)) = bytes.split_first_chunk::<FRAME_HEADER_LENGTH>() else {
        return Err(wrong_length(FRAME_HEADER_LENGTH));
    };
    let [_, session, t0, t1, t2, t3, p0, p1, l0, l1] = *header;
    let players = set([p0, p1]);
    if rest.len() != 2 * players.len() {
        return Err(wrong_length(FRAME_HEADER_LENGTH + 2 * players.len()));
    }
    let mut inputs = [Input::NONE; PlayerSet::CAPACITY];
    let (pairs, _) = rest.as_chunks::<2>();
    for (player, &bits) in players.iter().zip(pairs) {
        inputs[usize::from(player.0)] = Input::from_bits(u16::from_le_bytes(bits));
    }
    Ok(ServerMessage::Frame {
        session,
        tick: u32::from_le_bytes([t0, t1, t2, t3]),
        players,
        late: set([l0, l1]),
        inputs,
    })
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

fn push_set(bytes: &mut Vec<u8>, players: PlayerSet) {
    bytes.extend_from_slice(&players.bits().to_le_bytes());
}

fn set(bytes: [u8; 2]) -> PlayerSet {
    PlayerSet::from_bits(u16::from_le_bytes(bytes))
}
