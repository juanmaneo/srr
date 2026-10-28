//! Characters on screen: the select screen, picking and changing a character in the game, the
//! special moves of the stick figures, the fireball in both views and the HUD.

use std::collections::{HashMap, HashSet};

use srr::audio::Mood;
use srr::front::arena::{self, draw_fireball, figure_tag, rules_of};
use srr::front::controls::{Practice, ROWS, Row};
use srr::front::hud::{self, HealthBar};
use srr::front::select::{CARD_TOP, Choice, PREVIEW_TICKS, SelectPage};
use srr::front::stick::{ARMOR, FIRE, Figure, POWER, Point, special_ink};
use srr::front::{Frame, Game, Headless, Rgba, Screen, intensity, tps};
use srr::game::{
    AttackPhase, Character, Facing, FighterState, Input, Key, KeyBindings, Match, MatchConfig,
    PlayerId, Rival, Roster,
};
use srr::net::{Client, PlayerSet, ServerMessage, Status};

fn keys(list: &[Key]) -> HashSet<Key> {
    list.iter().copied().collect()
}

fn special() -> Input {
    Input {
        special: true,
        ..Input::NONE
    }
}

fn rects_of(frame: &Frame, color: Rgba) -> usize {
    frame.rects.iter().filter(|r| r.color == color).count()
}

/// A practice game past the title and the select screen, playing `character`.
fn practising_as(character: Character) -> Headless {
    let mut game = Headless::practice();
    game.skip_intro_as(character);
    game
}

/// A match where player 0 plays `character`, after `ticks` ticks holding its special move.
fn specialising(character: Character, ticks: u32) -> Match {
    let config = MatchConfig {
        countdown_ticks: 0,
        ..MatchConfig::default()
    }
    .with_character(PlayerId(0), character);
    let mut m = Match::with_spawns(Roster::duel(), config, &[300, 1200]).unwrap();
    for _ in 0..ticks {
        m.step(&HashMap::from([(PlayerId(0), special())]));
    }
    m
}

// ---------------------------------------------------------------------------------------------
// The select screen
// ---------------------------------------------------------------------------------------------

#[test]
fn the_select_screen_goes_round_the_characters() {
    let mut page = SelectPage::new(Character::Brick);
    assert_eq!(page.selected(), Character::Brick);
    assert_eq!(page.tick(&[]), Choice::Browsing);
    assert_eq!(
        page.tick(&[Key::A]),
        Choice::Browsing,
        "other keys do nothing"
    );
    assert_eq!(page.selected(), Character::Brick);
    page.tick(&[Key::ArrowLeft]);
    page.tick(&[Key::ArrowLeft]);
    assert_eq!(page.selected(), Character::Chalk);
    page.tick(&[Key::ArrowLeft]);
    assert_eq!(page.selected(), Character::Zip, "wraps around");
    assert_eq!(
        page.tick(&[Key::Enter, Key::Escape]),
        Choice::Picked(Character::Zip),
        "only the first key counts"
    );
}

#[test]
fn the_cards_show_every_character_its_special_move_and_its_stats() {
    let page = SelectPage::new(Character::Torch);
    let mut frame = Frame::new(Rgba::hex(0));
    page.draw(&mut frame);
    let text = frame.text();
    assert!(text.contains("CHOOSE YOUR FIGHTER"), "{text}");
    for character in Character::ALL {
        let data = character.data();
        assert!(text.contains(data.name), "{text}");
        assert!(text.contains(data.special_name), "{text}");
    }
    assert!(
        text.contains(Character::Torch.data().tagline),
        "the chosen one's line"
    );
    assert!(!text.contains(Character::Zip.data().tagline));
    for stat in ["Health", "Speed", "Power", "Reach", "Grab"] {
        assert_eq!(text.matches(stat).count(), 4, "{stat} on every card");
    }
    let borders: Vec<_> = frame
        .rects
        .iter()
        .filter(|r| r.y == CARD_TOP - 3.0)
        .collect();
    assert_eq!(borders.len(), 4, "one card each");
    assert_eq!(
        borders
            .iter()
            .filter(|r| r.color == Rgba::hex(0xffd84a))
            .count(),
        1,
        "the chosen card is framed"
    );
}

