//! Headless rendering: the game played and looked at without a window.
//!
//! `srr::front::render` is a software renderer for the engine-agnostic `Frame` (the same frame
//! Bevy and Fyrox draw): pixels in a `Pixmap`, PNG files and text snapshots.
//! `srr::front::Headless` drives the real `Game` with scripted keys and renders it in memory,
//! like Blitz renders Dioxus apps server-side. Nothing here opens a window or plays a sound.

use std::collections::HashSet;

use srr::front::hud::bar_slots;
use srr::front::intro::{COMPLETE_AT, PROMPT, SKIP_AFTER};
use srr::front::render::{
    FONT_COLUMNS, FONT_ROWS, Pixmap, Rgb8, SHADES, adler32, crc32, font_glyph, screenshot, shade,
    text_snapshot, to_rgb8,
};
use srr::front::{
    Align, Frame, Game, Headless, MONO_ADVANCE, Rgba, SCREEN_HEIGHT, SCREEN_WIDTH, Screen,
};
use srr::game::{FighterState, GrapplePosition, GrappleRole, Key, MatchConfig, PlayerId, Rival};
use srr::net::Client;

const BLACK: Rgba = Rgba::rgb(0.0, 0.0, 0.0);
const WHITE: Rgba = Rgba::rgb(1.0, 1.0, 1.0);
const RED: Rgba = Rgba::rgb(1.0, 0.0, 0.0);
const BLUE: Rgba = Rgba::rgb(0.0, 0.0, 1.0);

// ---------------------------------------------------------------------------------------------
// Helpers: a minimal PNG reader (stored deflate blocks only, which is what the encoder writes)
// ---------------------------------------------------------------------------------------------

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes[..4].try_into().unwrap())
}

/// The chunks of a PNG file as `(type, data)`, checking every CRC.
fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG signature");
    let mut found = Vec::new();
    let mut at = 8;
    while at < png.len() {
        let length = be32(&png[at..]) as usize;
        let kind: [u8; 4] = png[at + 4..at + 8].try_into().unwrap();
        let data = png[at + 8..at + 8 + length].to_vec();
        let crc = be32(&png[at + 8 + length..]);
        assert_eq!(crc, crc32(&png[at + 4..at + 8 + length]), "CRC of {kind:?}");
        found.push((kind, data));
        at += 12 + length;
    }
    assert_eq!(at, png.len(), "no trailing bytes");
    found
}

/// The payload of a zlib stream made of stored deflate blocks, checking the Adler-32.
fn inflate_stored(zlib: &[u8]) -> Vec<u8> {
    assert_eq!(zlib[0] & 0x0f, 8, "deflate");
    assert_eq!(
        (u16::from(zlib[0]) << 8 | u16::from(zlib[1])) % 31,
        0,
        "zlib header check"
    );
    let mut out = Vec::new();
    let mut at = 2;
    loop {
        let header = zlib[at];
        assert_eq!(header >> 1, 0, "stored block");
        let length = u16::from_le_bytes([zlib[at + 1], zlib[at + 2]]);
        let check = u16::from_le_bytes([zlib[at + 3], zlib[at + 4]]);
        assert_eq!(length, !check, "LEN / NLEN");
        out.extend(&zlib[at + 5..at + 5 + length as usize]);
        at += 5 + length as usize;
        if header & 1 == 1 {
            break;
        }
    }
    assert_eq!(be32(&zlib[at..]), adler32(&out), "Adler-32");
    assert_eq!(at + 4, zlib.len());
    out
}

