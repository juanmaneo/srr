//! A software renderer: the headless "engine" that turns a [`Frame`] into pixels or text
//! without a window, a GPU or an engine.
//!
//! This is server-side rendering for the game, in the spirit of
//! [Blitz](https://github.com/DioxusLabs/blitz) rendering Dioxus apps to images: the same
//! [`Frame`] that Bevy and Fyrox draw is rasterised here into a [`Pixmap`] (and from there into
//! a PNG file), or into a plain-text snapshot. Tests use it to look at what a player would see,
//! and `cargo xtask screenshots` uses it to save the screens to disk.
//!
//! The rules follow the engine contract of [`crate::front`]:
//!
//! * the whole target is cleared with [`Frame::background`] (letterbox bars included);
//! * the virtual screen is mapped with [`Viewport::fit`];
//! * rectangles, then labels, are painted in order with alpha blending. A pixel belongs to a
//!   rectangle when its centre is inside it;
//! * labels use a built-in 5 x 7 bitmap font on the monospace grid (one character is
//!   `size * MONO_ADVANCE` wide), so the ASCII art lines up like with Fira Mono.

use super::scene::{Frame, Label, MONO_ADVANCE, Rect, Rgba, SCREEN_HEIGHT, SCREEN_WIDTH, Viewport};

/// An 8-bit sRGB colour.
pub type Rgb8 = [u8; 3];

/// The 8-bit version of `color`, ignoring its opacity.
///
/// ```
/// use srr::front::Rgba;
/// use srr::front::render::to_rgb8;
///
/// assert_eq!(to_rgb8(Rgba::hex(0x3fc8ff)), [0x3f, 0xc8, 0xff]);
/// ```
pub fn to_rgb8(color: Rgba) -> Rgb8 {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    [channel(color.r), channel(color.g), channel(color.b)]
}

/// An opaque RGB image in memory, row by row from the top-left corner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixmap {
    width: usize,
    height: usize,
    pixels: Vec<Rgb8>,
}

impl Pixmap {
    /// A `width` x `height` image filled with `color` (its opacity is ignored).
    pub fn new(width: usize, height: usize, color: Rgba) -> Self {
        Pixmap {
            width,
            height,
            pixels: vec![to_rgb8(color); width * height],
        }
    }

    /// Width in pixels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Every pixel, row by row.
    pub fn pixels(&self) -> &[Rgb8] {
        &self.pixels
    }

    /// The pixel at column `x`, row `y`; `None` outside the image.
    pub fn pixel(&self, x: usize, y: usize) -> Option<Rgb8> {
        (x < self.width && y < self.height).then(|| self.pixels[y * self.width + x])
    }

    /// How many pixels are exactly `color`.
    pub fn count(&self, color: Rgb8) -> usize {
        self.pixels.iter().filter(|&&pixel| pixel == color).count()
    }

