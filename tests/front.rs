//! The engine-agnostic front end, part 1: the frame description (`srr::front` scene types),
//! the ASCII art toolbox (`srr::front::ascii`) and the graffiti intro (`srr::front::intro`).
//!
//! Everything here is a pure function of its inputs, so the tests simply build frames and
//! canvases and look at what they contain. The fight screen (arena, HUD) and the [`Game`]
//! state machine are covered in `front_fight.rs`.
//!
//! [`Game`]: srr::front::Game

use srr::front::ascii::{Canvas, GLYPH_HEIGHT, block_pixels, glyph, hash};
use srr::front::intro::{
    self, COLUMNS, COMPLETE_AT, Intro, PROMPT, ROWS, SHATTER_AT, SKIP_AFTER, TAGLINE,
};
use srr::front::{
    Align, Frame, Label, MONO_ADVANCE, Rect, Rgba, SCREEN_HEIGHT, SCREEN_WIDTH, Viewport,
};

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

const RED: Rgba = Rgba::rgb(1.0, 0.0, 0.0);
const BLUE: Rgba = Rgba::rgb(0.0, 0.0, 1.0);
const WHITE: Rgba = Rgba::rgb(1.0, 1.0, 1.0);

/// The intro's colours that matter to the tests (private in the module).
const FLASH: Rgba = Rgba::hex(0xffffff);
const MIST: Rgba = Rgba::hex(0xf2ecff);
const DRIP: Rgba = Rgba::hex(0xc8141e);
const ABERRATION: [Rgba; 2] = [Rgba::hex(0xff2bd6), Rgba::hex(0x2bf3ff)];

