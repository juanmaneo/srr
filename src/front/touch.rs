//! On-screen touch controls, for phones and tablets.
//!
//! The game only knows keys ([`Key`]); a touch screen has none. A [`TouchPad`] is a set of
//! on-screen buttons, laid out on the virtual screen for the screen shown, each pressing the
//! keys it stands for while a finger is on it. Several fingers press several buttons at once,
//! so the judo techniques (two attack buttons together) and turning around (down + a
//! direction) work like on a keyboard.
//!
//! * On the fight screen: a directional pad on the left (left, right, jump, crouch, and the
//!   diagonals in between), the two sidesteps next to it, the six attacks on the right (kicks
//!   above, punches below, from low to high), grab and special move under them, and small
//!   buttons for Escape (back: leave an online practice fight), Enter (ready), `T` (practice
//!   rival), `Tab` (view) and `P` (pause) at the top.
//!   The fight's buttons press the keys of the [`KeyBindings`] played with, so they follow
//!   any layout.
//! * On the select screen and the controls page (the pause menu): the pad (the arrow keys),
//!   OK (Enter) and BACK (Escape).
//! * On the title: START (Enter) and PRACTICE (`T`).
//!
//! Everything is drawn into the [`Frame`] like the rest of the game, so every engine shows it
//! and the tests can look at it.

use std::collections::HashSet;

use super::Screen;
use super::scene::{Align, Frame, Rgba};
use crate::game::{Key, KeyBindings};

/// Centre of the directional pad.
pub const PAD_CENTER: (f32, f32) = (150.0, 560.0);
/// Radius of the directional pad: touches further away are not on it.
pub const PAD_RADIUS: f32 = 110.0;
/// Where the fight screen's small notice line (the practice notice) goes with the touch
/// controls: under the top buttons, clear of the others.
pub const NOTICE_Y: f32 = 156.0;
/// Around the pad's centre, a touch presses no direction (so a resting thumb is harmless).
pub const PAD_DEAD_ZONE: f32 = 24.0;

const BUTTON: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.12,
};
const BUTTON_HELD: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.38,
};
const SPECIAL: Rgba = Rgba {
    r: 1.0,
    g: 0.37,
    b: 0.82,
    a: 0.22,
};
const LABEL: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.8,
};

/// What a button presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Press {
    /// This very key, whatever the layout (Enter, Escape, `T`, `Tab`, `P`...).
    Key(Key),
    /// The key bound to the action at this index of [`KeyBindings::ACTIONS`].
    Action(usize),
}

impl Press {
    /// The key pressed with `bindings`.
    pub fn key(self, bindings: &KeyBindings) -> Key {
        match self {
            Press::Key(key) => key,
            Press::Action(action) => bindings.keys()[action],
        }
    }
}

/// One rectangular button: where it is, what it reads, what it presses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Button {
    /// The text on the button.
    pub label: &'static str,
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
    /// What it presses.
    pub press: Press,
}

impl Button {
    /// Whether the point `(x, y)` is on the button (edges included).
    pub fn contains(&self, (x, y): (f32, f32)) -> bool {
        (self.x..=self.x + self.width).contains(&x) && (self.y..=self.y + self.height).contains(&y)
    }
}

/// A [`Button`] from its label, its `(x, y, width, height)` rectangle and what it presses.
macro_rules! button {
    ($label:expr, ($x:expr, $y:expr, $width:expr, $height:expr), $press:expr $(,)?) => {
        Button {
            label: $label,
            x: $x,
            y: $y,
            width: $width,
            height: $height,
            press: $press,
        }
    };
}

/// What the directional pad presses: the fight's bound directions, or the arrow keys of the
/// menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pad {
    /// Left, right, jump and down as bound in the [`KeyBindings`].
    Bound,
    /// The arrow keys, which always drive the menus.
    Arrows,
}

// Actions of `KeyBindings::ACTIONS`.
const LEFT: usize = 0;
const RIGHT: usize = 1;
const JUMP: usize = 2;
const DOWN: usize = 10;