    /// Blends `color` over every pixel whose centre lies in the rectangle `(x, y, width,
    /// height)` (in pixels, may stick out of the image).
    ///
    /// ```
    /// use srr::front::Rgba;
    /// use srr::front::render::Pixmap;
    ///
    /// let mut image = Pixmap::new(4, 4, Rgba::rgb(0.0, 0.0, 0.0));
    /// image.fill_rect(1.0, 1.0, 2.0, 2.0, Rgba::rgb(1.0, 1.0, 1.0).with_alpha(0.5));
    /// assert_eq!(image.pixel(1, 1), Some([128, 128, 128]));
    /// assert_eq!(image.pixel(0, 0), Some([0, 0, 0]));
    /// ```
    pub fn fill_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Rgba) {
        if color.a <= 0.0 {
            return;
        }
        // Pixel `i` has its centre at `i + 0.5`: it is covered when `start <= i + 0.5 < end`.
        let span = |start: f32, length: f32, limit: usize| {
            let first = (start - 0.5).ceil().clamp(0.0, limit as f32) as usize;
            let end = (start + length - 0.5).ceil().clamp(0.0, limit as f32) as usize;
            first..end.max(first)
        };
        let columns = span(x, width, self.width);
        let rows = span(y, height, self.height);
        let alpha = color.a.clamp(0.0, 1.0);
        let source = [color.r, color.g, color.b];
        for row in rows {
            for pixel in &mut self.pixels[row * self.width..][columns.clone()] {
                for (channel, value) in pixel.iter_mut().zip(source) {
                    let below = f32::from(*channel) / 255.0;
                    let mixed = below + (value.clamp(0.0, 1.0) - below) * alpha;
                    *channel = (mixed * 255.0).round() as u8;
                }
            }
        }
    }

    /// The image as a PNG file (8-bit RGB, uncompressed deflate blocks; no dependency).
    ///
    /// ```
    /// use srr::front::Rgba;
    /// use srr::front::render::Pixmap;
    ///
    /// let png = Pixmap::new(2, 2, Rgba::rgb(1.0, 0.0, 0.0)).to_png();
    /// assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    /// ```
    pub fn to_png(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(self.height * (1 + 3 * self.width));
        for row in self.pixels.chunks(self.width.max(1)).take(self.height) {
            raw.push(0); // filter: none
            raw.extend(row.iter().flatten());
        }

        let mut header = Vec::with_capacity(13);
        header.extend((self.width as u32).to_be_bytes());
        header.extend((self.height as u32).to_be_bytes());
        // 8 bits per channel, colour type 2 (RGB), deflate, no filter set, no interlacing.
        header.extend([8, 2, 0, 0, 0]);

        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png_chunk(&mut png, b"IHDR", &header);
        png_chunk(&mut png, b"IDAT", &zlib_stored(&raw));
        png_chunk(&mut png, b"IEND", &[]);
        png
    }
}

fn png_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend((data.len() as u32).to_be_bytes());
    let start = png.len();
    png.extend(kind);
    png.extend(data);
    let crc = crc32(&png[start..]);
    png.extend(crc.to_be_bytes());
}

/// A zlib stream holding `data` in stored (uncompressed) deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    const BLOCK: usize = 0xffff;
    let mut out = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = if data.is_empty() {
        vec![&[]]
    } else {
        data.chunks(BLOCK).collect()
    };
    let last = blocks.len() - 1;
    for (index, block) in blocks.into_iter().enumerate() {
        out.push(u8::from(index == last)); // BFINAL, BTYPE = 00 (stored)
        let length = block.len() as u16;
        out.extend(length.to_le_bytes());
        out.extend((!length).to_le_bytes());
        out.extend(block);
    }
    out.extend(adler32(data).to_be_bytes());
    out
}

/// The CRC-32 (ISO-HDLC, as used by PNG and zip) of `data`.
///
/// ```
/// use srr::front::render::crc32;
///
/// assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
/// ```
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

/// The Adler-32 checksum (as used by zlib) of `data`.
///
/// ```
/// use srr::front::render::adler32;
///
/// assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
/// ```
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

// ---------------------------------------------------------------------------------------------
// Frames to pixels
// ---------------------------------------------------------------------------------------------

/// Where the virtual screen goes on a target: `target = offset + virtual * scale`, per axis.
#[derive(Debug, Clone, Copy)]
struct Mapping {
    scale_x: f32,
    scale_y: f32,
    offset_x: f32,
    offset_y: f32,
}

impl Mapping {
    fn rect(&self, image: &mut Pixmap, rect: &Rect) {
        image.fill_rect(
            self.offset_x + rect.x * self.scale_x,
            self.offset_y + rect.y * self.scale_y,
            rect.width * self.scale_x,
            rect.height * self.scale_y,
            rect.color,
        );
    }
}

fn paint_rects(frame: &Frame, mapping: Mapping, width: usize, height: usize) -> Pixmap {
    let mut image = Pixmap::new(width, height, frame.background);
    for rect in &frame.rects {
        mapping.rect(&mut image, rect);
    }
    image
}