/// Width, height and pixels of a PNG written by `Pixmap::to_png`.
fn decode(png: &[u8]) -> (usize, usize, Vec<Rgb8>) {
    let chunks = chunks(png);
    let kinds: Vec<&[u8; 4]> = chunks.iter().map(|(kind, _)| kind).collect();
    assert_eq!(kinds, [b"IHDR", b"IDAT", b"IEND"]);
    let header = &chunks[0].1;
    assert_eq!(header.len(), 13);
    let (width, height) = (be32(header) as usize, be32(&header[4..]) as usize);
    assert_eq!(
        &header[8..],
        [8, 2, 0, 0, 0],
        "8-bit RGB, deflate, no interlace"
    );
    assert!(chunks[2].1.is_empty());

    let raw = inflate_stored(&chunks[1].1);
    assert_eq!(raw.len(), height * (1 + 3 * width));
    let mut pixels = Vec::new();
    for row in raw.chunks(1 + 3 * width) {
        assert_eq!(row[0], 0, "filter none");
        pixels.extend(row[1..].chunks(3).map(|p| [p[0], p[1], p[2]]));
    }
    (width, height, pixels)
}

/// The ink pixels (not `background`) of `image` inside the box `(x, y, width, height)`.
fn ink(image: &Pixmap, background: Rgb8, x: usize, y: usize, width: usize, height: usize) -> usize {
    (y..y + height)
        .flat_map(|row| (x..x + width).map(move |column| (column, row)))
        .filter(|&(column, row)| image.pixel(column, row).is_some_and(|p| p != background))
        .count()
}

fn practice() -> Headless {
    Headless::practice()
}

fn fighter_state(game: &Headless, player: u8) -> FighterState {
    let current = game.game().shown_match().expect("a duel");
    current.fighters()[usize::from(player)].state()
}

fn x_of(game: &Headless, player: u8) -> i32 {
    game.game().shown_match().unwrap().fighters()[usize::from(player)].x()
}

fn health_of(game: &Headless, player: u8) -> i32 {
    game.game().shown_match().unwrap().fighters()[usize::from(player)].health()
}

// ---------------------------------------------------------------------------------------------
// Colours and pixmaps
// ---------------------------------------------------------------------------------------------

#[test]
fn colours_become_8_bit_rounded_and_clamped() {
    assert_eq!(to_rgb8(WHITE), [255, 255, 255]);
    assert_eq!(to_rgb8(Rgba::rgb(0.5, 0.0, 2.0)), [128, 0, 255]);
    assert_eq!(to_rgb8(Rgba::rgb(-1.0, 0.25, 1.0)), [0, 64, 255]);
    assert_eq!(
        to_rgb8(RED.with_alpha(0.0)),
        [255, 0, 0],
        "opacity is ignored"
    );
}

#[test]
fn a_new_pixmap_is_filled_with_its_colour() {
    let image = Pixmap::new(3, 2, BLUE);
    assert_eq!((image.width(), image.height()), (3, 2));
    assert_eq!(image.pixels().len(), 6);
    assert_eq!(image.count([0, 0, 255]), 6);
    assert_eq!(image.pixel(2, 1), Some([0, 0, 255]));
    assert_eq!(image.pixel(3, 0), None);
    assert_eq!(image.pixel(0, 2), None);
}

#[test]
fn an_opaque_rectangle_covers_the_pixels_whose_centre_it_contains() {
    let mut image = Pixmap::new(4, 4, BLACK);
    image.fill_rect(0.4, 0.4, 1.2, 1.2, WHITE);
    assert_eq!(image.count([255, 255, 255]), 4);
    for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        assert_eq!(image.pixel(x, y), Some([255, 255, 255]), "({x}, {y})");
    }

    let mut thin = Pixmap::new(4, 4, BLACK);
    thin.fill_rect(0.6, 0.6, 0.8, 0.8, WHITE);
    assert_eq!(thin.count([0, 0, 0]), 16, "no pixel centre inside");
}

#[test]
fn rectangles_are_clipped_to_the_image() {
    let mut image = Pixmap::new(4, 3, BLACK);
    image.fill_rect(-10.0, 2.0, 100.0, 50.0, RED);
    assert_eq!(image.count([255, 0, 0]), 4, "only the last row");

    let mut outside = Pixmap::new(4, 3, BLACK);
    outside.fill_rect(-10.0, -10.0, 5.0, 5.0, RED);
    outside.fill_rect(10.0, 10.0, 5.0, 5.0, RED);
    outside.fill_rect(1.0, 1.0, -2.0, 1.0, RED);
    assert_eq!(outside.count([0, 0, 0]), 12);
}

