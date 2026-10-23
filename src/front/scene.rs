//! The engine-agnostic picture of one frame: coloured rectangles and one-line text labels on a
//! fixed virtual screen.
//!
//! Every screen of the game is drawn into a [`Frame`]. An engine backend only has to clear the
//! window with [`Frame::background`], then draw [`Frame::rects`] and then [`Frame::labels`] in
//! order, mapped from the [`SCREEN_WIDTH`] x [`SCREEN_HEIGHT`] virtual screen to the window with
//! a [`Viewport`]. That is the whole contract between the game and Bevy or Fyrox.

/// Width of the virtual screen every frame is drawn on.
pub const SCREEN_WIDTH: f32 = 1280.0;
/// Height of the virtual screen every frame is drawn on.
pub const SCREEN_HEIGHT: f32 = 720.0;
/// Horizontal advance of one character of the monospace font, as a fraction of the font size.
///
/// Labels are always drawn with a monospace font (Fira Mono: 0.6 em), which is what lets the
/// ASCII art line up when several labels are stacked on the same character grid.
pub const MONO_ADVANCE: f32 = 0.6;

/// A colour in sRGB, every channel within `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
    /// Opacity: 0 is invisible, 1 is opaque.
    pub a: f32,
}

impl Rgba {
    /// An opaque colour.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Rgba { r, g, b, a: 1.0 }
    }

    /// An opaque colour from `0xRRGGBB`.
    ///
    /// ```
    /// use srr::front::Rgba;
    ///
    /// assert_eq!(Rgba::hex(0xff0000), Rgba::rgb(1.0, 0.0, 0.0));
    /// ```
    pub const fn hex(rgb: u32) -> Self {
        Rgba::rgb(
            ((rgb >> 16) & 0xff) as f32 / 255.0,
            ((rgb >> 8) & 0xff) as f32 / 255.0,
            (rgb & 0xff) as f32 / 255.0,
        )
    }

    /// The same colour with opacity `a` (clamped to `0.0..=1.0`).
    pub fn with_alpha(self, a: f32) -> Self {
        Rgba {
            a: a.clamp(0.0, 1.0),
            ..self
        }
    }

    /// The colour with its red, green and blue multiplied by `factor`, clamped to `0.0..=1.0`.
    pub fn scaled(self, factor: f32) -> Self {
        Rgba {
            r: (self.r * factor).clamp(0.0, 1.0),
            g: (self.g * factor).clamp(0.0, 1.0),
            b: (self.b * factor).clamp(0.0, 1.0),
            a: self.a,
        }
    }

    /// The colour `t` of the way from `self` to `other` (`t` clamped to `0.0..=1.0`).
    pub fn mix(self, other: Rgba, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        Rgba {
            r: lerp(self.r, other.r),
            g: lerp(self.g, other.g),
            b: lerp(self.b, other.b),
            a: lerp(self.a, other.a),
        }
    }
}

/// A filled axis-aligned rectangle; `(x, y)` is its top-left corner, y grows downwards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width (never negative).
    pub width: f32,
    /// Height (never negative).
    pub height: f32,
    /// Fill colour.
    pub color: Rgba,
}

/// Which point of a label's line `x` designates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    /// `x` is the left end of the text.
    Left,
    /// `x` is the middle of the text.
    Center,
    /// `x` is the right end of the text.
    Right,
}

/// One line of monospace text. `y` is the top of the line, `x` depends on [`Label::align`].
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    /// The text: a single line, spaces are transparent.
    pub text: String,
    /// Horizontal anchor, see [`Align`].
    pub x: f32,
    /// Top of the line.
    pub y: f32,
    /// Font size in virtual pixels; one character is `size * MONO_ADVANCE` wide.
    pub size: f32,
    /// Text colour.
    pub color: Rgba,
    /// What `x` designates.
    pub align: Align,
}

impl Label {
    /// The left edge of the text, whatever its alignment.
    ///
    /// ```
    /// use srr::front::{Align, Label, Rgba};
    ///
    /// let label = Label {
    ///     text: "ab".into(), x: 100.0, y: 0.0, size: 10.0,
    ///     color: Rgba::rgb(1.0, 1.0, 1.0), align: Align::Right,
    /// };
    /// assert_eq!(label.width(), 12.0);
    /// assert_eq!(label.left(), 88.0);
    /// ```
    pub fn left(&self) -> f32 {
        match self.align {
            Align::Left => self.x,
            Align::Center => self.x - self.width() / 2.0,
            Align::Right => self.x - self.width(),
        }
    }