/// Paints `image` (the sprite named by it) onto `target`: its feet on its `(x, y)`, its
/// `height` target pixels tall, mirrored when it faces left. Nearest-neighbour sampling
/// with the sprite's own alpha, over the rectangles.
fn paint_image(target: &mut Pixmap, mapping: Mapping, image: &crate::front::scene::Image) {
    let Some(sprite) = crate::front::skin::sprite_by_name(image.name) else {
        return;
    };
    // The sprite's box on the target: `height` target pixels tall, its own aspect ratio.
    let target_h = image.height * mapping.scale_y;
    let target_w = target_h * sprite.width as f32 / sprite.height.max(1) as f32;
    if target_h < 1.0 || target_w < 1.0 {
        return;
    }
    // The feet at the bottom-centre of the box.
    let feet_x = mapping.offset_x + image.x * mapping.scale_x;
    let feet_y = mapping.offset_y + image.y * mapping.scale_y;
    let left = feet_x - target_w / 2.0;
    let top = feet_y - target_h;
    for row in 0..target_h.ceil() as usize {
        let ty = top + row as f32;
        if ty < 0.0 || ty >= target.height as f32 {
            continue;
        }
        // The source row, nearest-neighbour (the sprite's top is row 0).
        let sy = (((row as f32 / target_h) * sprite.height as f32) as usize).min(sprite.height - 1);
        for column in 0..target_w.ceil() as usize {
            let tx = left + column as f32;
            if tx < 0.0 || tx >= target.width as f32 {
                continue;
            }
            // The source column, mirrored when facing left.
            let sx = ((column as f32 / target_w) * sprite.width as f32) as usize;
            let sx = if image.facing_right {
                sx.min(sprite.width - 1)
            } else {
                sprite.width - 1 - sx.min(sprite.width - 1)
            };
            let i = (sy * sprite.width + sx) * 4;
            let alpha = f32::from(sprite.pixels[i + 3]) / 255.0;
            if alpha <= 0.0 {
                continue;
            }
            let source = [
                f32::from(sprite.pixels[i]) / 255.0,
                f32::from(sprite.pixels[i + 1]) / 255.0,
                f32::from(sprite.pixels[i + 2]) / 255.0,
            ];
            let px = target.width * (ty as usize) + tx as usize;
            for (channel, value) in target.pixels[px].iter_mut().zip(source) {
                let below = f32::from(*channel) / 255.0;
                let mixed = below + (value - below) * alpha;
                *channel = (mixed * 255.0).round() as u8;
            }
        }
    }
}

/// Rows of a glyph of the bitmap font.
pub const FONT_ROWS: usize = 7;
/// Columns of a glyph of the bitmap font.
pub const FONT_COLUMNS: usize = 5;

/// The 5 x 7 bitmap of `ch`: one byte per column, bit 0 is the top row. Printable ASCII only;
/// anything else is drawn as a hollow box.
///
/// ```
/// use srr::front::render::font_glyph;
///
/// assert_eq!(font_glyph(' '), [0; 5]);
/// assert_eq!(font_glyph('|'), [0x00, 0x00, 0x7f, 0x00, 0x00]);
/// assert_eq!(font_glyph('é'), [0x7f, 0x41, 0x41, 0x41, 0x7f]);
/// ```
pub fn font_glyph(ch: char) -> [u8; FONT_COLUMNS] {
    let code = ch as u32;
    if (0x20..0x7f).contains(&code) {
        FONT[(code - 0x20) as usize]
    } else {
        [0x7f, 0x41, 0x41, 0x41, 0x7f]
    }
}

/// Paints `label` on `image`. One character cell is `size * MONO_ADVANCE` wide and `size`
/// tall, split into 6 x 10 dots: the glyph takes 5 x 7 of them, below a 1.5 dot ascent.
fn paint_label(image: &mut Pixmap, mapping: Mapping, label: &Label) {
    let advance = label.size * MONO_ADVANCE;
    let dot_x = advance / 6.0 * mapping.scale_x;
    let dot_y = label.size / 10.0 * mapping.scale_y;
    let top = mapping.offset_y + label.y * mapping.scale_y + 1.5 * dot_y;
    let left = label.left();
    for (index, ch) in label.text.chars().enumerate() {
        let x = mapping.offset_x + (left + index as f32 * advance) * mapping.scale_x;
        for (column, bits) in font_glyph(ch).into_iter().enumerate() {
            for row in (0..FONT_ROWS).filter(|row| (bits >> row) & 1 == 1) {
                image.fill_rect(
                    x + column as f32 * dot_x,
                    top + row as f32 * dot_y,
                    dot_x,
                    dot_y,
                    label.color,
                );
            }
        }
    }
}

