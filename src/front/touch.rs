//! On-screen touch controls, for phones and tablets.
//!
//! The game only knows keys ([`Key`]); a touch screen has none. A [`TouchPad`] is a set of
//! on-screen buttons, laid out on the virtual screen for the screen shown, each pressing the
//! keys it stands for while a finger is on it. Several fingers press several buttons at once,
//! so the judo techniques (two attack buttons together) and turning around (down + a
//! direction) work like on a keyboard.
//!
//! * On the fight screen: a floating stick on the left (a ring the thumb drags a knob in:
//!   left, right, jump, crouch and the diagonals in between), the two sidesteps next to it,
//!   the six attacks on the right (kicks above, punches below, from low to high), grab and
//!   special move under them, and small buttons for Escape (back: leave an online practice
//!   fight), Enter (ready), `T` (practice rival), `Tab` (view) and `P` (pause) at the top.
//!   The fight's buttons press the keys of the [`KeyBindings`] played with, so they follow
//!   any layout.
//! * On the select screen and the controls page (the pause menu): the pad (the arrow keys),
//!   OK (Enter) and BACK (Escape); a tap on a character card picks it.
//! * On the title: START (Enter) and PRACTICE (`T`).
//!
//! Like GeForce Now's virtual controller, the controls can be hidden (the whole game is then
//! in the clear) and brought back: every pad has a small **TOGGLE** button, drawn even while
//! the controls are hidden (alone, in the corner); pressing it (or the `X` key) flips the
//! controls.
//!
//! Everything is drawn into the [`Frame`] like the rest of the game, so every engine shows it
//! and the tests can look at it. The stick is drawn as a fine disc plus a ring and a knob, so
//! it stays perfectly round at every scale; the buttons are translucent so the game shows
//! through them.

use std::collections::HashSet;

use super::Screen;
use super::scene::{Align, Frame, Rgba};
use crate::game::{Key, KeyBindings};

/// Centre of the stick's ring.
pub const PAD_CENTER: (f32, f32) = (150.0, 560.0);
/// Radius of the stick: touches further away are not on it.
pub const PAD_RADIUS: f32 = 110.0;
/// Radius of the knob the thumb drags inside the ring.
const KNOB_RADIUS: f32 = 34.0;
/// Radius of the thin circle drawn around the stick's area.
const RING_RADIUS: f32 = 86.0;
/// Rows the stick's disc is drawn with: fine enough to look perfectly round.
const DISC_ROWS: u32 = 72;
/// Where the fight screen's small notice line (the practice notice) goes with the touch
/// controls: under the top buttons, clear of the others.
pub const NOTICE_Y: f32 = 156.0;
/// Around the pad's centre, a touch presses no direction (so a resting thumb is harmless).
pub const PAD_DEAD_ZONE: f32 = 24.0;

/// The empty part of a button or of the stick: the game shows through it.
const BUTTON: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.12,
};
/// A button under a finger.
const BUTTON_HELD: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.38,
};
/// The special move's button, tinted so it stands out.
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
/// The thin ring around the stick's area.
const RING: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.25,
};
/// The knob the thumb drags, resting in the middle of the ring.
const KNOB: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.30,
};
/// The knob under a thumb.
const KNOB_HELD: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.55,
};
/// Rows a rounded corner is drawn with.
const CORNER_ROWS: u32 = 16;

/// What a button presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Press {
    /// This very key, whatever the layout (Enter, Escape, `T`, `Tab`, `P`...).
    Key(Key),
    /// The key bound to the action at this index of [`KeyBindings::ACTIONS`].
    Action(usize),
    /// Hide the controls when they are shown, bring them back when they are hidden.
    Toggle,
}

impl Press {
    /// The key pressed with `bindings` ([`None`] for the toggle, which is not a key).
    pub fn key(self, bindings: &KeyBindings) -> Option<Key> {
        match self {
            Press::Key(key) => Some(key),
            Press::Action(action) => Some(bindings.keys()[action]),
            Press::Toggle => None,
        }
    }