#[test]
fn the_previews_alternate_the_guard_and_the_special_move() {
    let mut page = SelectPage::new(Character::Torch);
    assert!(!page.shows_specials());
    let mut guard = Frame::new(Rgba::hex(0));
    page.draw(&mut guard);
    assert_eq!(rects_of(&guard, FIRE), 0, "nobody throws anything yet");
    for _ in 0..PREVIEW_TICKS {
        page.tick(&[]);
    }
    assert!(page.shows_specials());
    let mut specials = Frame::new(Rgba::hex(0));
    page.draw(&mut specials);
    assert!(rects_of(&specials, FIRE) > 0, "Torch's hands and fireball");
    assert!(rects_of(&specials, POWER) > 0, "the other special moves");
    for _ in 0..PREVIEW_TICKS {
        page.tick(&[]);
    }
    assert!(!page.shows_specials());
}

// ---------------------------------------------------------------------------------------------
// Picking a character in the game
// ---------------------------------------------------------------------------------------------

#[test]
fn the_title_leads_to_the_select_screen_then_the_fight() {
    let mut game = Headless::practice();
    while !game.game().intro().can_skip() {
        game.press(&[]);
    }
    game.tap(&[Key::Enter]);
    assert_eq!(game.game().screen(), Screen::Select);
    assert_eq!(game.game().music().mood(), Mood::Intro);
    assert!(game.text_snapshot(160, 45).contains("CHOOSE"));
    game.tap(&[Key::Escape]);
    assert_eq!(
        game.game().screen(),
        Screen::Intro,
        "Escape: back to the title"
    );
    game.tap(&[Key::Escape]);
    assert_eq!(
        game.game().screen(),
        Screen::Select,
        "Escape on the title too"
    );
    game.tap(&[Key::ArrowRight]);
    game.tap(&[Key::Enter]);
    assert_eq!(game.game().screen(), Screen::Fight);
    assert_eq!(game.game().character(), Character::Torch);
    let current = game.game().shown_match().unwrap();
    assert_eq!(current.config().character(PlayerId(0)), Character::Torch);
}

#[test]
fn the_select_screen_opens_on_the_current_character() {
    let mut game = practising_as(Character::Brick);
    assert_eq!(game.game().character(), Character::Brick);
    assert_eq!(
        game.game().shown_match().unwrap().fighters()[0].health(),
        130
    );
    // The pause menu, back to the title, and Enter again: Brick is still chosen.
    game.tap(&[Key::P]);
    for _ in 0..ROWS - 1 {
        game.tap(&[Key::ArrowDown]);
    }
    game.tap(&[Key::Enter]);
    assert_eq!(game.game().screen(), Screen::Fight, "resumed");
    let mut fresh = Game::new(Client::practice(MatchConfig::default()));
    fresh.set_character(Character::Zip);
    let mut fresh = Headless::new(fresh);
    fresh.skip_intro();
    assert_eq!(fresh.game().character(), Character::Zip, "kept");
    assert_eq!(fresh.game().select().selected(), Character::Zip);
}

#[test]
fn the_character_row_of_the_pause_menu_restarts_a_practice_round_with_the_new_one() {
    let mut game = practising_as(Character::Chalk);
    game.hold(&[], 30);
    game.tap(&[Key::P]);
    assert_eq!(game.game().controls().character(), Character::Chalk);
    for _ in 0..4 {
        game.tap(&[Key::ArrowUp]);
    }
    assert_eq!(game.game().controls().selected(), Row::Character);
    game.tap(&[Key::ArrowRight]);
    assert_eq!(game.game().character(), Character::Torch, "at once");
    let session = game.game().active_client().session().unwrap();
    assert_eq!(session.tick(), 0, "a fresh round");
    assert_eq!(
        session.current().config().character(PlayerId(0)),
        Character::Torch
    );
    game.tap(&[Key::Escape]);
    assert_eq!(game.game().screen(), Screen::Fight);
    // After the countdown, the special key throws a fireball.
    game.hold(&[], 120);
    game.hold(&[Key::F], 13);
    assert_eq!(game.game().shown_match().unwrap().projectiles().len(), 1);
}

