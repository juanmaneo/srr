//! The manga skin: **one sprite sheet per character**, sliced deterministically. Each of
//! the ten [`Variant`]s is a single PNG in `assets/skins/manga/` holding every movement of
//! the fighter in a fixed 3 x 3 grid, in the fixed order of [`Pose::ALL`] — the same sheet
//! layout for every character, so the ten girls can fight side by side with the 2D chalk
//! sticks at the same on-screen size (the arena draws every fighter 150 virtual pixels
//! tall, whatever it wears).
//!
//! The sheets are generated art (an open-weight image model paints each girl in the
//! KonoSuba school of bright cel shading, big glossy eyes and bold ink outlines; the move
//! list follows the broad tradition of sprite brawlers like Phantom Breaker: Battle
//! Grounds — nothing is copied from either), then normalised: each cell cropped to its
//! fighter, seated on the cell's floor, scaled to the same height. One image per character
//! is the point: a diffusion model asked for one pose at a time drifts (hair colour, hem
//! length, broken legs); one sheet keeps the fighter identical across its movements.
//!
//! Everything else the game draws is rectangles ([`Frame`]), so the headless renderer, Bevy
//! and Fyrox all draw the same art: the PNG is decoded here (a small DEFLATE decoder, no
//! dependency) into RGBA, sliced by arithmetic. The 3D view (`Tab`) reads the same
//! palettes ([`Palette`]) for its cel-shaded volumes.
//!
//! No character copies anyone's protected likeness: every face, haircut and outfit is the
//! game's own; only the broad manga rendering techniques (cel shading, big glossy eyes,
//! clean ink outlines) are borrowed.

use std::sync::OnceLock;

use super::scene::Rgba;
use crate::game::{Fighter, FighterState, Limb, Skin, Variant};

// -----------------------------------------------------------------------------------------
// The palettes (the 3D view's volumes)
// -----------------------------------------------------------------------------------------

/// The colours and the look a variant wears: what the 3D view reads for its cel volumes.
///
/// ```
/// use srr::front::skin::Palette;
/// use srr::game::Variant;
///
/// let palette = Palette::of(Variant::Brunette);
/// assert!(palette.hair != palette.skin);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// The hair.
    pub hair: Rgba,
    /// The shadow tone of the hair.
    pub hair_shadow: Rgba,
    /// The skin of the face and the hands.
    pub skin: Rgba,
    /// The shadow tone of the skin.
    pub skin_shadow: Rgba,
    /// The outfit (the torso, the legs — or the stockings under a dress).
    pub outfit: Rgba,
    /// The shadow tone of the outfit.
    pub outfit_shadow: Rgba,
    /// A second outfit colour: the trims, the shoes, the top.
    pub trim: Rgba,
    /// The eyes.
    pub eye: Rgba,
    /// The shape of the body and the hair, per variant.
    pub look: Look,
}

/// The body and hair proportions of a variant: how the 3D volumes differ per girl.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Half the width of the shoulders.
    pub shoulders: f32,
    /// Half the width of the waist.
    pub waist: f32,
    /// Half the width of the hips (a skirt flares wider).
    pub hips: f32,
    /// Half the width of an arm.
    pub arm: f32,
    /// Half the width of a leg (a stocking-covered leg is thin).
    pub leg: f32,
    /// The length of the hair behind the head, in head radii (0: short).
    pub hair_length: f32,
    /// The hair silhouette: twintails, a braid, a mane.
    pub hair_shape: HairShape,
}

/// The silhouette of the hair, drawn per variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HairShape {
    /// Twintails: two masses flying out from the sides of the head.
    Twintails,
    /// A long tied braid down the back.
    Braid,
    /// A flowing mane down the back.
    Mane,
    /// Short spiky hair.
    Spikes,
    /// A bob cut framing the face.
    Bob,
    /// Drill curls.
    Drills,
}

impl Look {
    /// The look of a manga girl: an hourglass body, thin limbs, long hair.
    const GIRL: Look = Look {
        shoulders: 8.0,
        waist: 5.0,
        hips: 9.0,
        arm: 2.6,
        leg: 3.0,
        hair_length: 2.8,
        hair_shape: HairShape::Mane,
    };
}

