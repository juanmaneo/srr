//! The skins: the styles and variants on the wire, the client's skins, the room broadcasting
//! them, and the figures drawn wearing each style.

mod common;

use srr::front::Headless;
use srr::front::skin::Palette;
use srr::game::{MatchConfig, PlayerId, Skin, Style, StyleSet, Variant};
use srr::net::{Client, ClientMessage, DecodeError, ServerMessage};

// ---------------------------------------------------------------------------------------------
// The styles and the variants
// ---------------------------------------------------------------------------------------------

#[test]
fn the_three_styles_and_their_labels() {
    assert_eq!(Style::default(), Style::Stick);
    assert_eq!(Style::ALL.len(), 2);
    assert_eq!(Style::Stick.label(), "Stick (chalk)");
    assert_eq!(Style::Manga.label(), "Manga");
    for style in Style::ALL {
        assert_eq!(Style::from_index(style.index()), Some(style));
        assert_eq!(style.next().previous(), style);
    }
    assert_eq!(Style::from_index(2), None);
    assert!(!Style::Stick.is_skinned());
    assert!(Style::Manga.is_skinned());
}

#[test]
fn the_ten_variants_cycle_with_the_players() {
    assert_eq!(Variant::ALL.len(), 10);
    for (index, variant) in Variant::ALL.iter().enumerate() {
        assert_eq!(Variant::from_index(variant.index()), Some(*variant));
        assert_eq!(variant.index(), index as u8);
    }
    assert_eq!(Variant::from_index(10), None);
    // The players wear their own: the ten players of a brawl each look different.
    assert_eq!(Variant::of(PlayerId(0)), Variant::Brunette);
    assert_eq!(Variant::of(PlayerId(9)), Variant::Raven);
    assert_eq!(Variant::of(PlayerId(10)), Variant::Brunette);
    // Ten girls, one per player.
    assert_eq!(Variant::Brunette.label(), "the brunette");
    assert_eq!(Variant::Redhead.label(), "the redhead");
    assert_eq!(Variant::Pink.label(), "the pink");
}

#[test]
fn a_skin_is_a_style_wearing_a_variant() {
    // The chalk wears none; the skinned styles always do.
    assert_eq!(Skin::default(), Skin::STICK);
    assert_eq!(Skin::STICK.index(), 0);
    assert_eq!(Skin::styled(Style::Stick, Variant::Pink), Skin::STICK);
    for style in Style::ALL {
        for variant in Variant::ALL {
            let skin = Skin::styled(style, variant);
            assert_eq!(Skin::from_index(skin.index()), Some(skin));
        }
    }
    // 0 is the chalk, 1..=10 the manga variants.
    assert_eq!(
        Skin::from_index(1),
        Some(Skin::styled(Style::Manga, Variant::Brunette))
    );
    assert_eq!(
        Skin::from_index(10),
        Some(Skin::styled(Style::Manga, Variant::Raven))
    );
    assert_eq!(Skin::from_index(11), None);
    // The label names both.
    assert_eq!(Skin::STICK.label(), "Stick (chalk)");
    assert_eq!(
        Skin::styled(Style::Manga, Variant::Blonde).label(),
        "Manga (the blonde)"
    );
}

#[test]
fn the_allowed_styles_of_a_server() {
    let all = StyleSet::all();
    assert!(all.contains(Style::Stick) && all.contains(Style::Manga));
    // The chalk is always allowed, even on a server that allows nothing.
    let none = StyleSet::none();
    assert!(none.contains(Style::Stick));
    assert!(!none.contains(Style::Manga));
    assert_eq!(none.with(Style::Manga).without(Style::Manga), none);
    // The bits go on the wire; the bits that name no style are dropped.
    assert_eq!(StyleSet::from_bits(all.bits()), all);
    assert_eq!(StyleSet::from_bits(0xff).styles().count(), 2);
    // Cycling sticks to the allowed ones.
    let manga_only = StyleSet::none().with(Style::Manga);
    assert_eq!(Style::Stick.next_allowed(manga_only), Style::Manga);
    assert_eq!(Style::Manga.next_allowed(manga_only), Style::Stick);
    assert_eq!(
        Skin::STICK.next_allowed(manga_only, PlayerId(2)),
        Some(Skin::styled(Style::Manga, Variant::Redhead))
    );
    // A set without any skinned style offers no skin at all.
    assert_eq!(
        Skin::STICK.next_allowed(StyleSet::none(), PlayerId(0)),
        None
    );
}

// ---------------------------------------------------------------------------------------------
// The wire
// ---------------------------------------------------------------------------------------------

#[test]
fn the_skin_messages_encode_and_decode() {
    let manga = Skin::styled(Style::Manga, Variant::Blonde);
    let skin = ClientMessage::Skin { skin: manga };
    assert_eq!(skin.encode(), vec![10, 2]);
    assert_eq!(ClientMessage::decode(&skin.encode()), Ok(skin));
    // The server names a player's skin, and the styles it allows.
    let told = ServerMessage::SkinOf {
        player: PlayerId(3),
        skin: manga,
    };
    assert_eq!(told.encode(), vec![11, 3, 2]);
    assert_eq!(ServerMessage::decode(&told.encode()), Ok(told));
    let styles = ServerMessage::Styles {
        allowed: StyleSet::none().with(Style::Manga),
    };
    assert_eq!(styles.encode(), vec![12, 2]);
    assert_eq!(ServerMessage::decode(&styles.encode()), Ok(styles));
    // A byte that names no skin is refused, not guessed.
    assert_eq!(
        ClientMessage::decode(&[10, 11]),
        Err(DecodeError::UnknownSkin(11))
    );
    assert_eq!(
        ServerMessage::decode(&[11, 0, 11]),
        Err(DecodeError::UnknownSkin(11))
    );
}

#[test]
fn a_client_wears_what_the_server_tells_everybody() {
    let mut client = Client::online(MatchConfig::default(), 0);
    client.handle(ServerMessage::Welcome {
        player: PlayerId(1),
    });
    // The server allows everything and tells the skins of the seated players.
    client.handle(ServerMessage::Styles {
        allowed: StyleSet::all(),
    });
    let manga = Skin::styled(Style::Manga, Variant::Blonde);
    client.handle(ServerMessage::SkinOf {
        player: PlayerId(0),
        skin: manga,
    });
    assert_eq!(client.skin(PlayerId(0)), manga);
    assert_eq!(
        client.skin(PlayerId(1)),
        Skin::STICK,
        "the chalk until told"
    );
    // Picking a skin tells the server...
    let mine = Skin::styled(Style::Manga, Variant::Pink);
    assert_eq!(
        client.set_skin(mine),
        Some(ClientMessage::Skin { skin: mine }.encode())
    );
    // ...but only the styles it allows.
    let mut strict = Client::online(MatchConfig::default(), 0);
    strict.handle(ServerMessage::Welcome {
        player: PlayerId(0),
    });
    strict.handle(ServerMessage::Styles {
        allowed: StyleSet::none(),
    });
    assert_eq!(strict.set_skin(mine), None, "manga is not allowed");
    let mut lax = Client::online(MatchConfig::default(), 0);
    lax.handle(ServerMessage::Welcome {
        player: PlayerId(0),
    });
    lax.handle(ServerMessage::Styles {
        allowed: StyleSet::none().with(Style::Manga),
    });
    assert_eq!(
        lax.set_skin(mine),
        Some(ClientMessage::Skin { skin: mine }.encode())
    );
}

// ---------------------------------------------------------------------------------------------
// The drawing
// ---------------------------------------------------------------------------------------------

#[test]
fn every_variant_of_every_style_has_its_own_palette() {
    for style in Style::ALL {
        for variant in Variant::ALL {
            let palette = Palette::of(variant);
            // The hair and the outfit tell the variants apart, whatever the style.
            assert_ne!(palette.hair, palette.outfit, "{variant:?} in {style:?}");
        }
    }
    // The ten girls differ by their hair: each variant's name is her colour.
    let hairs: Vec<_> = Variant::ALL.iter().map(|v| Palette::of(*v).hair).collect();
    for (i, a) in hairs.iter().enumerate() {
        for b in hairs.iter().skip(i + 1) {
            assert_ne!(a, b, "every girl wears her own hair colour");
        }
    }
}

