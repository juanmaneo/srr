//! The binary messages exchanged between the browsers and the relay server.
//!
//! Every message is one WebSocket binary frame: a tag byte followed by its fields, integers in
//! little-endian order. A set of players ([`PlayerSet`]) is two bytes, bit `i` for player `i`,
//! and a [`Character`] is its [index](Character::index) in one byte.
//!
//! | tag | message                       | direction | fields                                                   |
//! | --- | ----------------------------- | --------- | -------------------------------------------------------- |
//! | 1   | [`ServerMessage::Welcome`]    | to client | player `u8`                                              |
//! | 2   | [`ServerMessage::Start`]      | to client | session `u8`, players `u16`, then one character `u8` per player, by increasing id |
//! | 3   | [`ClientMessage::Input`]      | to server | session `u8`, tick `u32`, input `u16`                    |
//! | 4   | [`ServerMessage::PlayerLeft`] | to client | player `u8`                                              |
//! | 5   | [`ServerMessage::RoomFull`]   | to client | —                                                        |
//! | 6   | [`ServerMessage::Lobby`]      | to client | seated `u16`, ready `u16`                                |
//! | 7   | [`ClientMessage::Ready`]      | to server | character `u8`                                           |
//! | 8   | [`ServerMessage::Frame`]      | to client | session `u8`, tick `u32`, players `u16`, late `u16`, then one input `u16` per player, by increasing id |
//! | 9   | [`ServerMessage::Timing`]     | to client | tick `u32`, lead `i8`                                    |
//! | 10  | [`ClientMessage::Skin`]        | to server | skin `u8` (see [`Skin::index`](crate::game::Skin::index))  |
//! | 11  | [`ServerMessage::SkinOf`]     | to client | player `u8`, skin `u8`                                    |
//! | 12  | [`ServerMessage::Styles`]     | to client | styles `u8` (a bitmask, see [`StyleSet`](crate::game::StyleSet)) |
//! | 13  | [`ServerMessage::Watching`]   | to client | —                                                        |

use std::fmt;

use super::players::{MAX_PLAYERS, PlayerSet};
use crate::game::{Character, Input, PlayerId, Skin, StyleSet};

const TAG_WELCOME: u8 = 1;
const TAG_START: u8 = 2;
const TAG_INPUT: u8 = 3;
const TAG_PLAYER_LEFT: u8 = 4;
const TAG_ROOM_FULL: u8 = 5;
const TAG_LOBBY: u8 = 6;
const TAG_READY: u8 = 7;
const TAG_FRAME: u8 = 8;
const TAG_TIMING: u8 = 9;
const TAG_SKIN: u8 = 10;
const TAG_SKIN_OF: u8 = 11;
const TAG_STYLES: u8 = 12;
const TAG_WATCHING: u8 = 13;

/// Length of an input sent by a client: tag, session, tick and input bits.
const INPUT_LENGTH: usize = 1 + 1 + 4 + 2;
/// Length of a frame before its inputs: tag, session, tick, players and late players.
const FRAME_HEADER_LENGTH: usize = 1 + 1 + 4 + 2 + 2;

/// The inputs of one tick, indexed by player id (see [`ServerMessage::Frame`]).
pub type FrameInputs = [Input; PlayerSet::CAPACITY];

/// The character of every seat, indexed by player id (see [`ServerMessage::Start`]).
pub type Characters = [Character; MAX_PLAYERS];

/// Why bytes could not be decoded into a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The message has no bytes at all.
    Empty,
    /// The first byte is not the tag of a message this side can receive.
    UnknownTag(u8),
    /// A byte that should name a [`Character`] names none.
    UnknownCharacter(u8),
    /// A byte that should name a [`Skin`] names none.
    UnknownSkin(u8),
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
            DecodeError::UnknownCharacter(index) => write!(f, "unknown character {index}"),
            DecodeError::UnknownSkin(index) => write!(f, "unknown skin {index}"),
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
    /// The sender is ready for the next match (in the lobby, or once a round is over), as
    /// `character`. The server starts one when every seated player is ready.
    Ready {
        /// The character the sender fights as in the next match.
        character: Character,
    },
    /// The sender's fighter wears `skin` from now on; the server tells every seat at once
    /// (see [`ServerMessage::SkinOf`]), so every browser redraws it in the same tick.
    Skin {
        /// The skin worn. The server only lets through the styles it allows (see
        /// [`ServerMessage::Styles`]).
        skin: Skin,
    },
}