impl Palette {
    /// The palette of `variant`: ten girls of the KonoSuba school — bright hair colours,
    /// fighting dresses, thigh-high stockings — each her own girl, none anyone else's.
    pub fn of(variant: Variant) -> Palette {
        let skin = Rgba::hex(0xffe0c2);
        let shadow = |color: Rgba| color.scaled(0.62);
        let palette = |hair, skin, outfit, trim, eye, look| Palette {
            hair,
            hair_shadow: shadow(hair),
            skin,
            skin_shadow: shadow(skin),
            outfit,
            outfit_shadow: shadow(outfit),
            trim,
            eye,
            look,
        };
        match variant {
            // The brunette: brown twintails and a red fighting dress.
            Variant::Brunette => palette(
                Rgba::hex(0x7a4a28),
                skin,
                Rgba::hex(0xd83860),
                Rgba::hex(0xfff0e2),
                Rgba::hex(0x3a9e5c),
                Look {
                    hair_shape: HairShape::Twintails,
                    ..Look::GIRL
                },
            ),
            // The blonde: golden hair tied in a long braid, a blue fighting dress.
            Variant::Blonde => palette(
                Rgba::hex(0xf7d968),
                skin,
                Rgba::hex(0x4a42b8),
                Rgba::hex(0xf0f0ff),
                Rgba::hex(0x2e7ed0),
                Look {
                    hair_shape: HairShape::Braid,
                    ..Look::GIRL
                },
            ),
            // The redhead: a fierce red mane, a dark fighting dress with gold trims.
            Variant::Redhead => palette(
                Rgba::hex(0xe85830),
                skin,
                Rgba::hex(0x3a3448),
                Rgba::hex(0xffd24a),
                Rgba::hex(0x44b03c),
                Look {
                    hair_shape: HairShape::Mane,
                    ..Look::GIRL
                },
            ),
            // The pink: pastel pink hair in drills, a violet dress.
            Variant::Pink => palette(
                Rgba::hex(0xff9ec4),
                skin,
                Rgba::hex(0x8a52c8),
                Rgba::hex(0xfff0fa),
                Rgba::hex(0xd05a9e),
                Look {
                    hair_shape: HairShape::Drills,
                    ..Look::GIRL
                },
            ),
            // The white: silver bob hair, a green dress.
            Variant::White => palette(
                Rgba::hex(0xdcd8d4),
                skin,
                Rgba::hex(0x4a9e5c),
                Rgba::hex(0xf0fff4),
                Rgba::hex(0x2a7a3a),
                Look {
                    hair_shape: HairShape::Bob,
                    ..Look::GIRL
                },
            ),
            // The black: navy-black mane, a cream dress.
            Variant::Black => palette(
                Rgba::hex(0x3e3832),
                skin,
                Rgba::hex(0xf0e8d0),
                Rgba::hex(0xc8b088),
                Rgba::hex(0x6a4a2a),
                Look {
                    hair_length: 3.2,
                    hair_shape: HairShape::Mane,
                    ..Look::GIRL
                },
            ),
            // The green: green twintails, a red dress.
            Variant::Green => palette(
                Rgba::hex(0x5ac84a),
                skin,
                Rgba::hex(0xd03a4a),
                Rgba::hex(0xffe8e0),
                Rgba::hex(0x3a7a2a),
                Look {
                    hair_shape: HairShape::Twintails,
                    ..Look::GIRL
                },
            ),
            // The violet: violet mane, a gold dress.
            Variant::Violet => palette(
                Rgba::hex(0x9060e8),
                skin,
                Rgba::hex(0xe8b84a),
                Rgba::hex(0xfff8e0),
                Rgba::hex(0x6a3ad0),
                Look {
                    hair_shape: HairShape::Mane,
                    ..Look::GIRL
                },
            ),
            // The cyan: cyan bob, a magenta dress.
            Variant::Cyan => palette(
                Rgba::hex(0x4ad4e0),
                skin,
                Rgba::hex(0xd0408a),
                Rgba::hex(0xf0f8ff),
                Rgba::hex(0x2a8a9a),
                Look {
                    hair_shape: HairShape::Bob,
                    ..Look::GIRL
                },
            ),
            // The raven: dark blue mane, a teal dress.
            Variant::Raven => palette(
                Rgba::hex(0x3848a0),
                skin,
                Rgba::hex(0x3aa89a),
                Rgba::hex(0xd8fff4),
                Rgba::hex(0x2a4a8a),
                Look {
                    hair_length: 3.0,
                    hair_shape: HairShape::Mane,
                    ..Look::GIRL
                },
            ),
        }
    }
}

