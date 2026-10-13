//! The engine backends. One of them is compiled in, chosen with a Cargo feature:
//!
//! | Feature           | Backend          | Command                                              |
//! | ----------------- | ---------------- | ---------------------------------------------------- |
//! | `bevy` (default)  | [Bevy] 0.15      | `cargo run`                                          |
//! | `fyrox`           | [Fyrox] 1.0      | `cargo run --no-default-features --features fyrox`   |
//!
//! A backend is deliberately dumb. It implements [`Engine`]: open a window, call
//! [`Shell::tick`] [`TICKS_PER_SECOND`](crate::game::TICKS_PER_SECOND) times per second with the
//! keys held (mapped to [`Key`](crate::game::Key)s), and draw [`Shell::frame`] — a list of
//! rectangles and monospace labels on a virtual screen (see [`crate::front`]). The rules, the
//! screens, the HUD, the intro, the network and the music are all shared, so switching engines
//! changes nothing else. These files are checked by playing and excluded from the coverage gate.
//!
//! Without the `client` feature there is still a third, windowless way to run the game:
//! [`Headless`](crate::front::Headless) ticks the same game with scripted keys and draws its
//! frames with the software renderer of [`crate::front::render`]. The tests use it.
//!
//! When both features are enabled (e.g. `--all-features`), Bevy wins.
//!
//! [Bevy]: https://bevyengine.org
//! [Fyrox]: https://fyrox.rs

use crate::platform::Shell;

#[cfg(feature = "bevy")]
mod bevy;
#[cfg(feature = "fyrox")]
mod fyrox;

/// An engine able to run the game.
pub trait Engine {
    /// Human-readable name of the engine.
    const NAME: &'static str;

    /// Opens the window and runs `shell` until the window is closed.
    fn run(shell: Shell);
}

/// The engine selected by the Cargo features.
#[cfg(feature = "bevy")]
pub type Selected = bevy::Bevy;
/// The engine selected by the Cargo features.
#[cfg(all(feature = "fyrox", not(feature = "bevy")))]
pub type Selected = fyrox::Fyrox;

#[cfg(not(any(feature = "bevy", feature = "fyrox")))]
compile_error!("the `client` feature needs an engine: enable `bevy` (default) or `fyrox`");

/// Title of the window.
pub const WINDOW_TITLE: &str = "Shattered Realm: Rampage";
/// The canvas of `web/index.html` the game renders into in the browser.
pub const WEB_CANVAS: &str = "#srr-canvas";

/// Runs the game on the selected engine.
pub fn run() {
    <Selected as Engine>::run(Shell::new());
}