/// An online game seated as player 0, alone in the room, on the given screen.
fn online_alone() -> Game {
    let mut client = Client::online(MatchConfig::default(), 0);
    client.handle(ServerMessage::Welcome {
        player: PlayerId(0),
    });
    client.handle(ServerMessage::Lobby {
        seated: PlayerSet::first(1),
        ready: PlayerSet::EMPTY,
    });
    Game::new(client)
}

#[test]
fn online_the_pick_is_sent_with_readiness_and_practice_uses_it_too() {
    let mut game = online_alone();
    while !game.intro().can_skip() {
        game.tick(&keys(&[]));
    }
    // T: practising after the select screen.
    game.tick(&keys(&[Key::T]));
    assert_eq!(game.screen(), Screen::Select);
    for _ in 0..2 {
        game.tick(&keys(&[Key::ArrowRight]));
        game.tick(&keys(&[]));
    }
    assert!(
        game.tick(&keys(&[Key::Enter])).is_empty(),
        "nobody to tell yet"
    );
    assert_eq!(game.screen(), Screen::Fight);
    assert_eq!(game.practice(), Practice::WhileWaiting(Rival::Brawler));
    let practice = game.active_client().session().unwrap().current();
    assert_eq!(practice.config().character(PlayerId(0)), Character::Brick);

    // Leave the practice and get ready: the server hears Brick.
    game.tick(&keys(&[]));
    game.tick(&keys(&[Key::Escape]));
    game.tick(&keys(&[]));
    let sent = game.tick(&keys(&[Key::Enter]));
    assert_eq!(sent, vec![vec![7, 2]]);
    game.client_mut().handle(ServerMessage::Lobby {
        seated: PlayerSet::first(2),
        ready: PlayerSet::first(1),
    });
    // Changing one's mind while ready tells the server at once.
    game.tick(&keys(&[]));
    game.tick(&keys(&[Key::P]));
    for _ in 0..4 {
        game.tick(&keys(&[Key::ArrowUp]));
        game.tick(&keys(&[]));
    }
    let sent = game.tick(&keys(&[Key::ArrowRight]));
    assert_eq!(sent, vec![vec![7, 3]], "Zip");
    assert_eq!(game.character(), Character::Zip);
    assert_eq!(game.client().status(), Status::Lobby);
}

#[test]
fn a_practice_fight_started_online_plays_the_picked_character() {
    let mut game = online_alone();
    game.set_character(Character::Torch);
    game.set_practice(Practice::WhileWaiting(Rival::Dummy));
    let current = game.active_client().session().unwrap().current();
    assert_eq!(current.config().character(PlayerId(0)), Character::Torch);
    // And a change while practising reaches the practice fight too.
    assert!(game.set_character(Character::Zip).is_empty());
    let current = game.active_client().session().unwrap().current();
    assert_eq!(current.config().character(PlayerId(0)), Character::Zip);
}

// ---------------------------------------------------------------------------------------------
// The fight screen
// ---------------------------------------------------------------------------------------------

#[test]
fn name_tags_and_bars_name_the_characters() {
    let mut game = practising_as(Character::Brick);
    game.hold(&[], 5);
    let text = game.game().frame().text();
    assert!(text.contains("P1 Brick"), "{text}");
    assert!(text.contains("P1 (you) Brick  130 HP"), "{text}");
    let client = game.game().active_client();
    let current = client.session().unwrap().current();
    assert_eq!(figure_tag(client, current, PlayerId(1)), "P2 Chalk");
    assert_eq!(rules_of(current, &current.fighters()[0]).max_health, 130);
}