/// Renders `frame` into a `width` x `height` image, exactly as a window of that size would
/// show it: letterboxed, rectangles then labels.
///
/// ```
/// use srr::front::{Frame, Rgba};
/// use srr::front::render::{screenshot, to_rgb8};
///
/// let mut frame = Frame::new(Rgba::rgb(0.0, 0.0, 0.0));
/// frame.rect(0.0, 0.0, 640.0, 720.0, Rgba::rgb(1.0, 0.0, 0.0));
/// let image = screenshot(&frame, 128, 72);
/// assert_eq!(image.pixel(10, 10), Some([255, 0, 0]));
/// assert_eq!(image.pixel(100, 10), Some(to_rgb8(frame.background)));
/// ```
pub fn screenshot(frame: &Frame, width: usize, height: usize) -> Pixmap {
    let viewport = Viewport::fit(width as f32, height as f32);
    let mapping = Mapping {
        scale_x: viewport.scale,
        scale_y: viewport.scale,
        offset_x: viewport.offset_x,
        offset_y: viewport.offset_y,
    };
    let mut image = paint_rects(frame, mapping, width, height);
    for sprite in &frame.images {
        paint_image(&mut image, mapping, sprite);
    }
    for label in &frame.labels {
        paint_label(&mut image, mapping, label);
    }
    image
}

/// Characters from dark to bright, for the text snapshot.
pub const SHADES: &[u8] = b" .:-=+*#%@";

/// The character of [`SHADES`] for `color`'s luminance.
pub fn shade(color: Rgb8) -> char {
    let [r, g, b] = color.map(f32::from);
    let luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
    let index = (luminance * (SHADES.len() - 1) as f32).round() as usize;
    char::from(SHADES[index.min(SHADES.len() - 1)])
}

/// Renders `frame` as `rows` lines of `columns` characters, the whole virtual screen stretched
/// onto the grid (no letterbox).
///
/// Rectangles become [`SHADES`] by luminance over the background (which is drawn as spaces),
/// then each label is written one character per cell, from the cell holding the centre of its
/// first character (so text stays readable at any font size; spaces are transparent). Trailing
/// spaces are trimmed. Handy to eyeball a screen in a terminal or to assert on its layout.
///
/// ```
/// use srr::front::{Align, Frame, Rgba};
/// use srr::front::render::text_snapshot;
///
/// let mut frame = Frame::new(Rgba::rgb(0.0, 0.0, 0.0));
/// frame.rect(0.0, 0.0, 640.0, 360.0, Rgba::rgb(1.0, 1.0, 1.0));
/// frame.label("HI", 960.0, 540.0, 40.0, Rgba::rgb(1.0, 1.0, 1.0), Align::Center);
/// assert_eq!(text_snapshot(&frame, 4, 2), "@@\n  HI");
/// ```
pub fn text_snapshot(frame: &Frame, columns: usize, rows: usize) -> String {
    let mapping = Mapping {
        scale_x: columns as f32 / SCREEN_WIDTH,
        scale_y: rows as f32 / SCREEN_HEIGHT,
        offset_x: 0.0,
        offset_y: 0.0,
    };
    let background = to_rgb8(frame.background);
    let image = paint_rects(frame, mapping, columns, rows);
    let mut grid: Vec<Vec<char>> = image
        .pixels()
        .chunks(columns.max(1))
        .take(rows)
        .map(|row| {
            row.iter()
                .map(|&pixel| {
                    if pixel == background {
                        ' '
                    } else {
                        shade(pixel)
                    }
                })
                .collect()
        })
        .collect();
    for label in &frame.labels {
        let x = (label.left() + label.size * MONO_ADVANCE / 2.0) * mapping.scale_x;
        let y = (label.y + label.size / 2.0) * mapping.scale_y;
        let Some(row) = (y >= 0.0).then(|| grid.get_mut(y as usize)).flatten() else {
            continue;
        };
        let start = x.floor() as i64;
        for (column, ch) in (start..).zip(label.text.chars()) {
            let cell = usize::try_from(column).ok().and_then(|c| row.get_mut(c));
            if let (Some(cell), false) = (cell, ch == ' ') {
                *cell = ch;
            }
        }
    }
    let lines: Vec<String> = grid
        .into_iter()
        .map(|row| row.into_iter().collect::<String>().trim_end().to_string())
        .collect();
    lines.join("\n")
}

