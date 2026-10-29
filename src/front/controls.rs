//! The controls page: pick a keyboard preset ([`KeyBindings::PRESETS`]: the standard arrows +
//! keypad layout, which stays the default, or WASD + letters + space), remap any action, pick
//! your [`Character`], and choose who to practise against.
//!
//! Reached from the intro with `C`, or from the fight with `P`, which pauses it (the page is
//! then the pause menu, see [`ControlsPage::paused`]). `Up` / `Down` choose a row; on the
//! preset row `Left` / `Right` (or `Enter`) switch presets; on an action row `Enter` waits for
//! the new key (`Esc` cancels), and a key already bound elsewhere swaps with the old one; on
//! the character row `Left` / `Right` (or `Enter`) go round the characters; on the practice
//! row they choose the rival of a practice fight (see [`Practice`]); `Esc` (or `Enter` on the
//! last row, or `P` while paused) goes back to where the page was opened from.
//!
//! The page only edits a [`KeyBindings`], a [`Character`] and a [`Practice`] choice; the
//! [`Game`](super::Game) keeps the first and carries out the others.

use super::scene::{Align, Frame, Rgba, SCREEN_WIDTH};
use crate::game::{Character, Input, Key, KeyBindings, PlayerId, Rival, Skin, StyleSet, Technique};

/// Rows of the page: the preset, one per action, the character, the skin, the practice, then
/// "back".
pub const ROWS: usize = Input::BITS as usize + 5;

/// The key pausing the fight (and resuming it from the pause menu); it cannot be bound.
pub const PAUSE_KEY: Key = Key::P;

/// Top of the first row.
pub const FIRST_ROW_Y: f32 = 150.0;
/// Distance between two rows: the [`ROWS`] rows and the judo line below them fit the screen.
pub const ROW_HEIGHT: f32 = 28.0;
/// Where the names end and the keys start.
pub const COLUMN_X: f32 = SCREEN_WIDTH / 2.0;
/// Where the skin preview stands: on the right of the page, between the edge and the rows.
pub const PREVIEW_X: f32 = SCREEN_WIDTH - 150.0;
/// The feet of the skin preview: the rows leave the lower right of the page free.
pub const PREVIEW_FEET_Y: f32 = 420.0;
/// How tall the skin preview is drawn, in virtual pixels.
pub const PREVIEW_HEIGHT: f32 = 230.0;

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
    /// The character played (see [`Character`]).
    Character,
    /// The skin worn (see [`Skin`]): the styles the server allows, each wearing the player's
    /// own variant.
    Skin,
    /// The rival of a practice fight (see [`Practice`]).
    Practice,
    /// Back to the title, or back to the fight when paused.
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
    /// assert_eq!(Row::at(ROWS - 4), Row::Character);
    /// assert_eq!(Row::at(ROWS - 3), Row::Skin);
    /// assert_eq!(Row::at(ROWS - 2), Row::Practice);
    /// assert_eq!(Row::at(ROWS - 1), Row::Back);
    /// ```
    pub fn at(index: usize) -> Row {
        match index {
            0 => Row::Preset,
            i if i + 1 >= ROWS => Row::Back,
            i if i + 2 == ROWS => Row::Practice,
            i if i + 3 == ROWS => Row::Skin,
            i if i + 4 == ROWS => Row::Character,
            i => Row::Action(i - 1),
        }
    }
}

/// What the practice row offers: who to practise against, and online whether to practise at
/// all while the room stays open. `Left` / `Right` (or `Enter`, or `T` on the fight screen)
/// go round the choices ([`Practice::next`]); the [`Game`](super::Game) carries them out.
///
/// ```
/// use srr::front::controls::Practice;
/// use srr::game::Rival;
///
/// assert_eq!(Practice::Off.next(), Practice::WhileWaiting(Rival::Brawler));
/// assert_eq!(Practice::WhileWaiting(Rival::Dummy).next(), Practice::Off);
/// assert_eq!(Practice::Alone(Rival::Brawler).next(), Practice::Alone(Rival::Patient));
/// assert_eq!(Practice::Unavailable.next(), Practice::Unavailable);
/// assert_eq!(Practice::Off.previous(), Practice::WhileWaiting(Rival::Dummy));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Practice {
    /// No practice fight can start now: online, during a match or before being seated.
    #[default]
    Unavailable,
    /// Online, seated between matches, not practising: choosing a rival starts a practice
    /// fight while the room stays open.
    Off,
    /// Online: practising against this rival while the room stays open; [`Practice::Off`]
    /// ends it (`Esc` on the fight screen does too).
    WhileWaiting(Rival),
    /// A practice game (natively, or with `?practice`): only the rival changes.
    Alone(Rival),
}