/// The palette `skin` wears: the variant's own, or the plain chalk ink.
pub fn palette_of(skin: &Skin) -> Palette {
    skin.variant.map_or_else(
        || Palette {
            hair: super::stick::INK,
            hair_shadow: super::stick::INK,
            skin: super::stick::INK,
            skin_shadow: super::stick::INK,
            outfit: super::stick::INK,
            outfit_shadow: super::stick::INK,
            trim: super::stick::INK,
            eye: super::stick::INK,
            look: Look::GIRL,
        },
        Palette::of,
    )
}

// -----------------------------------------------------------------------------------------
// The sprite sheet engine
// -----------------------------------------------------------------------------------------

/// Every action the sprite sheet holds, in the fixed order of the grid: row-major, three
/// cells a row. A fighter without its own pose falls back to standing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pose {
    /// Standing in guard (the top-left cell).
    Stand,
    /// The two walk frames, alternating with the steps.
    WalkA,
    WalkB,
    /// A straight punch.
    Punch,
    /// A high kick.
    Kick,
    /// Crouching low.
    Crouch,
    /// In the air.
    Jump,
    /// Recoiling from a hit.
    Hit,
    /// Lying knocked out.
    Ko,
}

impl Pose {
    /// Every pose, in the fixed order of the sheet's cells (row-major).
    pub const ALL: [Pose; 9] = [
        Pose::Stand,
        Pose::WalkA,
        Pose::WalkB,
        Pose::Punch,
        Pose::Kick,
        Pose::Crouch,
        Pose::Jump,
        Pose::Hit,
        Pose::Ko,
    ];

    /// The index of the pose's cell in reading order.
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|pose| *pose == self)
            .expect("a pose")
    }
}

/// The cells of the grid, per row: three a row.
const GRID: usize = 3;
/// The width of one cell of the grid, in sheet pixels: every character, every pose, the
/// same box — the reason a manga girl can fight a chalk stick.
pub const CELL_WIDTH: usize = 120;
/// The height of one cell: as tall as a fighter is on screen (the arena draws 150).
pub const CELL_HEIGHT: usize = 160;
/// The sheet PNGs, one per character, in [`Variant::ALL`] order: the file is the fighter.
const SHEETS: [&[u8]; 10] = [
    include_bytes!("../../assets/skins/manga/brunette.png"),
    include_bytes!("../../assets/skins/manga/blonde.png"),
    include_bytes!("../../assets/skins/manga/redhead.png"),
    include_bytes!("../../assets/skins/manga/pink.png"),
    include_bytes!("../../assets/skins/manga/white.png"),
    include_bytes!("../../assets/skins/manga/black.png"),
    include_bytes!("../../assets/skins/manga/green.png"),
    include_bytes!("../../assets/skins/manga/violet.png"),
    include_bytes!("../../assets/skins/manga/cyan.png"),
    include_bytes!("../../assets/skins/manga/raven.png"),
];

/// Every cell of every sheet, decoded once: `[variant * 9 + pose]`, each `CELL_WIDTH` x
/// `CELL_HEIGHT` RGBA — the game lives as long as its art.
struct Cells {
    bytes: Vec<Vec<u8>>,
}

/// All the sheets, decoded on first use (the decode is deterministic; a `OnceLock` shares
/// one decode across every engine's texture cache).
static CELLS: OnceLock<Cells> = OnceLock::new();

/// The decoded cells, built once.
fn cells() -> &'static Cells {
    CELLS.get_or_init(|| {
        let mut bytes = Vec::with_capacity(SHEETS.len() * Pose::ALL.len());
        for sheet in SHEETS {
            let pixels = sheet_rgba(sheet).expect("the sheet decodes");
            for pose in Pose::ALL {
                bytes.push(cell(&pixels, GRID * CELL_WIDTH, pose.index()));
            }
        }
        Cells { bytes }
    })
}

