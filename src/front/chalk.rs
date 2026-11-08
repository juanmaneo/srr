//! The chalk sprite sheet: the XKCD stick figure of the arena, drawn once per [`Move`] into
//! one PNG, `assets/skins/chalk/chalk.png`, the way the manga girls come in one PNG each.
//!
//! It is generated, never painted: every cell is [`move_pose`](super::stick::move_pose) inked
//! by the very pen the arena uses ([`Figure::draw`]), so the chalk's movements are the
//! procedural ones, all thirty of them, the eight ground positions the manga sheets have no
//! cell for included. The layout is the manga contract widened to the chalk's reach (the
//! stick figure stretches its limbs, the girls do not): five columns by six rows facing right
//! over their mirrored copy facing left, [`CELL_WIDTH`] x [`CELL_HEIGHT`] each, the cells in
//! the order of [`Move::ALL`].
//!
//! The sheet is the picture the chalk is known by: the source art of an image-to-3D model of
//! the chalk (`docs/3d-asset-pipeline.md`), and the proof, a test of it, that no movement of
//! the game is missing from the style that has the fewest.
//! `cargo run --no-default-features --example sheets -- --write` writes it.

use super::moves::Move;
use super::render::{rgba_png, to_rgb8};
use super::scene::{Frame, Rgba};
use super::skin::Facing;
use super::stick::{Figure, move_pose};
use crate::game::{AttackPhase, FighterState, PlayerId};

/// The columns of the grid: five cells a row.
pub const COLUMNS: usize = 5;
/// The rows of one facing's grid: six rows of five, for the thirty moves.
pub const ROWS: usize = 6;
/// The width of a cell in sheet pixels: room for the longest kick of the stretching limbs.
pub const CELL_WIDTH: usize = 400;
/// The height of a cell: room for the highest flight of a throw.
pub const CELL_HEIGHT: usize = 320;
/// The width of the sheet: the grid's columns.
pub const SHEET_WIDTH: usize = COLUMNS * CELL_WIDTH;
/// The height of the sheet: the right-facing grid over its mirrored copy.
pub const SHEET_HEIGHT: usize = 2 * ROWS * CELL_HEIGHT;

/// Sheet pixels per virtual screen pixel: the figure is drawn 1.6 times as tall as in the
/// arena.
const SCALE: f32 = 1.6;
/// Where the feet stand in a right-facing cell, in virtual pixels: a little over a third in
/// from the left (room for the head of a fighter lying back), the floor 70 sheet pixels above
/// the bottom.
const FEET: (f32, f32) = (90.0, 156.0);

/// The pose is the active moment of a move (the contact of a strike), the figure the first
/// player's.
const PHASE: AttackPhase = AttackPhase::Active;

const _: () = assert!(Move::ALL.len() <= COLUMNS * ROWS, "the moves fit the grid");

/// The top-left sheet pixel of the cell of `movement` facing `facing`: the right-facing grid
/// is the top half, the left-facing one the bottom half.
///
/// ```
/// use srr::front::chalk::{CELL_HEIGHT, CELL_WIDTH, COLUMNS, ROWS, cell_origin};
/// use srr::front::moves::Move;
/// use srr::front::skin::Facing;
///
/// assert_eq!(cell_origin(Move::Stand, Facing::Right), (0, 0));
/// assert_eq!(cell_origin(Move::WalkA, Facing::Right), (CELL_WIDTH, 0));
/// assert_eq!(cell_origin(Move::Stand, Facing::Left), (0, ROWS * CELL_HEIGHT));
/// assert_eq!(
///     cell_origin(Move::Caught, Facing::Right),
///     ((COLUMNS - 1) * CELL_WIDTH, (ROWS - 1) * CELL_HEIGHT)
/// );
/// ```
pub fn cell_origin(movement: Move, facing: Facing) -> (usize, usize) {
    let half = match facing {
        Facing::Right => 0,
        Facing::Left => ROWS,
    };
    let index = movement.index();
    (
        index % COLUMNS * CELL_WIDTH,
        (index / COLUMNS + half) * CELL_HEIGHT,
    )
}

