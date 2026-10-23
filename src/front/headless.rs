//! The game without a window: a scripted keyboard, a software renderer, no sound card.
//!
//! [`Headless`] does what an engine backend does: it ticks the [`Game`] with the keys held and
//! draws its [`Frame`]. The keys come from code instead of a keyboard, and the frame goes to
//! [`render`] (pixels, PNG, text) instead of a GPU. Nothing is shown on screen
//! and nothing is played (the music is only steered through its [`MusicControl`] handle), so
//! tests and CI can play the real intro and fights end to end.
//!
//! ```
//! use srr::front::{Headless, Screen};
//!
//! let mut game = Headless::practice();
//! game.skip_intro();
//! assert_eq!(game.game().screen(), Screen::Fight);
//! assert!(game.text_snapshot(160, 45).contains("P2"), "the training dummy");
//! game.tap(&[srr::game::Key::T]);
//! assert!(game.text_snapshot(160, 45).contains("CPU"), "the computer took over");
//! ```
//!
//! [`MusicControl`]: crate::audio::MusicControl

use std::collections::HashSet;

use super::render::{self, Pixmap};
use super::{Frame, Game};
use crate::game::{Character, Key, MatchConfig};
use crate::net::Client;

/// Size of the image rendered by [`Headless::screenshot`] unless changed with
/// [`Headless::with_size`]: the virtual screen, 1:1.
pub const DEFAULT_SIZE: (usize, usize) = (1280, 720);

/// A [`Game`] driven by code and rendered in memory.
#[derive(Debug, Clone)]
pub struct Headless {
    game: Game,
    width: usize,
    height: usize,
    ticks: u64,
}

impl Headless {
    /// Runs `game`, rendering at [`DEFAULT_SIZE`].
    pub fn new(game: Game) -> Self {
        Headless {
            game,
            width: DEFAULT_SIZE.0,
            height: DEFAULT_SIZE.1,
            ticks: 0,
        }
    }

    /// A practice game against the training dummy with the default rules, like a native build.
    pub fn practice() -> Self {
        Headless::new(Game::new(Client::practice(MatchConfig::default())))
    }

    /// The same game rendered at `width` x `height` (a "window" size, letterboxed).
    pub fn with_size(self, width: usize, height: usize) -> Self {
        Headless {
            width,
            height,
            ..self
        }
    }

    /// The size [`Headless::screenshot`] renders at.
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// The game being played.
    pub fn game(&self) -> &Game {
        &self.game
    }

    /// The game being played, e.g. to hand its client what a server sent.
    pub fn game_mut(&mut self) -> &mut Game {
        &mut self.game
    }

    /// Fixed ticks played so far.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// One fixed tick with `keys` held. Returns the frames the client wants to send.
    pub fn press(&mut self, keys: &[Key]) -> Vec<Vec<u8>> {
        let pressed: HashSet<Key> = keys.iter().copied().collect();
        self.ticks += 1;
        self.game.tick(&pressed)
    }

    /// `ticks` fixed ticks with `keys` held; returns every frame to send, in order.
    pub fn hold(&mut self, keys: &[Key], ticks: u32) -> Vec<Vec<u8>> {
        (0..ticks).flat_map(|_| self.press(keys)).collect()
    }

    /// `ticks` fixed ticks with no key held.
    pub fn idle(&mut self, ticks: u32) -> Vec<Vec<u8>> {
        self.hold(&[], ticks)
    }

    /// Presses `keys` for one tick, then releases them for one tick.
    pub fn tap(&mut self, keys: &[Key]) -> Vec<Vec<u8>> {
        let mut sent = self.hold(keys, 1);
        sent.extend(self.idle(1));
        sent
    }

    /// Waits until the intro can be skipped, then taps Enter twice: past the title and the
    /// select screen (keeping the character chosen), the fight screen is up. Does nothing more
    /// than taps of Enter when the fight screen is already shown.
    pub fn skip_intro(&mut self) {
        while !self.game.intro().can_skip() {
            self.press(&[]);
        }
        self.tap(&[Key::Enter]);
        self.tap(&[Key::Enter]);
    }

    /// Like [`Headless::skip_intro`], picking `character` on the select screen.
    pub fn skip_intro_as(&mut self, character: Character) {
        while !self.game.intro().can_skip() {
            self.press(&[]);
        }
        self.tap(&[Key::Enter]);
        while self.game.select().selected() != character {
            self.tap(&[Key::ArrowRight]);
        }
        self.tap(&[Key::Enter]);
    }

    /// What an engine would draw now.
    pub fn frame(&self) -> Frame {
        self.game.frame()
    }

    /// The current frame rendered as a window of [`Headless::size`] would show it.
    pub fn screenshot(&self) -> Pixmap {
        render::screenshot(&self.frame(), self.width, self.height)
    }

    /// The current frame as `rows` lines of `columns` characters, see
    /// [`render::text_snapshot`].
    pub fn text_snapshot(&self, columns: usize, rows: usize) -> String {
        render::text_snapshot(&self.frame(), columns, rows)
    }
}