    /// The width of the text in the monospace font.
    pub fn width(&self) -> f32 {
        self.text.chars().count() as f32 * self.size * MONO_ADVANCE
    }
}

/// Everything to draw for one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// The colour the whole window is cleared with.
    pub background: Rgba,
    /// Drawn first, in order (later ones on top).
    pub rects: Vec<Rect>,
    /// Drawn above every rectangle, in order.
    pub labels: Vec<Label>,
}

impl Frame {
    /// An empty frame cleared with `background`.
    pub fn new(background: Rgba) -> Self {
        Frame {
            background,
            rects: Vec::new(),
            labels: Vec::new(),
        }
    }

    /// Adds a rectangle; negative sizes are clamped to zero.
    pub fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Rgba) {
        self.rects.push(Rect {
            x,
            y,
            width: width.max(0.0),
            height: height.max(0.0),
            color,
        });
    }

    /// Adds a label.
    pub fn label(
        &mut self,
        text: impl Into<String>,
        x: f32,
        y: f32,
        size: f32,
        color: Rgba,
        align: Align,
    ) {
        self.labels.push(Label {
            text: text.into(),
            x,
            y,
            size,
            color,
            align,
        });
    }

    /// Every label's text, one per line: handy to check what a screen says.
    pub fn text(&self) -> String {
        let lines: Vec<&str> = self.labels.iter().map(|l| l.text.as_str()).collect();
        lines.join("\n")
    }
}

/// How the virtual screen maps onto a window: uniformly scaled to fit, centred, letterboxed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// Window pixels per virtual pixel.
    pub scale: f32,
    /// Window x of the virtual screen's left edge.
    pub offset_x: f32,
    /// Window y of the virtual screen's top edge.
    pub offset_y: f32,
}

impl Viewport {
    /// The largest uniform scale that fits the virtual screen in a `width` x `height` window,
    /// centred. Degenerate windows (zero, negative or NaN sizes) get a scale of zero.
    ///
    /// ```
    /// use srr::front::Viewport;
    ///
    /// let wide = Viewport::fit(2560.0, 1000.0);
    /// assert_eq!(wide.scale, 1000.0 / 720.0);
    /// assert_eq!(wide.offset_y, 0.0);
    /// assert!(wide.offset_x > 0.0, "black bars on the sides");
    /// ```
    pub fn fit(width: f32, height: f32) -> Self {
        let (by_width, by_height) = (width / SCREEN_WIDTH, height / SCREEN_HEIGHT);
        let scale = by_width.min(by_height);
        // `f32::min` ignores NaN, so the window sizes themselves are checked too.
        if !(width > 0.0 && height > 0.0 && scale.is_finite() && scale > 0.0) {
            return Viewport {
                scale: 0.0,
                offset_x: 0.0,
                offset_y: 0.0,
            };
        }
        // The limiting axis is filled exactly (no rounding error); the other one is centred.
        if by_width <= by_height {
            Viewport {
                scale,
                offset_x: 0.0,
                offset_y: (height - SCREEN_HEIGHT * scale).max(0.0) / 2.0,
            }
        } else {
            Viewport {
                scale,
                offset_x: (width - SCREEN_WIDTH * scale).max(0.0) / 2.0,
                offset_y: 0.0,
            }
        }
    }

    /// Window coordinates (y down) of the virtual point `(x, y)`.
    pub fn to_window(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.offset_x + x * self.scale,
            self.offset_y + y * self.scale,
        )
    }

    /// The virtual point under the window point `(x, y)`: the inverse of
    /// [`Viewport::to_window`] (a touch on the screen, in the game's coordinates). A degenerate
    /// viewport maps everything to the origin.
    ///
    /// ```
    /// use srr::front::Viewport;
    ///
    /// let viewport = Viewport::fit(2560.0, 1000.0);
    /// let (x, y) = viewport.to_window(100.0, 200.0);
    /// let back = viewport.to_virtual(x, y);
    /// assert!((back.0 - 100.0).abs() < 1e-3 && (back.1 - 200.0).abs() < 1e-3);
    /// assert_eq!(Viewport::fit(0.0, 0.0).to_virtual(5.0, 5.0), (0.0, 0.0));
    /// ```
    pub fn to_virtual(&self, x: f32, y: f32) -> (f32, f32) {
        if self.scale > 0.0 {
            (
                (x - self.offset_x) / self.scale,
                (y - self.offset_y) / self.scale,
            )
        } else {
            (0.0, 0.0)
        }
    }
}
