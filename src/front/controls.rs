//! The controls page: pick a keyboard preset ([`KeyBindings::PRESETS`]: the standard arrows +
//! keypad layout, which stays the default, or WASD + letters + space), or remap any action.
//!
//! Reached from the intro with `C`. `Up` / `Down` choose a row; on the preset row `Left` /
//! `Right` (or `Enter`) switch presets; on an action row `Enter` waits for the new key (`Esc`
//! cancels), and a key already bound elsewhere swaps with the old one; `Esc` (or `Enter` on
//! the last row) goes back to the title.
//!
//! The page only edits a [`KeyBindings`]; the [`Game`](super::Game) keeps it and plays with it.

use super::scene::{Align, Frame, Rgba, SCREEN_WIDTH};
use crate::game::{Input, Key, KeyBindings, Technique};

/// Rows of the page: the preset, one per action, then "back".
pub const ROWS: usize = Input::BITS as usize + 2;

/// Top of the first row.
pub const FIRST_ROW_Y: f32 = 150.0;
/// Distance between two rows.
pub const ROW_HEIGHT: f32 = 34.0;
/// Where the names end and the keys start.
pub const COLUMN_X: f32 = SCREEN_WIDTH / 2.0;

const ROW_SIZE: f32 = 20.0;
const TITLE: Rgba = Rgba::hex(0xffd84a);
const HELP: Rgba = Rgba::hex(0x8a8098);
const NAME: Rgba = Rgba::hex(0xd8d0e8);
const KEY: Rgba = Rgba::hex(0xf4f0ff);
const WAITING: Rgba = Rgba::hex(0xffe14a);
const HIGHLIGHT: Rgba = Rgba::hex(0x3a1430);
const JUDO: Rgba = Rgba::hex(0x3fc8ff);

/// What a row of the page is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Row {
    /// The preset chooser.
    Preset,
    /// The action at this index of [`KeyBindings::ACTIONS`].
    Action(usize),
    /// Back to the title.
    Back,
}

impl Row {
    /// The row at `index`, top to bottom (`index` below [`ROWS`]).
    ///
    /// ```
    /// use srr::front::controls::{ROWS, Row};
    ///
    /// assert_eq!(Row::at(0), Row::Preset);
    /// assert_eq!(Row::at(1), Row::Action(0));
    /// assert_eq!(Row::at(ROWS - 1), Row::Back);
    /// ```
    pub fn at(index: usize) -> Row {
        match index {
            0 => Row::Preset,
            i if i + 1 >= ROWS => Row::Back,
            i => Row::Action(i - 1),
        }
    }
}

/// The name of the preset `bindings` is, or `"Custom"` once remapped.
///
/// ```
/// use srr::front::controls::preset_name;
/// use srr::game::{Key, KeyBindings};
///
/// assert_eq!(preset_name(&KeyBindings::STANDARD), "Standard (arrows + keypad)");
/// let mut mine = KeyBindings::WASD;
/// mine.bind(9, Key::K);
/// assert_eq!(preset_name(&mine), "Custom");
/// ```
pub fn preset_name(bindings: &KeyBindings) -> &'static str {
    preset_index(bindings).map_or("Custom", |index| KeyBindings::PRESETS[index].0)
}

fn preset_index(bindings: &KeyBindings) -> Option<usize> {
    KeyBindings::PRESETS
        .iter()
        .position(|(_, preset)| preset == bindings)
}

/// The keys to press together for `technique` (and to counter it) with `bindings`, e.g.
/// `"5+9"`.
///
/// ```
/// use srr::front::controls::technique_keys;
/// use srr::game::{KeyBindings, Technique};
///
/// assert_eq!(technique_keys(Technique::Sweep, &KeyBindings::STANDARD), "5+9");
/// assert_eq!(technique_keys(Technique::Throw, &KeyBindings::WASD), "Y+U");
/// ```
pub fn technique_keys(technique: Technique, bindings: &KeyBindings) -> String {
    let [first, second] = technique
        .buttons()
        .map(|attack| bindings.key_of(attack).label());
    format!("{first}+{second}")
}