impl Practice {
    /// The rival practised against, if any.
    pub fn rival(self) -> Option<Rival> {
        match self {
            Practice::WhileWaiting(rival) | Practice::Alone(rival) => Some(rival),
            Practice::Unavailable | Practice::Off => None,
        }
    }

    /// The choice after this one.
    pub fn next(self) -> Practice {
        match self {
            Practice::Unavailable => Practice::Unavailable,
            Practice::Off => Practice::WhileWaiting(Rival::ALL[0]),
            Practice::WhileWaiting(rival) if rival == Rival::ALL[Rival::ALL.len() - 1] => {
                Practice::Off
            }
            Practice::WhileWaiting(rival) => Practice::WhileWaiting(rival.next()),
            Practice::Alone(rival) => Practice::Alone(rival.next()),
        }
    }

    /// The choice before this one.
    pub fn previous(self) -> Practice {
        match self {
            Practice::Unavailable => Practice::Unavailable,
            Practice::Off => Practice::WhileWaiting(Rival::ALL[Rival::ALL.len() - 1]),
            Practice::WhileWaiting(rival) if rival == Rival::ALL[0] => Practice::Off,
            Practice::WhileWaiting(rival) => Practice::WhileWaiting(rival.previous()),
            Practice::Alone(rival) => Practice::Alone(rival.previous()),
        }
    }