/// The figure of `movement` facing `facing`, in virtual pixels of its cell.
fn figure_of(movement: Move, facing: Facing) -> Figure {
    let state = match movement {
        Move::Hit => FighterState::HitStun,
        Move::Ko => FighterState::Knockout,
        _ => FighterState::Idle,
    };
    // The left-facing cell is the mirror of the right-facing one: the feet stand as far
    // from the right edge as they do from the left one.
    let (feet_x, sign) = match facing {
        Facing::Right => (FEET.0, 1.0),
        Facing::Left => (CELL_WIDTH as f32 / SCALE - FEET.0, -1.0),
    };
    Figure::posed(
        &move_pose(movement, PHASE),
        (feet_x, FEET.1),
        sign,
        state,
        PlayerId(0),
    )
}

/// Paints the chalk of `frame` into the cell at `origin` of `sheet`: every pixel whose centre
/// is inside a rectangle (the rule of the software renderer) turns opaque in its colour.
fn paint(sheet: &mut [u8], origin: (usize, usize), frame: &Frame) {
    let span = |start: f32, length: f32, limit: usize| {
        let first = (start * SCALE - 0.5).ceil().clamp(0.0, limit as f32) as usize;
        let end = ((start + length) * SCALE - 0.5)
            .ceil()
            .clamp(0.0, limit as f32) as usize;
        first..end.max(first)
    };
    for rect in &frame.rects {
        let [r, g, b] = to_rgb8(rect.color);
        for y in span(rect.y, rect.height, CELL_HEIGHT) {
            for x in span(rect.x, rect.width, CELL_WIDTH) {
                let at = ((origin.1 + y) * SHEET_WIDTH + origin.0 + x) * 4;
                sheet[at..at + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
    }
}

/// The whole sheet as RGBA pixels, row by row, [`SHEET_WIDTH`] x [`SHEET_HEIGHT`]: the chalk
/// on a transparent background. Deterministic.
///
/// ```
/// use srr::front::chalk::{SHEET_HEIGHT, SHEET_WIDTH, sheet};
///
/// assert_eq!(sheet().len(), SHEET_WIDTH * SHEET_HEIGHT * 4);
/// ```
pub fn sheet() -> Vec<u8> {
    let mut pixels = vec![0u8; SHEET_WIDTH * SHEET_HEIGHT * 4];
    for facing in [Facing::Right, Facing::Left] {
        for movement in Move::ALL {
            let mut frame = Frame::new(Rgba::rgb(0.0, 0.0, 0.0));
            figure_of(movement, facing).draw(&mut frame);
            paint(&mut pixels, cell_origin(movement, facing), &frame);
        }
    }
    pixels
}

/// The sheet as a PNG file (8-bit RGBA, compressed): what `assets/skins/chalk/chalk.png`
/// holds.
pub fn png() -> Vec<u8> {
    rgba_png(SHEET_WIDTH, SHEET_HEIGHT, &sheet())
}

/// The cell of `movement` facing `facing` out of a decoded `sheet`: its rows, joined into one
/// RGBA run of [`CELL_WIDTH`] x [`CELL_HEIGHT`].
///
/// ```
/// use srr::front::chalk::{CELL_HEIGHT, CELL_WIDTH, cell, sheet};
/// use srr::front::moves::Move;
/// use srr::front::skin::Facing;
///
/// let sheet = sheet();
/// let stand = cell(&sheet, Move::Stand, Facing::Right);
/// assert_eq!(stand.len(), CELL_WIDTH * CELL_HEIGHT * 4);
/// assert!(stand.chunks(4).any(|pixel| pixel[3] == 255), "the chalk is there");
/// ```
pub fn cell(sheet: &[u8], movement: Move, facing: Facing) -> Vec<u8> {
    let (left, top) = cell_origin(movement, facing);
    let mut out = Vec::with_capacity(CELL_WIDTH * CELL_HEIGHT * 4);
    for y in top..top + CELL_HEIGHT {
        let from = (y * SHEET_WIDTH + left) * 4;
        out.extend_from_slice(&sheet[from..from + CELL_WIDTH * 4]);
    }
    out
}
