//! The character select screen: "CHOOSE YOUR FIGHTER".
//!
//! Shown between the title and the fight. One card per [`Character`], side by side: its name,
//! its special move, a live stick figure that alternates between the guard and the special
//! move (the Torch's card shows the fireball leaving its hands), and bars for its health,
//! speed, power and reach. The chosen character's description runs under the cards.
//!
//! `Left` / `Right` choose, `Enter` picks the character and goes to the fight, `Esc` goes
//! back to the title. The page only remembers the choice; the [`Game`](super::Game) carries
//! it out.

use std::collections::HashMap;

use super::arena::{draw_fireball, feet};
use super::scene::{Align, Frame, Rgba, SCREEN_WIDTH};
use super::stick::{Figure, Point};
use crate::game::{
    AttackPhase, Character, Facing, FighterState, Input, Key, Match, MatchConfig, PlayerId, Roster,
};

/// Width of a card.
pub const CARD_WIDTH: f32 = 280.0;
/// Space between two cards.
pub const CARD_GAP: f32 = 24.0;
/// Top of the cards.
pub const CARD_TOP: f32 = 140.0;
/// Height of a card.
pub const CARD_HEIGHT: f32 = 470.0;
/// Ticks a preview holds each of its two poses (the guard, then the special move).
pub const PREVIEW_TICKS: u32 = 45;

/// Screen y of the feet of the preview figures.
const PREVIEW_FEET: f32 = 400.0;
/// Where the previews stand in the arena, far from anybody.
const PREVIEW_X: i32 = 300;

const TITLE: Rgba = Rgba::hex(0xffd84a);
const HELP: Rgba = Rgba::hex(0x8a8098);
const CARD: Rgba = Rgba::hex(0x15111d);
const CARD_CHOSEN: Rgba = Rgba::hex(0x2a1430);
const BORDER: Rgba = Rgba::hex(0x3a3448);
const BORDER_CHOSEN: Rgba = Rgba::hex(0xffd84a);
const NAME: Rgba = Rgba::hex(0xf4f0ff);
const SPECIAL: Rgba = Rgba::hex(0xff5fd2);
const STAT: Rgba = Rgba::hex(0x8a8098);
const BAR_EMPTY: Rgba = Rgba::hex(0x2a2436);
const BAR_FULL: Rgba = Rgba::hex(0x3fc8ff);
const TAGLINE: Rgba = Rgba::hex(0xd8d0e8);

/// What a key did on the select screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Choice {
    /// Still choosing.
    Browsing,
    /// `Enter`: fight as the chosen character.
    Picked(Character),
    /// `Esc`: back to the title.
    Back,
}

/// A character in its guard and in the middle of its special move, to draw on its card.
#[derive(Debug, Clone)]
struct Preview {
    guard: Match,
    special: Match,
}

impl Preview {
    /// A match where `character` stands alone, before and once its special move is out.
    fn of(character: Character) -> Preview {
        let config = MatchConfig {
            countdown_ticks: 0,
            ..MatchConfig::default()
        }
        .with_character(PlayerId(0), character);
        let guard = Match::with_spawns(Roster::duel(), config, &[PREVIEW_X, 1200])
            .expect("the default rules with spawns inside the arena");
        let mut special = guard.clone();
        let press = HashMap::from([(
            PlayerId(0),
            Input {
                special: true,
                ..Input::NONE
            },
        )]);
        let active = FighterState::Special(AttackPhase::Active);
        while special.fighters()[0].state() != active {
            special.step(&press);
        }
        Preview { guard, special }
    }
}

/// The state of the select screen: the character chosen and the previews' clock.
///
/// ```
/// use srr::front::select::{Choice, SelectPage};
/// use srr::game::{Character, Key};
///
/// let mut page = SelectPage::new(Character::Chalk);
/// assert_eq!(page.tick(&[Key::ArrowRight]), Choice::Browsing);
/// assert_eq!(page.selected(), Character::Torch);
/// assert_eq!(page.tick(&[Key::Enter]), Choice::Picked(Character::Torch));
/// assert_eq!(page.tick(&[Key::Escape]), Choice::Back);
/// ```
#[derive(Debug, Clone)]
pub struct SelectPage {
    selected: Character,
    ticks: u32,
    previews: Vec<Preview>,
}

impl SelectPage {
    /// The page with `selected` chosen.
    pub fn new(selected: Character) -> Self {
        SelectPage {
            selected,
            ticks: 0,
            previews: Character::ALL.into_iter().map(Preview::of).collect(),
        }
    }

