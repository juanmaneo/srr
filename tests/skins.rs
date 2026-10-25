//! The skins: the styles and variants on the wire, the client's skins, the room broadcasting
//! them, and the figures drawn wearing each style.

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
    assert_eq!(Style::ALL.len(), 3);
    assert_eq!(Style::Stick.label(), "Stick (chalk)");
    assert_eq!(Style::Realistic.label(), "Realistic");
    assert_eq!(Style::Manga.label(), "Manga");
    for style in Style::ALL {
        assert_eq!(Style::from_index(style.index()), Some(style));
        assert_eq!(style.next().previous(), style);
    }
    assert_eq!(Style::from_index(3), None);
    assert!(!Style::Stick.is_skinned());
    assert!(Style::Realistic.is_skinned() && Style::Manga.is_skinned());
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
    assert_eq!(Variant::of(PlayerId(9)), Variant::Mystic);
    assert_eq!(Variant::of(PlayerId(10)), Variant::Brunette);
    // Three women, then seven others.
    assert_eq!(Variant::Brunette.label(), "the brunette");
    assert_eq!(Variant::Redhead.label(), "the redhead");
    assert_eq!(Variant::Ace.label(), "the ace");
}

#[test]
fn a_skin_is_a_style_wearing_a_variant() {
    // The chalk wears none; the skinned styles always do.
    assert_eq!(Skin::default(), Skin::STICK);
    assert_eq!(Skin::STICK.index(), 0);
    assert_eq!(Skin::styled(Style::Stick, Variant::Ace), Skin::STICK);
    for style in Style::ALL {
        for variant in Variant::ALL {
            let skin = Skin::styled(style, variant);
            assert_eq!(Skin::from_index(skin.index()), Some(skin));
        }
    }
    // 0 is the chalk, 1..=10 the realistic variants, 11..=20 the manga ones.
    assert_eq!(
        Skin::from_index(1),
        Some(Skin::styled(Style::Realistic, Variant::Brunette))
    );
    assert_eq!(
        Skin::from_index(11),
        Some(Skin::styled(Style::Manga, Variant::Brunette))
    );
    assert_eq!(
        Skin::from_index(20),
        Some(Skin::styled(Style::Manga, Variant::Mystic))
    );
    assert_eq!(Skin::from_index(21), None);
    // The label names both.
    assert_eq!(Skin::STICK.label(), "Stick (chalk)");
    assert_eq!(
        Skin::styled(Style::Realistic, Variant::Blonde).label(),
        "Realistic (the blonde)"
    );
}

#[test]
fn the_allowed_styles_of_a_server() {
    let all = StyleSet::all();
    assert!(
        all.contains(Style::Stick) && all.contains(Style::Realistic) && all.contains(Style::Manga)
    );
    // The chalk is always allowed, even on a server that allows nothing.
    let none = StyleSet::none();
    assert!(none.contains(Style::Stick));
    assert!(!none.contains(Style::Manga));
    assert_eq!(none.with(Style::Manga).without(Style::Manga), none);
    // The bits go on the wire; the bits that name no style are dropped.
    assert_eq!(StyleSet::from_bits(all.bits()), all);
    assert_eq!(StyleSet::from_bits(0xff).styles().count(), 3);
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
    assert_eq!(skin.encode(), vec![10, 12]);
    assert_eq!(ClientMessage::decode(&skin.encode()), Ok(skin));
    // The server names a player's skin, and the styles it allows.
    let told = ServerMessage::SkinOf {
        player: PlayerId(3),
        skin: manga,
    };
    assert_eq!(told.encode(), vec![11, 3, 12]);
    assert_eq!(ServerMessage::decode(&told.encode()), Ok(told));
    let styles = ServerMessage::Styles {
        allowed: StyleSet::none().with(Style::Realistic),
    };
    assert_eq!(styles.encode(), vec![12, 2]);
    assert_eq!(ServerMessage::decode(&styles.encode()), Ok(styles));
    // A byte that names no skin is refused, not guessed.
    assert_eq!(
        ClientMessage::decode(&[10, 21]),
        Err(DecodeError::UnknownSkin(21))
    );
    assert_eq!(
        ServerMessage::decode(&[11, 0, 21]),
        Err(DecodeError::UnknownSkin(21))
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
    let mine = Skin::styled(Style::Realistic, Variant::Ace);
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
        allowed: StyleSet::none().with(Style::Manga),
    });
    assert_eq!(strict.set_skin(mine), None, "realistic is not allowed");
    assert_eq!(
        strict.set_skin(manga),
        Some(ClientMessage::Skin { skin: manga }.encode())
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
    // The three women and the men differ by their hair.
    let women = [
        Palette::of(Variant::Brunette),
        Palette::of(Variant::Blonde),
        Palette::of(Variant::Redhead),
    ];
    for (a, b) in women.iter().zip(women.iter().skip(1)) {
        assert_ne!(a.hair, b.hair);
    }
}

#[test]
fn a_fighter_wearing_a_skin_is_drawn_in_its_colours() {
    let mut game = Headless::practice();
    game.skip_intro();
    // The chalk is drawn as the plain ink: none of the variant's hair colour on screen.
    let hair = Palette::of(Variant::of(PlayerId(0))).hair;
    let near = |a: u8, c: f32| (f32::from(a) / 255.0 - c).abs() < 0.04;
    let hair_pixels = |shot: &srr::front::render::Pixmap| {
        shot.pixels()
            .iter()
            .filter(|&&[r, g, b]| near(r, hair.r) && near(g, hair.g) && near(b, hair.b))
            .count()
    };
    let before = hair_pixels(&game.screenshot());
    // Dress player 1 in the manga take of their variant: its hair shows at once.
    game.game_mut().client_mut().handle(ServerMessage::SkinOf {
        player: PlayerId(0),
        skin: Skin::of_player(Style::Manga, PlayerId(0)),
    });
    let after = hair_pixels(&game.screenshot());
    assert!(
        after > before + 40,
        "the manga hair shows: {before} pixels before, {after} after"
    );
    // A realistic skin shows its outfit too — compared with the bare chalk, since the
    // two skinned styles share each variant's palette.
    let outfit = Palette::of(Variant::of(PlayerId(0))).outfit;
    let outfit_pixels = |shot: &srr::front::render::Pixmap| {
        shot.pixels()
            .iter()
            .filter(|&&[r, g, b]| near(r, outfit.r) && near(g, outfit.g) && near(b, outfit.b))
            .count()
    };
    let mut fresh = Headless::practice();
    fresh.skip_intro();
    let bare = outfit_pixels(&fresh.screenshot());
    fresh.game_mut().client_mut().handle(ServerMessage::SkinOf {
        player: PlayerId(0),
        skin: Skin::of_player(Style::Realistic, PlayerId(0)),
    });
    let dressed = outfit_pixels(&fresh.screenshot());
    assert!(
        dressed > bare + 40,
        "the realistic outfit shows: {bare} pixels before, {dressed} after"
    );
}

#[test]
fn the_skin_row_of_the_pause_menu_changes_the_fighter_at_once() {
    use srr::front::Screen;
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
        Skin::of_player(Style::Realistic, PlayerId(0))
    );
    let text = game.text_snapshot(200, 60);
    assert!(text.contains("Realistic.(the.brunette)"), "{text}");
    game.tap(&[Key::ArrowRight]);
    assert_eq!(
        game.game().skin(),
        Skin::of_player(Style::Manga, PlayerId(0))
    );
    // And back to the chalk.
    game.tap(&[Key::ArrowRight]);
    assert_eq!(game.game().skin(), Skin::STICK);
}
