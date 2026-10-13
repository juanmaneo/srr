//! The title screen: "SHATTERED REALM: RAMPAGE" spray-painted in ASCII art on a basement brick
//! wall, the way the Streets of Rage covers splash the title over the city.
//!
//! Timeline at 60 ticks per second (everything is a pure function of the tick):
//!
//! | Ticks       | What happens                                                         |
//! | ----------- | -------------------------------------------------------------------- |
//! | 0 – 40      | the brick wall appears, row by row                                   |
//! | 30 – 230    | "SHATTERED", "REALM:" (cold neon) then "RAMPAGE" (hot, slanted) are   |
//! |             | sprayed from left to right in a cloud of paint mist, with a 3D shadow |
//! | 220 – 330   | the red paint of "RAMPAGE" drips down the wall                       |
//! | 236 – 252   | the title *shatters*: white flash, glitching rows, chromatic split,   |
//! |             | and cracks stay across the letters                                   |
//! | 260 –       | the tagline is typed, then "PRESS ENTER" blinks                       |
//! | 420 –       | a short glitch burst every 5 seconds keeps the wall alive            |

use super::ascii::{Canvas, GLYPH_HEIGHT, block_pixels, hash};
use super::scene::{Align, Frame, Rgba, SCREEN_HEIGHT, SCREEN_WIDTH};

/// Columns of the intro's character grid.
pub const COLUMNS: usize = 116;
/// Rows of the intro's character grid.
pub const ROWS: usize = 40;
/// Font size of the grid's characters.
pub const CELL_SIZE: f32 = 18.0;
/// Distance between two rows of the grid.
pub const LINE_HEIGHT: f32 = 18.0;

/// Ticks before Enter / Escape can skip the intro (so that a key still held while the page
/// loads does not skip it by accident).
pub const SKIP_AFTER: u32 = 20;
/// Tick from which the whole title, the tagline and the prompt are on the wall.
pub const COMPLETE_AT: u32 = 330;
/// First tick of the shatter: flash, glitch and cracks.
pub const SHATTER_AT: u32 = 236;
const SHATTER_TICKS: u32 = 16;
const FLASH_TICKS: u32 = 4;
const WALL_TICKS: u32 = 40;
const DRIPS_AT: u32 = 220;
const TAGLINE_AT: u32 = 260;
const PROMPT_AT: u32 = 312;
const PROMPT_BLINK: u32 = 30;
const IDLE_GLITCH_AT: u32 = 420;
const IDLE_GLITCH_PERIOD: u32 = 300;
const IDLE_GLITCH_TICKS: u32 = 10;

/// The line typed under the title.
pub const TAGLINE: &str = "-= a street brawler where every glitch is a move =-";
/// The blinking invitation.
pub const PROMPT: &str = "PRESS ENTER";

/// The screen's colour.
pub const BACKGROUND: Rgba = Rgba::hex(0x0b0910);
const BRICK: Rgba = Rgba::hex(0x2a2036);
const MORTAR: Rgba = Rgba::hex(0x1c1626);
const COLD: [Rgba; 5] = [
    Rgba::hex(0x8afcff),
    Rgba::hex(0x3fd0ff),
    Rgba::hex(0x2a8cff),
    Rgba::hex(0x6a4dff),
    Rgba::hex(0xb43cff),
];
const HOT: [Rgba; 10] = [
    Rgba::hex(0xfff35c),
    Rgba::hex(0xffe14a),
    Rgba::hex(0xffc93a),
    Rgba::hex(0xffa62e),
    Rgba::hex(0xff8a25),
    Rgba::hex(0xff6a1f),
    Rgba::hex(0xff4a1a),
    Rgba::hex(0xf0301a),
    Rgba::hex(0xd81e1e),
    Rgba::hex(0xb01428),
];
const COLD_SHADOW: Rgba = Rgba::hex(0x1d1250);
const HOT_SHADOW: Rgba = Rgba::hex(0x4a0a16);
const COLD_HAZE: Rgba = Rgba::hex(0x3c2c86);
const HOT_HAZE: Rgba = Rgba::hex(0x7c2a1c);
const DRIP: Rgba = Rgba::hex(0xc8141e);
const MIST: Rgba = Rgba::hex(0xf2ecff);
const FLASH: Rgba = Rgba::hex(0xffffff);
const TAGLINE_COLOR: Rgba = Rgba::hex(0xd8d0e8);
const PROMPT_COLOR: Rgba = Rgba::hex(0xffe14a);
const TAGLINE_ROW: i32 = 32;
const PROMPT_ROW: i32 = 35;
const STATUS_Y: f32 = SCREEN_HEIGHT - 40.0;
const STATUS_SIZE: f32 = 18.0;
const STATUS_COLOR: Rgba = Rgba::hex(0x8a8098);
const ABERRATION: [(f32, Rgba); 2] = [(-4.0, Rgba::hex(0xff2bd6)), (4.0, Rgba::hex(0x2bf3ff))];

