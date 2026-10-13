//! Online free-for-all over the local network: the wire protocol, lock-step and the client state.
//!
//! Like [`crate::game`], nothing in here touches sockets, clocks or an engine. The browser glue
//! (in `crate::platform`) moves bytes between a WebSocket and a [`Client`], and the relay
//! server (`srr-server`) seats up to [`MAX_PLAYERS`] browsers, starts a match once everybody
//! seated is ready, and forwards every input to the other players.
//!
//! The match is simulated on every machine with deterministic lock-step: every tick, each
//! player sends their input for a tick a few steps ahead ([`INPUT_DELAY`]); a tick is simulated
//! once the inputs of every player for it are known. All machines therefore run the same match
//! from the same inputs without ever sending game state.
//!
//! A [`Client`] can also play alone, without any server: against the idle training dummy
//! ([`Client::practice`]) or against the computer ([`Client::versus_bot`], see
//! [`crate::game::Bot`]).

mod client;
mod lockstep;
mod players;
mod protocol;

pub use client::{Client, ClientError, Rival, Status};
pub use lockstep::{INPUT_DELAY, Session, SessionError};
pub use players::{MAX_PLAYERS, PlayerSet};
pub use protocol::{ClientMessage, DecodeError, ServerMessage};