    /// The character chosen.
    pub fn selected(&self) -> Character {
        self.selected
    }

    /// One tick with the keys freshly pressed, in [`Key`] order; only the first one counts.
    pub fn tick(&mut self, fresh: &[Key]) -> Choice {
        self.ticks = self.ticks.wrapping_add(1);
        match fresh.first() {
            Some(Key::ArrowRight) => self.selected = self.selected.next(),
            Some(Key::ArrowLeft) => self.selected = self.selected.previous(),
            Some(Key::Enter) => return Choice::Picked(self.selected),
            Some(Key::Escape) => return Choice::Back,
            _ => {}
        }
        Choice::Browsing
    }

    /// Whether the previews show the special moves now (rather than the guard).
    pub fn shows_specials(&self) -> bool {
        !(self.ticks / PREVIEW_TICKS).is_multiple_of(2)
    }

    /// Draws the page.
    pub fn draw(&self, frame: &mut Frame) {
        let center = SCREEN_WIDTH / 2.0;
        frame.label(
            "CHOOSE YOUR FIGHTER",
            center,
            50.0,
            44.0,
            TITLE,
            Align::Center,
        );
        frame.label(
            "Left / Right: choose   Enter: fight   Esc: back",
            center,
            108.0,
            16.0,
            HELP,
            Align::Center,
        );
        let count = Character::ALL.len() as f32;
        let left = (SCREEN_WIDTH - count * CARD_WIDTH - (count - 1.0) * CARD_GAP) / 2.0;
        for ((index, character), preview) in
            Character::ALL.into_iter().enumerate().zip(&self.previews)
        {
            let x = left + index as f32 * (CARD_WIDTH + CARD_GAP);
            self.draw_card(frame, x, character, preview);
        }
        frame.label(
            self.selected.data().tagline,
            center,
            CARD_TOP + CARD_HEIGHT + 24.0,
            18.0,
            TAGLINE,
            Align::Center,
        );
    }

    fn draw_card(&self, frame: &mut Frame, x: f32, character: Character, preview: &Preview) {
        let chosen = character == self.selected;
        let (fill, border) = if chosen {
            (CARD_CHOSEN, BORDER_CHOSEN)
        } else {
            (CARD, BORDER)
        };
        frame.rect(
            x - 3.0,
            CARD_TOP - 3.0,
            CARD_WIDTH + 6.0,
            CARD_HEIGHT + 6.0,
            border,
        );
        frame.rect(x, CARD_TOP, CARD_WIDTH, CARD_HEIGHT, fill);
        let middle = x + CARD_WIDTH / 2.0;
        let data = character.data();
        frame.label(
            data.name,
            middle,
            CARD_TOP + 16.0,
            32.0,
            NAME,
            Align::Center,
        );
        frame.label(
            data.special_name,
            middle,
            CARD_TOP + 58.0,
            18.0,
            SPECIAL,
            Align::Center,
        );

        // The figure, its feet on the card, a little left of the middle so the move has room.
        let shown = if self.shows_specials() {
            &preview.special
        } else {
            &preview.guard
        };
        let fighter = &shown.fighters()[0];
        let rules = shown.rules(PlayerId(0)).unwrap_or(shown.config());
        let (feet_x, feet_y) = feet(fighter, shown.config());
        let anchor = x + CARD_WIDTH * 0.3;
        Figure::of(fighter, rules, self.ticks)
            .shifted(anchor - feet_x)
            .raised(feet_y - PREVIEW_FEET)
            .draw(frame);
        if self.shows_specials() && rules.special.projectile.is_some() {
            let hands = Point {
                x: anchor + 70.0,
                y: PREVIEW_FEET - 72.0,
            };
            draw_fireball(frame, hands, Facing::Right, self.ticks);
        }

        let stats = [
            ("Health", data.health_percent),
            ("Speed", data.speed_percent),
            ("Power", data.power_percent),
            ("Reach", data.reach_percent),
            ("Grab", data.grab_reach_percent),
        ];
        for (row, (name, percent)) in stats.into_iter().enumerate() {
            let y = PREVIEW_FEET + 30.0 + row as f32 * 32.0;
            frame.label(name, x + 16.0, y, 16.0, STAT, Align::Left);
            let bar_x = x + 96.0;
            let full = CARD_WIDTH - 112.0;
            frame.rect(bar_x, y + 3.0, full, 12.0, BAR_EMPTY);
            let length = full * (percent as f32 / 150.0).min(1.0);
            frame.rect(bar_x, y + 3.0, length, 12.0, BAR_FULL);
        }
    }
}