/// The cell of `pose` out of a decoded sheet: its rows, joined into one RGBA run.
fn cell(pixels: &[u8], sheet_width: usize, index: usize) -> Vec<u8> {
    let column = index % GRID;
    let row = index / GRID;
    let stride = sheet_width * 4;
    let mut out = Vec::with_capacity(CELL_WIDTH * CELL_HEIGHT * 4);
    for y in 0..CELL_HEIGHT {
        let from = (row * CELL_HEIGHT + y) * stride + column * CELL_WIDTH * 4;
        out.extend_from_slice(&pixels[from..from + CELL_WIDTH * 4]);
    }
    out
}

/// A decoded sprite: the RGBA bytes of one cell, row by row, and its size.
///
/// ```
/// use srr::front::skin::{CELL_HEIGHT, CELL_WIDTH, Pose, sprite};
/// use srr::game::Variant;
///
/// let sprite = sprite(Variant::Brunette, Pose::Stand);
/// assert_eq!(sprite.width, CELL_WIDTH);
/// assert_eq!(sprite.height, CELL_HEIGHT);
/// assert_eq!(sprite.pixels.len(), CELL_WIDTH * CELL_HEIGHT * 4);
/// ```
#[derive(Debug)]
pub struct Sprite<'a> {
    /// The cell's pixels, RGBA, row by row from the top-left corner.
    pub pixels: &'a [u8],
    /// The cell's width in pixels.
    pub width: usize,
    /// The cell's height in pixels.
    pub height: usize,
}

/// The sprite of `variant` in `pose`: one cell of the character's sheet.
///
/// ```
/// use srr::front::skin::{Pose, sprite};
/// use srr::game::Variant;
///
/// for variant in Variant::ALL {
///     for pose in Pose::ALL {
///         let sprite = sprite(variant, pose);
///         assert!(sprite.width > 0 && sprite.height > 0);
///     }
/// }
/// ```
pub fn sprite(variant: Variant, pose: Pose) -> Sprite<'static> {
    let index = variant.index() as usize * Pose::ALL.len() + pose.index();
    Sprite {
        pixels: &cells().bytes[index],
        width: CELL_WIDTH,
        height: CELL_HEIGHT,
    }
}

// -----------------------------------------------------------------------------------------
// The PNG decode (no dependency: the sheets are 8-bit RGBA, one IDAT, zlib)
// -----------------------------------------------------------------------------------------

/// Decodes a sheet PNG: [`png_rgba`] with the 3 x 3 grid contract enforced — the cells
/// are sliced by arithmetic, so the sheet must be exactly the size the game was built for.
///
/// ```
/// # use srr::front::skin::sheet_rgba;
/// let not_a_sheet = sheet_rgba(b"not a png");
/// assert_eq!(not_a_sheet, None);
/// ```
pub fn sheet_rgba(bytes: &[u8]) -> Option<Vec<u8>> {
    let pixels = png_rgba(bytes)?;
    // The 3 x 3 grid contract, by size alone: the cells are sliced by arithmetic,
    // so the sheet must be exactly the box the game was built for.
    if pixels.len() != GRID * CELL_WIDTH * GRID * CELL_HEIGHT * 4 {
        return None;
    }
    Some(pixels)
}

