//! Shattered Realm: Rampage — a versus fighting game built around intentional glitches.
//!
//! * [`game`]: the engine-agnostic rules (deterministic, fixed-step, fully tested).
//! * [`net`]: the online protocol (up to ten players) and rollback session (engine-agnostic,
//!   fully tested).
//! * [`front`]: the screens — graffiti ASCII intro, arena, versus HUD — drawn as plain
//!   rectangles and labels, plus the music cues (engine-agnostic, fully tested).
//! * [`audio`]: the procedurally generated acid techno soundtrack (pure DSP, fully tested).
//! * `platform` (feature `client`): browser WebSocket, audio output and the [`front`] game
//!   wired together, independent of the engine.
//! * `engine` (feature `bevy` or `fyrox`): the thin backend that opens the window, reads the
//!   keyboard and draws the frames.

pub mod audio;
#[cfg(feature = "client")]
pub mod engine;
pub mod front;
pub mod game;
pub mod net;
#[cfg(feature = "client")]
pub mod platform;