fn label(text: &str, x: f32, size: f32, align: Align) -> Label {
    Label {
        text: text.to_string(),
        x,
        y: 0.0,
        size,
        color: WHITE,
        align,
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn same_color(a: Rgba, b: Rgba) -> bool {
    close(a.r, b.r) && close(a.g, b.g) && close(a.b, b.b) && close(a.a, b.a)
}

/// Every cell of `canvas`, with its coordinates.
fn cells(canvas: &Canvas) -> Vec<(i32, i32, char, Rgba)> {
    let mut found = Vec::new();
    for y in 0..canvas.height() as i32 {
        for x in 0..canvas.width() as i32 {
            if let Some((ch, color)) = canvas.get(x, y) {
                found.push((x, y, ch, color));
            }
        }
    }
    found
}

fn has_cell(canvas: &Canvas, wanted: impl Fn(char, Rgba) -> bool) -> bool {
    cells(canvas)
        .into_iter()
        .any(|(_, _, ch, color)| wanted(ch, color))
}

/// The intro drawn at `tick` with `status` under it.
fn intro_frame(tick: u32, status: &str) -> Frame {
    let mut frame = Frame::new(intro::BACKGROUND);
    Intro::at(tick).draw(&mut frame, status);
    frame
}

// ---------------------------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------------------------

#[test]
fn hex_colours_split_into_opaque_channels() {
    assert_eq!(Rgba::hex(0xff0000), Rgba::rgb(1.0, 0.0, 0.0));
    assert_eq!(Rgba::hex(0x00ff00), Rgba::rgb(0.0, 1.0, 0.0));
    assert_eq!(Rgba::hex(0x0000ff), Rgba::rgb(0.0, 0.0, 1.0));
    let grey = Rgba::hex(0x336699);
    assert!(close(grey.r, 0x33 as f32 / 255.0));
    assert!(close(grey.g, 0x66 as f32 / 255.0));
    assert!(close(grey.b, 0x99 as f32 / 255.0));
    assert_eq!(grey.a, 1.0);
    assert_eq!(
        Rgba::hex(0xff_123456),
        Rgba::hex(0x123456),
        "bits above 24 are ignored"
    );
}

#[test]
fn with_alpha_keeps_the_channels_and_clamps_the_opacity() {
    let color = Rgba::rgb(0.2, 0.4, 0.6);
    let half = color.with_alpha(0.5);
    assert_eq!((half.r, half.g, half.b, half.a), (0.2, 0.4, 0.6, 0.5));
    assert_eq!(color.with_alpha(2.0).a, 1.0);
    assert_eq!(color.with_alpha(-1.0).a, 0.0);
}

#[test]
fn scaled_multiplies_the_channels_clamped_and_keeps_the_opacity() {
    let color = Rgba::rgb(0.5, 0.25, 0.8).with_alpha(0.3);
    let darker = color.scaled(0.5);
    assert_eq!(
        (darker.r, darker.g, darker.b, darker.a),
        (0.25, 0.125, 0.4, 0.3)
    );
    let brighter = color.scaled(2.0);
    assert_eq!(
        (brighter.r, brighter.g, brighter.b, brighter.a),
        (1.0, 0.5, 1.0, 0.3)
    );
    let negative = color.scaled(-1.0);
    assert_eq!((negative.r, negative.g, negative.b), (0.0, 0.0, 0.0));
}

#[test]
fn mix_interpolates_every_channel_with_t_clamped() {
    let from = Rgba::rgb(0.0, 0.0, 0.0).with_alpha(0.0);
    let to = Rgba::rgb(1.0, 0.5, 0.2);
    let middle = from.mix(to, 0.5);
    assert!(same_color(
        middle,
        Rgba::rgb(0.5, 0.25, 0.1).with_alpha(0.5)
    ));
    assert_eq!(from.mix(to, 0.0), from);
    assert_eq!(from.mix(to, 1.0), to);
    assert_eq!(from.mix(to, 7.0), to, "t above 1 stops at the target");
    assert_eq!(from.mix(to, -3.0), from, "t below 0 stays at the start");
}

// ---------------------------------------------------------------------------------------------
// Labels, frames and the viewport
// ---------------------------------------------------------------------------------------------

#[test]
fn a_label_is_as_wide_as_its_characters_in_the_monospace_font() {
    let text = label("abcd", 0.0, 10.0, Align::Left);
    assert!(close(text.width(), 4.0 * 10.0 * MONO_ADVANCE));
    assert_eq!(label("", 0.0, 10.0, Align::Left).width(), 0.0);
    assert!(
        close(label("été", 0.0, 10.0, Align::Left).width(), 18.0),
        "characters, not bytes"
    );
}

#[test]
fn a_label_left_edge_depends_on_its_alignment() {
    // "abcd" at size 10 is 24 wide.
    assert_eq!(label("abcd", 100.0, 10.0, Align::Left).left(), 100.0);
    assert!(close(
        label("abcd", 100.0, 10.0, Align::Center).left(),
        88.0
    ));
    assert!(close(label("abcd", 100.0, 10.0, Align::Right).left(), 76.0));
}

#[test]
fn a_new_frame_is_empty() {
    let frame = Frame::new(BLUE);
    assert_eq!(frame.background, BLUE);
    assert!(frame.rects.is_empty());
    assert!(frame.labels.is_empty());
    assert_eq!(frame.text(), "");
}

#[test]
fn frame_rectangles_keep_their_order_and_never_have_negative_sizes() {
    let mut frame = Frame::new(BLUE);
    frame.rect(1.0, 2.0, 3.0, 4.0, RED);
    frame.rect(5.0, 6.0, -7.0, -8.0, WHITE);
    assert_eq!(
        frame.rects,
        vec![
            Rect {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
                color: RED,
            },
            Rect {
                x: 5.0,
                y: 6.0,
                width: 0.0,
                height: 0.0,
                color: WHITE,
            },
        ]
    );
}

#[test]
fn frame_labels_are_listed_in_order_and_their_text_joined_by_lines() {
    let mut frame = Frame::new(BLUE);
    frame.label("first", 10.0, 20.0, 12.0, RED, Align::Center);
    frame.label(String::from("second"), 1.0, 2.0, 3.0, WHITE, Align::Right);
    assert_eq!(
        frame.labels[0],
        Label {
            text: "first".into(),
            x: 10.0,
            y: 20.0,
            size: 12.0,
            color: RED,
            align: Align::Center,
        }
    );
    assert_eq!(frame.labels[1].align, Align::Right);
    assert_eq!(frame.text(), "first\nsecond");
}

#[test]
fn a_window_with_the_virtual_aspect_ratio_is_filled_exactly() {
    let viewport = Viewport::fit(SCREEN_WIDTH * 2.0, SCREEN_HEIGHT * 2.0);
    assert_eq!(
        viewport,
        Viewport {
            scale: 2.0,
            offset_x: 0.0,
            offset_y: 0.0,
        }
    );
    assert_eq!(viewport.to_window(0.0, 0.0), (0.0, 0.0));
    assert_eq!(
        viewport.to_window(SCREEN_WIDTH, SCREEN_HEIGHT),
        (SCREEN_WIDTH * 2.0, SCREEN_HEIGHT * 2.0)
    );
}

#[test]
fn a_wide_window_gets_black_bars_on_the_sides() {
    let viewport = Viewport::fit(2560.0, 720.0);
    assert_eq!(viewport.scale, 1.0);
    assert_eq!(viewport.offset_x, 640.0);
    assert_eq!(viewport.offset_y, 0.0);
    assert_eq!(viewport.to_window(10.0, 20.0), (650.0, 20.0));
}

#[test]
fn a_tall_window_gets_black_bars_above_and_below() {
    let viewport = Viewport::fit(640.0, 1000.0);
    assert_eq!(viewport.scale, 0.5);
    assert_eq!(viewport.offset_x, 0.0);
    assert_eq!(viewport.offset_y, (1000.0 - 360.0) / 2.0);
    assert_eq!(viewport.to_window(100.0, 100.0), (50.0, 370.0));
}

#[test]
fn degenerate_windows_get_a_zero_scale() {
    let zero = Viewport {
        scale: 0.0,
        offset_x: 0.0,
        offset_y: 0.0,
    };
    for (width, height) in [
        (0.0, 720.0),
        (1280.0, 0.0),
        (0.0, 0.0),
        (-100.0, 720.0),
        (1280.0, -5.0),
        (f32::NAN, 720.0),
        (1280.0, f32::NAN),
        (f32::INFINITY, f32::INFINITY),
    ] {
        assert_eq!(Viewport::fit(width, height), zero, "{width} x {height}");
    }
    assert_eq!(zero.to_window(500.0, 500.0), (0.0, 0.0));
}

// ---------------------------------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------------------------------

#[test]
fn a_new_canvas_is_transparent() {
    let canvas = Canvas::new(4, 3);
    assert_eq!((canvas.width(), canvas.height()), (4, 3));
    assert!(cells(&canvas).is_empty());
    assert_eq!(canvas.to_string(), "\n\n\n");
}

#[test]
fn put_paints_one_cell_and_a_space_clears_it() {
    let mut canvas = Canvas::new(4, 3);
    canvas.put(1, 2, 'x', RED);
    assert_eq!(canvas.get(1, 2), Some(('x', RED)));
    canvas.put(1, 2, 'y', BLUE);
    assert_eq!(canvas.get(1, 2), Some(('y', BLUE)), "a cell is overwritten");
    canvas.put(1, 2, ' ', RED);
    assert_eq!(canvas.get(1, 2), None);
}

#[test]
fn cells_outside_the_canvas_are_ignored_and_read_as_empty() {
    let mut canvas = Canvas::new(4, 3);
    let before = canvas.clone();
    for (x, y) in [
        (-1, 0),
        (0, -1),
        (4, 0),
        (0, 3),
        (i32::MIN, i32::MAX),
        (100, 100),
    ] {
        canvas.put(x, y, '#', RED);
        assert_eq!(canvas.get(x, y), None, "({x}, {y})");
    }
    assert_eq!(canvas, before);
}

#[test]
fn clear_empties_a_block_and_ignores_the_part_outside() {
    let mut canvas = Canvas::new(4, 3);
    for y in 0..3 {
        canvas.text(0, y, "abcd", RED);
    }
    canvas.clear(-1, 1, 3, 5);
    assert_eq!(canvas.to_string(), "abcd\n  cd\n  cd\n");
    canvas.clear(0, 0, 0, 3);
    assert_eq!(
        canvas.to_string(),
        "abcd\n  cd\n  cd\n",
        "an empty block clears nothing"
    );
}

#[test]
fn text_writes_one_character_per_cell_and_skips_spaces() {
    let mut canvas = Canvas::new(6, 1);
    canvas.put(1, 0, 'x', BLUE);
    canvas.text(0, 0, "a b cdefgh", RED);
    assert_eq!(canvas.get(0, 0), Some(('a', RED)));
    assert_eq!(
        canvas.get(1, 0),
        Some(('x', BLUE)),
        "the space did not erase it"
    );
    assert_eq!(canvas.get(2, 0), Some(('b', RED)));
    assert_eq!(
        canvas.to_string(),
        "axb cd\n",
        "what does not fit is dropped"
    );
    canvas.text(-2, 0, "zzZ", BLUE);
    assert_eq!(
        canvas.get(0, 0),
        Some(('Z', BLUE)),
        "text may start left of the canvas"
    );
}

#[test]
fn display_lists_the_rows_without_trailing_spaces() {
    let mut canvas = Canvas::new(5, 2);
    canvas.put(0, 0, 'a', RED);
    canvas.put(2, 0, 'b', BLUE);
    canvas.put(4, 1, 'c', RED);
    assert_eq!(canvas.to_string(), "a b\n    c\n");
}

#[test]
fn display_reports_the_error_of_a_writer_that_fails() {
    use std::fmt::{self, Write};

    struct Broken;
    impl Write for Broken {
        fn write_str(&mut self, _: &str) -> fmt::Result {
            Err(fmt::Error)
        }
    }

    assert!(write!(Broken, "{}", Canvas::new(3, 2)).is_err());
}

#[test]
fn draw_adds_one_padded_label_per_colour_of_each_row() {
    let mut canvas = Canvas::new(5, 3);
    canvas.put(0, 0, 'a', RED);
    canvas.put(2, 0, 'b', BLUE);
    canvas.put(3, 0, 'c', RED);
    // Row 1 stays empty.
    canvas.put(2, 2, 'z', BLUE);
    let mut frame = Frame::new(WHITE);
    canvas.draw(&mut frame, 10.0, 20.0, 10.0, 12.0, None);
    let advance = 10.0 * MONO_ADVANCE;
    let found: Vec<(&str, f32, f32, Rgba)> = frame
        .labels
        .iter()
        .map(|l| (l.text.as_str(), l.x, l.y, l.color))
        .collect();
    assert_eq!(
        found,
        vec![
            ("a  c", 10.0, 20.0, RED),
            ("b", 10.0 + 2.0 * advance, 20.0, BLUE),
            ("z", 10.0 + 2.0 * advance, 44.0, BLUE),
        ]
    );
    assert!(
        frame
            .labels
            .iter()
            .all(|l| l.size == 10.0 && l.align == Align::Left)
    );
    assert!(frame.rects.is_empty());
}

#[test]
fn a_tint_replaces_every_colour_so_each_row_is_a_single_label() {
    let mut canvas = Canvas::new(5, 2);
    canvas.put(1, 0, 'a', RED);
    canvas.put(2, 0, 'b', BLUE);
    canvas.put(4, 0, 'c', RED);
    let tint = WHITE.with_alpha(0.5);
    let mut frame = Frame::new(BLUE);
    canvas.draw(&mut frame, 0.0, 0.0, 10.0, 10.0, Some(tint));
    assert_eq!(frame.labels.len(), 1, "row 1 is empty");
    assert_eq!(frame.labels[0].text, "ab c");
    assert_eq!(frame.labels[0].color, tint);
    assert!(close(frame.labels[0].x, 10.0 * MONO_ADVANCE));
}

#[test]
fn an_empty_canvas_draws_nothing() {
    let mut frame = Frame::new(BLUE);
    Canvas::new(8, 8).draw(&mut frame, 0.0, 0.0, 10.0, 10.0, None);
    Canvas::new(0, 0).draw(&mut frame, 0.0, 0.0, 10.0, 10.0, Some(RED));
    assert_eq!(frame, Frame::new(BLUE));
}

// ---------------------------------------------------------------------------------------------
// Block font and hash
// ---------------------------------------------------------------------------------------------

#[test]
fn every_supported_character_has_a_rectangular_glyph() {
    for ch in "ADEFGHIKLMOPRSTY:! ".chars() {
        let rows = glyph(ch).unwrap_or_else(|| panic!("no glyph for {ch:?}"));
        assert_eq!(rows.len(), GLYPH_HEIGHT);
        let width = rows[0].len();
        assert!(width > 0);
        for row in rows {
            assert_eq!(row.len(), width, "{ch:?}");
            assert!(row.chars().all(|c| c == '#' || c == '.'), "{ch:?}");
        }
        let ink = rows.iter().flat_map(|r| r.chars()).any(|c| c == '#');
        assert_eq!(ink, ch != ' ', "only the space has no ink");
    }
}

#[test]
fn glyphs_are_case_insensitive() {
    for ch in "adefghiklmoprsty".chars() {
        assert_eq!(glyph(ch), glyph(ch.to_ascii_uppercase()), "{ch}");
        assert!(glyph(ch).is_some());
    }
}

#[test]
fn unsupported_characters_have_no_glyph() {
    for ch in ['B', 'z', '0', '9', '?', '-', '.', 'é', '\n'] {
        assert_eq!(glyph(ch), None, "{ch:?}");
    }
}

#[test]
fn every_banner_word_is_drawn_whole() {
    // The HUD's big words: a letter missing from the font would be skipped silently ("READY"
    // was drawn "READ" before the font had a Y).
    for word in ["READY", "FIGHT!", "KO", "TIME"] {
        for ch in word.chars() {
            assert!(glyph(ch).is_some(), "{word:?} lacks {ch:?}");
        }
    }
    assert!(block_pixels("READY").1 > block_pixels("READ").1);
}

#[test]
fn some_glyphs_look_like_their_letter() {
    assert_eq!(glyph('T').unwrap()[0], "#####");
    assert_eq!(glyph('I').unwrap(), ["###", ".#.", ".#.", ".#.", "###"]);
    assert_eq!(
        glyph('!').unwrap()[3],
        ".",
        "the gap of the exclamation mark"
    );
}

#[test]
fn block_pixels_of_nothing_is_empty() {
    assert_eq!(block_pixels(""), (Vec::new(), 0));
    assert_eq!(
        block_pixels("??"),
        (Vec::new(), 0),
        "unknown characters are skipped"
    );
}

#[test]
fn block_pixels_put_one_blank_column_between_glyphs() {
    let (one, width) = block_pixels("I");
    assert_eq!(width, 3);
    assert_eq!(one.len(), 3 + 1 + 1 + 1 + 3);
    let (two, width) = block_pixels("II");
    assert_eq!(width, 7);
    assert_eq!(two.len(), 2 * one.len());
    assert!(
        two.contains(&(4, 0)) && two.contains(&(5, 2)),
        "the second I starts at 4"
    );
    assert!(!two.iter().any(|&(x, _)| x == 3), "the blank column");
    let (spaced, width) = block_pixels("I I");
    assert_eq!(width, 3 + 1 + 3 + 1 + 3);
    assert_eq!(spaced.len(), two.len(), "the space has no ink");
}

#[test]
fn block_pixels_skip_unknown_characters() {
    assert_eq!(block_pixels("K?O"), block_pixels("KO"));
    let (pixels, width) = block_pixels("ko");
    assert_eq!(width, 11);
    assert!(
        pixels
            .iter()
            .all(|&(x, y)| (0..11).contains(&x) && (0..5).contains(&y))
    );
}

#[test]
fn the_hash_is_deterministic_and_well_mixed() {
    assert_eq!(hash(1, 2, 3), hash(1, 2, 3));
    assert_ne!(hash(1, 2, 3), hash(3, 2, 1));
    assert_ne!(hash(0, 0, 0), hash(0, 0, 1));
    assert_eq!(hash(-5, i32::MIN, i32::MAX), hash(-5, i32::MIN, i32::MAX));
    let values: std::collections::HashSet<u32> = (0..1000).map(|i| hash(i, 7, 13)).collect();
    assert_eq!(values.len(), 1000, "no collision on a small range");
    let odd = (0..1000).filter(|&i| hash(i, 0, 0) % 2 == 1).count();
    assert!((400..600).contains(&odd), "the low bit is balanced: {odd}");
}

// ---------------------------------------------------------------------------------------------
// Intro: timing
// ---------------------------------------------------------------------------------------------

#[test]
fn the_intro_starts_at_tick_zero_and_counts_ticks() {
    let mut intro = Intro::new();
    assert_eq!(intro, Intro::default());
    assert_eq!(intro.ticks(), 0);
    intro.tick();
    intro.tick();
    assert_eq!(intro.ticks(), 2);
    assert_eq!(intro, Intro::at(2));
}

#[test]
fn the_intro_tick_counter_saturates() {
    let mut intro = Intro::at(u32::MAX - 1);
    intro.tick();
    intro.tick();
    assert_eq!(intro.ticks(), u32::MAX);
}

#[test]
fn the_intro_can_be_skipped_only_after_a_short_delay() {
    assert!(!Intro::new().can_skip());
    assert!(!Intro::at(SKIP_AFTER - 1).can_skip());
    assert!(Intro::at(SKIP_AFTER).can_skip());
    assert!(Intro::at(u32::MAX).can_skip());
}

#[test]
fn the_intro_is_complete_once_everything_is_on_the_wall() {
    assert!(!Intro::at(COMPLETE_AT - 1).is_complete());
    assert!(Intro::at(COMPLETE_AT).is_complete());
    let art = Intro::at(COMPLETE_AT).canvas().to_string();
    assert!(art.contains(TAGLINE));
    assert!(art.contains(PROMPT));
}

#[test]
fn the_intro_glitches_during_the_shatter_and_in_short_idle_bursts() {
    let glitching: Vec<u32> = (0..1100).filter(|&t| Intro::at(t).is_glitching()).collect();
    let mut expected: Vec<u32> = (SHATTER_AT..SHATTER_AT + 16).collect();
    for burst in [420, 720, 1020] {
        expected.extend(burst..burst + 10);
    }
    assert_eq!(glitching, expected);
}

// ---------------------------------------------------------------------------------------------
// Intro: the art
// ---------------------------------------------------------------------------------------------

#[test]
fn the_wall_appears_row_by_row() {
    let empty = Intro::at(0).canvas();
    assert_eq!((empty.width(), empty.height()), (COLUMNS, ROWS));
    assert!(cells(&empty).is_empty(), "nothing on the first tick");

    let half = Intro::at(20).canvas();
    let rows: Vec<i32> = cells(&half).into_iter().map(|(_, y, _, _)| y).collect();
    assert!(rows.iter().all(|&y| y < 20), "only the top half is up");
    assert!(rows.contains(&19));
    assert!(
        cells(&half)
            .iter()
            .all(|&(_, _, ch, _)| ch == '_' || ch == '|')
    );

    let full = Intro::at(40).canvas();
    assert!(
        cells(&full)
            .iter()
            .any(|&(_, y, _, _)| y == ROWS as i32 - 1)
    );
    let mortar = full
        .to_string()
        .lines()
        .nth(2)
        .unwrap()
        .matches('_')
        .count();
    assert!(
        mortar > COLUMNS * 3 / 4 && mortar < COLUMNS,
        "a mortar row with a few worn-out spots: {mortar}"
    );
}

#[test]
fn the_words_are_sprayed_from_left_to_right_in_a_cloud_of_mist() {
    assert!(!Intro::at(29).canvas().to_string().contains('#'));
    let spraying = Intro::at(50).canvas();
    let ink: Vec<i32> = cells(&spraying)
        .into_iter()
        .filter(|&(_, _, ch, _)| ch == '#')
        .map(|(x, _, _, _)| x)
        .collect();
    assert!(!ink.is_empty(), "SHATTERED is being sprayed");
    let done = Intro::at(110).canvas();
    let ink_done = cells(&done).iter().filter(|c| c.2 == '#').count();
    assert!(ink.len() < ink_done, "more ink once the word is done");
    assert!(
        ink.iter().max()
            < cells(&done)
                .iter()
                .filter(|c| c.2 == '#')
                .map(|c| c.0)
                .max()
                .as_ref()
    );
    assert!(
        has_cell(&spraying, |_, color| color == MIST),
        "paint mist at the nozzle"
    );
    assert!(
        !has_cell(&Intro::at(COMPLETE_AT).canvas(), |_, color| color == MIST),
        "no mist once every word is sprayed"
    );
    assert!(Intro::at(200).canvas().to_string().contains('@'), "RAMPAGE");
}

#[test]
fn the_title_flashes_white_when_it_shatters() {
    let flash = Intro::at(SHATTER_AT).canvas();
    let ink: Vec<Rgba> = cells(&flash)
        .into_iter()
        .filter(|&(_, _, ch, _)| ch == '#' || ch == '@')
        .map(|(_, _, _, color)| color)
        .collect();
    assert!(!ink.is_empty());
    assert!(ink.iter().all(|&color| color == FLASH));
    let after = Intro::at(SHATTER_AT + 20).canvas();
    assert!(!has_cell(&after, |ch, color| ch == '#' && color == FLASH));
}

#[test]
fn cracks_stay_across_the_letters_after_the_shatter() {
    let crack = intro::BACKGROUND.mix(MIST, 0.25);
    let is_crack = |ch: char, color: Rgba| matches!(ch, '/' | '\\' | '|') && color == crack;
    assert!(!has_cell(&Intro::at(SHATTER_AT - 1).canvas(), is_crack));
    assert!(has_cell(&Intro::at(SHATTER_AT + 20).canvas(), is_crack));
    assert!(has_cell(&Intro::at(5000).canvas(), is_crack));
}

#[test]
fn the_red_paint_drips_down_the_wall() {
    let is_drip = |_: char, color: Rgba| color == DRIP;
    assert!(!has_cell(&Intro::at(219).canvas(), is_drip));
    let full = Intro::at(COMPLETE_AT).canvas();
    assert!(has_cell(&full, |ch, color| ch == '|' && color == DRIP));
    assert!(
        has_cell(&full, |ch, color| ch == 'v' && color == DRIP),
        "the drips' ends"
    );
    let early = cells(&Intro::at(260).canvas())
        .iter()
        .filter(|c| c.3 == DRIP)
        .count();
    let late = cells(&full).iter().filter(|c| c.3 == DRIP).count();
    assert!(early < late, "drips grow: {early} then {late}");
}

#[test]
fn the_tagline_is_typed_one_character_per_tick() {
    // The tagline is centred on row 32.
    let first = (COLUMNS - TAGLINE.len()) as i32 / 2;
    let line = |tick: u32| {
        let canvas = Intro::at(tick).canvas();
        let text: String = (first..first + TAGLINE.len() as i32)
            .map(|x| canvas.get(x, 32).map_or(' ', |(ch, _)| ch))
            .collect();
        text.trim_end().to_string()
    };
    assert!(!line(259).starts_with("-="));
    assert_eq!(line(260), "", "the band is cleaned, nothing typed yet");
    assert_eq!(line(264), "-= a");
    assert_eq!(line(260 + TAGLINE.len() as u32), TAGLINE);
    assert_eq!(line(2000), TAGLINE);
}

#[test]
fn the_prompt_blinks_every_half_second() {
    let shows = |tick: u32| Intro::at(tick).canvas().to_string().contains(PROMPT);
    assert!(!shows(311));
    assert!(shows(312));
    assert!(shows(341));
    assert!(!shows(342));
    assert!(!shows(371));
    assert!(shows(372));
}

#[test]
fn glitching_rows_are_shifted_sideways() {
    // Between 425 and 431 only the glitch differs: the prompt is off at both, the drips and
    // the tagline are complete.
    let calm = Intro::at(430).canvas();
    assert_eq!(calm, Intro::at(431).canvas());
    let glitch = Intro::at(425).canvas();
    assert_ne!(glitch, calm);
    let calm_rows: Vec<String> = calm.to_string().lines().map(String::from).collect();
    let glitch_rows: Vec<String> = glitch.to_string().lines().map(String::from).collect();
    let shifted = calm_rows
        .iter()
        .zip(&glitch_rows)
        .filter(|(a, b)| a != b)
        .count();
    assert!(shifted > 0 && shifted < ROWS, "some rows only: {shifted}");
}

// ---------------------------------------------------------------------------------------------
// Intro: drawing
// ---------------------------------------------------------------------------------------------

#[test]
fn the_intro_draws_the_status_line_centred_at_the_bottom() {
    let status = "connecting to the server...";
    let frame = intro_frame(COMPLETE_AT, status);
    let centred: Vec<&Label> = frame
        .labels
        .iter()
        .filter(|l| l.align == Align::Center)
        .collect();
    assert_eq!(centred.len(), 1);
    assert_eq!(centred[0].text, status);
    assert_eq!(centred[0].x, SCREEN_WIDTH / 2.0);
    assert!(centred[0].y > SCREEN_HEIGHT * 0.9);
    assert!(frame.text().contains(TAGLINE));
    assert!(frame.text().contains(PROMPT));
    assert_eq!(frame.background, intro::BACKGROUND);
    assert!(frame.rects.is_empty(), "the intro is only text");
}

#[test]
fn the_wall_behind_the_status_line_is_cleaned() {
    let with = intro_frame(COMPLETE_AT, "status");
    let without = intro_frame(COMPLETE_AT, "");
    let wall_chars = |frame: &Frame| {
        frame
            .labels
            .iter()
            .filter(|l| l.align == Align::Left)
            .map(|l| l.text.chars().filter(|c| *c != ' ').count())
            .sum::<usize>()
    };
    assert!(wall_chars(&with) < wall_chars(&without));
}

#[test]
fn an_empty_status_adds_no_label() {
    let frame = intro_frame(COMPLETE_AT, "");
    assert!(frame.labels.iter().all(|l| l.align == Align::Left));
    assert!(frame.labels.iter().all(|l| l.size == intro::CELL_SIZE));
}

#[test]
fn the_glitch_splits_the_colours() {
    let is_aberration = |l: &Label| {
        ABERRATION
            .iter()
            .any(|tint| same_color(l.color, tint.with_alpha(0.55)))
    };
    let calm = intro_frame(430, "status");
    assert!(!calm.labels.iter().any(is_aberration));
    let glitch = intro_frame(425, "status");
    for tint in ABERRATION {
        let copies: Vec<&Label> = glitch
            .labels
            .iter()
            .filter(|l| same_color(l.color, tint.with_alpha(0.55)))
            .collect();
        assert!(!copies.is_empty(), "a {tint:?} copy of the art");
    }
    let left = glitch.labels.iter().map(|l| l.x).fold(f32::MAX, f32::min);
    let base_left = calm.labels.iter().map(|l| l.x).fold(f32::MAX, f32::min);
    assert!(left < base_left, "the magenta copy is offset to the left");
    assert_eq!(
        glitch.labels.last().unwrap().text,
        "status",
        "the status stays on top"
    );
}

#[test]
fn every_moment_of_the_intro_draws_inside_the_screen() {
    let cell = intro::CELL_SIZE * MONO_ADVANCE;
    for tick in 0..=800 {
        let frame = intro_frame(tick, "practice mode: a training dummy is waiting");
        assert_eq!(
            frame.labels.last().map(|l| l.text.as_str()),
            Some("practice mode: a training dummy is waiting")
        );
        for l in &frame.labels {
            assert!(!l.text.is_empty(), "tick {tick}");
            assert!(
                !l.text.ends_with(' '),
                "tick {tick}: padded only on the left"
            );
            assert!(l.left() >= -cell && l.left() + l.width() <= SCREEN_WIDTH + cell);
            assert!(l.y >= 0.0 && l.y < SCREEN_HEIGHT);
        }
    }
}