    /// What the practice row reads.
    fn label(self) -> String {
        match self {
            Practice::Unavailable => "- (not during a match)".to_string(),
            Practice::Off => "< off >".to_string(),
            Practice::WhileWaiting(rival) => {
                format!("< {} - the room stays open >", rival.name())
            }
            Practice::Alone(rival) => format!("< {} >", rival.name()),
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
/// assert!(moves.starts_with("Move: A / D   Sidestep: Q / E   Jump: W   Crouch: S"));
/// assert!(attacks.contains("Grab: Space   Judo: H+U, Y+U   Special: F"));
/// ```
pub fn controls_lines(bindings: &KeyBindings) -> [String; 2] {
    let b = bindings;
    let [left, right, jump, down] = [b.left, b.right, b.jump, b.down].map(Key::label);
    let [wall, street] = [b.sidestep_in, b.sidestep_out].map(Key::label);
    let punches = [b.punch_low, b.punch_medium, b.punch_high].map(Key::label);
    let kicks = [b.kick_low, b.kick_medium, b.kick_high].map(Key::label);
    [
        format!(
            "Move: {left} / {right}   Sidestep: {wall} / {street}   Jump: {jump}   \
             Crouch: {down}   Turn: {down}+{left} / {down}+{right}   Block: step back"
        ),
        format!(
            "Punch: {}   Kick: {}   Grab: {}   Judo: {}, {}   Special: {}   Enter: ready   \
             P: pause   Tab: view",
            punches.join(" "),
            kicks.join(" "),
            b.grab.label(),
            technique_keys(Technique::Sweep, b),
            technique_keys(Technique::Throw, b),
            b.special.label(),
        ),
    ]
}

/// The state of the controls page: the row chosen, whether it waits for a new key, whether
/// it is the pause menu of a fight, the character and the practice choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControlsPage {
    selected: usize,
    waiting: bool,
    paused: bool,
    character: Character,
    skin: Skin,
    /// The player whose variant the skin row wears.
    player: PlayerId,
    /// The styles the server allows, for the skin row.
    styles: StyleSet,
    practice: Practice,
}

impl Default for ControlsPage {
    fn default() -> Self {
        ControlsPage {
            selected: 0,
            waiting: false,
            paused: false,
            character: Character::default(),
            skin: Skin::STICK,
            player: PlayerId(0),
            styles: StyleSet::all(),
            practice: Practice::Unavailable,
        }
    }
}

impl ControlsPage {
    /// The page with the preset row chosen, opened from the title, with no practice choice
    /// yet ([`Practice::Unavailable`], see [`ControlsPage::with_practice`]).
    pub fn new() -> Self {
        Self::default()
    }

    /// The page as the pause menu: opened from the fight with [`PAUSE_KEY`], which also
    /// leaves it, and going back to the fight.
    pub fn paused() -> Self {
        ControlsPage {
            paused: true,
            ..Self::default()
        }
    }

    /// The page with `practice` on its practice row.
    pub fn with_practice(self, practice: Practice) -> Self {
        ControlsPage { practice, ..self }
    }

    /// The page with `character` on its character row.
    pub fn with_character(self, character: Character) -> Self {
        ControlsPage { character, ..self }
    }

    /// The page with `skin` on its skin row, offering the styles of `styles` (see
    /// [`Client::styles`](crate::net::Client::styles); a set with only the chalk offers no
    /// choice and the row reads empty).
    pub fn with_skin(self, skin: Skin, styles: StyleSet) -> Self {
        ControlsPage {
            skin,
            styles,
            ..self
        }
    }

    /// The page whose skin row wears `player`'s own variant.
    pub fn with_player(self, player: PlayerId) -> Self {
        ControlsPage { player, ..self }
    }

    /// The skin, as changed on the skin row.
    pub fn skin(&self) -> Skin {
        self.skin
    }

    /// The character, as changed on the character row.
    pub fn character(&self) -> Character {
        self.character
    }

    /// The row chosen.
    pub fn selected(&self) -> Row {
        Row::at(self.selected)
    }

    /// The practice choice, as changed on the practice row.
    pub fn practice(&self) -> Practice {
        self.practice
    }

    /// Whether the next key pressed will be bound to the chosen action.
    pub fn is_waiting(&self) -> bool {
        self.waiting
    }

    /// Whether the page is the pause menu of a fight (see [`ControlsPage::paused`]).
    pub fn is_paused(&self) -> bool {
        self.paused
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
            (PAUSE_KEY, _) if self.paused => return true,
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
            (Key::ArrowRight | Key::Enter, Row::Character) => {
                self.character = self.character.next();
            }
            (Key::ArrowLeft, Row::Character) => self.character = self.character.previous(),
            (Key::ArrowRight | Key::Enter, Row::Skin) => self.skin = self.skin.next_choice(),
            (Key::ArrowLeft, Row::Skin) => self.skin = self.skin.previous_choice(),
            (Key::ArrowRight | Key::Enter, Row::Practice) => {
                self.practice = self.practice.next();
            }
            (Key::ArrowLeft, Row::Practice) => self.practice = self.practice.previous(),
            (Key::Enter, Row::Action(_)) => self.waiting = true,
            _ => {}
        }
        false
    }

    /// Draws the page for `bindings`.
    pub fn draw(&self, frame: &mut Frame, bindings: &KeyBindings) {
        let center = SCREEN_WIDTH / 2.0;
        let (title, help) = if self.paused {
            (
                "PAUSED",
                "Up / Down: choose   Left / Right: preset, character, practice   \
                 Enter: change   Esc / P: resume",
            )
        } else {
            (
                "CONTROLS",
                "Up / Down: choose   Left / Right: preset, character, practice   \
                 Enter: change   Esc: back",
            )
        };
        frame.label(title, center, 50.0, 44.0, TITLE, Align::Center);
        frame.label(help, center, 108.0, 16.0, HELP, Align::Center);
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
                Row::Character => {
                    let data = self.character.data();
                    (
                        "Character",
                        format!("< {} - {} >", data.name, data.special_name),
                    )
                }
                Row::Skin => {
                    // Only the styles the server allows are offered (a set with no skin at
                    // all shows no choice either).
                    let value = if self.styles.styles().count() > 1 {
                        format!("< {} >", self.skin.label())
                    } else {
                        String::new()
                    };
                    ("Skin", value)
                }
                Row::Practice => ("Practice", self.practice.label()),
                Row::Back => (
                    if self.paused {
                        "Resume the fight"
                    } else {
                        "Back to the title"
                    },
                    String::new(),
                ),
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

        // The preview of the chosen skin, standing on the right of the page: what the
        // fighter will look like, live as the row is cycled.
        if let Some(name) = crate::front::skin::sprite_name_of(&self.skin) {
            frame.image(
                name,
                crate::front::skin::Pose::Stand,
                PREVIEW_X,
                PREVIEW_FEET_Y,
                PREVIEW_HEIGHT,
                crate::front::skin::Facing::Right,
            );
            frame.label(
                self.skin.variant.map(|v| v.label()).unwrap_or_default(),
                PREVIEW_X,
                PREVIEW_FEET_Y + 28.0,
                16.0,
                KEY,
                Align::Center,
            );
        }
    }
}