#[test]
fn translucent_rectangles_blend_and_invisible_ones_do_nothing() {
    let mut image = Pixmap::new(2, 1, BLACK);
    image.fill_rect(0.0, 0.0, 2.0, 1.0, WHITE.with_alpha(0.5));
    assert_eq!(image.pixel(0, 0), Some([128, 128, 128]));
    image.fill_rect(0.0, 0.0, 1.0, 1.0, RED.with_alpha(0.5));
    assert_eq!(image.pixel(0, 0), Some([192, 64, 64]));
    let before = image.clone();
    image.fill_rect(0.0, 0.0, 2.0, 1.0, BLUE.with_alpha(0.0));
    assert_eq!(image, before);
}

// ---------------------------------------------------------------------------------------------
// PNG
// ---------------------------------------------------------------------------------------------

#[test]
fn checksums_match_the_reference_values() {
    assert_eq!(crc32(b""), 0);
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    assert_eq!(crc32(b"IEND"), 0xae42_6082, "every PNG ends with this CRC");
    assert_eq!(adler32(b""), 1);
    assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
}

#[test]
fn a_png_round_trips_every_pixel() {
    let mut image = Pixmap::new(5, 3, BLUE);
    image.fill_rect(1.0, 1.0, 3.0, 1.0, RED);
    image.fill_rect(4.0, 0.0, 1.0, 1.0, WHITE.with_alpha(0.25));
    let (width, height, pixels) = decode(&image.to_png());
    assert_eq!((width, height), (5, 3));
    assert_eq!(pixels, image.pixels());
}

#[test]
fn big_images_are_split_into_several_stored_blocks() {
    // 200 x 200 RGB rows are 120 200 bytes: two deflate blocks of at most 65 535 bytes.
    let mut image = Pixmap::new(200, 200, RED);
    image.fill_rect(50.0, 150.0, 100.0, 40.0, WHITE);
    let png = image.to_png();
    let idat = &chunks(&png)[1].1;
    assert_eq!(idat[2], 0, "the first block is not the last one");
    let (width, height, pixels) = decode(&png);
    assert_eq!((width, height), (200, 200));
    assert_eq!(pixels, image.pixels());
}

#[test]
fn an_empty_image_still_makes_a_well_formed_stream() {
    let (width, height, pixels) = decode(&Pixmap::new(0, 0, BLACK).to_png());
    assert_eq!((width, height), (0, 0));
    assert!(pixels.is_empty());
}

// ---------------------------------------------------------------------------------------------
// The bitmap font
// ---------------------------------------------------------------------------------------------

#[test]
fn every_printable_character_has_its_own_glyph_within_seven_rows() {
    let printable: Vec<char> = (' '..='~').collect();
    assert_eq!(printable.len(), 95);
    let glyphs: Vec<[u8; FONT_COLUMNS]> = printable.iter().map(|&ch| font_glyph(ch)).collect();
    for (ch, glyph) in printable.iter().zip(&glyphs) {
        assert!(
            glyph.iter().all(|column| column >> FONT_ROWS == 0),
            "{ch:?}"
        );
        assert_eq!(
            glyph.iter().any(|&column| column != 0),
            *ch != ' ',
            "{ch:?}"
        );
    }
    let distinct: HashSet<_> = glyphs.iter().collect();
    assert_eq!(distinct.len(), glyphs.len(), "two characters share a glyph");
}

