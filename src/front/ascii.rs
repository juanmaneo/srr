//! ASCII art: a grid of coloured characters, a chunky block font to write graffiti with, and a
//! cheap deterministic hash for "random" spray dots that look the same on every machine.

use std::fmt;

use super::scene::{Align, Frame, MONO_ADVANCE, Rgba};

/// A grid of coloured characters; empty cells are transparent.
#[derive(Debug, Clone, PartialEq)]
pub struct Canvas {
    width: usize,
    height: usize,
    cells: Vec<Option<(char, Rgba)>>,
}

impl Canvas {
    /// An empty `width` x `height` canvas.
    pub fn new(width: usize, height: usize) -> Self {
        Canvas {
            width,
            height,
            cells: vec![None; width * height],
        }
    }

    /// Number of columns.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Number of rows.
    pub fn height(&self) -> usize {
        self.height
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let x = usize::try_from(x).ok().filter(|&x| x < self.width)?;
        let y = usize::try_from(y).ok().filter(|&y| y < self.height)?;
        Some(y * self.width + x)
    }

    /// Paints `ch` at column `x`, row `y`; outside the canvas nothing happens. A space clears
    /// the cell.
    pub fn put(&mut self, x: i32, y: i32, ch: char, color: Rgba) {
        if let Some(index) = self.index(x, y) {
            self.cells[index] = (ch != ' ').then_some((ch, color));
        }
    }

    /// The cell at column `x`, row `y`; `None` when empty or outside.
    pub fn get(&self, x: i32, y: i32) -> Option<(char, Rgba)> {
        self.index(x, y).and_then(|index| self.cells[index])
    }

    /// Empties the `width` x `height` cells whose top-left one is at column `x`, row `y`.
    pub fn clear(&mut self, x: i32, y: i32, width: i32, height: i32) {
        for row in y..y + height {
            for column in x..x + width {
                self.put(column, row, ' ', Rgba::rgb(0.0, 0.0, 0.0));
            }
        }
    }

    /// Writes `text` from column `x` on row `y`, one character per cell (spaces are skipped,
    /// so they do not erase what is below).
    pub fn text(&mut self, x: i32, y: i32, text: &str, color: Rgba) {
        for (offset, ch) in (0..).zip(text.chars()) {
            if ch != ' ' {
                self.put(x + offset, y, ch, color);
            }
        }
    }

    /// Adds the canvas to `frame` with its top-left corner at `(left, top)`, characters of
    /// font `size` and rows `line_height` apart. `tint` replaces every colour when given.
    ///
    /// Each row becomes one label per colour, padded with spaces, so that the monospace labels
    /// stack exactly onto the character grid. Empty rows add nothing.
    pub fn draw(
        &self,
        frame: &mut Frame,
        left: f32,
        top: f32,
        size: f32,
        line_height: f32,
        tint: Option<Rgba>,
    ) {
        let advance = size * MONO_ADVANCE;
        for y in 0..self.height {
            let row = &self.cells[y * self.width..(y + 1) * self.width];
            let mut layers: Vec<(Rgba, Vec<char>)> = Vec::new();
            for (x, cell) in row.iter().enumerate() {
                let Some((ch, color)) = *cell else { continue };
                let color = tint.unwrap_or(color);
                let layer = match layers.iter().position(|(c, _)| *c == color) {
                    Some(found) => found,
                    None => {
                        layers.push((color, vec![' '; self.width]));
                        layers.len() - 1
                    }
                };
                layers[layer].1[x] = ch;
            }
            for (color, chars) in layers {
                let start = chars.iter().position(|&c| c != ' ').unwrap_or(0);
                let text: String = chars[start..].iter().collect();
                frame.label(
                    text.trim_end(),
                    left + start as f32 * advance,
                    top + y as f32 * line_height,
                    size,
                    color,
                    Align::Left,
                );
            }
        }
    }
}

