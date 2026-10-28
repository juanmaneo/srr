//! The engine-independent client shell: the [`Game`] wired to the browser's WebSocket and to
//! the audio output.
//!
//! An engine backend ([`crate::engine`]) owns one [`Shell`], calls [`Shell::tick`] once per
//! fixed tick with the keys held, and draws [`Shell::frame`]. Like the backends, this module
//! only moves bytes and samples around; it is checked by playing and excluded from the
//! coverage gate.
//!
//! In the browser the shell talks to the relay server; add `?practice` to the URL to play
//! alone (against the training dummy, or the computer: `T` switches), or `?spectate` to
//! watch the room without playing (the page for a TV, say). Native builds always play
//! alone.
//!
//! Natively the music starts with the intro. In the browser it starts on the first key press,
//! click or touch, from inside the DOM event (see `web::AudioUnlock`), because of the autoplay
//! policy; the intro shows which of the two applies ([`crate::front::SoundState`]).

use std::collections::HashSet;

use crate::front::{Frame, Game, GamepadState};
use crate::game::{Key, MatchConfig};
use crate::net::Client;

pub mod sound;
#[cfg(target_arch = "wasm32")]
mod web;
/// The running game as seen by an engine backend.
pub struct Shell {
    game: Game,
    /// The layout when the game was last ticked: saved to localStorage once it changes
    /// (browser only — there is nowhere to save it natively).
    #[cfg(target_arch = "wasm32")]
    saved_bindings: crate::game::KeyBindings,
    #[cfg(target_arch = "wasm32")]
    connection: Option<web::Connection>,
    /// Owns the music once a gesture started it.
    #[cfg(target_arch = "wasm32")]
    audio: Option<web::AudioUnlock>,
    /// Kept alive while the music plays; dropping it stops the music.
    #[cfg(not(target_arch = "wasm32"))]
    _sound: Option<sound::Sound>,
}

impl std::fmt::Debug for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shell")
            .field("game", &self.game)
            .finish_non_exhaustive()
    }
}

impl Default for Shell {
    fn default() -> Self {
        Shell::new()
    }
}

impl Shell {
    /// Practice natively and with `?practice`; otherwise connects to the server that served
    /// the page.
    pub fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let (client, connection) = if web::practice_requested() {
                (Client::practice(MatchConfig::default()), None)
            } else {
                web::connect()
            };
            let mut game = Game::new(client);
            let saved = web::saved_bindings();
            if let Some(bindings) = saved {
                game.set_bindings(bindings);
            }
            let audio = web::AudioUnlock::install(game.music().clone());
            if audio.is_none() {
                game.set_sound(crate::front::SoundState::Unavailable);
            }
            Shell {
                saved_bindings: *game.bindings(),
                game,
                connection,
                audio,
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::front::SoundState;

            let mut game = Game::new(Client::practice(MatchConfig::default()));
            let sound = sound::Sound::start(game.music().clone());
            game.set_sound(if sound.is_some() {
                SoundState::Playing
            } else {
                SoundState::Unavailable
            });
            Shell {
                game,
                _sound: sound,
            }
        }
    }

    /// One fixed tick: reads the network, advances the game with the keys held, the fingers
    /// on the screen (virtual screen coordinates, see
    /// [`Viewport::to_virtual`](crate::front::Viewport::to_virtual)) and the state of the
    /// gamepads, and sends this tick's input.
    pub fn tick(&mut self, pressed: &HashSet<Key>, touches: &[(f32, f32)], gamepads: GamepadState) {
        #[cfg(target_arch = "wasm32")]
        if *self.game.bindings() != self.saved_bindings {
            web::save_bindings(self.game.bindings());
            self.saved_bindings = *self.game.bindings();
        }
        self.game.set_touches(touches);
        self.game.set_gamepad(gamepads);
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(connection) = &self.connection {
                connection.receive(&mut self.game);
            }
            if let Some(audio) = &self.audio {
                self.game.set_sound(audio.state());
            }
        }

        let outgoing = self.game.tick(pressed);

        #[cfg(target_arch = "wasm32")]
        if let Some(connection) = &self.connection {
            for frame in &outgoing {
                connection.send(frame);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        drop(outgoing);
    }

    /// What to draw now.
    pub fn frame(&self) -> Frame {
        self.game.frame()
    }
}