/// The buttons of the fight screen.
const FIGHT: [Button; 15] = [
    button!("WALL", (275.0, 450.0, 86.0, 54.0), Press::Action(11)),
    button!("STREET", (275.0, 616.0, 86.0, 54.0), Press::Action(12)),
    button!("LK", (920.0, 430.0, 82.0, 82.0), Press::Action(6)),
    button!("MK", (1012.0, 430.0, 82.0, 82.0), Press::Action(7)),
    button!("HK", (1104.0, 430.0, 82.0, 82.0), Press::Action(8)),
    button!("LP", (920.0, 522.0, 82.0, 82.0), Press::Action(3)),
    button!("MP", (1012.0, 522.0, 82.0, 82.0), Press::Action(4)),
    button!("HP", (1104.0, 522.0, 82.0, 82.0), Press::Action(5)),
    button!("GRAB", (920.0, 614.0, 128.0, 66.0), Press::Action(9)),
    button!("SPECIAL", (1058.0, 614.0, 128.0, 66.0), Press::Action(13)),
    button!("BACK", (806.0, 104.0, 84.0, 40.0), Press::Key(Key::Escape)),
    button!("READY", (900.0, 104.0, 84.0, 40.0), Press::Key(Key::Enter)),
    button!("RIVAL", (994.0, 104.0, 84.0, 40.0), Press::Key(Key::T)),
    button!("VIEW", (1088.0, 104.0, 84.0, 40.0), Press::Key(Key::Tab)),
    button!("PAUSE", (1182.0, 104.0, 84.0, 40.0), Press::Key(Key::P)),
];

/// The buttons of the select screen and of the controls page.
const MENU: [Button; 2] = [
    button!(
        "BACK",
        (1050.0, 450.0, 170.0, 64.0),
        Press::Key(Key::Escape),
    ),
    button!("OK", (1050.0, 530.0, 170.0, 96.0), Press::Key(Key::Enter)),
];

/// The buttons of the title.
const TITLE: [Button; 2] = [
    button!("PRACTICE", (1010.0, 450.0, 210.0, 64.0), Press::Key(Key::T)),
    button!(
        "START",
        (1010.0, 530.0, 210.0, 96.0),
        Press::Key(Key::Enter),
    ),
];

/// The touch controls of one screen.
///
/// ```
/// use srr::front::Screen;
/// use srr::front::touch::{PAD_CENTER, TouchPad};
/// use srr::game::{Key, KeyBindings};
///
/// let pad = TouchPad::for_screen(Screen::Fight);
/// let bindings = KeyBindings::STANDARD;
/// // A thumb pushing the pad to the right, and a finger on the high kick.
/// let right = (PAD_CENTER.0 + 80.0, PAD_CENTER.1);
/// let high_kick = (1140.0, 470.0);
/// let keys = pad.keys(&[right, high_kick], &bindings);
/// assert_eq!(keys, [Key::ArrowRight, Key::Numpad9].into_iter().collect());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchPad {
    pad: Option<Pad>,
    buttons: &'static [Button],
}

impl TouchPad {
    /// The controls of `screen`.
    pub fn for_screen(screen: Screen) -> TouchPad {
        match screen {
            Screen::Fight => TouchPad {
                pad: Some(Pad::Bound),
                buttons: &FIGHT,
            },
            Screen::Select | Screen::Controls => TouchPad {
                pad: Some(Pad::Arrows),
                buttons: &MENU,
            },
            Screen::Intro => TouchPad {
                pad: None,
                buttons: &TITLE,
            },
        }
    }

    /// The directional pad, if the screen has one.
    pub fn pad(&self) -> Option<Pad> {
        self.pad
    }

    /// The buttons besides the pad.
    pub fn buttons(&self) -> &'static [Button] {
        self.buttons
    }

    /// The keys held by fingers on `touches` (points of the virtual screen), with
    /// `bindings` for the fight's buttons.
    pub fn keys(&self, touches: &[(f32, f32)], bindings: &KeyBindings) -> HashSet<Key> {
        let mut keys = HashSet::new();
        for &touch in touches {
            if let Some(pad) = self.pad {
                keys.extend(pad_keys(pad, touch, bindings));
            }
            keys.extend(
                self.buttons
                    .iter()
                    .filter(|button| button.contains(touch))
                    .map(|button| button.press.key(bindings)),
            );
        }
        keys
    }

    /// Draws the controls over `frame`; the buttons and pad directions under a finger of
    /// `touches` are lit.
    pub fn draw(&self, frame: &mut Frame, touches: &[(f32, f32)]) {
        if self.pad.is_some() {
            draw_pad(frame, touches);
        }
        for button in self.buttons {
            let held = touches.iter().any(|&touch| button.contains(touch));
            let color = match (held, button.press) {
                (true, _) => BUTTON_HELD,
                (false, Press::Action(13)) => SPECIAL,
                (false, _) => BUTTON,
            };
            frame.rect(button.x, button.y, button.width, button.height, color);
            let size = if button.height < 50.0 { 14.0 } else { 18.0 };
            frame.label(
                button.label,
                button.x + button.width / 2.0,
                button.y + (button.height - size) / 2.0,
                size,
                LABEL,
                Align::Center,
            );
        }
    }
}