impl fmt::Display for Canvas {
    /// The characters only, rows separated by newlines, trailing spaces removed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for y in 0..self.height {
            let row: String = self.cells[y * self.width..(y + 1) * self.width]
                .iter()
                .map(|cell| cell.map_or(' ', |(ch, _)| ch))
                .collect();
            writeln!(f, "{}", row.trim_end())?;
        }
        Ok(())
    }
}

/// Rows of every glyph of the block font.
pub const GLYPH_HEIGHT: usize = 5;

/// The 5-row bitmap of `ch` (`#` is ink), or `None` when the font lacks it. Letters are
/// case-insensitive.
///
/// The font only holds what the title and the HUD need: the letters of "Shattered Realm:
/// Rampage" and of the banners ("READY", "FIGHT!", "KO", "TIME"), the colon, the
/// exclamation mark and the space. A missing letter is silently skipped by [`block_pixels`],
/// so a new banner word needs its letters here.
pub fn glyph(ch: char) -> Option<[&'static str; GLYPH_HEIGHT]> {
    let rows = match ch.to_ascii_uppercase() {
        'A' => [".###.", "#...#", "#####", "#...#", "#...#"],
        'D' => ["####.", "#...#", "#...#", "#...#", "####."],
        'E' => ["#####", "#....", "####.", "#....", "#####"],
        'F' => ["#####", "#....", "####.", "#....", "#...."],
        'G' => [".####", "#....", "#..##", "#...#", ".###."],
        'H' => ["#...#", "#...#", "#####", "#...#", "#...#"],
        'I' => ["###", ".#.", ".#.", ".#.", "###"],
        'K' => ["#...#", "#..#.", "###..", "#..#.", "#...#"],
        'L' => ["#....", "#....", "#....", "#....", "#####"],
        'M' => ["#...#", "##.##", "#.#.#", "#...#", "#...#"],
        'O' => [".###.", "#...#", "#...#", "#...#", ".###."],
        'P' => ["####.", "#...#", "####.", "#....", "#...."],
        'R' => ["####.", "#...#", "####.", "#..#.", "#...#"],
        'S' => [".####", "#....", ".###.", "....#", "####."],
        'T' => ["#####", "..#..", "..#..", "..#..", "..#.."],
        'Y' => ["#...#", "#...#", ".###.", "..#..", "..#.."],
        ':' => [".", "#", ".", "#", "."],
        '!' => ["#", "#", "#", ".", "#"],
        ' ' => ["...", "...", "...", "...", "..."],
        _ => return None,
    };
    Some(rows)
}

/// The ink pixels of `text` written in the block font, as `(column, row)` pairs, with one
/// blank pixel column between glyphs. Characters missing from the font are skipped. Also
/// returns the width in pixels.
///
/// ```
/// use srr::front::ascii::block_pixels;
///
/// let (pixels, width) = block_pixels("T");
/// assert_eq!(width, 5);
/// assert!(pixels.contains(&(2, 4)), "the stem of the T");
/// ```
pub fn block_pixels(text: &str) -> (Vec<(i32, i32)>, i32) {
    let mut pixels = Vec::new();
    let mut x = 0;
    for rows in text.chars().filter_map(glyph) {
        let mut glyph_width = 0;
        for (y, row) in (0..).zip(rows) {
            for (dx, cell) in (0..).zip(row.chars()) {
                if cell == '#' {
                    pixels.push((x + dx, y));
                }
                glyph_width = glyph_width.max(dx + 1);
            }
        }
        x += glyph_width + 1;
    }
    (pixels, (x - 1).max(0))
}

/// A cheap, well-mixed 32-bit hash of three integers (deterministic on every platform), used
/// to scatter spray dots, drips and cracks.
pub fn hash(a: i32, b: i32, c: i32) -> u32 {
    let mut h = (a as u32)
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add((b as u32).wrapping_mul(0x85EB_CA77))
        .wrapping_add((c as u32).wrapping_mul(0xC2B2_AE3D));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}