/// Decodes an 8-bit RGBA PNG (non-interlaced) into raw RGBA pixels, row by row.
///
/// ```
/// # use srr::front::skin::png_rgba;
/// let too_short = png_rgba(b"not a png");
/// assert_eq!(too_short, None);
/// ```
pub fn png_rgba(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.get(0..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let mut width = 0;
    let mut height = 0;
    let mut idat = Vec::new();
    let mut cursor = 8;
    while let Some((kind, data, next)) = chunk(bytes, cursor) {
        match kind {
            b"IHDR" => {
                width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
                height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
                // 8-bit truecolour with alpha, the only flavour the art pipeline writes.
                if data[8] != 8 || data[9] != 6 || data[12] != 0 {
                    return None;
                }
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        cursor = next;
    }
    if width == 0 || height == 0 {
        return None;
    }
    let raw = inflate_zlib(&idat)?;
    let stride = width * 4;
    let mut pixels = vec![0u8; width * height * 4];
    let mut previous = vec![0u8; stride];
    let mut at = 0;
    for y in 0..height {
        let filter = *raw.get(at)?;
        at += 1;
        let line = raw.get(at..at + stride)?;
        at += stride;
        let mut current = line.to_vec();
        unfilter(filter, &mut current, &previous)?;
        pixels[y * stride..(y + 1) * stride].copy_from_slice(&current);
        previous = current;
    }
    Some(pixels)
}

/// The chunk at `cursor`: `(kind, data, cursor of the next chunk)`.
fn chunk(bytes: &[u8], cursor: usize) -> Option<(&[u8; 4], &[u8], usize)> {
    // The header is fixed-size: once the slice is in bounds, the four bytes are there.
    let header: [u8; 4] = bytes
        .get(cursor..cursor + 4)?
        .try_into()
        .expect("four bytes");
    let length = u32::from_be_bytes(header) as usize;
    let kind: &[u8; 4] = bytes
        .get(cursor + 4..cursor + 8)?
        .try_into()
        .expect("four bytes");
    let data = bytes.get(cursor + 8..cursor + 8 + length)?;
    Some((kind, data, cursor + 12 + length))
}

/// Undoes one scanline filter (RFC 2083, the PNG filter algorithms).
fn unfilter(filter: u8, line: &mut [u8], previous: &[u8]) -> Option<()> {
    let stride = line.len();
    match filter {
        // None: the bytes are the scanline.
        0 => {}
        // Sub: the byte plus the one `bpp` bytes back.
        1 => {
            for i in 4..stride {
                line[i] = line[i].wrapping_add(line[i - 4]);
            }
        }
        // Up: the byte plus the one above.
        2 => {
            for i in 0..stride {
                line[i] = line[i].wrapping_add(previous[i]);
            }
        }
        // Average: the byte plus half the sum of the left and the above.
        3 => {
            for i in 0..stride {
                let left = u16::from(if i >= 4 { line[i - 4] } else { 0 });
                let above = u16::from(previous[i]);
                line[i] = line[i].wrapping_add(((left + above) / 2) as u8);
            }
        }
        // Paeth: the byte plus the nearest of the left, the above and the corner.
        4 => {
            for i in 0..stride {
                let left = i32::from(if i >= 4 { line[i - 4] } else { 0 });
                let above = i32::from(previous[i]);
                let corner = i32::from(if i >= 4 { previous[i - 4] } else { 0 });
                line[i] = line[i].wrapping_add(paeth(left, above, corner) as u8);
            }
        }
        _ => return None,
    }
    Some(())
}

/// The Paeth predictor (RFC 2083): the nearest of the left, the above and the corner,
/// by absolute distance, the left winning the ties.
fn paeth(left: i32, above: i32, corner: i32) -> i32 {
    let estimate = left + above - corner;
    let (distance_left, distance_above, distance_corner) = (
        (estimate - left).abs(),
        (estimate - above).abs(),
        (estimate - corner).abs(),
    );
    if distance_left <= distance_above && distance_left <= distance_corner {
        left
    } else if distance_above <= distance_corner {
        above
    } else {
        corner
    }
}

/// Inflates a zlib stream (RFC 1950): the header, then the DEFLATE data.
fn inflate_zlib(bytes: &[u8]) -> Option<Vec<u8>> {
    // The two-byte header: the compression method and the check bits.
    if bytes.first()? & 0x0F != 8 || bytes.get(1)? & 0x20 != 0 {
        return None;
    }
    inflate(&bytes[2..])
}

/// Inflates a DEFLATE stream (RFC 1951): the raw bytes.
///
/// ```
/// # use srr::front::skin::inflate;
/// // A stored block: one byte, then the end.
/// let stored = [0x01, 0x01, 0x00, 0xfe, 0xff, 0x42];
/// assert_eq!(inflate(&stored), Some(vec![0x42]));
/// ```
pub fn inflate(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut reader = BitReader::new(bytes);
    loop {
        // The block header (the last flag, then the kind) is one three-bit read.
        let header = reader.bits(3)?;
        let (last, kind) = (header & 1, header >> 1);
        match kind {
            // A stored block: back to byte alignment, then the length and its inverse.
            0 => {
                reader.align();
                let len = u16::from(reader.byte()?) | (u16::from(reader.byte()?) << 8);
                let nlen = u16::from(reader.byte()?) | (u16::from(reader.byte()?) << 8);
                if len != !nlen {
                    return None;
                }
                out.extend(reader.take(len as usize)?);
            }
            // The fixed Huffman codes.
            1 => inflate_block(&mut reader, &fixed_literals(), &fixed_distances(), &mut out)?,
            // The dynamic Huffman codes the block declares in its head.
            2 => {
                let (literals, distances) = dynamic_codes(&mut reader)?;
                inflate_block(&mut reader, &literals, &distances, &mut out)?;
            }
            _ => return None,
        }
        if last == 1 {
            return Some(out);
        }
    }
}

/// The bit reader of a DEFLATE stream: the bits of a byte, the least significant first.
struct BitReader<'a> {
    bytes: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        BitReader { bytes, bit: 0 }
    }

    /// One bit.
    fn bit(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.bit / 8)?;
        let value = (byte >> (self.bit % 8)) & 1;
        self.bit += 1;
        Some(value)
    }

    /// The low `n` bits, the least significant bit first.
    fn bits(&mut self, n: u32) -> Option<u16> {
        let mut value = 0u16;
        for i in 0..n {
            value |= u16::from(self.bit()?) << i;
        }
        Some(value)
    }

    /// Skips to the next byte boundary.
    fn align(&mut self) {
        self.bit = self.bit.div_ceil(8) * 8;
    }

    /// One byte, byte-aligned.
    fn byte(&mut self) -> Option<u8> {
        let value = self.byte_unaligned()?;
        self.align();
        Some(value)
    }

    /// One byte where it stands.
    fn byte_unaligned(&mut self) -> Option<u8> {
        let mut value = 0u8;
        for i in 0..8 {
            value |= self.bit()? << i;
        }
        Some(value)
    }

    /// `n` raw bytes, byte-aligned.
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        self.align();
        let from = self.bit / 8;
        let bytes = self.bytes.get(from..from + n)?;
        self.bit += n * 8;
        Some(bytes)
    }
}