#[test]
fn a_tough_character_starts_with_a_full_bar_and_no_tension() {
    let config = MatchConfig {
        countdown_ticks: 0,
        ..MatchConfig::default()
    }
    .with_character(PlayerId(0), Character::Brick);
    let m = Match::new(Roster::duel(), config).unwrap();
    assert_eq!(intensity(&m), 0.0, "130 of 130 is a full bar");
}

#[test]
fn the_fireball_gauge_refills_under_the_bar() {
    let mut client = Client::practice(MatchConfig {
        countdown_ticks: 0,
        ..MatchConfig::default()
    });
    client.set_character(Character::Torch);
    let bars = |client: &Client| -> Vec<HealthBar> {
        client
            .session()
            .unwrap()
            .current()
            .fighters()
            .iter()
            .map(|f| HealthBar::new(f.player(), f.health()))
            .collect()
    };
    let draw = |client: &Client| {
        let mut frame = Frame::new(Rgba::hex(0));
        hud::draw(&mut frame, client, &bars(client), 1, &KeyBindings::STANDARD);
        frame
    };
    assert_eq!(rects_of(&draw(&client), FIRE), 0, "ready: no gauge");
    for _ in 0..13 {
        client.tick(special());
    }
    let frame = draw(&client);
    let gauge: Vec<_> = frame.rects.iter().filter(|r| r.color == FIRE).collect();
    assert_eq!(gauge.len(), 1);
    assert_eq!(gauge[0].height, 3.0);
    let empty = frame
        .rects
        .iter()
        .find(|r| r.color == Rgba::hex(0x5a3a2a))
        .expect("the empty gauge");
    assert!(
        gauge[0].width < empty.width / 2.0,
        "it just started cooling"
    );
}

#[test]
fn the_fireball_flies_in_both_views() {
    let mut game = practising_as(Character::Torch);
    game.hold(&[], 120);
    game.hold(&[Key::F], 13);
    assert_eq!(game.game().shown_match().unwrap().projectiles().len(), 1);
    assert!(
        rects_of(&game.game().frame(), Rgba::hex(0xfff6d8)) > 0,
        "the hot core"
    );

    let client = game.game().active_client();
    let camera = tps::aim(client);
    let mut with_shot = Frame::new(tps::BACKGROUND);
    tps::draw(&mut with_shot, client, &camera, 1);
    let mut without = Frame::new(tps::BACKGROUND);
    tps::draw(
        &mut without,
        &Client::practice(MatchConfig::default()),
        &camera,
        1,
    );
    assert!(with_shot.rects.len() > without.rects.len());
}

#[test]
fn a_fireball_trails_its_flames_behind_it() {
    let center = Point { x: 600.0, y: 300.0 };
    for (facing, behind_left) in [(Facing::Right, true), (Facing::Left, false)] {
        let mut frame = Frame::new(Rgba::hex(0));
        draw_fireball(&mut frame, center, facing, 0);
        let flames: Vec<_> = frame
            .rects
            .iter()
            .filter(|r| r.width >= 18.0 && (3.5..=10.0).contains(&r.height))
            .collect();
        assert_eq!(flames.len(), 3, "{facing:?}");
        for flame in flames {
            if behind_left {
                assert!(flame.x + flame.width < center.x, "{facing:?}");
            } else {
                assert!(flame.x > center.x, "{facing:?}");
            }
        }
    }
    const { assert!(arena::FIREBALL_RADIUS > 0.0) };
}

// ---------------------------------------------------------------------------------------------
// The stick figures' special moves
// ---------------------------------------------------------------------------------------------

