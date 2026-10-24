//! A PlayStation controller (a DualShock 4 on the PS4, a DualSense on the PS5): the game only
//! knows keys ([`Key`]); a pad has buttons and a stick instead. A [`GamepadState`] is the
//! buttons held and the position of the left stick, translated into the keys they stand for
//! by [`GamepadState::keys`], like the touch controls translate fingers
//! ([`touch`](super::touch)).
//!
//! The mapping follows the standard gamepad layout (the same buttons on a PS4 and a PS5 pad,
//! and on an Xbox pad, with A B X Y instead of the shapes), laid out like the on-screen
//! controls of a phone:
//!
//! * **Move** with the left stick or the D-pad: left / right to walk (stepping back blocks,
//!   Tekken style), up to jump, down to crouch, and down + a direction to turn around (push
//!   the stick diagonally). A thumb resting on the stick presses nothing (see
//!   [`STICK_DEAD_ZONE`]).
//! * **The six attacks**, weak on the left: `Square` low punch, `Triangle` medium punch,
//!   `R1` high punch, `Cross` low kick, `Circle` medium kick, `R2` high kick — so the judo
//!   techniques are two buttons pressed together, `Triangle` + `R2` (the sweep) and `Circle`
//!   + `R2` (the throw), and countering one is the same pair.
//! * `L1` **grabs**, `L2` plays the character's **special move**, `L3` sidesteps **to the
//!   wall** and `R3` **to the street**.
//! * The menus: the stick or the D-pad chooses (the arrow keys), `Cross` confirms (`Enter`
//!   — on the fight screen it also readies for the next match), `Circle` goes back (`Esc`),
//!   `Share` backs out of the fight (leaves an online practice), `Options` pauses it and
//!   resumes (on the title it opens the controls page), `Triangle` opens the controls page
//!   from the title and `Square` goes practising from it (`T`). The PS button does nothing:
//!   the console, the browser or the OS keeps it. Switching the view stays on `Tab`.
//!
//! The buttons press the keys of the [`KeyBindings`] played with, so the pad follows any
//! layout, like the touch buttons do. Every pad connected to the machine steers the same
//! fighter ([`GamepadState::merge`]): each player plays on their own machine anyway.
//!
//! Everything here is pure state, drawn by nothing and tested headlessly; the engine backend
//! reads the pads ([`crate::engine`]) and hands their state over. The Fyrox backend does not
//! read gamepads — use the Bevy one (the default) to play with a pad.

use std::collections::HashSet;

use super::Screen;
use crate::game::{Key, KeyBindings};

/// How far the left stick must lean to press a direction: a thumb resting on the stick
/// presses nothing, and a worn stick does not drift the fighter around.
///
/// ```
/// use srr::front::gamepad::{GamepadState, STICK_DEAD_ZONE};
/// use srr::front::Screen;
/// use srr::game::KeyBindings;
///
/// let resting = GamepadState::new().with_stick(STICK_DEAD_ZONE / 2.0, 0.0);
/// assert!(resting.keys(&KeyBindings::STANDARD, Screen::Fight).is_empty());
/// ```
pub const STICK_DEAD_ZONE: f32 = 0.35;

/// One of the standard buttons of a gamepad, named after the PlayStation pads: [`South`] is
/// Cross, [`East`] Circle, [`North`] Triangle, [`West`] Square, [`LeftTrigger`] L2,
/// [`LeftTrigger2`] L1, [`RightTrigger`] R2, [`RightTrigger2`] R1, [`Select`] Share,
/// [`Start`] Options, [`Mode`] the PS button, [`LeftThumb`] L3 and [`RightThumb`] R3.
///
/// These are the buttons every pad reports the same way, natively and in the browser's
/// Gamepad API. `Mode` (the PS button) is never read.
///
/// [`South`]: PadButton::South
/// [`East`]: PadButton::East
/// [`North`]: PadButton::North
/// [`West`]: PadButton::West
/// [`LeftTrigger`]: PadButton::LeftTrigger
/// [`LeftTrigger2`]: PadButton::LeftTrigger2
/// [`RightTrigger`]: PadButton::RightTrigger
/// [`RightTrigger2`]: PadButton::RightTrigger2
/// [`Select`]: PadButton::Select
/// [`Start`]: PadButton::Start
/// [`Mode`]: PadButton::Mode
/// [`LeftThumb`]: PadButton::LeftThumb
/// [`RightThumb`]: PadButton::RightThumb
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PadButton {
    /// Cross.
    South,
    /// Circle.
    East,
    /// Triangle.
    North,
    /// Square.
    West,
    /// L2.
    LeftTrigger,
    /// L1.
    LeftTrigger2,
    /// R2.
    RightTrigger,
    /// R1.
    RightTrigger2,
    /// Share.
    Select,
    /// Options.
    Start,
    /// The PS button.
    Mode,
    /// L3: the left stick click.
    LeftThumb,
    /// R3: the right stick click.
    RightThumb,
    /// The up button of the D-pad.
    DPadUp,
    /// The down button of the D-pad.
    DPadDown,
    /// The left button of the D-pad.
    DPadLeft,
    /// The right button of the D-pad.
    DPadRight,
}