/// The two lines of controls shown at the bottom of the fight screen.
///
/// ```
/// use srr::front::controls::controls_lines;
/// use srr::game::KeyBindings;
///
/// let [moves, attacks] = controls_lines(&KeyBindings::WASD);
/// assert!(moves.starts_with("Move: A / D   Jump: W   Crouch: S   Turn: S+A / S+D"));
/// assert!(attacks.contains("Grab: Space   Judo: H+U, Y+U"));
/// ```
pub fn controls_lines(bindings: &KeyBindings) -> [String; 2] {
    let b = bindings;
    let [left, right, jump, down] = [b.left, b.right, b.jump, b.down].map(Key::label);
    let punches = [b.punch_low, b.punch_medium, b.punch_high].map(Key::label);
    let kicks = [b.kick_low, b.kick_medium, b.kick_high].map(Key::label);
    [
        format!(
            "Move: {left} / {right}   Jump: {jump}   Crouch: {down}   \
             Turn: {down}+{left} / {down}+{right}   Block: step back"
        ),
        format!(
            "Punch: {}   Kick: {}   Grab: {}   Judo: {}, {}   Enter: ready",
            punches.join(" "),
            kicks.join(" "),
            b.grab.label(),
            technique_keys(Technique::Sweep, b),
            technique_keys(Technique::Throw, b),
        ),
    ]
}

/// The state of the controls page: the row chosen, and whether it waits for a new key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ControlsPage {
    selected: usize,
    waiting: bool,
}

impl ControlsPage {
    /// The page with the preset row chosen.
    pub fn new() -> Self {
        Self::default()
    }

    /// The row chosen.
    pub fn selected(&self) -> Row {
        Row::at(self.selected)
    }

    /// Whether the next key pressed will be bound to the chosen action.
    pub fn is_waiting(&self) -> bool {
        self.waiting
    }

    /// One tick with the keys freshly pressed, in [`Key`] order; only the first one counts.
    /// Edits `bindings`, and returns true when the player leaves the page.
    pub fn tick(&mut self, fresh: &[Key], bindings: &mut KeyBindings) -> bool {
        let Some(&key) = fresh.first() else {
            return false;
        };
        if self.waiting {
            if key == Key::Escape || bindings.bind(self.selected - 1, key) {
                self.waiting = false;
            }
            return false;
        }
        let count = KeyBindings::PRESETS.len();
        match (key, self.selected()) {
            (Key::Escape, _) | (Key::Enter, Row::Back) => return true,
            (Key::ArrowUp, _) => self.selected = (self.selected + ROWS - 1) % ROWS,
            (Key::ArrowDown, _) => self.selected = (self.selected + 1) % ROWS,
            (Key::ArrowRight | Key::Enter, Row::Preset) => {
                let next = preset_index(bindings).map_or(0, |index| (index + 1) % count);
                *bindings = KeyBindings::PRESETS[next].1;
            }
            (Key::ArrowLeft, Row::Preset) => {
                let previous =
                    preset_index(bindings).map_or(0, |index| (index + count - 1) % count);
                *bindings = KeyBindings::PRESETS[previous].1;
            }
            (Key::Enter, Row::Action(_)) => self.waiting = true,
            _ => {}
        }
        false
    }

    /// Draws the page for `bindings`.
    pub fn draw(&self, frame: &mut Frame, bindings: &KeyBindings) {
        let center = SCREEN_WIDTH / 2.0;
        frame.label("CONTROLS", center, 50.0, 44.0, TITLE, Align::Center);
        frame.label(
            "Up / Down: choose   Left / Right: preset   Enter: change   Esc: back",
            center,
            108.0,
            16.0,
            HELP,
            Align::Center,
        );
        let keys = bindings.keys();
        for index in 0..ROWS {
            let y = FIRST_ROW_Y + index as f32 * ROW_HEIGHT;
            let chosen = index == self.selected;
            if chosen {
                frame.rect(
                    COLUMN_X - 420.0,
                    y - 6.0,
                    840.0,
                    ROW_HEIGHT - 2.0,
                    HIGHLIGHT,
                );
            }
            let (name, value) = match Row::at(index) {
                Row::Preset => ("Preset", format!("< {} >", preset_name(bindings))),
                Row::Action(action) => {
                    let value = if chosen && self.waiting {
                        "press a key...  (Esc: cancel)".to_string()
                    } else {
                        keys[action].label().to_string()
                    };
                    (KeyBindings::ACTIONS[action], value)
                }
                Row::Back => ("Back to the title", String::new()),
            };
            let marker = if chosen { "> " } else { "" };
            frame.label(
                format!("{marker}{name}"),
                COLUMN_X - 30.0,
                y,
                ROW_SIZE,
                NAME,
                Align::Right,
            );
            let color = if chosen && self.waiting { WAITING } else { KEY };
            frame.label(value, COLUMN_X + 30.0, y, ROW_SIZE, color, Align::Left);
        }
        frame.label(
            format!(
                "Judo: {} juji sweep, {} ippon-seoi-nage (the same keys counter)",
                technique_keys(Technique::Sweep, bindings),
                technique_keys(Technique::Throw, bindings),
            ),
            center,
            FIRST_ROW_Y + ROWS as f32 * ROW_HEIGHT + 16.0,
            16.0,
            JUDO,
            Align::Center,
        );
    }
}