/// The directions a finger at `touch` presses on the pad: none outside it or in its dead
/// zone, otherwise left or right and up or down by the side it leans to (both on a
/// diagonal).
///
/// ```
/// use srr::front::touch::{PAD_CENTER, Pad, pad_keys};
/// use srr::game::{Key, KeyBindings};
///
/// let down_left = (PAD_CENTER.0 - 60.0, PAD_CENTER.1 + 60.0);
/// let keys = pad_keys(Pad::Bound, down_left, &KeyBindings::WASD);
/// assert_eq!(keys, vec![Key::A, Key::S], "turning around in WASD");
/// assert!(pad_keys(Pad::Arrows, PAD_CENTER, &KeyBindings::WASD).is_empty());
/// ```
pub fn pad_keys(pad: Pad, (x, y): (f32, f32), bindings: &KeyBindings) -> Vec<Key> {
    let (dx, dy) = (x - PAD_CENTER.0, y - PAD_CENTER.1);
    if dx.hypot(dy) > PAD_RADIUS {
        return Vec::new();
    }
    // A direction counts once the finger leans that way by the dead zone, and at least half
    // as much as the other way (so a mostly horizontal push is not also a jump).
    let leans =
        |along: f32, across: f32| along.abs() > PAD_DEAD_ZONE && along.abs() * 2.0 >= across.abs();
    let keys = bindings.keys();
    let key = |action: usize, arrow: Key| match pad {
        Pad::Bound => keys[action],
        Pad::Arrows => arrow,
    };
    let mut pressed = Vec::new();
    if leans(dx, dy) {
        pressed.push(if dx < 0.0 {
            key(LEFT, Key::ArrowLeft)
        } else {
            key(RIGHT, Key::ArrowRight)
        });
    }
    if leans(dy, dx) {
        pressed.push(if dy < 0.0 {
            key(JUMP, Key::ArrowUp)
        } else {
            key(DOWN, Key::ArrowDown)
        });
    }
    pressed
}

/// The pad: a disc of squares (the frame only has rectangles) and its four arrows, the one a
/// finger pushes lit.
fn draw_pad(frame: &mut Frame, touches: &[(f32, f32)]) {
    let (cx, cy) = PAD_CENTER;
    // The disc, as rows of rectangles.
    let rows = 12;
    let step = 2.0 * PAD_RADIUS / rows as f32;
    for row in 0..rows {
        let middle = -PAD_RADIUS + (row as f32 + 0.5) * step;
        let half = (PAD_RADIUS * PAD_RADIUS - middle * middle).sqrt();
        frame.rect(
            cx - half,
            cy - PAD_RADIUS + row as f32 * step,
            2.0 * half,
            step,
            BUTTON,
        );
    }
    let reach = PAD_RADIUS * 0.62;
    let arrows = [
        ("<", (-reach, 0.0), Key::ArrowLeft),
        (">", (reach, 0.0), Key::ArrowRight),
        ("^", (0.0, -reach), Key::ArrowUp),
        ("v", (0.0, reach), Key::ArrowDown),
    ];
    let pushed: Vec<Key> = touches
        .iter()
        .flat_map(|&touch| pad_keys(Pad::Arrows, touch, &KeyBindings::STANDARD))
        .collect();
    for (label, (dx, dy), arrow) in arrows {
        let side = 52.0;
        let color = if pushed.contains(&arrow) {
            BUTTON_HELD
        } else {
            BUTTON
        };
        frame.rect(
            cx + dx - side / 2.0,
            cy + dy - side / 2.0,
            side,
            side,
            color,
        );
        frame.label(label, cx + dx, cy + dy - 11.0, 22.0, LABEL, Align::Center);
    }
}