impl PadButton {
    /// Every standard button, in the order of the enum.
    ///
    /// ```
    /// use srr::front::gamepad::PadButton;
    ///
    /// assert_eq!(PadButton::ALL.len(), 17);
    /// assert_eq!(PadButton::ALL[0], PadButton::South);
    /// assert_eq!(PadButton::ALL[16], PadButton::DPadRight);
    /// ```
    pub const ALL: [PadButton; 17] = [
        PadButton::South,
        PadButton::East,
        PadButton::North,
        PadButton::West,
        PadButton::LeftTrigger,
        PadButton::LeftTrigger2,
        PadButton::RightTrigger,
        PadButton::RightTrigger2,
        PadButton::Select,
        PadButton::Start,
        PadButton::Mode,
        PadButton::LeftThumb,
        PadButton::RightThumb,
        PadButton::DPadUp,
        PadButton::DPadDown,
        PadButton::DPadLeft,
        PadButton::DPadRight,
    ];
}

// Actions of `KeyBindings::ACTIONS`.
const LEFT: usize = 0;
const RIGHT: usize = 1;
const JUMP: usize = 2;
const PUNCH_LOW: usize = 3;
const PUNCH_MEDIUM: usize = 4;
const PUNCH_HIGH: usize = 5;
const KICK_LOW: usize = 6;
const KICK_MEDIUM: usize = 7;
const KICK_HIGH: usize = 8;
const GRAB: usize = 9;
const DOWN: usize = 10;
const SIDESTEP_IN: usize = 11;
const SIDESTEP_OUT: usize = 12;
const SPECIAL: usize = 13;

/// The buttons held on a gamepad, and where its left stick leans; what an engine backend
/// hands to [`Game::set_gamepad`](super::Game::set_gamepad) once per tick.
///
/// ```
/// use srr::front::gamepad::{GamepadState, PadButton};
/// use srr::front::Screen;
/// use srr::game::{Key, KeyBindings};
///
/// // Square and the stick pushed right: a low punch while walking forward.
/// let pad = GamepadState::new().with_buttons(&[PadButton::West]).with_stick(0.9, 0.0);
/// let keys = pad.keys(&KeyBindings::STANDARD, Screen::Fight);
/// assert_eq!(keys, [Key::Numpad1, Key::ArrowRight].into_iter().collect());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GamepadState {
    /// The standard buttons held, one bit per button of [`PadButton::ALL`].
    buttons: u32,
    /// The left stick, `(x, y)`: -1 left or down, +1 right or up.
    left_stick: (f32, f32),
}

impl GamepadState {
    /// A pad with nothing held and a centred stick.
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds `button` down too.
    pub fn with_button(self, button: PadButton) -> Self {
        let mut pad = self;
        pad.press(button);
        pad
    }

    /// The same state with every button of `buttons` held down.
    pub fn with_buttons(self, buttons: &[PadButton]) -> Self {
        buttons
            .iter()
            .fold(self, |pad, &button| pad.with_button(button))
    }

    /// Holds `button` down: the state of a pad as its buttons come in.
    pub fn press(&mut self, button: PadButton) {
        self.buttons |= 1 << button as usize;
    }

    /// Releases every button (the stick is kept): a pad left alone presses nothing but may
    /// still lean its stick.
    pub fn release(&mut self) {
        self.buttons = 0;
    }

    /// Leans the left stick on `(x, y)`.
    pub fn set_stick(&mut self, x: f32, y: f32) {
        self.left_stick = (x, y);
    }

    /// The same state with the left stick on `(x, y)`.
    pub fn with_stick(self, x: f32, y: f32) -> Self {
        Self {
            left_stick: (x, y),
            ..self
        }
    }

    /// The left stick, `(x, y)`: -1 left or down, +1 right or up.
    pub fn left_stick(&self) -> (f32, f32) {
        self.left_stick
    }

    /// Whether `button` is held.
    pub fn button(&self, button: PadButton) -> bool {
        self.buttons & (1 << button as usize) != 0
    }

    /// Every button held, in [`PadButton::ALL`] order.
    ///
    /// ```
    /// use srr::front::gamepad::{GamepadState, PadButton};
    ///
    /// let pad = GamepadState::new().with_buttons(&[PadButton::North, PadButton::Start]);
    /// assert_eq!(pad.buttons(), vec![PadButton::North, PadButton::Start]);
    /// ```
    pub fn buttons(&self) -> Vec<PadButton> {
        PadButton::ALL
            .iter()
            .copied()
            .filter(|&button| self.button(button))
            .collect()
    }