#[test]
fn other_characters_are_drawn_as_a_hollow_box() {
    let hollow = [0x7f, 0x41, 0x41, 0x41, 0x7f];
    for ch in ['é', '█', '\n', '\u{7f}'] {
        assert_eq!(font_glyph(ch), hollow, "{ch:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// Screenshots
// ---------------------------------------------------------------------------------------------

#[test]
fn the_background_fills_the_whole_window_letterbox_included() {
    let mut frame = Frame::new(BLUE);
    frame.rect(0.0, 0.0, SCREEN_WIDTH, SCREEN_HEIGHT, RED);
    // Twice as wide as 16:9: bars of 320 pixels on both sides.
    let image = screenshot(&frame, 1920, 540);
    assert_eq!(image.pixel(0, 270), Some([0, 0, 255]));
    assert_eq!(image.pixel(1919, 270), Some([0, 0, 255]));
    assert_eq!(image.pixel(479, 0), Some([0, 0, 255]));
    assert_eq!(image.pixel(480, 0), Some([255, 0, 0]));
    assert_eq!(image.pixel(1439, 539), Some([255, 0, 0]));
    assert_eq!(image.pixel(1440, 539), Some([0, 0, 255]));
}

#[test]
fn rectangles_are_scaled_to_the_window() {
    let mut frame = Frame::new(BLACK);
    frame.rect(100.0, 100.0, 200.0, 100.0, WHITE);
    let image = screenshot(&frame, 640, 360);
    assert_eq!(image.count([255, 255, 255]), 100 * 50);
    assert_eq!(image.pixel(50, 50), Some([255, 255, 255]));
    assert_eq!(image.pixel(149, 74), Some([255, 255, 255]));
    assert_eq!(image.pixel(150, 74), Some([0, 0, 0]));
}

#[test]
fn labels_ink_their_character_cells_in_their_colour_above_the_rectangles() {
    let mut frame = Frame::new(BLACK);
    frame.label("A", 100.0, 100.0, 100.0, RED, Align::Left);
    frame.rect(0.0, 0.0, 50.0, 50.0, WHITE);
    let image = screenshot(&frame, 1280, 720);
    // One cell is 60 x 100 pixels: the glyph is in it, in red, and nowhere else.
    let inside = ink(&image, [0, 0, 0], 100, 100, 60, 100);
    assert!(inside > 300, "{inside} red pixels");
    assert_eq!(image.count([255, 0, 0]), inside);

    let mut covered = Frame::new(BLACK);
    covered.label("A", 100.0, 100.0, 100.0, RED, Align::Left);
    covered.rect(100.0, 100.0, 60.0, 100.0, WHITE);
    let image = screenshot(&covered, 1280, 720);
    assert_eq!(
        image.count([255, 0, 0]),
        inside,
        "labels are drawn after every rectangle"
    );
}

#[test]
fn spaces_are_transparent_and_alignment_moves_the_text() {
    let mut blank = Frame::new(BLACK);
    blank.label("    ", 640.0, 300.0, 40.0, WHITE, Align::Center);
    assert_eq!(screenshot(&blank, 1280, 720).count([0, 0, 0]), 1280 * 720);

    let advance = 40.0 * MONO_ADVANCE;
    for (align, x) in [
        (Align::Left, 600.0),
        (Align::Center, 612.0),
        (Align::Right, 624.0),
    ] {
        let mut frame = Frame::new(BLACK);
        frame.label("I", x, 300.0, 40.0, WHITE, align);
        let image = screenshot(&frame, 1280, 720);
        let total = image.count([255, 255, 255]);
        assert!(total > 0, "{align:?}");
        assert_eq!(
            ink(&image, [0, 0, 0], 600, 300, advance as usize, 40),
            total,
            "{align:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Text snapshots
// ---------------------------------------------------------------------------------------------

#[test]
fn shades_go_from_dark_to_bright() {
    assert_eq!(shade([0, 0, 0]), ' ');
    assert_eq!(shade([255, 255, 255]), '@');
    assert_eq!(shade([128, 128, 128]), char::from(SHADES[5]));
    assert!(SHADES.contains(&(shade([255, 0, 0]) as u8)));
}

#[test]
fn rectangles_become_shades_and_the_background_spaces() {
    let mut frame = Frame::new(Rgba::rgb(0.5, 0.5, 0.5));
    frame.rect(0.0, 0.0, 640.0, 720.0, WHITE);
    frame.rect(640.0, 360.0, 320.0, 360.0, BLACK);
    // Black is the darkest shade: a space too. Trailing spaces are trimmed.
    let text = text_snapshot(&frame, 4, 2);
    assert_eq!(text, "@@\n@@");
    assert_eq!(text_snapshot(&Frame::new(BLACK), 3, 3), "\n\n");
}

#[test]
fn labels_are_written_one_character_per_cell_from_their_first_character() {
    let mut frame = Frame::new(BLACK);
    // Big letters on a coarse grid still read as a word.
    frame.label("RAMPAGE", 0.0, 0.0, 100.0, WHITE, Align::Left);
    frame.label("a b", 640.0, 360.0, 10.0, WHITE, Align::Left);
    let text = text_snapshot(&frame, 20, 4);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "RAMPAGE");
    assert_eq!(lines[2], "          a b");
}

#[test]
fn labels_off_the_grid_are_clipped() {
    let mut frame = Frame::new(BLACK);
    frame.label("above", 100.0, -100.0, 20.0, WHITE, Align::Left);
    frame.label("below", 100.0, 800.0, 20.0, WHITE, Align::Left);
    // Cells are 10 x 10: the first character's centre is at x = -18, in column -2.
    frame.label("leftover", -24.0, 355.0, 20.0, WHITE, Align::Left);
    frame.label("cut", 1270.0, 0.0, 20.0, WHITE, Align::Left);
    let text = text_snapshot(&frame, 128, 72);
    assert_eq!(text.split('\n').count(), 72);
    assert!(!text.contains("above") && !text.contains("below"));
    assert_eq!(text.lines().nth(36), Some("ftover"));
    let cut = format!("{}c", " ".repeat(127));
    assert_eq!(
        text.lines().nth(1),
        Some(cut.as_str()),
        "the last column only"
    );
}

// ---------------------------------------------------------------------------------------------
// The headless game
// ---------------------------------------------------------------------------------------------

#[test]
fn a_headless_game_starts_on_the_intro_at_the_virtual_screen_size() {
    let game = practice();
    assert_eq!(game.game().screen(), Screen::Intro);
    assert_eq!(game.size(), (1280, 720));
    assert_eq!(game.ticks(), 0);
    assert!(game.game().client().is_local());
    let image = game.screenshot();
    assert_eq!((image.width(), image.height()), (1280, 720));
    assert_eq!(game.frame(), game.game().frame());
}

#[test]
fn the_window_size_can_be_changed() {
    let game = practice().with_size(320, 240);
    assert_eq!(game.size(), (320, 240));
    let image = game.screenshot();
    assert_eq!((image.width(), image.height()), (320, 240));
}

#[test]
fn the_intro_reveals_the_prompt_in_the_text_snapshot() {
    let mut game = practice();
    game.idle(SKIP_AFTER);
    assert!(!game.text_snapshot(160, 60).contains("ENTER"));
    game.idle(COMPLETE_AT - SKIP_AFTER);
    assert_eq!(game.ticks(), u64::from(COMPLETE_AT));
    // Spaces are transparent: the brick wall shows between the words, like in a window.
    let text = game.text_snapshot(160, 60);
    let prompt = PROMPT.split(' ').collect::<Vec<_>>();
    assert!(
        text.lines()
            .any(|line| prompt.iter().all(|word| line.contains(word))),
        "{text}"
    );
    assert!(text.contains("practice"), "{text}");
}

#[test]
fn skipping_the_intro_shows_the_versus_hud() {
    let mut game = practice();
    game.skip_intro();
    assert_eq!(game.game().screen(), Screen::Fight);
    assert!(game.ticks() > u64::from(SKIP_AFTER));
    let text = game.text_snapshot(160, 45);
    assert!(text.contains("P1 (you)"), "{text}");
    assert!(text.contains("100 HP"), "{text}");
    assert!(
        text.contains("Practice:") && text.contains("dummy"),
        "{text}"
    );

    // Skipping again only taps Enter twice (meaningless during the round).
    let ticks = game.ticks();
    game.skip_intro();
    assert_eq!(game.ticks(), ticks + 4);
    assert_eq!(game.game().screen(), Screen::Fight);
}

/// Skips the intro: the training dummy it is.
fn fight_the_dummy(game: &mut Headless) {
    game.skip_intro();
    assert_eq!(game.game().client().rival(), Some(Rival::Dummy));
}

#[test]
fn practice_sends_nothing_whatever_the_keys() {
    let mut game = practice();
    assert!(game.press(&[Key::Enter]).is_empty());
    assert!(game.idle(5).is_empty());
    assert!(game.tap(&[Key::Numpad5]).is_empty());
    assert!(game.hold(&[Key::ArrowRight], 3).is_empty());
    assert_eq!(game.ticks(), 1 + 5 + 2 + 3);
    assert!(game.game_mut().client_mut().is_local());
}

#[test]
fn t_brings_the_computer_in_on_the_screen() {
    let mut game = practice();
    game.skip_intro();
    let dummy = game.text_snapshot(160, 45);
    assert!(dummy.contains("switch.to.the.computer"), "{dummy}");
    assert!(!dummy.contains("CPU"), "{dummy}");
    game.tap(&[Key::T]);
    game.idle(MatchConfig::default().countdown_ticks + 90);
    let text = game.text_snapshot(160, 45);
    assert!(text.contains("switch.to.the.patient.computer"), "{text}");
    assert!(text.contains("CPU"), "{text}");
    assert!(
        x_of(&game, 1) < 800,
        "the computer came for the idle player"
    );
}

#[test]
fn hits_empty_the_health_bar_on_the_screenshot() {
    let mut game = practice();
    fight_the_dummy(&mut game);
    game.idle(MatchConfig::default().countdown_ticks + 1);
    while x_of(&game, 1) - x_of(&game, 0) > 60 {
        game.press(&[Key::D]);
    }

    // The dummy's bar is on the right and empties towards the timer, i.e. from its left end.
    let slot = bar_slots(2)[1];
    let probe = (
        (slot.x + 4.0) as usize,
        (slot.y + slot.height - 2.0) as usize,
    );
    let full = game.screenshot().pixel(probe.0, probe.1);

    game.tap(&[Key::J]);
    game.idle(40);
    assert!(health_of(&game, 1) < 100, "the high punch connected");
    let hit = game.screenshot().pixel(probe.0, probe.1);
    assert_ne!(hit, full, "the left end of the dummy's bar emptied");
    assert!(
        game.text_snapshot(160, 45)
            .contains(&format!("{} HP", health_of(&game, 1)))
    );
}

#[test]
fn an_ippon_seoi_nage_shows_up_on_the_hud() {
    let mut game = practice();
    fight_the_dummy(&mut game);
    game.idle(MatchConfig::default().countdown_ticks + 1);
    while x_of(&game, 1) - x_of(&game, 0) > 55 {
        game.press(&[Key::D]);
    }
    game.press(&[Key::Y, Key::U]);
    game.idle(12);
    assert_eq!(
        fighter_state(&game, 0),
        FighterState::Grappling(GrappleRole::Top, GrapplePosition::Throw)
    );
    let text = game.text_snapshot(160, 45);
    assert!(text.contains("[IPPON-SEOI-NAGE]"), "{text}");
    assert!(
        text.contains("COUNTER Y+U"),
        "the dummy is told how to counter: {text}"
    );
}

#[test]
fn a_headless_game_works_with_any_client() {
    let game = Headless::new(Game::new(Client::practice(MatchConfig::default())));
    assert_eq!(game.game().local_player(), Some(PlayerId(0)));
}