impl ClientMessage {
    /// The bytes of one WebSocket binary frame.
    ///
    /// ```
    /// use srr::game::{Character, Input};
    /// use srr::net::ClientMessage;
    ///
    /// let message = ClientMessage::Input { session: 1, tick: 42, input: Input::NONE };
    /// assert_eq!(ClientMessage::decode(&message.encode()), Ok(message));
    /// let ready = ClientMessage::Ready { character: Character::Torch };
    /// assert_eq!(ready.encode(), vec![7, 1]);
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
            ClientMessage::Ready { character } => vec![TAG_READY, character.index()],
            ClientMessage::Skin { skin } => vec![TAG_SKIN, skin.index()],
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
            TAG_READY => {
                let [_, character] = fixed::<2>(bytes)?;
                Ok(ClientMessage::Ready {
                    character: character_of(character)?,
                })
            }
            TAG_SKIN => {
                let [_, skin] = fixed::<2>(bytes)?;
                Ok(ClientMessage::Skin {
                    skin: skin_of(skin)?,
                })
            }
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
        /// The character of each player of `players`, by id; only theirs are sent (the others
        /// decode as [`Character::Chalk`]).
        characters: Characters,
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
    /// `player`'s fighter wears `skin` from now on, in every browser: sent to all the seats
    /// the moment the player picks it, and to a newcomer for every seated player.
    SkinOf {
        /// The player whose skin changed.
        player: PlayerId,
        /// The skin they wear now.
        skin: Skin,
    },
    /// The styles this server allows its players to wear (the bits of
    /// [`StyleSet::bits`](crate::game::StyleSet::bits)); sent once on connection, before any
    /// [`ServerMessage::SkinOf`]. A skin outside the set is never sent.
    Styles {
        /// The allowed styles (the chalk is always allowed, whatever the bits say).
        allowed: StyleSet,
    },
    /// The receiver is a spectator (a watcher of the room, the page asked for it with
    /// `?spectate`): it is told everything the players are told, but nothing it sends is
    /// listened to. It sees the room as an observer, never seated, never ready.
    Watching,
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
            ServerMessage::Start {
                session,
                players,
                characters,
            } => {
                let mut bytes = vec![TAG_START, session];
                push_set(&mut bytes, players);
                bytes.extend(
                    players
                        .iter()
                        .map(|player| character_at(&characters, player).index()),
                );
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
            ServerMessage::SkinOf { player, skin } => vec![TAG_SKIN_OF, player.0, skin.index()],
            ServerMessage::Styles { allowed } => vec![TAG_STYLES, allowed.bits()],
            ServerMessage::Watching => vec![TAG_WATCHING],
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
            TAG_START => decode_start(bytes),
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
            TAG_SKIN_OF => {
                let [_, player, skin] = fixed::<3>(bytes)?;
                Ok(ServerMessage::SkinOf {
                    player: PlayerId(player),
                    skin: skin_of(skin)?,
                })
            }
            TAG_STYLES => {
                let [_, allowed] = fixed::<2>(bytes)?;
                Ok(ServerMessage::Styles {
                    allowed: StyleSet::from_bits(allowed),
                })
            }
            TAG_WATCHING => fixed::<1>(bytes).map(|_| ServerMessage::Watching),
            tag => Err(DecodeError::UnknownTag(tag)),
        }
    }
}

/// A start: the session and the players, then as many characters as there are players.
fn decode_start(bytes: &[u8]) -> Result<ServerMessage, DecodeError> {
    const HEADER: usize = 1 + 1 + 2;
    let wrong_length = |expected| DecodeError::WrongLength {
        tag: TAG_START,
        expected,
        actual: bytes.len(),
    };
    let Some((&[_, session, p0, p1], rest)) = bytes.split_first_chunk::<HEADER>() else {
        return Err(wrong_length(HEADER));
    };
    let players = set([p0, p1]);
    if rest.len() != players.len() {
        return Err(wrong_length(HEADER + players.len()));
    }
    let mut characters = [Character::Chalk; MAX_PLAYERS];
    for (player, &index) in players.iter().zip(rest) {
        let character = character_of(index)?;
        if let Some(slot) = characters.get_mut(usize::from(player.0)) {
            *slot = character;
        }
    }
    Ok(ServerMessage::Start {
        session,
        players,
        characters,
    })
}

/// The character of `player` in `characters` ([`Character::Chalk`] beyond the seats).
pub fn character_at(characters: &Characters, player: PlayerId) -> Character {
    characters
        .get(usize::from(player.0))
        .copied()
        .unwrap_or_default()
}

fn character_of(index: u8) -> Result<Character, DecodeError> {
    Character::from_index(index).ok_or(DecodeError::UnknownCharacter(index))
}

fn skin_of(index: u8) -> Result<Skin, DecodeError> {
    Skin::from_index(index).ok_or(DecodeError::UnknownSkin(index))
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