/// One sprayed word of the title.
struct Word {
    text: &'static str,
    /// Grid cell of the word's top-left corner (before the slant).
    x: i32,
    y: i32,
    /// Cells per font pixel, horizontally and vertically.
    scale: (i32, i32),
    /// Rows per column of slant (0: upright); the top leans right.
    slant: i32,
    /// When the spraying starts and how long it lasts.
    start: u32,
    duration: u32,
    fill: char,
    palette: &'static [Rgba],
    shadow: Rgba,
    haze: Rgba,
    /// Shadow depth in cells, to the right and down.
    depth: i32,
    drips: bool,
    cracks: u32,
}

const WORDS: [Word; 3] = [
    Word {
        text: "SHATTERED",
        x: 5,
        y: 5,
        scale: (2, 1),
        slant: 0,
        start: 30,
        duration: 80,
        fill: '#',
        palette: &COLD,
        shadow: COLD_SHADOW,
        haze: COLD_HAZE,
        depth: 1,
        drips: false,
        cracks: 3,
    },
    Word {
        text: "REALM:",
        x: 27,
        y: 12,
        scale: (2, 1),
        slant: 0,
        start: 90,
        duration: 60,
        fill: '#',
        palette: &COLD,
        shadow: COLD_SHADOW,
        haze: COLD_HAZE,
        depth: 1,
        drips: false,
        cracks: 1,
    },
    Word {
        text: "RAMPAGE",
        x: 14,
        y: 19,
        scale: (2, 2),
        slant: 2,
        start: 150,
        duration: 80,
        fill: '@',
        palette: &HOT,
        shadow: HOT_SHADOW,
        haze: HOT_HAZE,
        depth: 2,
        drips: true,
        cracks: 2,
    },
];

/// One ink cell of a word, with its row inside the word (for the colour gradient).
#[derive(Clone, Copy)]
struct Ink {
    x: i32,
    y: i32,
    row: i32,
}

impl Word {
    fn rows(&self) -> i32 {
        GLYPH_HEIGHT as i32 * self.scale.1
    }

    fn ink(&self) -> Vec<Ink> {
        let (pixels, _) = block_pixels(self.text);
        let (sx, sy) = self.scale;
        let rows = self.rows();
        let mut cells = Vec::with_capacity(pixels.len() * (sx * sy) as usize);
        for (px, py) in pixels {
            for dy in 0..sy {
                let row = py * sy + dy;
                let lean = if self.slant > 0 {
                    (rows - 1 - row) / self.slant
                } else {
                    0
                };
                for dx in 0..sx {
                    cells.push(Ink {
                        x: self.x + px * sx + dx + lean,
                        y: self.y + row,
                        row,
                    });
                }
            }
        }
        cells
    }

    /// Width in cells, slant included.
    fn columns(&self) -> i32 {
        let lean = if self.slant > 0 {
            (self.rows() - 1) / self.slant
        } else {
            0
        };
        block_pixels(self.text).1 * self.scale.0 + lean
    }

    /// The last column reached by the spray at `tick`: everything left of it is painted.
    fn spray_front(&self, tick: u32) -> i32 {
        let elapsed = tick.saturating_sub(self.start).min(self.duration);
        let span = self.columns() + 6;
        self.x - 3 + (span as i64 * elapsed as i64 / self.duration as i64) as i32
    }

    fn spraying(&self, tick: u32) -> bool {
        (self.start..self.start + self.duration).contains(&tick)
    }

    fn color(&self, row: i32) -> Rgba {
        let index = row as usize * self.palette.len() / self.rows() as usize;
        self.palette[index]
    }
}

/// Whether the rows glitch at `tick`: during the shatter and in the idle bursts.
fn glitching(tick: u32) -> bool {
    let shatter = (SHATTER_AT..SHATTER_AT + SHATTER_TICKS).contains(&tick);
    let idle =
        tick >= IDLE_GLITCH_AT && (tick - IDLE_GLITCH_AT) % IDLE_GLITCH_PERIOD < IDLE_GLITCH_TICKS;
    shatter || idle
}

/// The intro's state: just the number of ticks it has been on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Intro {
    tick: u32,
}

impl Intro {
    /// The intro, at its first tick.
    pub fn new() -> Self {
        Intro::default()
    }

    /// The intro at an arbitrary tick (to preview or test a moment of the animation).
    pub fn at(tick: u32) -> Self {
        Intro { tick }
    }

    /// Ticks elapsed.
    pub fn ticks(&self) -> u32 {
        self.tick
    }

