//! Online free-for-all of up to [`MAX_PLAYERS`] players: the wire protocol, rollback and the
//! client state.
//!
//! Like [`crate::game`], nothing in here touches sockets, clocks or an engine. The browser glue
//! (in `crate::platform`) moves bytes between a WebSocket and a [`Client`], and the relay
//! server (`srr-server`) seats up to [`MAX_PLAYERS`] browsers and starts a match once everybody
//! seated is ready.
//!
//! Every machine simulates the same deterministic match from the same inputs, without ever
//! sending game state, and the network is built so that one bad connection cannot spoil the
//! party for the others:
//!
//! * each browser sends its input for a tick a little ahead ([`INPUT_DELAY`]);
//! * the server gathers the inputs of every player into one frame per tick
//!   ([`ServerMessage::Frame`]), sent as soon as all of them are there; when somebody's input
//!   is late, it stops waiting after a short grace period and repeats their previous input
//!   instead, so only the laggard's fighter hesitates;
//! * each browser shows the match ahead of the frames, guessing the inputs it does not know
//!   yet, and rolls back when a frame proves a guess wrong ([`Session`]);
//! * the server tells each browser how early its inputs arrive ([`ServerMessage::Timing`]),
//!   and the browser runs slightly faster or slower to keep a small margin ([`Client`]); far
//!   from the server it delays its inputs more (up to [`MAX_INPUT_DELAY`]) rather than
//!   guessing the others for too long;
//! * a player who leaves does not end the match for everybody else: their fighter stands still.
//!
//! A [`Client`] can also play alone, without any server: against the idle training dummy
//! ([`Client::practice`]) or against the computer ([`Client::versus_bot`], see
//! [`crate::game::Bot`]).

mod client;
mod players;
mod protocol;
mod rollback;

pub use client::{Client, ClientError, MAX_INPUT_DELAY, Rival, Status, TARGET_LEAD};
pub use players::{MAX_PLAYERS, PlayerSet};
pub use protocol::{ClientMessage, DecodeError, FrameInputs, ServerMessage};
pub use rollback::{INPUT_DELAY, LAG_MEMORY, MAX_ROLLBACK, Session, SessionError};