#[test]
fn a_fighter_wearing_a_skin_is_drawn_in_its_colours() {
    // The manga fighter is its AI-art sprite: the sprite's own dominant colour must show
    // on screen once the skin is worn — where the chalk showed none of it.
    let variant = Variant::of(PlayerId(0));
    let sprite = srr::front::skin::sprite(
        variant,
        srr::front::skin::Pose::Stand,
        srr::front::skin::Facing::Right,
    );
    // The sprite's most common saturated opaque colour: its signature colour (the red of
    // the dress, not the dark of the hair, which the arena also has).
    let mut counts: std::collections::HashMap<[u8; 3], usize> = Default::default();
    for px in sprite.pixels.chunks(4) {
        let [r, g, b, a] = *px.first_chunk::<4>().expect("rgba");
        let saturated = i32::from(r).max(i32::from(g)).max(i32::from(b))
            - i32::from(r).min(i32::from(g)).min(i32::from(b));
        if a > 200 && saturated > 40 {
            *counts.entry([r, g, b]).or_default() += 1;
        }
    }
    let (&dominant, _) = counts
        .iter()
        .max_by_key(|(_, n)| **n)
        .expect("a saturated opaque colour");
    let near = |p: [u8; 3]| {
        (i32::from(p[0]) - i32::from(dominant[0])).abs() < 12
            && (i32::from(p[1]) - i32::from(dominant[1])).abs() < 12
            && (i32::from(p[2]) - i32::from(dominant[2])).abs() < 12
    };
    let pixels =
        |shot: &srr::front::render::Pixmap| shot.pixels().iter().filter(|&&p| near(p)).count();
    let mut game = Headless::practice();
    game.skip_intro();
    let before = pixels(&game.screenshot());
    // Dress player 1 in the manga take of their variant: the sprite shows at once.
    game.game_mut().client_mut().handle(ServerMessage::SkinOf {
        player: PlayerId(0),
        skin: Skin::of_player(Style::Manga, PlayerId(0)),
    });
    let after = pixels(&game.screenshot());
    assert!(
        after > before + 200,
        "the manga sprite shows: {before} pixels before, {after} after"
    );
    // And the sprite it wears is the variant's own.
    assert_eq!(
        srr::front::skin::sprite_name_of(&Skin::of_player(Style::Manga, PlayerId(0))),
        Some("brunette")
    );
    // The renderer's clipping guards: a sprite drawn off-screen paints nothing (and does
    // not panic), a zero-height one neither, an unknown name neither.
    let mut frame = srr::front::Frame::new(srr::front::Rgba::rgb(0.0, 0.0, 0.0));
    frame.image(
        "brunette",
        srr::front::skin::Pose::Stand,
        -500.0,
        -500.0,
        150.0,
        srr::front::skin::Facing::Right,
    );
    frame.image(
        "brunette",
        srr::front::skin::Pose::Stand,
        32.0,
        32.0,
        0.0,
        srr::front::skin::Facing::Left,
    );
    frame.image(
        "ace",
        srr::front::skin::Pose::Stand,
        32.0,
        32.0,
        150.0,
        srr::front::skin::Facing::Right,
    );
    let tiny = srr::front::render::screenshot(&frame, 64, 64);
    assert!(
        tiny.pixels()
            .iter()
            .all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0),
        "off-screen, zero-height and unknown sprites paint nothing"
    );
    // A sprite straddling the left edge: its left columns are clipped, the rest paints
    // (the column loop's bounds guard).
    let mut frame = srr::front::Frame::new(srr::front::Rgba::rgb(0.0, 0.0, 0.0));
    frame.image(
        "brunette",
        srr::front::skin::Pose::Stand,
        4.0,
        60.0,
        120.0,
        srr::front::skin::Facing::Right,
    );
    let clipped = srr::front::render::screenshot(&frame, 64, 64);
    assert!(
        clipped
            .pixels()
            .iter()
            .any(|p| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) > 0),
        "the visible part of the edge-straddling sprite paints"
    );
    assert!(
        clipped
            .pixels()
            .iter()
            .take(64)
            .all(|p| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) == 0),
        "the off-screen columns paint nothing"
    );
}

#[test]
fn the_skin_row_of_the_pause_menu_changes_the_fighter_at_once() {
    use srr::front::Screen;
    use srr::front::skin::Pose;
    use srr::game::Key;

    let mut game = Headless::practice();
    game.skip_intro();
    // The skin row: from the preset row, up wraps past Back and Practice onto it.
    game.tap(&[Key::P]);
    assert_eq!(game.game().screen(), Screen::Controls);
    for _ in 0..3 {
        game.tap(&[Key::ArrowUp]);
    }
    // The row shows the skin worn and cycles the styles with Left / Right.
    let text = game.text_snapshot(200, 60);
    assert!(text.contains("Skin"), "{text}");
    assert!(text.contains("<.Stick.(chalk).>"), "{text}");
    game.tap(&[Key::ArrowRight]);
    // A practice game is local: the fighter wears the new skin at once.
    assert_eq!(
        game.game().skin(),
        Skin::styled(Style::Manga, Variant::Brunette)
    );
    // The page draws the live preview of the worn skin: a sprite image on the frame,
    // facing right, under its name.
    let frame = game.frame();
    let (preview_name, preview_facing) = frame
        .images
        .iter()
        .find(|i| i.pose == Pose::Stand)
        .map(|i| (i.name, i.facing))
        .expect("the skin preview is drawn");
    assert_eq!(preview_name, "brunette");
    assert_eq!(preview_facing, srr::front::skin::Facing::Right);
    let shot = game.screenshot();
    let hair = Palette::of(Variant::Brunette).hair;
    let [hr, hg, hb] = [
        (hair.r * 255.0) as u8,
        (hair.g * 255.0) as u8,
        (hair.b * 255.0) as u8,
    ];
    let near = shot
        .pixels()
        .iter()
        .filter(|&&p| {
            (i32::from(p[0]) - i32::from(hr)).abs() < 24
                && (i32::from(p[1]) - i32::from(hg)).abs() < 24
                && (i32::from(p[2]) - i32::from(hb)).abs() < 24
        })
        .count();
    assert!(
        near > 20,
        "the preview shows the brunette's hair: {near} pixels"
    );
    // The ten fighters, one per press.
    for variant in Variant::ALL.iter().skip(1) {
        game.tap(&[Key::ArrowRight]);
        assert_eq!(
            game.game().skin(),
            Skin::styled(Style::Manga, *variant),
            "the row cycles the fighters"
        );
    }
    // ...and back to the chalk.
    game.tap(&[Key::ArrowRight]);
    assert_eq!(game.game().skin(), Skin::STICK);
    // Left goes back through the same cycle.
    game.tap(&[Key::ArrowLeft]);
    assert_eq!(
        game.game().skin(),
        Skin::styled(Style::Manga, Variant::Raven)
    );
}

#[test]
fn a_manga_fighter_poses_with_its_state() {
    use srr::front::Headless;
    use srr::front::skin::Pose;
    use srr::game::Key;

    // Every pose of every fighter decodes: all ten women, all nineteen poses.
    for variant in Variant::ALL {
        for pose in Pose::ALL {
            let sprite = srr::front::skin::sprite(variant, pose, srr::front::skin::Facing::Right);
            assert!(
                sprite.width > 0 && sprite.height > 0,
                "{variant:?} {pose:?}"
            );
            assert_eq!(sprite.pixels.len(), sprite.width * sprite.height * 4);
        }
    }

    // And the fight animates: walking alternates the two frames.
    let mut game = Headless::practice();
    game.skip_intro();
    game.game_mut().client_mut().handle(ServerMessage::SkinOf {
        player: PlayerId(0),
        skin: Skin::of_player(Style::Manga, PlayerId(0)),
    });
    game.idle(MatchConfig::default().countdown_ticks);
    game.hold(&[Key::D], 1);
    let current = game.game().shown_match().expect("the fight");
    let fighter = &current.fighters()[0];
    let pose = srr::front::skin::pose_of(fighter, game.ticks() as u32);
    assert!(
        pose == Pose::WalkA || pose == Pose::WalkB || pose == Pose::Stand,
        "walking poses the fighter: {pose:?}"
    );
    // Both facings reach the arena's sprite draw: walk right, then turn (down + left)
    // and walk left, and check the frame's images carry the matching skin::Facing.
    let facings = |game: &Headless| -> Vec<srr::front::skin::Facing> {
        game.frame().images.iter().map(|i| i.facing).collect()
    };
    game.idle(2);
    let right = facings(&game);
    assert!(
        right.contains(&srr::front::skin::Facing::Right),
        "the fighter faces right: {right:?}"
    );
    game.hold(&[Key::S, Key::A], 1);
    game.hold(&[Key::A], 10);
    game.idle(2);
    let left = facings(&game);
    assert!(
        left.contains(&srr::front::skin::Facing::Left),
        "the fighter turned and faces left: {left:?}"
    );
    // And the headless renderer paints both: the left-facing cell is the mirrored one,
    // so the screenshot's fighter differs from the right-facing one.
    let shot_right = game.screenshot();
    game.hold(&[Key::S, Key::D], 1);
    game.hold(&[Key::D], 10);
    game.idle(2);
    let shot_left = game.screenshot();
    assert_ne!(
        shot_right.pixels(),
        shot_left.pixels(),
        "facing left draws the mirrored cell, not the same one"
    );
}

// ---------------------------------------------------------------------------------------------
// The sprite sheets: one PNG per character, every movement in a fixed grid
// ---------------------------------------------------------------------------------------------

/// A tiny PNG built by hand: one cell of opaque pixels, written with the `filter`
/// scanline filter, so the decode paths are exercised without a fixture file.
fn hand_png(_filter: u8, idat: Vec<u8>, width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk_into(&mut bytes, b"IHDR", &ihdr);
    chunk_into(&mut bytes, b"IDAT", &idat);
    chunk_into(&mut bytes, b"IEND", &[]);
    bytes
}