    /// Advances the animation by one tick.
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }

    /// Whether Enter / Escape may skip it now.
    pub fn can_skip(&self) -> bool {
        self.tick >= SKIP_AFTER
    }

    /// Whether the whole title, the tagline and the prompt are shown.
    pub fn is_complete(&self) -> bool {
        self.tick >= COMPLETE_AT
    }

    /// Whether the title is glitching (rows shifted, colours split) at this tick.
    pub fn is_glitching(&self) -> bool {
        glitching(self.tick)
    }

    /// The ASCII art on the wall at this tick ([`COLUMNS`] x [`ROWS`]).
    ///
    /// ```
    /// use srr::front::intro::{Intro, COMPLETE_AT};
    ///
    /// let art = Intro::at(COMPLETE_AT).canvas().to_string();
    /// assert!(art.contains("@@@@"), "RAMPAGE is sprayed");
    /// assert!(art.contains("PRESS ENTER"));
    /// ```
    pub fn canvas(&self) -> Canvas {
        let t = self.tick;
        let mut canvas = Canvas::new(COLUMNS, ROWS);
        wall(&mut canvas, t);
        for word in &WORDS {
            spray(&mut canvas, word, t);
        }
        // The texts are stencilled on a clean band of wall so that the bricks don't show
        // through their spaces.
        if t >= TAGLINE_AT {
            let typed = (t - TAGLINE_AT) as usize;
            let shown: String = TAGLINE.chars().take(typed).collect();
            let x = centered(TAGLINE.len());
            canvas.clear(x - 2, TAGLINE_ROW, TAGLINE.len() as i32 + 4, 1);
            canvas.text(x, TAGLINE_ROW, &shown, TAGLINE_COLOR);
        }
        if t >= PROMPT_AT {
            let x = centered(PROMPT.len());
            canvas.clear(x - 2, PROMPT_ROW, PROMPT.len() as i32 + 4, 1);
            if ((t - PROMPT_AT) / PROMPT_BLINK).is_multiple_of(2) {
                canvas.text(x, PROMPT_ROW, PROMPT, PROMPT_COLOR);
            }
        }
        if glitching(t) {
            for y in 0..ROWS as i32 {
                let h = hash(y / 2, (t / 3) as i32, 99);
                if h.is_multiple_of(3) {
                    shift_row(&mut canvas, y, (h / 3 % 9) as i32 - 4);
                }
            }
        }
        canvas
    }

    /// Draws the intro, with `status` (connection state, mode...) at the bottom of the screen.
    pub fn draw(&self, frame: &mut Frame, status: &str) {
        let mut canvas = self.canvas();
        let cell_width = CELL_SIZE * super::scene::MONO_ADVANCE;
        let left = (SCREEN_WIDTH - COLUMNS as f32 * cell_width) / 2.0;
        let top = (SCREEN_HEIGHT - ROWS as f32 * LINE_HEIGHT) / 2.0;
        if !status.is_empty() {
            // Clean wall behind the status line too.
            let width = status.chars().count() as f32 * STATUS_SIZE * super::scene::MONO_ADVANCE;
            let columns = (width / cell_width).ceil() as i32 + 4;
            let first_row = ((STATUS_Y - top) / LINE_HEIGHT).floor() as i32;
            let last_row = ((STATUS_Y + STATUS_SIZE - top) / LINE_HEIGHT).floor() as i32;
            canvas.clear(
                (COLUMNS as i32 - columns) / 2,
                first_row,
                columns,
                last_row - first_row + 1,
            );
        }
        if self.is_glitching() {
            for (dx, tint) in ABERRATION {
                canvas.draw(
                    frame,
                    left + dx,
                    top,
                    CELL_SIZE,
                    LINE_HEIGHT,
                    Some(tint.with_alpha(0.55)),
                );
            }
        }
        canvas.draw(frame, left, top, CELL_SIZE, LINE_HEIGHT, None);
        if !status.is_empty() {
            frame.label(
                status,
                SCREEN_WIDTH / 2.0,
                STATUS_Y,
                STATUS_SIZE,
                STATUS_COLOR,
                Align::Center,
            );
        }
    }
}

fn centered(len: usize) -> i32 {
    (COLUMNS as i32 - len as i32) / 2
}

/// Moves every cell of row `y` by `dx` columns (cells pushed out are lost).
fn shift_row(canvas: &mut Canvas, y: i32, dx: i32) {
    let width = canvas.width() as i32;
    let row: Vec<_> = (0..width).map(|x| canvas.get(x, y)).collect();
    for x in 0..width {
        let source = x - dx;
        let cell = if (0..width).contains(&source) {
            row[source as usize]
        } else {
            None
        };
        match cell {
            Some((ch, color)) => canvas.put(x, y, ch, color),
            None => canvas.put(x, y, ' ', BACKGROUND),
        }
    }
}

