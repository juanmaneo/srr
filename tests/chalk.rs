//! The chalk sprite sheet (`srr::front::chalk`): the stick figure of the arena in every
//! movement of the game, in the grid of the manga sheets. It is generated from the procedural
//! poses, so these tests hold it to them: a cell per movement, every figure whole inside its
//! cell, the left half the mirror of the right, the committed PNG the generator's output.

use std::collections::HashSet;

use srr::front::chalk::{
    CELL_HEIGHT, CELL_WIDTH, COLUMNS, ROWS, SHEET_HEIGHT, SHEET_WIDTH, cell, cell_origin, png,
    sheet,
};
use srr::front::moves::Move;
use srr::front::render::rgba_png;
use srr::front::skin::{Facing, Pose, png_rgba};

/// The bounding box `(left, top, right, bottom)` of the opaque pixels of a cell, and how many
/// there are.
fn extent(cell: &[u8]) -> Option<((usize, usize, usize, usize), usize)> {
    let mut count = 0;
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for (at, pixel) in cell.chunks(4).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        count += 1;
        let (x, y) = (at % CELL_WIDTH, at / CELL_WIDTH);
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
        });
    }
    bounds.map(|bounds| (bounds, count))
}

#[test]
fn the_sheet_has_a_cell_for_every_movement_in_the_grid_of_the_manga() {
    assert_eq!(COLUMNS, 5, "five columns, like the manga sheets");
    assert_eq!(Move::ALL.len(), 30);
    assert_eq!(ROWS * COLUMNS, Move::ALL.len(), "the grid is full");
    assert_eq!(SHEET_WIDTH, COLUMNS * CELL_WIDTH);
    assert_eq!(
        SHEET_HEIGHT,
        2 * ROWS * CELL_HEIGHT,
        "both facings, one over the other"
    );
    // The cells are in reading order, the first twenty-two the manga poses' places.
    let mut seen = HashSet::new();
    for (index, movement) in Move::ALL.into_iter().enumerate() {
        for facing in [Facing::Right, Facing::Left] {
            let (left, top) = cell_origin(movement, facing);
            assert!(left + CELL_WIDTH <= SHEET_WIDTH && top + CELL_HEIGHT <= SHEET_HEIGHT);
            assert!(seen.insert((left, top)), "{movement:?} shares its cell");
        }
        let (left, top) = cell_origin(movement, Facing::Right);
        assert_eq!(left, index % COLUMNS * CELL_WIDTH);
        assert_eq!(top, index / COLUMNS * CELL_HEIGHT);
        if let Some(pose) = Pose::ALL.get(index) {
            assert_eq!(movement.pose(), *pose);
        }
        let (_, below) = cell_origin(movement, Facing::Left);
        assert_eq!(
            below,
            top + ROWS * CELL_HEIGHT,
            "the mirror is the bottom half"
        );
    }
}

#[test]
fn every_figure_is_whole_inside_its_cell_and_no_two_are_alike() {
    let sheet = sheet();
    assert_eq!(sheet.len(), SHEET_WIDTH * SHEET_HEIGHT * 4);
    let mut cells = HashSet::new();
    for movement in Move::ALL {
        for facing in [Facing::Right, Facing::Left] {
            let pixels = cell(&sheet, movement, facing);
            assert_eq!(pixels.len(), CELL_WIDTH * CELL_HEIGHT * 4);
            let ((left, top, right, bottom), count) =
                extent(&pixels).unwrap_or_else(|| panic!("{movement:?} {facing:?} is empty"));
            assert!(
                count > 300,
                "{movement:?} {facing:?}: {count} pixels of chalk"
            );
            // Nothing is cut off: a margin of a few pixels all around.
            assert!(
                left >= 4 && top >= 4 && right < CELL_WIDTH - 4 && bottom < CELL_HEIGHT - 4,
                "{movement:?} {facing:?} reaches the edge of its cell: {left},{top} {right},{bottom}"
            );
            // Opaque chalk or nothing: the background is knocked out.
            assert!(
                pixels
                    .chunks(4)
                    .all(|pixel| pixel[3] == 0 || pixel[3] == 255)
            );
            assert!(
                cells.insert(pixels),
                "{movement:?} {facing:?} repeats another cell"
            );
        }
    }
}