    /// Whether this press is the toggle.
    pub fn is_toggle(self) -> bool {
        self == Press::Toggle
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

/// The button that hides the controls, drawn even while they are hidden.
const TOGGLE: Button = button!("TOGGLE", (14.0, 14.0, 78.0, 36.0), Press::Toggle);

/// The buttons of the fight screen.
const FIGHT: [Button; 16] = [
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
    TOGGLE,
];

/// The buttons of the select screen and of the controls page.
const MENU: [Button; 3] = [
    button!(
        "BACK",
        (1050.0, 450.0, 170.0, 64.0),
        Press::Key(Key::Escape),
    ),
    button!("OK", (1050.0, 530.0, 170.0, 96.0), Press::Key(Key::Enter)),
    TOGGLE,
];

/// The buttons of the title.
const TITLE: [Button; 3] = [
    button!("PRACTICE", (1010.0, 450.0, 210.0, 64.0), Press::Key(Key::T)),
    button!(
        "START",
        (1010.0, 530.0, 210.0, 96.0),
        Press::Key(Key::Enter),
    ),
    TOGGLE,
];

/// The corners of every screen's buttons are rounded by the same radius, capped by the
/// smallest button.
const CORNER: f32 = 14.0;

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
    /// `bindings` for the fight's buttons. The toggle presses no key (it is handled by the
    /// [`Game`](super::Game)); a finger on the stick leans it like the pad does.
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
                    .filter_map(|button| button.press.key(bindings)),
            );
        }
        keys
    }

    /// Whether a finger on `touches` presses the toggle button.
    pub fn toggled(&self, touches: &[(f32, f32)]) -> bool {
        touches.iter().any(|&touch| TOGGLE.contains(touch))
    }

    /// Draws the controls over `frame`; the buttons and pad directions under a finger of
    /// `touches` are lit.
    pub fn draw(&self, frame: &mut Frame, touches: &[(f32, f32)]) {
        if self.pad.is_some() {
            draw_stick(frame, touches);
        }
        for button in self.buttons {
            let held = touches.iter().any(|&touch| button.contains(touch));
            let color = match (held, button.press) {
                (true, _) => BUTTON_HELD,
                (false, Press::Action(13)) => SPECIAL,
                (false, _) => BUTTON,
            };
            round_rect(
                frame,
                button.x,
                button.y,
                button.width,
                button.height,
                color,
                CORNER,
            );
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

    /// Draws only the toggle button (the controls are hidden): a finger on it lights it up.
    pub fn draw_toggle(&self, frame: &mut Frame, touches: &[(f32, f32)]) {
        let held = touches.iter().any(|&touch| TOGGLE.contains(touch));
        round_rect(
            frame,
            TOGGLE.x,
            TOGGLE.y,
            TOGGLE.width,
            TOGGLE.height,
            if held { BUTTON_HELD } else { BUTTON },
            CORNER,
        );
        frame.label(
            TOGGLE.label,
            TOGGLE.x + TOGGLE.width / 2.0,
            TOGGLE.y + (TOGGLE.height - 14.0) / 2.0,
            14.0,
            LABEL,
            Align::Center,
        );
    }
}

/// A rectangle with corners rounded by `radius`, as five rectangles plus the four corners
/// drawn row by row.
fn round_rect(
    frame: &mut Frame,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Rgba,
    radius: f32,
) {
    let radius = radius.min(width / 2.0).min(height / 2.0);
    frame.rect(x + radius, y, width - 2.0 * radius, height, color);
    frame.rect(x, y + radius, radius, height - 2.0 * radius, color);
    frame.rect(
        x + width - radius,
        y + radius,
        radius,
        height - 2.0 * radius,
        color,
    );
    let rows = CORNER_ROWS.max(1);
    let step = radius / rows as f32;
    for row in 0..rows {
        let along = (row as f32 + 0.5) * step;
        let inset = radius - (radius * radius - along * along).sqrt();
        let (dy_top, dy_bottom) = (
            y + row as f32 * step,
            y + height - radius + row as f32 * step,
        );
        frame.rect(x + radius - inset, dy_top, radius + inset, step, color);
        frame.rect(x + radius - inset, dy_bottom, radius + inset, step, color);
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

/// The stick: a faint translucent disc, a thin ring around its edge, and a round knob that
/// follows the thumb and springs back to the middle when the finger is lifted (a floating
/// joystick, like the virtual controllers of GeForce Now and mobile fighting games). All of
/// it is drawn as rows of rectangles (the frame only has rectangles), fine enough
/// ([`DISC_ROWS`] rows) to look perfectly round at every scale.
fn draw_stick(frame: &mut Frame, touches: &[(f32, f32)]) {
    let (cx, cy) = PAD_CENTER;
    // The disc, fine rows so the edge is smooth.
    let step = 2.0 * PAD_RADIUS / DISC_ROWS as f32;
    for row in 0..DISC_ROWS {
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
    // The ring: a thin band just inside the disc's edge.
    draw_ring(frame, cx, cy, RING_RADIUS);
    // The knob: centred on the thumb when one leans on the stick, clamped inside the ring;
    // resting in the middle otherwise.
    let lean = touches
        .iter()
        .map(|&touch| (touch.0 - cx, touch.1 - cy))
        .find(|&(dx, dy)| dx.hypot(dy) <= PAD_RADIUS);
    let (knob_x, knob_y) = match lean {
        Some((dx, dy)) => {
            let far = dx.hypot(dy);
            let reach = RING_RADIUS - KNOB_RADIUS;
            if far > reach {
                (cx + dx / far * reach, cy + dy / far * reach)
            } else {
                (cx + dx, cy + dy)
            }
        }
        None => (cx, cy),
    };
    let rows = 36;
    let step = 2.0 * KNOB_RADIUS / rows as f32;
    for row in 0..rows {
        let middle = -KNOB_RADIUS + (row as f32 + 0.5) * step;
        let half = (KNOB_RADIUS * KNOB_RADIUS - middle * middle).sqrt();
        let color = if lean.is_some() { KNOB_HELD } else { KNOB };
        frame.rect(
            knob_x - half,
            knob_y - KNOB_RADIUS + row as f32 * step,
            2.0 * half,
            step,
            color,
        );
    }
}

/// A thin circle of radius `radius` centred on `(cx, cy)`: each row is drawn as the two bands
/// between the outer and the inner edge of the circle.
fn draw_ring(frame: &mut Frame, cx: f32, cy: f32, radius: f32) {
    let thickness = 3.0;
    let rows = 96;
    let outer = radius + thickness / 2.0;
    let inner = (radius - thickness / 2.0).max(0.0);
    let step = 2.0 * outer / rows as f32;
    for row in 0..rows {
        let middle = -outer + (row as f32 + 0.5) * step;
        let half = (outer * outer - middle * middle).sqrt();
        let inner_half = if middle.abs() >= inner {
            0.0
        } else {
            (inner * inner - middle * middle).sqrt()
        };
        let y = cy - outer + row as f32 * step;
        let band = half - inner_half;
        frame.rect(cx - half, y, band, step, RING);
        frame.rect(cx + inner_half, y, band, step, RING);
    }
}