/// Bricks 8 cells wide and 3 rows high, every other course offset by half a brick, with a few
/// worn-out spots. Rows appear from the top during the first ticks.
fn wall(canvas: &mut Canvas, tick: u32) {
    let rows = (ROWS as u32 * tick.min(WALL_TICKS) / WALL_TICKS) as i32;
    for y in 0..rows {
        let course = y / 3;
        let offset = if course % 2 == 0 { 0 } else { 4 };
        for x in 0..COLUMNS as i32 {
            if hash(x, y, 7).is_multiple_of(9) {
                continue;
            }
            if y % 3 == 2 {
                canvas.put(x, y, '_', MORTAR);
            } else if (x + offset) % 8 == 0 {
                canvas.put(x, y, '|', BRICK);
            }
        }
    }
}

/// Paints `word` as far as the spray went at `tick`: haze, shadow, ink, cracks, drips and the
/// paint mist at the spray front.
fn spray(canvas: &mut Canvas, word: &Word, tick: u32) {
    if tick < word.start {
        return;
    }
    let front = word.spray_front(tick);
    let ink: Vec<Ink> = word.ink().into_iter().filter(|c| c.x <= front).collect();
    let is_ink = |x: i32, y: i32| ink.iter().any(|c| c.x == x && c.y == y);

    // Overspray around the letters.
    for cell in &ink {
        for (dx, dy) in [(-2, 0), (2, 0), (0, -1), (0, 1), (-1, -1), (1, 1)] {
            let (x, y) = (cell.x + dx, cell.y + dy);
            if hash(x, y, 13).is_multiple_of(7) && !is_ink(x, y) {
                canvas.put(
                    x,
                    y,
                    if hash(x, y, 14).is_multiple_of(2) {
                        '.'
                    } else {
                        '\''
                    },
                    word.haze,
                );
            }
        }
    }
    // 3D extrusion towards the bottom right.
    for cell in &ink {
        for depth in 1..=word.depth {
            let (x, y) = (cell.x + depth, cell.y + depth.min(1));
            if !is_ink(x, y) {
                canvas.put(x, y, if depth == 1 { ':' } else { '.' }, word.shadow);
            }
        }
    }
    let flash = (SHATTER_AT..SHATTER_AT + FLASH_TICKS).contains(&tick);
    for cell in &ink {
        let color = if flash { FLASH } else { word.color(cell.row) };
        canvas.put(cell.x, cell.y, word.fill, color);
    }
    if tick >= SHATTER_AT {
        cracks(canvas, word, &is_ink);
    }
    if word.drips && tick >= DRIPS_AT {
        drips(canvas, word, &ink, tick);
    }
    if word.spraying(tick) {
        mist(canvas, word, front, tick);
    }
}

/// Lightning-shaped cracks through the letters, fixed for each word.
fn cracks(canvas: &mut Canvas, word: &Word, is_ink: &dyn Fn(i32, i32) -> bool) {
    let salt = word.start as i32;
    for crack in 0..word.cracks as i32 {
        let columns = word.columns().max(1) as u32;
        let mut x = word.x + (hash(crack, salt, 21) % columns) as i32;
        for y in word.y..word.y + word.rows() {
            let step = (hash(crack, y, salt) % 3) as i32 - 1;
            let ch = match step {
                -1 => '/',
                1 => '\\',
                _ => '|',
            };
            if is_ink(x, y) {
                canvas.put(x, y, ch, BACKGROUND.mix(MIST, 0.25));
            }
            x += step;
        }
    }
}

/// Paint running down from the bottom edge of the word, each drip at its own pace.
fn drips(canvas: &mut Canvas, word: &Word, ink: &[Ink], tick: u32) {
    let bottom = word.y + word.rows() - 1;
    let elapsed = (tick - DRIPS_AT) as i32;
    for cell in ink.iter().filter(|c| c.y == bottom) {
        let h = hash(cell.x, 31, 0);
        if !h.is_multiple_of(4) {
            continue;
        }
        let length = 1 + (h / 4 % 5) as i32;
        let delay = (h / 32 % 40) as i32;
        let grown = ((elapsed - delay) / 10).clamp(0, length);
        for dy in 1..=grown {
            let ch = if dy == length { 'v' } else { '|' };
            canvas.put(cell.x, bottom + dy, ch, DRIP);
        }
    }
}

/// The cloud of paint around the nozzle while a word is being sprayed.
fn mist(canvas: &mut Canvas, word: &Word, front: i32, tick: u32) {
    for y in word.y - 1..=word.y + word.rows() {
        for x in front - 3..=front + 1 {
            let h = hash(x, y, tick as i32);
            if h.is_multiple_of(3) {
                let ch = ['.', '*', ':', '\''][(h / 3 % 4) as usize];
                canvas.put(x, y, ch, MIST);
            }
        }
    }
}