/// A canonical Huffman code (RFC 1951 section 3.2.2).
struct Huffman {
    /// How many codes are `n` bits long: `counts[n]`.
    counts: [u16; 16],
    /// The symbols, ordered by code length, then by code.
    symbols: Vec<u16>,
}

impl Huffman {
    /// The code of `lengths`: one slot per symbol, 0 for unused.
    fn new(lengths: &[u8]) -> Self {
        let mut counts = [0u16; 16];
        for &length in lengths {
            counts[length as usize] += 1;
        }
        counts[0] = 0;
        let mut offsets = [0u16; 16];
        for length in 1..16 {
            offsets[length] = offsets[length - 1] + counts[length - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                symbols[offsets[length as usize] as usize] = symbol as u16;
                offsets[length as usize] += 1;
            }
        }
        Huffman { counts, symbols }
    }

    /// Reads one symbol: the bits, the most significant of the code first.
    fn read(&self, reader: &mut BitReader<'_>) -> Option<u16> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for length in 1..16 {
            code |= i32::from(reader.bit()?);
            let count = i32::from(self.counts[length]);
            if code - first < count {
                return self.symbols.get((index + code - first) as usize).copied();
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        None
    }
}

/// The fixed literal code (RFC 1951 section 3.2.6): seven to nine bits.
fn fixed_literals() -> Huffman {
    let mut lengths = [0u8; 288];
    for (symbol, length) in lengths.iter_mut().enumerate() {
        *length = match symbol {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    Huffman::new(&lengths)
}

/// The fixed distance code: five bits, thirty codes.
fn fixed_distances() -> Huffman {
    Huffman::new(&[5u8; 30])
}

/// The dynamic Huffman codes a block declares in its head (RFC 1951 section 3.2.7).
fn dynamic_codes(reader: &mut BitReader<'_>) -> Option<(Huffman, Huffman)> {
    // The three counts arrive together (HLIT, HDIST, HCLEN), read as one: the DEFLATE
    // header is bit-packed, so a short stream fails the whole read at once.
    let counts = reader.bits(14)?;
    let literals = usize::from(counts & 0x1F) + 257;
    let distances = usize::from((counts >> 5) & 0x1F) + 1;
    let declared = usize::from((counts >> 10) & 0xF) + 4;
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut lengths = [0u8; 19];
    for &slot in ORDER.iter().take(declared) {
        lengths[slot] = reader.bits(3)? as u8;
    }
    let code_lengths = Huffman::new(&lengths);
    let mut all = Vec::with_capacity(literals + distances);
    while all.len() < literals + distances {
        let symbol = code_lengths.read(reader)?;
        match symbol {
            0..=15 => all.push(symbol as u8),
            // Copy the previous length.
            16 => {
                let repeat = usize::from(reader.bits(2)?) + 3;
                let last = *all.last()?;
                for _ in 0..repeat {
                    all.push(last);
                }
            }
            // A run of zeros.
            17 => all.extend(std::iter::repeat_n(0, usize::from(reader.bits(3)?) + 3)),
            // The longer runs of zeros: the table holds only the nineteen code-length
            // symbols, so nothing past 18 is ever read.
            18.. => all.extend(std::iter::repeat_n(0, usize::from(reader.bits(7)?) + 11)),
        }
    }
    if all.len() != literals + distances {
        return None;
    }
    Some((
        Huffman::new(&all[..literals]),
        Huffman::new(&all[literals..]),
    ))
}

/// The length and distance bases of the LZ77 codes (RFC 1951 section 3.2.5).
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Reads one LZ77 block to its end-of-block symbol.
fn inflate_block(
    reader: &mut BitReader<'_>,
    literals: &Huffman,
    distances: &Huffman,
    out: &mut Vec<u8>,
) -> Option<()> {
    loop {
        let symbol = literals.read(reader)?;
        match symbol {
            0..=255 => out.push(symbol as u8),
            256 => return Some(()),
            257..=285 => {
                let length_code = usize::from(symbol - 257);
                let length = usize::from(LENGTH_BASE[length_code])
                    + usize::from(reader.bits(LENGTH_EXTRA[length_code])?);
                let distance_code = usize::from(distances.read(reader)?);
                let distance = usize::from(DIST_BASE[distance_code])
                    + usize::from(reader.bits(DIST_EXTRA[distance_code])?);
                if distance > out.len() {
                    return None;
                }
                let from = out.len() - distance;
                for i in 0..length {
                    let byte = out[from + i];
                    out.push(byte);
                }
            }
            _ => return None,
        }
    }
}

// -----------------------------------------------------------------------------------------
// The names (the engines' texture caches)
// -----------------------------------------------------------------------------------------

/// The names of the fighters, in [`Variant::ALL`](crate::game::Variant::ALL) order: the
/// sheet of each is the bare name.
pub const SPRITE_NAMES: [&str; 10] = [
    "brunette", "blonde", "redhead", "pink", "white", "black", "green", "violet", "cyan", "raven",
];

/// The sprite named `name` in `pose` (`None` when the name is no fighter).
pub fn sprite_by_name(name: &str, pose: Pose) -> Option<Sprite<'static>> {
    Variant::ALL
        .iter()
        .find(|variant| SPRITE_NAMES[variant.index() as usize] == name)
        .map(|variant| sprite(*variant, pose))
}

/// The name of the sheet `skin` wears: what [`Frame::image`](crate::front::Frame::image)
/// draws for a manga fighter (`None` for the chalk).
pub fn sprite_name_of(skin: &Skin) -> Option<&'static str> {
    skin.variant
        .map(|variant| SPRITE_NAMES[variant.index() as usize])
}

/// The pose `fighter` is in: the cell of the sheet the sprite engine draws for its state.
/// Anything without its own pose (blocking, grabs, grapples, specials) stands in guard —
/// the art carries the movement, the game's logic the rules.
pub fn pose_of(fighter: &Fighter, fight_ticks: u32) -> Pose {
    use crate::game::AttackPhase::Active;
    match fighter.state() {
        FighterState::Walking => {
            // The two walk frames alternate with the screen's stride.
            if (fight_ticks / 8).is_multiple_of(2) {
                Pose::WalkA
            } else {
                Pose::WalkB
            }
        }
        FighterState::Airborne => Pose::Jump,
        FighterState::Crouching => Pose::Crouch,
        FighterState::HitStun => Pose::Hit,
        FighterState::Knockout => Pose::Ko,
        // An attacking fighter always has its attack (the state is set with the strike).
        FighterState::Attacking(Active) => match fighter
            .attack()
            .expect("an attacking fighter has its attack")
            .limb
        {
            Limb::Punch => Pose::Punch,
            Limb::Kick => Pose::Kick,
        },
        _ => Pose::Stand,
    }
}