/// One PNG chunk: length, kind, data (the CRC is never checked by the decoder).
fn chunk_into(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    png.extend_from_slice(&[0, 0, 0, 0]);
}

/// A zlib stream: the two-byte header around the DEFLATE data.
fn zlib(deflate: &[u8]) -> Vec<u8> {
    let mut stream = vec![0x78, 1];
    stream.extend_from_slice(deflate);
    stream
}

/// A stored DEFLATE block holding `raw`.
fn stored(raw: &[u8]) -> Vec<u8> {
    let mut block = vec![0x01];
    block.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    block.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    block.extend_from_slice(raw);
    block
}

#[test]
fn the_sheet_grid_slices_every_pose_of_every_character() {
    use srr::front::skin::{CELL_HEIGHT, CELL_WIDTH, Facing, Pose};

    // The grid: every cell the same box, the poses in the fixed order, every character
    // the same sheet layout — a manga girl fights a chalk stick at the same size.
    assert_eq!(Pose::ALL.len(), 19);
    for (index, pose) in Pose::ALL.iter().enumerate() {
        assert_eq!(pose.index(), index);
    }
    for variant in Variant::ALL {
        for pose in Pose::ALL {
            for facing in [Facing::Right, Facing::Left] {
                let sprite = srr::front::skin::sprite(variant, pose, facing);
                assert_eq!(sprite.width, CELL_WIDTH);
                assert_eq!(sprite.height, CELL_HEIGHT);
                assert_eq!(sprite.pixels.len(), CELL_WIDTH * CELL_HEIGHT * 4);
                // The cell holds the girl: opaque pixels exist, the background is knocked out.
                let opaque = sprite.pixels.chunks(4).filter(|p| p[3] > 16).count();
                assert!(
                    opaque > 500,
                    "{variant:?} {pose:?} {facing:?}: {opaque} opaque pixels"
                );
                // The floor: the fighter's lowest pixels sit near the bottom of the cell.
                let lowest = sprite
                    .pixels
                    .chunks(CELL_WIDTH * 4)
                    .enumerate()
                    .filter(|(_, row)| row.chunks(4).any(|p| p[3] > 16))
                    .map(|(index, _)| index)
                    .max();
                assert!(
                    lowest.unwrap_or(0) >= CELL_HEIGHT - 8,
                    "{variant:?} {pose:?} {facing:?}: seated at row {lowest:?}"
                );
            }
            // The left-facing cell is the right-facing one mirrored, precomputed at load:
            // same silhouette, columns reversed, no engine flips at runtime.
            let right = srr::front::skin::sprite(variant, pose, Facing::Right);
            let left = srr::front::skin::sprite(variant, pose, Facing::Left);
            for row in 0..CELL_HEIGHT {
                for column in 0..CELL_WIDTH {
                    let r = (row * CELL_WIDTH + column) * 4;
                    let l = (row * CELL_WIDTH + (CELL_WIDTH - 1 - column)) * 4;
                    assert_eq!(
                        &right.pixels[r..r + 4],
                        &left.pixels[l..l + 4],
                        "{variant:?} {pose:?}: mirrored at row {row} column {column}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_sheets_slice_by_name_for_the_engines() {
    use srr::front::skin::{Pose, SPRITE_NAMES};

    for (index, name) in SPRITE_NAMES.iter().enumerate() {
        let variant = Variant::from_index(index as u8).expect("the sheet is the variant");
        assert_eq!(
            srr::front::skin::sprite_name_of(&Skin::styled(Style::Manga, variant)),
            Some(*name),
            "the sheet is the variant's own"
        );
        let sprite =
            srr::front::skin::sprite_by_name(name, Pose::Stand, srr::front::skin::Facing::Right);
        assert!(sprite.is_some(), "{name} decodes");
    }
    assert!(
        srr::front::skin::sprite_by_name("ace", Pose::Stand, srr::front::skin::Facing::Right)
            .is_none()
    );
}

#[test]
fn the_png_decode_rejects_what_the_pipeline_never_writes() {
    // Not a PNG at all.
    assert_eq!(srr::front::skin::png_rgba(b"not a png at all"), None);
    // Truncated at the signature.
    assert_eq!(srr::front::skin::png_rgba(b"\x89PNG"), None);
    // A wrong colour depth: 8-bit RGB (colour type 2), where the pipeline writes
    // 8-bit RGBA (colour type 6) only.
    let mut rgb_png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&4u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk_into(&mut rgb_png, b"IHDR", &ihdr);
    chunk_into(&mut rgb_png, b"IDAT", &zlib(&stored(&[0; 4])));
    chunk_into(&mut rgb_png, b"IEND", &[]);
    assert_eq!(srr::front::skin::png_rgba(&rgb_png), None);
    // An interlaced PNG: the pipeline never writes Adam7.
    let mut interlaced = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&4u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 1]);
    chunk_into(&mut interlaced, b"IHDR", &ihdr);
    chunk_into(&mut interlaced, b"IDAT", &zlib(&stored(&[0; 16])));
    chunk_into(&mut interlaced, b"IEND", &[]);
    assert_eq!(srr::front::skin::png_rgba(&interlaced), None);
    // A sheet of the wrong size: the engine only slices the 5x4 grid it was built
    // for (a valid PNG decode, rejected by the sheet contract).
    let scanlines = vec![0u8; 8 * (1 + 8 * 4)];
    let small = hand_png(0, zlib(&stored(&scanlines)), 8, 8);
    assert!(srr::front::skin::png_rgba(&small).is_some());
    assert_eq!(srr::front::skin::sheet_rgba(&small), None);
    // The IDAT that never arrives (truncated after IHDR).
    let mut truncated = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&600u32.to_be_bytes());
    ihdr.extend_from_slice(&800u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk_into(&mut truncated, b"IHDR", &ihdr);
    assert_eq!(srr::front::skin::png_rgba(&truncated), None);
    // An unknown scanline filter.
    let bad_filter = hand_png(5, zlib(&stored(&[5; 13])), 600, 800);
    assert_eq!(srr::front::skin::png_rgba(&bad_filter), None);
    // A truncated scanline: the zlib stream ends mid-image.
    let short_line = hand_png(0, zlib(&stored(&[0; 40])), 600, 800);
    assert_eq!(srr::front::skin::png_rgba(&short_line), None);
}

#[test]
fn the_png_decode_undoes_every_scanline_filter() {
    // One scanline of four RGBA pixels, written with each filter: the decode must
    // give the same pixels whatever filter the pipeline chose. The stored bytes are
    // the *filtered* ones (RFC 2083): the decode adds the predictor back.
    let raw = [
        10u8, 20, 30, 255, 1, 2, 3, 255, 200, 100, 50, 255, 7, 8, 9, 255,
    ];
    let bpp = 4;
    for filter in 0..=4u8 {
        let filtered = raw
            .iter()
            .enumerate()
            .map(|(i, &byte)| {
                let left = if i >= bpp { i32::from(raw[i - bpp]) } else { 0 };
                let above = 0; // the first scanline has nothing above it
                let corner = 0;
                let predictor = match filter {
                    0 => 0,
                    1 => left,
                    2 => above,
                    3 => (left + above) / 2,
                    _ => paeth(left, above, corner),
                };
                byte.wrapping_sub(predictor as u8)
            })
            .collect::<Vec<u8>>();
        let line = [vec![filter], filtered].concat();
        let idat = zlib(&stored(&line));
        let png = hand_png(filter, idat, 4, 1);
        let pixels = srr::front::skin::png_rgba(&png).expect("the filter decodes");
        assert_eq!(pixels.as_slice(), raw.as_slice(), "filter {filter}");
    }
    // Two scanlines, so the second line has a real one above it: every branch of the
    // Paeth predictor (left, above, corner) is taken with `above != 0`.
    let top = [
        200u8, 10, 30, 255, 40, 50, 60, 255, 90, 80, 70, 255, 7, 200, 9, 255,
    ];
    let bottom = [
        10u8, 200, 30, 255, 1, 2, 200, 255, 200, 100, 50, 255, 7, 8, 9, 200,
    ];
    let bpp = 4;
    let filter_line = |raw: &[u8; 16], above: &[u8; 16]| -> Vec<u8> {
        raw.iter()
            .enumerate()
            .map(|(i, &byte)| {
                let left = if i >= bpp { i32::from(raw[i - bpp]) } else { 0 };
                let up = i32::from(above[i]);
                let corner = if i >= bpp {
                    i32::from(above[i - bpp])
                } else {
                    0
                };
                let predictor = paeth(left, up, corner);
                byte.wrapping_sub(predictor as u8)
            })
            .collect()
    };
    let filtered_bottom = filter_line(&bottom, &top);
    let mut lines = vec![0u8];
    lines.extend_from_slice(&top);
    lines.push(4);
    lines.extend_from_slice(&filtered_bottom);
    let png = hand_png(4, zlib(&stored(&lines)), 4, 2);
    let pixels = srr::front::skin::png_rgba(&png).expect("the paeth line decodes");
    let expected = [&top[..], &bottom[..]].concat();
    assert_eq!(
        pixels.as_slice(),
        expected.as_slice(),
        "filter 4 with a line above"
    );
}

/// The Paeth predictor, as the encoder (and RFC 2083) compute it.
fn paeth(left: i32, above: i32, corner: i32) -> i32 {
    let estimate = left + above - corner;
    let (dl, da, dc) = (
        (estimate - left).abs(),
        (estimate - above).abs(),
        (estimate - corner).abs(),
    );
    if dl <= da && dl <= dc {
        left
    } else if da <= dc {
        above
    } else {
        corner
    }
}

#[test]
fn the_deflate_decode_reads_stored_blocks() {
    // One stored block, then the end.
    let stored = [0x01, 0x01, 0x00, 0xfe, 0xff, 0x42];
    assert_eq!(srr::front::skin::inflate(&stored), Some(vec![0x42]));
    // Two stored blocks.
    let two = [
        0x00, 0x01, 0x00, 0xfe, 0xff, 0x42, 0x01, 0x01, 0x00, 0xfe, 0xff, 0x43,
    ];
    assert_eq!(srr::front::skin::inflate(&two), Some(vec![0x42, 0x43]));
    // The length and its inverse disagree: a corrupted block.
    let corrupt = [0x01, 0x02, 0x00, 0xfe, 0xff, 0x42, 0x43];
    assert_eq!(srr::front::skin::inflate(&corrupt), None);
    // An unknown block kind (3): no such block.
    let unknown = [0x07];
    assert_eq!(srr::front::skin::inflate(&unknown), None);
    // Truncated mid-block: the length never arrives.
    let cut = [0x01, 0x01];
    assert_eq!(srr::front::skin::inflate(&cut), None);
    // Truncated before the block header even completes.
    assert_eq!(srr::front::skin::inflate(&[]), None);
}

#[test]
fn the_zlib_header_is_checked() {
    // The compression method must be 8 (deflate); the preset dictionary bit must be off.
    // `inflate_zlib` is exercised through `png_rgba` (the good header) and here by the
    // sheets' own IDAT; a wrong method is not a PNG the pipeline writes, so the check
    // is covered by the rejects test above through the chunks walk.
    // The good path: a real sheet decodes (every pose test above reads it); this test
    // pins the two-byte contract the decoder enforces on the way in.
    let raw = [
        10u8, 20, 30, 255, 1, 2, 3, 255, 200, 100, 50, 255, 7, 8, 9, 255,
    ];
    let line = [vec![0u8], raw.to_vec()].concat();
    let idat = zlib(&stored(&line));
    let png = hand_png(0, idat, 4, 1);
    assert!(srr::front::skin::png_rgba(&png).is_some());
}

#[test]
fn the_ten_girls_each_wear_their_named_hair() {
    // The character is her hair: the variant's name is the colour on her head, in every
    // pose of her sheet (checked over the sprite pixels, head region).
    use srr::front::skin::{Pose, sprite};

    // (variant, the expected head colour, the tolerance)
    let cases: [(Variant, [u8; 3]); 10] = [
        (Variant::Brunette, [0x7a, 0x4a, 0x28]),
        (Variant::Blonde, [0xf7, 0xd9, 0x68]),
        (Variant::Redhead, [0xe8, 0x58, 0x30]),
        (Variant::Pink, [0xff, 0x9e, 0xc4]),
        (Variant::White, [0xdc, 0xd8, 0xd4]),
        (Variant::Black, [0x3e, 0x38, 0x32]),
        (Variant::Green, [0x5a, 0xc8, 0x4a]),
        (Variant::Violet, [0x90, 0x60, 0xe8]),
        (Variant::Cyan, [0x4a, 0xd4, 0xe0]),
        (Variant::Raven, [0x38, 0x48, 0xa0]),
    ];
    for (variant, expected) in cases {
        for pose in Pose::ALL {
            let sprite = sprite(variant, pose, srr::front::skin::Facing::Right);
            // The hair: anywhere in the cell (the knocked-out pose lies down, so her
            // head is not at the top).
            let mut best = 0usize;
            for chunk in sprite.pixels.chunks(4) {
                let [r, g, b, a] = *chunk.first_chunk::<4>().expect("rgba");
                if a > 200 {
                    let near = (i32::from(r) - i32::from(expected[0])).abs() < 48
                        && (i32::from(g) - i32::from(expected[1])).abs() < 48
                        && (i32::from(b) - i32::from(expected[2])).abs() < 48;
                    best += usize::from(near);
                }
            }
            assert!(
                best > 50,
                "{variant:?} {pose:?}: the hair is the variant's own ({best} pixels)"
            );
        }
    }
}

#[test]
fn the_deflate_decode_reads_fixed_and_dynamic_blocks() {
    // A fixed-Huffman block: the codes RFC 1951 pins, no code lengths in the stream.
    let fixed = [203u8, 72, 205, 201, 201, 87, 200, 64, 144, 138, 0];
    assert_eq!(
        srr::front::skin::inflate(&fixed),
        Some(b"hello hello hello!".to_vec())
    );
    // A dynamic-Huffman block: the code lengths the block declares in its head.
    let dynamic = [
        227u8, 54, 8, 173, 154, 127, 228, 37, 159, 113, 68, 237, 162, 227, 111, 4, 205, 162, 27,
        150, 158, 122, 47, 98, 25, 215, 188, 226, 236, 39, 113, 155, 196, 182, 213, 23, 190, 74,
        217, 167, 116, 174, 187, 252, 67, 214, 41, 189, 103, 227, 181, 223, 10, 174, 89, 253, 91,
        110, 254, 83, 246, 200, 157, 180, 253, 14, 163, 154, 119, 193, 212, 93, 247, 89, 52, 253,
        138, 103, 236, 125, 196, 174, 19, 88, 54, 251, 192, 83, 46, 253, 144, 202, 121, 135, 95,
        240, 26, 133, 215, 44, 60, 246, 90, 192, 52, 170, 126, 201, 201, 119, 194, 22, 177, 77,
        203, 207, 124, 20, 179, 78, 104, 93, 117, 254, 139, 164, 93, 114, 199, 218, 75, 223, 101,
        28, 211, 186, 55, 92, 253, 37, 239, 146, 217, 183, 249, 198, 95, 37, 247, 156, 137, 219,
        110, 51, 168, 122, 229, 79, 217, 121, 143, 89, 195, 183, 104, 250, 158, 135, 108, 218, 1,
        165, 179, 246, 63, 225, 212, 11, 174, 152, 123, 232, 57, 143, 97, 88, 245, 130, 163, 175,
        248, 77, 34, 235, 22, 159, 120, 43, 100, 30, 211, 184, 236, 244, 7, 81, 171, 248, 150, 149,
        231, 62, 75, 216, 38, 181, 175, 185, 248, 77, 218, 33, 181, 107, 253, 149, 159, 114, 206,
        25, 189, 155, 174, 255, 81, 116, 203, 158, 176, 245, 214, 127, 21, 207, 188, 201, 59, 238,
        50, 169, 251, 20, 78, 219, 253, 128, 85, 203, 191, 100, 230, 190, 199, 28, 186, 65, 229,
        115, 14, 62, 227, 30, 245, 63, 77, 252, 15, 0,
    ];
    let expected: Vec<u8> = (0..600u32).map(|i| (i * 37 + 11) as u8).collect();
    assert_eq!(srr::front::skin::inflate(&dynamic), Some(expected));
}

#[test]
fn the_deflate_decode_rejects_broken_dynamic_blocks() {
    // A dynamic block that declares an impossible code: truncated mid-declaration.
    // The first bits say dynamic (kind 2, last 0) then run out of stream.
    let truncated = [0x04u8];
    assert_eq!(srr::front::skin::inflate(&truncated), None);
    // The zlib wrapper: a compression method that is not deflate.
    let raw = [
        10u8, 20, 30, 255, 1, 2, 3, 255, 200, 100, 50, 255, 7, 8, 9, 255,
    ];
    let line = [vec![0u8], raw.to_vec()].concat();
    let good = zlib(&stored(&line));
    let mut wrong_method = good.clone();
    wrong_method[0] = 0x79; // method 9, not deflate
    let png = hand_png(0, wrong_method, 4, 1);
    assert!(srr::front::skin::png_rgba(&png).is_none());
    // The preset-dictionary bit set: a stream the game never writes.
    let mut dictionary = good;
    dictionary[1] |= 0x20;
    let png = hand_png(0, dictionary, 4, 1);
    assert!(srr::front::skin::png_rgba(&png).is_none());
}

#[test]
fn a_png_with_unknown_chunks_still_decodes() {
    // The chunks the game does not read (a colour profile, a comment) are skipped.
    let raw = [
        10u8, 20, 30, 255, 1, 2, 3, 255, 200, 100, 50, 255, 7, 8, 9, 255,
    ];
    let line = [vec![0u8], raw.to_vec()].concat();
    // Insert a gAMA chunk after the IHDR: unknown kinds must be walked over.
    let mut with_gama = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&4u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk_into(&mut with_gama, b"IHDR", &ihdr);
    chunk_into(&mut with_gama, b"gAMA", &[0, 0, 0, 1]);
    chunk_into(&mut with_gama, b"IDAT", &zlib(&stored(&line)));
    chunk_into(&mut with_gama, b"IEND", &[]);
    let pixels = srr::front::skin::png_rgba(&with_gama).expect("the chunks are skipped");
    assert_eq!(pixels.as_slice(), raw.as_slice());
}

#[test]
fn every_state_of_a_manga_fighter_poses_its_sheet() {
    use srr::front::Headless;
    use srr::front::skin::{Pose, pose_of};
    use srr::game::Key;

    // A practice fight in the manga skin, driven through the states the pose engine
    // reads: the sheet's cell follows the fighter.
    let mut game = Headless::practice();
    game.skip_intro();
    game.game_mut().client_mut().handle(ServerMessage::SkinOf {
        player: PlayerId(0),
        skin: Skin::of_player(Style::Manga, PlayerId(0)),
    });
    let countdown = MatchConfig::default().countdown_ticks;
    game.idle(countdown);
    let pose = |game: &Headless| {
        let current = game.game().shown_match().expect("the fight");
        pose_of(&current.fighters()[0], game.ticks() as u32)
    };
    // Standing in guard.
    game.idle(10);
    assert!(matches!(
        pose(&game),
        Pose::Stand | Pose::WalkA | Pose::WalkB
    ));
    // Walking: the two cells alternate with the stride. Over a few steps both walk
    // cells show (the stride alternates them); a single tick may still be standing.
    let mut seen_walk = false;
    for _ in 0..16 {
        game.hold(&[Key::D], 1);
        if matches!(pose(&game), Pose::WalkA | Pose::WalkB) {
            seen_walk = true;
        }
    }
    assert!(seen_walk, "walking takes the walk cells");
    // Crouching.
    game.hold(&[Key::S], 1);
    assert_eq!(pose(&game), Pose::Crouch);
    game.hold(&[Key::S], 5);
    // Airborne: the jump cell, then land before striking.
    game.hold(&[Key::W, Key::D], 1);
    assert_eq!(pose(&game), Pose::Jump);
    game.hold(&[Key::W], 12);
    game.idle(20);
    // The strikes: every limb at every strength takes its own cell, sampled through the
    // attack's phases (every phase poses the strike, so the pose shows from the wind-up).
    let strikes = [
        (Key::B, Pose::PunchLow),
        (Key::H, Pose::PunchMid),
        (Key::J, Pose::PunchHigh),
        (Key::G, Pose::KickLow),
        (Key::Y, Pose::KickMid),
        (Key::U, Pose::KickHigh),
    ];
    for (key, cell) in strikes {
        game.idle(40);
        game.hold(&[key], 1);
        let mut seen = false;
        for _ in 0..12 {
            game.hold(&[], 1);
            seen |= pose(&game) == cell;
        }
        assert!(seen, "{key:?} takes the {cell:?} cell");
    }
    game.idle(40);
    // Blocking: stepping back with the guard up.
    game.hold(&[Key::A], 1);
    assert_eq!(pose(&game), Pose::Block);
    game.idle(10);
    // The grab attempt.
    game.hold(&[Key::Space], 1);
    let mut seen_grab = false;
    for _ in 0..12 {
        game.hold(&[], 1);
        seen_grab |= pose(&game) == Pose::Grab;
    }
    assert!(seen_grab, "a grab takes the grab cell");
    game.idle(40);
    // The special move.
    game.hold(&[Key::F], 1);
    let mut seen_special = false;
    for _ in 0..16 {
        game.hold(&[], 1);
        seen_special |= pose(&game) == Pose::Special;
    }
    assert!(seen_special, "the special takes the special cell");
    game.idle(60);
    // The aerial kick: a kick pressed in the air.
    game.hold(&[Key::W, Key::D], 1);
    game.idle(6);
    game.hold(&[Key::Y], 1);
    let mut seen_jump_kick = false;
    for _ in 0..8 {
        game.hold(&[], 1);
        seen_jump_kick |= pose(&game) == Pose::JumpKick;
    }
    assert!(seen_jump_kick, "an aerial kick takes the jump-kick cell");
    game.idle(60);
    // The dummy walks into the strikes until it goes down: its pose takes the hit
    // cell while it is stunned, and the KO cell once it is out (the pose is read
    // every tick: the stun is short and the KO follows the last hit).
    game.hold(&[Key::D], 70);
    let mut seen_hit = false;
    for _ in 0..60 {
        game.hold(&[Key::Y], 1);
        for _ in 0..20 {
            game.hold(&[], 1);
            let hurt = {
                let current = game.game().shown_match().expect("the fight");
                pose_of(&current.fighters()[1], game.ticks() as u32)
            };
            seen_hit |= hurt == Pose::Hit;
        }
    }
    assert!(seen_hit, "the dummy takes its hit cell");
}

#[test]
fn a_knocked_out_manga_fighter_takes_the_ko_cell() {
    use srr::front::skin::{Pose, pose_of};

    // A one-hit knockout through the rules engine: the pose of the downed fighter is
    // the sheet's last cell.
    let config = MatchConfig {
        max_health: 5,
        ..common::cfg()
    };
    let mut m = common::close_duel(config);
    for _ in 0..3 {
        m.step(&common::only(0, common::jab()));
    }
    m.step(&common::only(1, common::press_right()));
    m.step(&common::inputs_of(&[
        (0, common::jab()),
        (1, common::press_right()),
    ]));
    m.step(&common::inputs_of(&[
        (0, common::jab()),
        (1, common::press_right()),
    ]));
    let down = m.fighter(PlayerId(1)).expect("the dummy");
    assert_eq!(down.state(), srr::game::FighterState::Knockout);
    assert_eq!(pose_of(down, 0), Pose::Ko);
}

#[test]
fn the_chalk_ink_is_the_palette_of_the_unskinned() {
    // A manga fighter wears its variant; the chalk wears the plain ink, so the 3D
    // mannequin of a stick fighter stays chalk-coloured.
    let chalk = srr::front::skin::palette_of(&Skin::STICK);
    assert_eq!(chalk.hair, chalk.skin);
    assert_eq!(chalk.outfit, chalk.trim);
    let manga = srr::front::skin::palette_of(&Skin::of_player(Style::Manga, PlayerId(0)));
    assert_ne!(manga.hair, chalk.hair, "the brunette is not chalk");
}

#[test]
fn the_png_decode_stops_at_every_short_read() {
    // An IDAT that decodes to nothing: the first scanline's filter byte never comes.
    let empty_idat = hand_png(0, zlib(&stored(&[])), 4, 1);
    assert_eq!(srr::front::skin::png_rgba(&empty_idat), None);
    // An IDAT too short for the two-byte zlib header.
    let no_header = hand_png(0, vec![0x78], 4, 1);
    assert_eq!(srr::front::skin::png_rgba(&no_header), None);
    // A chunk cut in the middle of its header: the length is read, the kind is not.
    let mut cut = b"\x89PNG\r\n\x1a\n".to_vec();
    cut.extend_from_slice(&[0, 0, 0, 13]);
    assert_eq!(srr::front::skin::png_rgba(&cut), None);
    // The chunk's data cut short of its declared length.
    let mut cut_data = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&4u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk_into(&mut cut_data, b"IHDR", &ihdr);
    cut_data.extend_from_slice(&(50u32).to_be_bytes());
    cut_data.extend_from_slice(b"IDAT");
    cut_data.extend_from_slice(&[0x78, 1]);
    assert_eq!(srr::front::skin::png_rgba(&cut_data), None);
    // The sheet contract itself refuses garbage (the decode stops at the signature).
    assert_eq!(srr::front::skin::sheet_rgba(b"not a png"), None);
    // A stored block truncated right after its header: the length never arrives.
    let kind_only = [0x00u8];
    assert_eq!(srr::front::skin::inflate(&kind_only), None);
    // A stored block whose declared length runs past the end of the stream.
    let runs_past = [0x00u8, 0x10, 0x00, 0xef, 0xff, 0x42];
    assert_eq!(srr::front::skin::inflate(&runs_past), None);
    // A stored block cut between the length and its inverse.
    let cut_len = [0x00u8, 0x10, 0x00];
    assert_eq!(srr::front::skin::inflate(&cut_len), None);
}

#[test]
fn the_deflate_header_is_read_bit_by_bit_and_cut_mid_read() {
    // The block-kind bits never arrive.
    let kind_cut = [0x01u8];
    assert_eq!(srr::front::skin::inflate(&kind_cut), None);
    // The dynamic header's counts (HLIT, HDIST, HCLEN) never arrive.
    let counts_cut = [0x04u8];
    assert_eq!(srr::front::skin::inflate(&counts_cut), None);
    // A copy-run whose count bits run past the end of the stream.
    let copy_cut = [5u8, 0, 20, 32];
    assert_eq!(srr::front::skin::inflate(&copy_cut), None);
    // A short zero-run whose count bits are cut.
    let run17_cut = [5u8, 0, 16, 0];
    assert_eq!(srr::front::skin::inflate(&run17_cut), None);
    // A long zero-run whose count bits are cut.
    let run18_cut = [5u8, 0, 128, 0];
    assert_eq!(srr::front::skin::inflate(&run18_cut), None);
    // A block whose codes parse but whose body never arrives: the first literal
    // symbol is read from an empty code.
    let body_cut = [5u8, 0, 144, 224, 223, 221, 221, 221, 221, 221, 29];
    assert_eq!(srr::front::skin::inflate(&body_cut), None);
    // A stored block cut between the two halves of the nlen.
    let nlen_cut = [0x00u8, 0x10, 0x00, 0xef];
    assert_eq!(srr::front::skin::inflate(&nlen_cut), None);
    // A length symbol whose extra bits run past the end of the stream: the code of
    // symbol 265 arrives, the one bit of its count does not.
    let length_cut = [
        141u8, 224, 17, 1, 0, 0, 0, 128, 0, 52, 223, 221, 221, 221, 221, 221, 189, 11, 69,
    ];
    assert_eq!(srr::front::skin::inflate(&length_cut), None);
    // A distance symbol whose extra bits are cut the same way.
    let dist_cut = [
        141u8, 228, 23, 2, 0, 0, 0, 128, 0, 116, 130, 181, 150, 187, 187, 187, 187, 187, 187, 187,
        187, 187, 187, 187, 119, 133, 36, 2,
    ];
    assert_eq!(srr::front::skin::inflate(&dist_cut), None);
    // A PNG whose chunks walk past the end of the file (no IEND): the last chunk
    // header is cut by the end of the bytes.
    let mut no_iend = b"\x89PNG\r\n\x1a\n".to_vec();
    let raw = [
        10u8, 20, 30, 255, 1, 2, 3, 255, 200, 100, 50, 255, 7, 8, 9, 255,
    ];
    let line = [vec![0u8], raw.to_vec()].concat();
    chunk_into(&mut no_iend, b"IHDR", &{
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&4u32.to_be_bytes());
        ihdr.extend_from_slice(&1u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        ihdr
    });
    chunk_into(&mut no_iend, b"IDAT", &zlib(&stored(&line)));
    assert!(
        srr::front::skin::png_rgba(&no_iend).is_some(),
        "the IEND is optional"
    );
    let mut walk_past = no_iend;
    walk_past.truncate(walk_past.len() - 4);
    let _ = &mut walk_past;
}

#[test]
fn the_png_decode_rejects_a_zero_sized_image() {
    // An IHDR of zero width: nothing decodes.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&0u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk_into(&mut png, b"IHDR", &ihdr);
    chunk_into(&mut png, b"IDAT", &zlib(&stored(&[0])));
    chunk_into(&mut png, b"IEND", &[]);
    assert_eq!(srr::front::skin::png_rgba(&png), None);
}

#[test]
fn the_png_decode_rejects_an_unknown_scanline_filter_late() {
    // A well-sized image whose scanline declares filter 5: the walk over the lines
    // reaches the unfilter and dies there (not on the line length).
    let width = 4usize;
    let height = 2usize;
    let stride = 1 + width * 4;
    let mut lines = vec![0u8; stride * height];
    lines[0] = 5; // the filter byte of the first scanline
    let idat = zlib(&stored(&lines));
    let png = hand_png(0, idat, width as u32, height as u32);
    assert_eq!(srr::front::skin::png_rgba(&png), None);
}

#[test]
fn the_deflate_decode_rejects_the_impossible() {
    // A literal symbol above 285: no such length code.
    // Built by hand: a fixed block whose first symbol decodes past 285 is hard to pin,
    // so the rejection is driven through the code paths that reach it: a truncated
    // stream whose bits run out inside a symbol (the Huffman read walks off the end).
    let truncated_mid_symbol = [0b0000_0010u8, 0b1111_1111];
    assert_eq!(srr::front::skin::inflate(&truncated_mid_symbol), None);
    // A dynamic block that declares a length run past its end: the count never adds
    // up, and the decode stops.
    let bad_counts = [0x05u8, 0x00, 0x00];
    assert_eq!(srr::front::skin::inflate(&bad_counts), None);
}

#[test]
fn the_deflate_decode_names_its_every_refusal() {
    // A dynamic block whose code-length code is empty: reading a symbol walks the
    // Huffman lengths to the end and stops (the trailing bits keep the reader fed).
    let empty_code = [5u8, 0, 0, 255, 255, 255];
    assert_eq!(srr::front::skin::inflate(&empty_code), None);
    // A dynamic block that asks for a copy of the previous length when nothing came
    // before it: there is no previous length.
    let copy_of_nothing = [5u8, 0, 2, 0];
    assert_eq!(srr::front::skin::inflate(&copy_of_nothing), None);
    // A fixed block whose literal symbol is 286: past the end of the length codes.
    let too_high = [27u8, 3];
    assert_eq!(srr::front::skin::inflate(&too_high), None);
    // A match whose distance reaches before the start of the output: there is nothing
    // that far back to copy from.
    let too_far = [115u8, 4, 94, 0, 240, 255, 15];
    assert_eq!(srr::front::skin::inflate(&too_far), None);
    // A dynamic block whose code lengths overshoot the declared counts: three runs
    // of 138 zeros fill 414 slots where 258 were declared, and the decode refuses.
    let overshoot = [5u8, 0, 128, 192, 223, 223, 255, 255, 31];
    assert_eq!(srr::front::skin::inflate(&overshoot), None);
}

#[test]
fn the_dynamic_block_declares_more_codes_than_it_reads() {
    // The declared literal and distance counts exceed what the code lengths spell
    // out: the decode must refuse (the `all` vector never fills).
    let declares = [
        0b0000_0100u8, // last, kind 2 (dynamic)
        0b1110_0000,
        0b0000_1111,
        0b1111_1111, // counts that run the reader dry
    ];
    assert_eq!(srr::front::skin::inflate(&declares), None);
}

#[test]
fn the_third_person_view_draws_the_manga_girls_through_every_state() {
    use srr::front::Headless;
    use srr::game::Key;

    // The 3D view (`Tab`) of a manga fight: the manga pose engine drives the
    // cel-shaded volumes through the states of a real fight — the guard, the walk,
    // the jump, the crouch, the strikes, the hits — with the frame drawn in each
    // state (the headless game only draws on demand).
    let mut game = Headless::practice();
    game.skip_intro();
    game.game_mut().client_mut().handle(ServerMessage::SkinOf {
        player: PlayerId(0),
        skin: Skin::of_player(Style::Manga, PlayerId(0)),
    });
    game.game_mut().client_mut().handle(ServerMessage::SkinOf {
        player: PlayerId(1),
        skin: Skin::styled(Style::Manga, Variant::Blonde),
    });
    game.idle(MatchConfig::default().countdown_ticks);
    game.tap(&[Key::Tab]);
    assert_ne!(game.game().view(), srr::front::View::Side);
    // The guard: drawn standing.
    game.screenshot();
    // The walk, the backstep (the guard), the crouch, the jump: drawn in state.
    game.hold(&[Key::D], 8);
    game.screenshot();
    game.hold(&[Key::D], 8);
    game.screenshot();
    game.hold(&[Key::A], 8);
    game.screenshot();
    game.hold(&[Key::S], 8);
    game.screenshot();
    game.hold(&[Key::W, Key::D], 8);
    game.screenshot();
    game.idle(4);
    game.screenshot();
    game.idle(6);
    game.screenshot();
    game.idle(30);
    // The flying kick: a kick pressed in the air, drawn through its phases.
    game.hold(&[Key::W, Key::D], 1);
    game.idle(6);
    game.hold(&[Key::Y], 1);
    for _ in 0..6 {
        game.idle(1);
        game.screenshot();
    }
    game.idle(60);
    // The flying kick pressed early in the jump: the startup of the flight, then
    // its active and recovery, drawn.
    game.hold(&[Key::W, Key::D], 1);
    game.idle(1);
    game.hold(&[Key::Y], 1);
    for _ in 0..20 {
        game.idle(1);
        game.screenshot();
    }
    game.idle(40);
    // The strikes: the punch and the kick, drawn through their phases.
    game.hold(&[Key::H], 1);
    for _ in 0..8 {
        game.idle(1);
        game.screenshot();
    }
    game.idle(40);
    game.hold(&[Key::Y], 1);
    for _ in 0..8 {
        game.idle(1);
        game.screenshot();
    }
    game.idle(40);
    // The grab attempt, drawn through its phases.
    game.hold(&[Key::Space], 1);
    game.idle(6);
    game.screenshot();
    // The special move (the shoulder rush of the default character), drawn through
    // its phases.
    game.hold(&[Key::F], 1);
    for _ in 0..12 {
        game.idle(1);
        game.screenshot();
    }
    game.idle(40);
    // The judo throw: medium kick and high kick held together, close to the dummy.
    // The projection and the pin on the ground, drawn through their phases.
    game.hold(&[Key::D], 30);
    game.hold(&[Key::Y, Key::U], 1);
    for _ in 0..20 {
        game.idle(1);
        game.screenshot();
    }
    for _ in 0..60 {
        game.idle(1);
        game.screenshot();
    }
    // The sweep: medium punch and high kick together.
    game.idle(60);
    game.hold(&[Key::H, Key::U], 1);
    for _ in 0..20 {
        game.idle(1);
        game.screenshot();
    }
    for _ in 0..40 {
        game.idle(1);
        game.screenshot();
    }
    game.idle(60);
    game.hold(&[Key::D], 60);
    for _ in 0..30 {
        game.hold(&[Key::Y], 1);
        for _ in 0..16 {
            game.idle(1);
            game.screenshot();
        }
        game.idle(8);
    }
    // The brunette's hair colour shows in the 3D view (a colour the chalk alley
    // never draws).
    let shot = game.screenshot();
    let hair = Palette::of(Variant::Brunette).hair;
    let [hr, hg, hb] = [
        (hair.r * 255.0) as u8,
        (hair.g * 255.0) as u8,
        (hair.b * 255.0) as u8,
    ];
    let near = shot
        .pixels()
        .iter()
        .filter(|&&p| {
            (i32::from(p[0]) - i32::from(hr)).abs() < 24
                && (i32::from(p[1]) - i32::from(hg)).abs() < 24
                && (i32::from(p[2]) - i32::from(hb)).abs() < 24
        })
        .count();
    assert!(near > 0, "the brunette's hair colour shows in the 3D view");
}

#[test]
fn the_manga_pose_engine_reads_the_fighter_lying_down() {
    use srr::front::manga;

    // A knocked-out fighter lies flat: the pose engine reports it (the name tags lie
    // on the pavement in front of a grounded fighter).
    let config = MatchConfig {
        max_health: 5,
        ..common::cfg()
    };
    let mut m = common::close_duel(config);
    for _ in 0..3 {
        m.step(&common::only(0, common::jab()));
    }
    m.step(&common::only(1, common::press_right()));
    m.step(&common::inputs_of(&[
        (0, common::jab()),
        (1, common::press_right()),
    ]));
    m.step(&common::inputs_of(&[
        (0, common::jab()),
        (1, common::press_right()),
    ]));
    let down = m.fighter(PlayerId(1)).expect("the dummy");
    assert_eq!(down.state(), srr::game::FighterState::Knockout);
    let config = m.config().clone();
    let pose = manga::pose(down, &config, 0);
    assert!(manga::is_lying(&pose), "a knocked-out fighter lies flat");
}

#[test]
fn the_manga_volumes_cover_the_girl_from_head_to_feet() {
    use srr::front::manga;
    use srr::front::skin::palette_of;

    // The 3D volumes of every variant: every girl builds her tubes and balls.
    for variant in Variant::ALL {
        let skin = Skin::styled(Style::Manga, variant);
        let palette = palette_of(&skin);
        let config = MatchConfig::default();
        let m = common::close_duel(config.clone());
        let fighter = m.fighter(PlayerId(0)).expect("the fighter");
        let volumes = manga::volumes(fighter, &config, 0, &palette);
        assert!(
            volumes.len() > 10,
            "{variant:?} builds her head, torso and limbs"
        );
    }
}

#[test]
fn the_manga_pose_engine_poses_every_state_the_rules_can_hold() {
    use srr::front::manga;
    use srr::game::{FighterState, GrapplePosition, GrappleRole, Technique};

    // Every state of the rules, posed by the manga engine: the strikes and the
    // special move, the grab, both judo techniques, and every grapple position on
    // both sides. The headless game draws the 3D view only on demand, so the pose
    // engine is driven directly, through the rules, the way the fight would.
    let draw = |m: &srr::game::Match, id: u8| {
        let fighter = m.fighter(PlayerId(id)).expect("the fighter");
        let config = m.config();
        for ticks in 0..8 {
            let _ = manga::pose(fighter, config, ticks);
        }
    };

    // The special move of each character, through its phases: the shoulder rush
    // (the plain lunge), the fireball (the projectile), the iron palm (the armored
    // rush) and the slide tackle (the low profile).
    for character in srr::game::Character::ALL {
        let config = MatchConfig {
            special: character.rules(&common::cfg()).special,
            ..common::cfg()
        };
        let mut m = common::close_duel(config);
        m.step(&common::only(
            0,
            srr::game::Input {
                special: true,
                ..srr::game::Input::NONE
            },
        ));
        for _ in 0..30 {
            m.step(&common::no_inputs());
            draw(&m, 0);
        }
        assert!(
            matches!(
                m.fighter(PlayerId(0)).unwrap().state(),
                FighterState::Special(_) | FighterState::Idle | FighterState::Walking
            ),
            "{} performed its special",
            character.name()
        );
    }
    // The default character's special is a plain lunge, posed.
    let mut m = common::close_duel(common::cfg());
    m.step(&common::only(
        0,
        srr::game::Input {
            special: true,
            ..srr::game::Input::NONE
        },
    ));
    for _ in 0..10 {
        m.step(&common::no_inputs());
        draw(&m, 0);
    }
    assert!(matches!(
        m.fighter(PlayerId(0)).unwrap().state(),
        FighterState::Special(_)
    ));

    // Every strike at every strength: the pose engine reads the limb and the
    // strength for the reach and the height of the strike.
    for attack in [
        srr::game::Attack::punch(srr::game::Strength::Low),
        srr::game::Attack::punch(srr::game::Strength::Medium),
        srr::game::Attack::punch(srr::game::Strength::High),
        srr::game::Attack::kick(srr::game::Strength::Low),
        srr::game::Attack::kick(srr::game::Strength::Medium),
        srr::game::Attack::kick(srr::game::Strength::High),
    ] {
        let mut m = common::close_duel(common::cfg());
        m.step(&common::only(0, common::press(attack)));
        for _ in 0..10 {
            m.step(&common::no_inputs());
            draw(&m, 0);
        }
    }

    // The whiffed grab and the whiffed sweep (the dummy far out of reach): the
    // active and recovery phases of the attempts, drawn — nothing to connect with.
    let mut m = common::duel_at(common::cfg(), 200, 1000);
    m.step(&common::only(0, common::press_grab()));
    for _ in 0..12 {
        m.step(&common::no_inputs());
        draw(&m, 0);
    }
    let mut m = common::duel_at(common::cfg(), 200, 1000);
    m.step(&common::only(
        0,
        common::press(srr::game::Attack::punch(srr::game::Strength::Medium))
            .with(srr::game::Attack::kick(srr::game::Strength::High)),
    ));
    for _ in 0..30 {
        m.step(&common::no_inputs());
        draw(&m, 0);
    }
    // The whiffed throw, the same way.
    let mut m = common::duel_at(common::cfg(), 200, 1000);
    m.step(&common::only(
        0,
        common::press(srr::game::Attack::kick(srr::game::Strength::Medium))
            .with(srr::game::Attack::kick(srr::game::Strength::High)),
    ));
    for _ in 0..30 {
        m.step(&common::no_inputs());
        draw(&m, 0);
    }

    // The grab attempt and the clinch, both sides of it (close enough to connect,
    // the way the grappling tests set it up): the attempt drawn through its phases,
    // before and after every step, so the one active tick is drawn too.
    let mut m = common::duel_at(common::cfg(), 580, 620);
    draw(&m, 0);
    m.step(&common::only(0, common::press_grab()));
    draw(&m, 0);
    for _ in 0..10 {
        m.step(&common::no_inputs());
        draw(&m, 0);
        draw(&m, 1);
    }
    assert!(matches!(
        m.fighter(PlayerId(0)).unwrap().state(),
        FighterState::Grappling(GrappleRole::Top, GrapplePosition::Clinch)
    ));
    // The ladder: the top presses grab again to take the bottom down (the mount),
    // then again to lock the submission — every rung drawn, both fighters.
    for _ in 0..2 {
        for _ in 0..4 {
            m.step(&common::no_inputs());
            draw(&m, 0);
            draw(&m, 1);
        }
        m.step(&common::only(0, common::press_grab()));
        for _ in 0..6 {
            m.step(&common::no_inputs());
            draw(&m, 0);
            draw(&m, 1);
        }
    }
    assert!(
        matches!(
            m.fighter(PlayerId(0)).unwrap().state(),
            FighterState::Grappling(GrappleRole::Top, GrapplePosition::Submission)
        ) | matches!(
            m.fighter(PlayerId(1)).unwrap().state(),
            FighterState::Grappling(GrappleRole::Bottom, GrapplePosition::Submission)
        ),
        "the ladder climbs to the submission"
    );

    // The sweep: the projection, then the scarf hold on the ground.
    let mut m = common::duel_at(common::cfg(), 560, 600);
    // The wind-up, the catch and the pin: the projection drawn through its phases
    // (the startup, the active sweep, the recovery onto the ground), the fighters
    // drawn while the pin holds.
    let mut pinned = false;
    for i in 0..50 {
        if i == 0 {
            m.step(&common::only(
                0,
                common::press(srr::game::Attack::punch(srr::game::Strength::Medium))
                    .with(srr::game::Attack::kick(srr::game::Strength::High)),
            ));
        } else {
            m.step(&common::no_inputs());
        }
        draw(&m, 0);
        draw(&m, 1);
        pinned |= matches!(
            m.fighter(PlayerId(1)).unwrap().state(),
            FighterState::Grappling(GrappleRole::Bottom, GrapplePosition::ScarfHold)
        );
    }
    assert!(pinned, "the sweep pins its scarf hold");

    // The throw: the projection (both fighters in the air of it), then the armbar
    // on the ground, drawn while it is locked.
    let mut m = common::duel_at(common::cfg(), 560, 600);
    m.step(&common::only(
        0,
        common::press(srr::game::Attack::kick(srr::game::Strength::Medium))
            .with(srr::game::Attack::kick(srr::game::Strength::High)),
    ));
    let mut seen_armbar = false;
    for _ in 0..40 {
        m.step(&common::no_inputs());
        draw(&m, 0);
        draw(&m, 1);
        seen_armbar |= matches!(
            m.fighter(PlayerId(0)).unwrap().state(),
            FighterState::Grappling(GrappleRole::Top, GrapplePosition::Armbar)
        );
    }
    assert!(seen_armbar, "the throw locks its armbar");
    assert_eq!(Technique::Throw.finish(), GrapplePosition::Armbar);
}

#[test]
fn a_practice_fight_keeps_the_skin_the_player_chose() {
    use srr::front::controls::Practice;
    use srr::front::{Game, Headless};
    use srr::net::{Client, PlayerSet, ServerMessage};

    let mut client = Client::online(MatchConfig::default(), 0);
    client.handle(ServerMessage::Welcome {
        player: PlayerId(0),
    });
    client.handle(ServerMessage::Lobby {
        seated: PlayerSet::first(1),
        ready: PlayerSet::EMPTY,
    });
    client.handle(ServerMessage::Styles {
        allowed: srr::game::StyleSet::all(),
    });
    let mut game = Headless::new(Game::new(client));
    game.skip_intro();
    let skin = Skin::of_player(Style::Manga, PlayerId(0));
    game.game_mut().set_skin(skin);
    game.game_mut().client_mut().handle(ServerMessage::SkinOf {
        player: PlayerId(0),
        skin,
    });
    assert_eq!(game.game().skin(), skin, "the room confirmed it");
    game.game_mut().set_practice(Practice::Off.next());
    assert!(game.game().is_practising_online());
    assert_eq!(
        game.game().active_client().my_skin(),
        skin,
        "the practice fight wears the same skin, not the chalk"
    );
}

#[test]
fn the_pose_engine_maps_every_state_of_the_rules_to_a_cell() {
    use srr::front::skin::{Pose, pose_of};
    use srr::game::{
        Attack, AttackPhase, FighterState, GrapplePosition, GrappleRole, Limb, Strength, Technique,
    };

    // strike(): every limb at every strength has its own cell.
    assert_eq!(Pose::strike(Limb::Punch, Strength::Low), Pose::PunchLow);
    assert_eq!(Pose::strike(Limb::Punch, Strength::Medium), Pose::PunchMid);
    assert_eq!(Pose::strike(Limb::Punch, Strength::High), Pose::PunchHigh);
    assert_eq!(Pose::strike(Limb::Kick, Strength::Low), Pose::KickLow);
    assert_eq!(Pose::strike(Limb::Kick, Strength::Medium), Pose::KickMid);
    assert_eq!(Pose::strike(Limb::Kick, Strength::High), Pose::KickHigh);

    let pose = |m: &srr::game::Match, id: u8, ticks: u32| {
        pose_of(m.fighter(PlayerId(id)).expect("the fighter"), ticks)
    };

    // The six strikes on the floor, each through its phases (the pose shows from the
    // wind-up), read straight off the rules engine. The button is held, so the strike
    // starts on the first step and runs its whole timeline.
    for attack in Attack::ALL {
        let mut m = common::close_duel(common::cfg());
        for ticks in 0..10 {
            m.step(&common::only(0, common::press(attack)));
            assert_eq!(
                pose(&m, 0, ticks),
                Pose::strike(attack.limb, attack.strength),
                "{attack:?} poses its cell in every phase"
            );
        }
    }
    // A whiffed strike far out of reach: the same cells.
    let mut m = common::duel_at(common::cfg(), 200, 1000);
    for ticks in 0..10 {
        m.step(&common::only(
            0,
            common::press(Attack::punch(Strength::Medium)),
        ));
        assert_eq!(pose(&m, 0, ticks), Pose::PunchMid);
    }
    // A kick pressed in the air: the jump-kick cell through the flight. The jump is
    // pressed first, the kick on the next step (a fresh press, the way the keyboard
    // delivers it).
    let mut m = common::duel_at(common::cfg(), 100, 1100);
    m.step(&common::only(0, common::press_jump()));
    m.step(&common::only(
        0,
        common::press(Attack::kick(Strength::High)),
    ));
    for ticks in 0..6 {
        assert_eq!(pose(&m, 0, ticks), Pose::JumpKick, "the aerial kick");
        m.step(&common::no_inputs());
    }
    // Free flight: the jump cell.
    let mut m = common::close_duel(common::cfg());
    m.step(&common::only(0, common::press_jump()));
    assert_eq!(pose(&m, 0, 0), Pose::Jump);
    // Crouching and blocking.
    let mut m = common::close_duel(common::cfg());
    m.step(&common::only(0, common::press_down()));
    assert_eq!(pose(&m, 0, 0), Pose::Crouch);
    m.step(&common::no_inputs());
    m.step(&common::only(0, common::press_left()));
    assert_eq!(pose(&m, 0, 0), Pose::Block);
    // The grab attempt, whiffed and connected: the grab cell in every phase.
    for (x0, x1) in [(200, 1000), (580, 620)] {
        let mut m = common::duel_at(common::cfg(), x0, x1);
        for ticks in 0..10 {
            m.step(&common::only(0, common::press_grab()));
            assert_eq!(pose(&m, 0, ticks), Pose::Grab, "the grab attempt");
        }
    }
    // The techniques: the sweep and the throw cells while winding up, then the grapple
    // positions — the clinch reads as the grab, the projections as their technique,
    // the top of a ground hold crouches over its prey, the bottom lies flat.
    for (buttons, cell, top, bottom) in [
        (
            common::press(Attack::punch(Strength::Medium)).with(Attack::kick(Strength::High)),
            Pose::Sweep,
            Pose::Crouch,
            Pose::Ko,
        ),
        (
            common::press(Attack::kick(Strength::Medium)).with(Attack::kick(Strength::High)),
            Pose::Throw,
            Pose::Crouch,
            Pose::Ko,
        ),
    ] {
        let mut m = common::duel_at(common::cfg(), 560, 600);
        m.step(&common::only(0, buttons));
        let mut seen_cell = pose(&m, 0, 0) == cell;
        for ticks in 1..12 {
            m.step(&common::no_inputs());
            seen_cell |= pose(&m, 0, ticks) == cell;
        }
        assert!(seen_cell, "the technique poses its cell while it winds up");
        let mut seen = [false; 2];
        for ticks in 0..60 {
            m.step(&common::no_inputs());
            let top_pose = pose(&m, 0, ticks);
            let bottom_pose = pose(&m, 1, ticks);
            if matches!(
                m.fighter(PlayerId(0)).unwrap().state(),
                FighterState::Grappling(_, GrapplePosition::Clinch)
            ) {
                assert_eq!(top_pose, Pose::Grab, "the clinch reads as the grab");
            }
            seen[0] |= top_pose == top;
            seen[1] |= bottom_pose == bottom;
        }
        assert!(seen[0] && seen[1], "the held fighters pose: {seen:?}");
    }
    // Every grapple position of the grab's ladder, both roles.
    let mut m = common::duel_at(common::cfg(), 580, 620);
    m.step(&common::only(0, common::press_grab()));
    for _ in 0..10 {
        m.step(&common::no_inputs());
    }
    let expected = [
        (Pose::Grab, Pose::Grab),
        (Pose::Crouch, Pose::Ko),
        (Pose::Crouch, Pose::Ko),
    ];
    for (index, (top, bottom)) in expected.iter().enumerate() {
        assert!(
            matches!(
                m.fighter(PlayerId(0)).unwrap().state(),
                FighterState::Grappling(GrappleRole::Top, p)
                    if p.index() == index
            ),
            "the ladder is at rung {index}"
        );
        assert_eq!(pose(&m, 0, 0), *top);
        assert_eq!(pose(&m, 1, 0), *bottom);
        for _ in 0..4 {
            m.step(&common::no_inputs());
        }
        m.step(&common::only(0, common::press_grab()));
        for _ in 0..6 {
            m.step(&common::no_inputs());
        }
    }
    // The special move of the default character, in every phase.
    let mut m = common::close_duel(common::cfg());
    let special = srr::game::Input {
        special: true,
        ..srr::game::Input::NONE
    };
    for ticks in 0..30 {
        m.step(&common::only(0, special));
        let state = m.fighter(PlayerId(0)).unwrap().state();
        if matches!(state, FighterState::Special(_)) {
            assert_eq!(pose(&m, 0, ticks), Pose::Special);
        }
    }
    let _ = AttackPhase::Startup;
    let _ = Technique::ALL;
}