    /// Two pads in the hand of the same player: the buttons of both, and the stick of the
    /// one leaning furthest. Every pad connected steers the same fighter.
    ///
    /// ```
    /// use srr::front::gamepad::{GamepadState, PadButton};
    ///
    /// let square = GamepadState::new().with_button(PadButton::West).with_stick(0.2, 0.0);
    /// let stick = GamepadState::new().with_button(PadButton::Start).with_stick(0.9, -0.8);
    /// let merged = square.merge(stick);
    /// assert_eq!(merged.buttons(), vec![PadButton::West, PadButton::Start]);
    /// assert_eq!(merged.left_stick(), (0.9, -0.8));
    /// ```
    pub fn merge(self, other: Self) -> Self {
        let lean = |(x, y): (f32, f32)| x.hypot(y);
        let left_stick = if lean(other.left_stick) > lean(self.left_stick) {
            other.left_stick
        } else {
            self.left_stick
        };
        Self {
            buttons: self.buttons | other.buttons,
            left_stick,
        }
    }

    /// The keys the pad presses with `bindings` on `screen`: the bound keys of the actions
    /// on the fight screen, the arrow keys of the menus, and the menu keys of the pad's own
    /// buttons (see the [module documentation](self)).
    ///
    /// ```
    /// use srr::front::gamepad::{GamepadState, PadButton};
    /// use srr::front::Screen;
    /// use srr::game::{Key, KeyBindings};
    ///
    /// let pad = GamepadState::new().with_button(PadButton::South);
    /// // Cross confirms everywhere...
    /// assert_eq!(
    ///     pad.keys(&KeyBindings::STANDARD, Screen::Select),
    ///     [Key::Enter].into_iter().collect()
    /// );
    /// // ...and on the fight screen it is also the low kick, and readies (Enter).
    /// assert_eq!(
    ///     pad.keys(&KeyBindings::STANDARD, Screen::Fight),
    ///     [Key::Numpad4, Key::Enter].into_iter().collect()
    /// );
    /// ```
    pub fn keys(&self, bindings: &KeyBindings, screen: Screen) -> HashSet<Key> {
        let mut keys = HashSet::new();
        let (left, right, up, down) = self.directions();
        if screen == Screen::Fight {
            // The D-pad and the stick press the bound keys of the layout played with.
            let bound = bindings.keys();
            let pressed = [
                (left, bound[LEFT]),
                (right, bound[RIGHT]),
                (up, bound[JUMP]),
                (down, bound[DOWN]),
                (self.button(PadButton::West), bound[PUNCH_LOW]),
                (self.button(PadButton::North), bound[PUNCH_MEDIUM]),
                (self.button(PadButton::RightTrigger2), bound[PUNCH_HIGH]),
                (self.button(PadButton::South), bound[KICK_LOW]),
                (self.button(PadButton::East), bound[KICK_MEDIUM]),
                (self.button(PadButton::RightTrigger), bound[KICK_HIGH]),
                (self.button(PadButton::LeftTrigger2), bound[GRAB]),
                (self.button(PadButton::LeftTrigger), bound[SPECIAL]),
                (self.button(PadButton::LeftThumb), bound[SIDESTEP_IN]),
                (self.button(PadButton::RightThumb), bound[SIDESTEP_OUT]),
            ];
            keys.extend(
                pressed
                    .into_iter()
                    .filter_map(|(held, key)| held.then_some(key)),
            );
            // The pad's own menu keys: Cross readies between the rounds, Share leaves an
            // online practice fight, Options pauses.
            keys.extend(
                [
                    (PadButton::South, Key::Enter),
                    (PadButton::Select, Key::Escape),
                    (PadButton::Start, super::PAUSE_KEY),
                ]
                .into_iter()
                .filter_map(|(button, key)| self.button(button).then_some(key)),
            );
        } else {
            // The menus are driven by the arrow keys, whatever the layout.
            keys.extend(
                [
                    (left, Key::ArrowLeft),
                    (right, Key::ArrowRight),
                    (up, Key::ArrowUp),
                    (down, Key::ArrowDown),
                ]
                .into_iter()
                .filter_map(|(held, key)| held.then_some(key)),
            );
            keys.extend(
                [
                    (PadButton::South, Key::Enter),
                    (PadButton::East, Key::Escape),
                ]
                .into_iter()
                .filter_map(|(button, key)| self.button(button).then_some(key)),
            );
            // The title also opens its two side pages with the pad.
            if screen == Screen::Intro {
                keys.extend(
                    [
                        (PadButton::North, super::CONTROLS_KEY),
                        (PadButton::West, super::PRACTICE_KEY),
                    ]
                    .into_iter()
                    .filter_map(|(button, key)| self.button(button).then_some(key)),
                );
            }
            // The pause menu resumes with Options, like P does.
            if screen == Screen::Controls {
                keys.extend(
                    [(PadButton::Start, super::PAUSE_KEY)]
                        .into_iter()
                        .filter_map(|(button, key)| self.button(button).then_some(key)),
                );
            }
        }
        keys
    }

    /// The directions the D-pad and the left stick press: left, right, up, then down.
    fn directions(&self) -> (bool, bool, bool, bool) {
        let (x, y) = self.left_stick;
        (
            self.button(PadButton::DPadLeft) || x < -STICK_DEAD_ZONE,
            self.button(PadButton::DPadRight) || x > STICK_DEAD_ZONE,
            self.button(PadButton::DPadUp) || y > STICK_DEAD_ZONE,
            self.button(PadButton::DPadDown) || y < -STICK_DEAD_ZONE,
        )
    }
}