#[test]
fn the_left_half_is_the_mirror_of_the_right_half() {
    let sheet = sheet();
    for movement in Move::ALL {
        let right = cell(&sheet, movement, Facing::Right);
        let left = cell(&sheet, movement, Facing::Left);
        let ((rl, rt, rr, rb), right_count) = extent(&right).unwrap();
        let ((ll, lt, lr, lb), left_count) = extent(&left).unwrap();
        // The same box, flipped across the middle of the cell, to the pixel or one off.
        let mirrored = (CELL_WIDTH - 1 - rr, rt, CELL_WIDTH - 1 - rl, rb);
        let close = |a: usize, b: usize| a.abs_diff(b) <= 1;
        assert!(
            close(ll, mirrored.0)
                && close(lt, mirrored.1)
                && close(lr, mirrored.2)
                && close(lb, mirrored.3),
            "{movement:?}: {:?} against {mirrored:?}",
            (ll, lt, lr, lb)
        );
        assert!(
            right_count.abs_diff(left_count) * 50 < right_count,
            "{movement:?}: {right_count} against {left_count} pixels"
        );
        // Most of the chalk lies exactly on the flipped chalk.
        let mut agree = 0;
        for y in 0..CELL_HEIGHT {
            for x in 0..CELL_WIDTH {
                let here = right[(y * CELL_WIDTH + x) * 4 + 3] != 0;
                let there = left[(y * CELL_WIDTH + (CELL_WIDTH - 1 - x)) * 4 + 3] != 0;
                agree += usize::from(here && there);
            }
        }
        assert!(
            agree * 10 >= right_count * 9,
            "{movement:?}: {agree} of {right_count}"
        );
    }
}

#[test]
fn the_chalk_is_drawn_in_the_colours_of_the_arena() {
    // The guard is chalk and the hat the first player's; a punch is the colour of its
    // strength, a grab green, a technique blue, a submission red.
    let sheet = sheet();
    let colors = |movement: Move| -> HashSet<[u8; 3]> {
        cell(&sheet, movement, Facing::Right)
            .chunks(4)
            .filter(|pixel| pixel[3] == 255)
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect()
    };
    let chalk = [0xd9, 0xd4, 0xca];
    assert!(colors(Move::Stand).contains(&chalk));
    assert_eq!(
        colors(Move::Stand).len(),
        3,
        "chalk, the dimmer back limbs and the hat"
    );
    assert!(colors(Move::PunchMid).contains(&[0xff, 0xa6, 0x33]));
    assert!(colors(Move::PunchHigh).contains(&[0xff, 0x40, 0x26]));
    assert!(colors(Move::Grab).contains(&[0x6c, 0xff, 0x8a]));
    assert!(colors(Move::Throw).contains(&[0x3f, 0xc8, 0xff]));
    assert!(colors(Move::Submission).contains(&[0xff, 0x3a, 0x4a]));
    assert!(colors(Move::SpecialFireball).contains(&[0xff, 0x8a, 0x1e]));
    assert!(colors(Move::SpecialPalm).contains(&[0xff, 0x5f, 0xd2]));
    assert!(
        colors(Move::Hit).contains(&[0xff, 0xf3, 0x5c]),
        "the stars of a stun"
    );
    assert!(
        !colors(Move::Ko).contains(&chalk),
        "a knocked-out fighter is dimmed"
    );
}

#[test]
fn the_committed_sheet_is_what_the_generator_draws() {
    let committed = std::fs::read("assets/skins/chalk/chalk.png")
        .unwrap_or_else(|e| panic!("assets/skins/chalk/chalk.png: {e}"));
    assert!(
        committed == png(),
        "assets/skins/chalk/chalk.png is stale: \
         cargo run --no-default-features --example sheets -- --write"
    );
    // And it is a sheet the game's own decoder reads, pixel for pixel.
    let pixels = png_rgba(&committed).expect("an 8-bit RGBA PNG");
    assert!(pixels == sheet());
    assert!(
        committed.len() < 1_000_000,
        "the sheet is compressed: {} bytes",
        committed.len()
    );
}

#[test]
fn the_compressed_png_reads_back_whatever_it_holds() {
    // Runs of every length around the limits of a DEFLATE match (3 to 258), long runs, runs
    // of literals above 143 (nine-bit codes), and rows that are all one byte.
    let mut pixels = Vec::new();
    for run in [1usize, 2, 3, 4, 10, 114, 115, 257, 258, 259, 300, 1000] {
        pixels.extend(std::iter::repeat_n(7u8, run * 4));
        pixels.extend([200, 150, 250, 255]);
    }
    pixels.extend((0..400u32).map(|i| (i * 37 % 256) as u8));
    pixels.resize(pixels.len().next_multiple_of(4 * 10), 0);
    let width = 10;
    let height = pixels.len() / 4 / width;
    let compressed = rgba_png(width, height, &pixels);
    assert_eq!(png_rgba(&compressed), Some(pixels.clone()));
    assert!(compressed.len() < pixels.len(), "the runs shrink it");
    // One row, one pixel.
    assert_eq!(
        png_rgba(&rgba_png(1, 1, &[1, 2, 3, 4])),
        Some(vec![1, 2, 3, 4])
    );
}