/// The classic 5 x 7 font for `' '..='~'`, one byte per column, bit 0 at the top.
const FONT: [[u8; FONT_COLUMNS]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], // ' '
    [0x00, 0x00, 0x5f, 0x00, 0x00], // !
    [0x00, 0x07, 0x00, 0x07, 0x00], // "
    [0x14, 0x7f, 0x14, 0x7f, 0x14], // #
    [0x24, 0x2a, 0x7f, 0x2a, 0x12], // $
    [0x23, 0x13, 0x08, 0x64, 0x62], // %
    [0x36, 0x49, 0x55, 0x22, 0x50], // &
    [0x00, 0x05, 0x03, 0x00, 0x00], // '
    [0x00, 0x1c, 0x22, 0x41, 0x00], // (
    [0x00, 0x41, 0x22, 0x1c, 0x00], // )
    [0x14, 0x08, 0x3e, 0x08, 0x14], // *
    [0x08, 0x08, 0x3e, 0x08, 0x08], // +
    [0x00, 0x50, 0x30, 0x00, 0x00], // ,
    [0x08, 0x08, 0x08, 0x08, 0x08], // -
    [0x00, 0x60, 0x60, 0x00, 0x00], // .
    [0x20, 0x10, 0x08, 0x04, 0x02], // /
    [0x3e, 0x51, 0x49, 0x45, 0x3e], // 0
    [0x00, 0x42, 0x7f, 0x40, 0x00], // 1
    [0x42, 0x61, 0x51, 0x49, 0x46], // 2
    [0x21, 0x41, 0x45, 0x4b, 0x31], // 3
    [0x18, 0x14, 0x12, 0x7f, 0x10], // 4
    [0x27, 0x45, 0x45, 0x45, 0x39], // 5
    [0x3c, 0x4a, 0x49, 0x49, 0x30], // 6
    [0x01, 0x71, 0x09, 0x05, 0x03], // 7
    [0x36, 0x49, 0x49, 0x49, 0x36], // 8
    [0x06, 0x49, 0x49, 0x29, 0x1e], // 9
    [0x00, 0x36, 0x36, 0x00, 0x00], // :
    [0x00, 0x56, 0x36, 0x00, 0x00], // ;
    [0x08, 0x14, 0x22, 0x41, 0x00], // <
    [0x14, 0x14, 0x14, 0x14, 0x14], // =
    [0x00, 0x41, 0x22, 0x14, 0x08], // >
    [0x02, 0x01, 0x51, 0x09, 0x06], // ?
    [0x32, 0x49, 0x79, 0x41, 0x3e], // @
    [0x7e, 0x11, 0x11, 0x11, 0x7e], // A
    [0x7f, 0x49, 0x49, 0x49, 0x36], // B
    [0x3e, 0x41, 0x41, 0x41, 0x22], // C
    [0x7f, 0x41, 0x41, 0x22, 0x1c], // D
    [0x7f, 0x49, 0x49, 0x49, 0x41], // E
    [0x7f, 0x09, 0x09, 0x09, 0x01], // F
    [0x3e, 0x41, 0x49, 0x49, 0x7a], // G
    [0x7f, 0x08, 0x08, 0x08, 0x7f], // H
    [0x00, 0x41, 0x7f, 0x41, 0x00], // I
    [0x20, 0x40, 0x41, 0x3f, 0x01], // J
    [0x7f, 0x08, 0x14, 0x22, 0x41], // K
    [0x7f, 0x40, 0x40, 0x40, 0x40], // L
    [0x7f, 0x02, 0x0c, 0x02, 0x7f], // M
    [0x7f, 0x04, 0x08, 0x10, 0x7f], // N
    [0x3e, 0x41, 0x41, 0x41, 0x3e], // O
    [0x7f, 0x09, 0x09, 0x09, 0x06], // P
    [0x3e, 0x41, 0x51, 0x21, 0x5e], // Q
    [0x7f, 0x09, 0x19, 0x29, 0x46], // R
    [0x46, 0x49, 0x49, 0x49, 0x31], // S
    [0x01, 0x01, 0x7f, 0x01, 0x01], // T
    [0x3f, 0x40, 0x40, 0x40, 0x3f], // U
    [0x1f, 0x20, 0x40, 0x20, 0x1f], // V
    [0x3f, 0x40, 0x38, 0x40, 0x3f], // W
    [0x63, 0x14, 0x08, 0x14, 0x63], // X
    [0x07, 0x08, 0x70, 0x08, 0x07], // Y
    [0x61, 0x51, 0x49, 0x45, 0x43], // Z
    [0x00, 0x7f, 0x41, 0x41, 0x00], // [
    [0x02, 0x04, 0x08, 0x10, 0x20], // \
    [0x00, 0x41, 0x41, 0x7f, 0x00], // ]
    [0x04, 0x02, 0x01, 0x02, 0x04], // ^
    [0x40, 0x40, 0x40, 0x40, 0x40], // _
    [0x00, 0x01, 0x02, 0x04, 0x00], // `
    [0x20, 0x54, 0x54, 0x54, 0x78], // a
    [0x7f, 0x48, 0x44, 0x44, 0x38], // b
    [0x38, 0x44, 0x44, 0x44, 0x20], // c
    [0x38, 0x44, 0x44, 0x48, 0x7f], // d
    [0x38, 0x54, 0x54, 0x54, 0x18], // e
    [0x08, 0x7e, 0x09, 0x01, 0x02], // f
    [0x0c, 0x52, 0x52, 0x52, 0x3e], // g
    [0x7f, 0x08, 0x04, 0x04, 0x78], // h
    [0x00, 0x44, 0x7d, 0x40, 0x00], // i
    [0x20, 0x40, 0x44, 0x3d, 0x00], // j
    [0x7f, 0x10, 0x28, 0x44, 0x00], // k
    [0x00, 0x41, 0x7f, 0x40, 0x00], // l
    [0x7c, 0x04, 0x18, 0x04, 0x78], // m
    [0x7c, 0x08, 0x04, 0x04, 0x78], // n
    [0x38, 0x44, 0x44, 0x44, 0x38], // o
    [0x7c, 0x14, 0x14, 0x14, 0x08], // p
    [0x08, 0x14, 0x14, 0x18, 0x7c], // q
    [0x7c, 0x08, 0x04, 0x04, 0x08], // r
    [0x48, 0x54, 0x54, 0x54, 0x20], // s
    [0x04, 0x3f, 0x44, 0x40, 0x20], // t
    [0x3c, 0x40, 0x40, 0x20, 0x7c], // u
    [0x1c, 0x20, 0x40, 0x20, 0x1c], // v
    [0x3c, 0x40, 0x30, 0x40, 0x3c], // w
    [0x44, 0x28, 0x10, 0x28, 0x44], // x
    [0x0c, 0x50, 0x50, 0x50, 0x3c], // y
    [0x44, 0x64, 0x54, 0x4c, 0x44], // z
    [0x00, 0x08, 0x36, 0x41, 0x00], // {
    [0x00, 0x00, 0x7f, 0x00, 0x00], // |
    [0x00, 0x41, 0x36, 0x08, 0x00], // }
    [0x08, 0x04, 0x08, 0x10, 0x08], // ~
];