#[test]
fn every_special_move_has_its_pose_in_every_phase() {
    for character in Character::ALL {
        let rules = character.rules(&MatchConfig::default());
        let total = rules.special.total();
        let mut seen = Vec::new();
        for ticks in 1..=total {
            let m = specialising(character, ticks);
            let fighter = &m.fighters()[0];
            let FighterState::Special(phase) = fighter.state() else {
                panic!("{character}: not in its special move after {ticks} ticks");
            };
            let figure = Figure::of(fighter, &rules, ticks);
            let [front, back] = special_ink(&rules.special, phase);
            let limbs = if rules.special.low_profile {
                [&figure.legs[0], &figure.legs[1]]
            } else {
                [&figure.arms[0], &figure.arms[1]]
            };
            assert_eq!(limbs[0].ink, front, "{character} {phase:?}");
            assert_eq!(limbs[1].ink, back, "{character} {phase:?}");
            if !seen.contains(&phase) {
                seen.push(phase);
            }
        }
        assert_eq!(seen.len(), 3, "{character}: all three phases");
    }
}

#[test]
fn the_special_inks_follow_the_kind_of_move() {
    let rules = |character: Character| character.rules(&MatchConfig::default()).special;
    let recovery = AttackPhase::Recovery;
    for character in Character::ALL {
        assert_eq!(special_ink(&rules(character), recovery), [None, None]);
    }
    let torch = rules(Character::Torch);
    assert_eq!(
        special_ink(&torch, AttackPhase::Startup),
        [Some(FIRE), Some(FIRE)]
    );
    let brick = rules(Character::Brick);
    assert_eq!(
        special_ink(&brick, AttackPhase::Startup),
        [Some(ARMOR), Some(ARMOR)]
    );
    assert_eq!(
        special_ink(&brick, AttackPhase::Active),
        [Some(POWER), None]
    );
    let chalk = rules(Character::Chalk);
    assert_eq!(special_ink(&chalk, AttackPhase::Startup), [None, None]);
}

#[test]
fn the_slide_lies_low_and_the_rush_leans_forward() {
    let slide = specialising(Character::Zip, 6);
    let rules = Character::Zip.rules(&MatchConfig::default());
    let figure = Figure::of(&slide.fighters()[0], &rules, 0);
    let standing = Figure::of(&specialising(Character::Zip, 0).fighters()[0], &rules, 0);
    assert!(figure.head.y > standing.head.y + 30.0, "the head drops");

    let rush = specialising(Character::Chalk, 6);
    let base = MatchConfig::default();
    let figure = Figure::of(&rush.fighters()[0], &base, 0);
    assert!(
        figure.neck.x > figure.hip.x + 15.0,
        "bent forward, facing right"
    );
}

#[test]
fn a_figure_can_be_shifted_sideways() {
    let m = specialising(Character::Chalk, 0);
    let base = MatchConfig::default();
    let figure = Figure::of(&m.fighters()[0], &base, 0);
    let moved = figure.shifted(50.0);
    assert_eq!(moved.head.x, figure.head.x + 50.0);
    assert_eq!(moved.head.y, figure.head.y);
    assert_eq!(moved.legs[1].end.x, figure.legs[1].end.x + 50.0);
    assert_eq!(moved.arms[0].joint.x, figure.arms[0].joint.x + 50.0);
}

#[test]
fn a_broken_off_fist_floats_where_it_snapped_off() {
    use srr::front::arena::{draw_fist, fist_center};
    use srr::game::AttackData;
    use srr::game::DetachedFist;

    let config = MatchConfig {
        countdown_ticks: 0,
        ..MatchConfig::default()
    };
    let fist = DetachedFist::new(
        300,
        20,
        0,
        5,
        AttackData {
            damage: 6,
            knockback: 0,
            hitstun: 0,
            ..*config.attack(srr::game::Attack::punch(srr::game::Strength::Low))
        },
    );
    let center = fist_center(&fist, &config);
    assert_eq!(
        center.y,
        srr::front::arena::FLOOR_Y
            - 20.0 * arena::scale(&config)
            - srr::front::arena::FIREBALL_HEIGHT
    );
    let before = Frame::new(Rgba::hex(0));
    let mut drawn = Frame::new(Rgba::hex(0));
    draw_fist(&mut drawn, center, 0);
    assert!(drawn.rects.len() > before.rects.len());
    assert_eq!(rects_of(&drawn, Rgba::hex(0xfff6d8)), 4);
}
