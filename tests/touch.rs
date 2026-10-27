//! The touch controls: the pads of every screen, the keys they press, how they are drawn, and
//! a whole game played with fingers only.

use std::collections::HashSet;

use srr::front::hud::CONTROLS_Y;
use srr::front::touch::{
    self, Button, PAD_CENTER, PAD_DEAD_ZONE, PAD_RADIUS, Pad, Press, TouchPad, pad_keys,
};
use srr::front::{Game, Headless, Rgba, Screen, Viewport};
use srr::game::{Character, FighterState, Key, KeyBindings, MatchConfig, PlayerId, Technique};
use srr::net::{Client, PlayerSet, ServerMessage};

fn keys(list: &[Key]) -> HashSet<Key> {
    list.iter().copied().collect()
}

/// The middle of the button reading `label` on `screen`.
fn at(screen: Screen, label: &str) -> (f32, f32) {
    let button = button(screen, label);
    (
        button.x + button.width / 2.0,
        button.y + button.height / 2.0,
    )
}

/// A point on the middle of a character's card on the select screen (Torch is the second
/// card, Brick the third).
fn torch_card() -> (f32, f32) {
    card_middle(1)
}

fn brick_card() -> (f32, f32) {
    card_middle(2)
}

fn card_middle(index: usize) -> (f32, f32) {
    use srr::front::select::{CARD_GAP, CARD_HEIGHT, CARD_TOP, CARD_WIDTH};
    let count = Character::ALL.len() as f32;
    let left = (1280.0 - (count * CARD_WIDTH + (count - 1.0) * CARD_GAP)) / 2.0;
    (
        left + index as f32 * (CARD_WIDTH + CARD_GAP) + CARD_WIDTH / 2.0,
        CARD_TOP + CARD_HEIGHT / 2.0,
    )
}

fn button(screen: Screen, label: &str) -> Button {
    *TouchPad::for_screen(screen)
        .buttons()
        .iter()
        .find(|button| button.label == label)
        .unwrap_or_else(|| panic!("no {label} button on {screen:?}"))
}

/// A point on the pad, pushed by `(dx, dy)` from its centre.
fn pad(dx: f32, dy: f32) -> (f32, f32) {
    (PAD_CENTER.0 + dx, PAD_CENTER.1 + dy)
}

const HELD: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.38,
};
const KNOB_HELD: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.55,
};

// ---------------------------------------------------------------------------------------------
// The pads
// ---------------------------------------------------------------------------------------------

#[test]
fn every_screen_has_its_controls() {
    let fight = TouchPad::for_screen(Screen::Fight);
    assert_eq!(fight.pad(), Some(Pad::Bound));
    let labels: Vec<&str> = fight.buttons().iter().map(|b| b.label).collect();
    assert_eq!(
        labels,
        [
            "WALL", "STREET", "LK", "MK", "HK", "LP", "MP", "HP", "GRAB", "SPECIAL", "BACK",
            "READY", "RIVAL", "VIEW", "PAUSE", "TOGGLE"
        ]
    );
    for screen in [Screen::Select, Screen::Controls] {
        let menu = TouchPad::for_screen(screen);
        assert_eq!(menu.pad(), Some(Pad::Arrows));
        let labels: Vec<&str> = menu.buttons().iter().map(|b| b.label).collect();
        assert_eq!(labels, ["BACK", "OK", "TOGGLE"]);
    }
    let title = TouchPad::for_screen(Screen::Intro);
    assert_eq!(title.pad(), None);
    let labels: Vec<&str> = title.buttons().iter().map(|b| b.label).collect();
    assert_eq!(labels, ["PRACTICE", "START", "TOGGLE"]);
}

#[test]
fn no_two_buttons_overlap_and_all_fit_the_screen() {
    for screen in [
        Screen::Intro,
        Screen::Select,
        Screen::Controls,
        Screen::Fight,
    ] {
        let buttons = TouchPad::for_screen(screen).buttons();
        for (index, a) in buttons.iter().enumerate() {
            assert!(a.x >= 0.0 && a.x + a.width <= 1280.0, "{}", a.label);
            assert!(a.y >= 0.0 && a.y + a.height <= 720.0, "{}", a.label);
            assert!(
                a.width >= 60.0 && a.height >= 36.0,
                "{} is big enough",
                a.label
            );
            for b in &buttons[index + 1..] {
                let apart = a.x + a.width <= b.x
                    || b.x + b.width <= a.x
                    || a.y + a.height <= b.y
                    || b.y + b.height <= a.y;
                assert!(apart, "{} and {} overlap", a.label, b.label);
            }
            let (cx, cy) = (a.x + a.width / 2.0, a.y + a.height / 2.0);
            assert!(
                (cx - PAD_CENTER.0).hypot(cy - PAD_CENTER.1) > PAD_RADIUS,
                "{} is off the pad",
                a.label
            );
        }
    }
}

#[test]
fn the_fight_buttons_press_the_bound_keys() {
    let expectations = [
        ("LP", Key::Numpad1, Key::B),
        ("MP", Key::Numpad5, Key::H),
        ("HP", Key::Numpad6, Key::J),
        ("LK", Key::Numpad4, Key::G),
        ("MK", Key::Numpad8, Key::Y),
        ("HK", Key::Numpad9, Key::U),
        ("GRAB", Key::Numpad0, Key::Space),
        ("SPECIAL", Key::Numpad3, Key::F),
        ("WALL", Key::Numpad7, Key::Q),
        ("STREET", Key::Numpad2, Key::E),
        ("BACK", Key::Escape, Key::Escape),
        ("READY", Key::Enter, Key::Enter),
        ("RIVAL", Key::T, Key::T),
        ("VIEW", Key::Tab, Key::Tab),
        ("PAUSE", Key::P, Key::P),
    ];
    let fight = TouchPad::for_screen(Screen::Fight);
    for (label, standard, wasd) in expectations {
        let point = at(Screen::Fight, label);
        assert_eq!(
            fight.keys(&[point], &KeyBindings::STANDARD),
            keys(&[standard]),
            "{label}"
        );
        assert_eq!(
            fight.keys(&[point], &KeyBindings::WASD),
            keys(&[wasd]),
            "{label}"
        );
    }
    assert_eq!(Press::Action(13).key(&KeyBindings::WASD), Some(Key::F));
    assert_eq!(Press::Key(Key::Tab).key(&KeyBindings::WASD), Some(Key::Tab));
    assert_eq!(Press::Toggle.key(&KeyBindings::WASD), None);
    assert!(Press::Toggle.is_toggle());
    assert!(!Press::Key(Key::Enter).is_toggle());
}

#[test]
fn several_fingers_press_several_keys() {
    let fight = TouchPad::for_screen(Screen::Fight);
    let sweep = [at(Screen::Fight, "MP"), at(Screen::Fight, "HK")];
    let [first, second] = Technique::Sweep.buttons();
    let bindings = KeyBindings::STANDARD;
    assert_eq!(
        fight.keys(&sweep, &bindings),
        keys(&[bindings.key_of(first), bindings.key_of(second)])
    );
    assert!(fight.keys(&[], &bindings).is_empty());
    assert!(
        fight.keys(&[(640.0, 300.0)], &bindings).is_empty(),
        "the arena"
    );
}

#[test]
fn a_button_holds_its_edges_and_nothing_beyond() {
    let lp = button(Screen::Fight, "LP");
    assert!(lp.contains((lp.x, lp.y)));
    assert!(lp.contains((lp.x + lp.width, lp.y + lp.height)));
    assert!(!lp.contains((lp.x - 0.5, lp.y)));
    assert!(!lp.contains((lp.x, lp.y + lp.height + 0.5)));
}

#[test]
fn the_pad_presses_the_directions_it_leans_to() {
    let b = KeyBindings::STANDARD;
    let push = |dx, dy| pad_keys(Pad::Bound, pad(dx, dy), &b);
    assert_eq!(push(80.0, 0.0), [Key::ArrowRight]);
    assert_eq!(push(-80.0, 5.0), [Key::ArrowLeft]);
    assert_eq!(push(0.0, -80.0), [Key::ArrowUp], "jump");
    assert_eq!(push(3.0, 80.0), [Key::ArrowDown], "crouch");
    assert_eq!(
        push(60.0, 60.0),
        [Key::ArrowRight, Key::ArrowDown],
        "turn right"
    );
    assert_eq!(
        push(-60.0, -60.0),
        [Key::ArrowLeft, Key::ArrowUp],
        "jump left"
    );
    assert_eq!(
        push(80.0, 30.0),
        [Key::ArrowRight],
        "mostly sideways: no crouch"
    );
    assert!(push(PAD_DEAD_ZONE - 1.0, 0.0).is_empty(), "the dead zone");
    assert!(push(PAD_RADIUS + 1.0, 0.0).is_empty(), "off the pad");

    let wasd = KeyBindings::WASD;
    assert_eq!(pad_keys(Pad::Bound, pad(0.0, -80.0), &wasd), [Key::W]);
    assert_eq!(
        pad_keys(Pad::Arrows, pad(0.0, -80.0), &wasd),
        [Key::ArrowUp],
        "menus always take the arrows"
    );
    assert_eq!(
        pad_keys(Pad::Arrows, pad(80.0, 0.0), &wasd),
        [Key::ArrowRight]
    );
    assert_eq!(
        pad_keys(Pad::Arrows, pad(-80.0, 0.0), &wasd),
        [Key::ArrowLeft]
    );
    assert_eq!(
        pad_keys(Pad::Arrows, pad(0.0, 80.0), &wasd),
        [Key::ArrowDown]
    );
}

// ---------------------------------------------------------------------------------------------
// The game
// ---------------------------------------------------------------------------------------------

#[test]
fn the_controls_only_show_once_the_screen_is_touched() {
    let mut game = Headless::practice();
    game.skip_intro();
    let keyboard = game.frame();
    assert!(!game.game().touch_mode());
    assert!(!keyboard.text().contains("SPECIAL"));
    assert!(
        keyboard.labels.iter().any(|l| l.y >= CONTROLS_Y),
        "the key reminder"
    );

    game.touch(&[(640.0, 300.0)]);
    assert!(game.game().touch_mode());
    let touch = game.frame();
    let text = touch.text();
    for label in ["LP", "HK", "GRAB", "SPECIAL", "PAUSE", "TOGGLE"] {
        assert!(text.lines().any(|line| line == label), "{label}: {text}");
    }
    assert!(keyboard.text().contains("Punch:"));
    assert!(
        !text.contains("Punch:") && !text.contains("Move:"),
        "the key reminder gives way to the buttons"
    );
    game.idle(5);
    assert!(game.game().touch_mode(), "for good");
    assert_eq!(game.game().touch_pad(), TouchPad::for_screen(Screen::Fight));
}

#[test]
fn the_buttons_under_a_finger_light_up() {
    let mut game = Game::new(Client::practice(MatchConfig::default()));
    let start = at(Screen::Intro, "START");
    game.set_touches(&[start]);
    let lit = game.frame();
    assert_eq!(
        lit.rects.iter().filter(|r| r.color == HELD).count(),
        35,
        "the start button, drawn rounded"
    );

    let mut game = Headless::practice();
    game.skip_intro();
    game.game_mut()
        .set_touches(&[pad(80.0, 0.0), at(Screen::Fight, "SPECIAL")]);
    let frame = game.frame();
    assert_eq!(
        frame.rects.iter().filter(|r| r.color == KNOB_HELD).count(),
        36,
        "the knob follows the thumb"
    );
    assert_eq!(
        frame.rects.iter().filter(|r| r.color == HELD).count(),
        35,
        "the special button, drawn rounded"
    );
    game.game_mut().set_touches(&[]);
    let frame = game.frame();
    assert_eq!(
        frame.rects.iter().filter(|r| r.color == HELD).count(),
        0,
        "nothing held"
    );
    assert_eq!(
        frame.rects.iter().filter(|r| r.color == KNOB_HELD).count(),
        0,
        "the knob rests"
    );
    assert!(
        frame
            .rects
            .iter()
            .any(|r| r.color.r == 1.0 && r.color.g < 0.5),
        "the special button stands out"
    );

    // A thumb resting near the middle: the knob sits on it, still inside the ring.
    let mut game = Headless::practice();
    game.skip_intro();
    game.game_mut().set_touches(&[pad(10.0, 6.0)]);
    let frame = game.frame();
    let knob = frame
        .rects
        .iter()
        .filter(|r| r.color == KNOB_HELD)
        .max_by(|a, b| a.width.partial_cmp(&b.width).unwrap());
    let knob = knob.expect("the knob's widest row under the thumb");
    let (dx, dy) = (
        knob.x + knob.width / 2.0 - PAD_CENTER.0,
        knob.y + knob.height / 2.0 - PAD_CENTER.1,
    );
    assert!(
        (dx - 10.0).abs() < 0.5 && (dy - 6.0).abs() < 2.0,
        "({dx}, {dy})"
    );
    assert!(dx.hypot(dy) <= 86.0 - 34.0, "inside the ring");
}

#[test]
fn a_whole_practice_game_can_be_played_with_fingers() {
    let mut game = Headless::practice();
    while !game.game().intro().can_skip() {
        game.idle(1);
    }
    // START: the select screen; the pad picks Torch; OK: the fight.
    game.tap_at(at(Screen::Intro, "START"));
    assert_eq!(game.game().screen(), Screen::Select);
    // A tap on Torch's card selects it; a tap on OK picks it.
    game.tap_at(torch_card());
    assert_eq!(game.game().select().selected(), Character::Torch);
    game.tap_at(at(Screen::Select, "OK"));
    assert_eq!(game.game().screen(), Screen::Fight);
    assert_eq!(game.game().character(), Character::Torch);

    // After the countdown: walk right with the pad, then throw a fireball.
    game.idle(MatchConfig::default().countdown_ticks);
    let start_x = game.game().shown_match().unwrap().fighters()[0].x();
    game.hold_touch(&[pad(80.0, 0.0)], 10);
    assert!(game.game().shown_match().unwrap().fighters()[0].x() > start_x);
    game.hold_touch(&[at(Screen::Fight, "SPECIAL")], 13);
    assert_eq!(game.game().shown_match().unwrap().projectiles().len(), 1);

    // Two fingers at once: a judo technique.
    game.idle(40);
    game.hold_touch(&[at(Screen::Fight, "MK"), at(Screen::Fight, "HK")], 1);
    assert!(matches!(
        game.game().shown_match().unwrap().fighters()[0].state(),
        FighterState::Technique(Technique::Throw, _)
    ));

    // PAUSE opens the pause menu, BACK resumes; RIVAL calls the computer in.
    game.idle(60);
    game.tap_at(at(Screen::Fight, "PAUSE"));
    assert_eq!(game.game().screen(), Screen::Controls);
    assert!(game.game().is_paused());
    game.tap_at(pad(0.0, -80.0));
    game.tap_at(at(Screen::Controls, "BACK"));
    assert_eq!(game.game().screen(), Screen::Fight);
    game.tap_at(at(Screen::Fight, "RIVAL"));
    assert!(game.text_snapshot(160, 45).contains("CPU"));
}

#[test]
fn practice_starts_from_the_title_button() {
    let mut game = Headless::practice();
    while !game.game().intro().can_skip() {
        game.idle(1);
    }
    game.tap_at(at(Screen::Intro, "PRACTICE"));
    assert_eq!(game.game().screen(), Screen::Select);
    game.tap_at(at(Screen::Select, "BACK"));
    assert_eq!(game.game().screen(), Screen::Intro);
}

#[test]
fn a_touch_on_the_window_lands_on_the_virtual_screen() {
    // A phone in landscape, 2400 x 1080 pixels: black bars on the sides.
    let viewport = Viewport::fit(2400.0, 1080.0);
    let special = at(Screen::Fight, "SPECIAL");
    let (wx, wy) = viewport.to_window(special.0, special.1);
    let back = viewport.to_virtual(wx, wy);
    let fight = TouchPad::for_screen(Screen::Fight);
    assert_eq!(
        fight.keys(&[back], &KeyBindings::STANDARD),
        keys(&[Key::Numpad3])
    );
}

#[test]
fn the_practice_notice_names_the_buttons_and_moves_up() {
    let mut game = Headless::practice();
    game.skip_intro();
    let notice = game.game().practice_notice().unwrap();
    assert!(notice.contains("T: switch to"), "{notice}");
    let keyboard = game.frame();
    let line = keyboard.labels.iter().find(|l| l.text == notice).unwrap();
    assert_eq!(line.y, srr::front::hud::NOTICE_Y);

    game.touch(&[(640.0, 300.0)]);
    let notice = game.game().practice_notice().unwrap();
    assert!(notice.contains("RIVAL: switch to"), "{notice}");
    let frame = game.frame();
    let line = frame.labels.iter().find(|l| l.text == notice).unwrap();
    assert_eq!(line.y, touch::NOTICE_Y, "under the top buttons");
}

#[test]
fn a_skin_picked_while_practising_online_dresses_the_practice_fighter() {
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
    game.tap(&[srr::game::Key::T]);
    assert!(game.game().is_practising_online());
    let skin = srr::game::Skin::of_player(srr::game::Style::Manga, PlayerId(0));
    assert!(
        !game.game_mut().set_skin(skin).is_empty(),
        "tells the server"
    );
    game.game_mut().client_mut().handle(ServerMessage::SkinOf {
        player: PlayerId(0),
        skin,
    });
    assert_eq!(game.game().skin(), skin, "once the server confirms it");
}

#[test]
fn online_back_leaves_the_practice_fight() {
    let mut client = Client::online(MatchConfig::default(), 0);
    client.handle(ServerMessage::Welcome {
        player: PlayerId(0),
    });
    client.handle(ServerMessage::Lobby {
        seated: PlayerSet::first(1),
        ready: PlayerSet::EMPTY,
    });
    let mut game = Headless::new(Game::new(client));
    game.touch(&[(640.0, 300.0)]);
    game.skip_intro();
    assert_eq!(
        game.game().practice_notice().as_deref(),
        Some("RIVAL: practice while waiting")
    );
    game.tap_at(at(Screen::Fight, "RIVAL"));
    assert!(game.game().is_practising_online());
    let notice = game.game().practice_notice().unwrap();
    assert!(notice.ends_with("BACK: back to the room"), "{notice}");
    // Somebody joins: the notice says how to leave.
    game.game_mut().client_mut().handle(ServerMessage::Lobby {
        seated: PlayerSet::first(2),
        ready: PlayerSet::EMPTY,
    });
    let notice = game.game().practice_notice().unwrap();
    assert_eq!(notice, "P2 is here - BACK: leave the practice");
    game.tap_at(at(Screen::Fight, "BACK"));
    assert!(!game.game().is_practising_online());
}

#[test]
fn the_toggle_button_hides_the_controls_and_brings_them_back() {
    let mut game = Headless::practice();
    game.skip_intro();
    game.touch(&[(640.0, 300.0)]);
    assert_eq!(game.game().screen(), Screen::Fight);
    assert!(game.game().touch_controls(), "shown at first");
    assert!(game.frame().text().contains("SPECIAL"));
    assert!(
        !game.frame().text().contains("Punch:"),
        "the buttons replace the key reminder"
    );

    game.tap_at(at(Screen::Fight, "TOGGLE"));
    assert!(!game.game().touch_controls(), "hidden");
    let hidden = game.frame();
    assert!(!hidden.text().contains("SPECIAL"));
    assert!(!hidden.text().contains("LP"), "only the toggle stays");
    assert!(hidden.text().contains("TOGGLE"), "the way back");
    assert!(
        hidden.labels.iter().any(|l| l.y >= CONTROLS_Y),
        "the fight is in the clear"
    );

    game.tap_at(at(Screen::Fight, "TOGGLE"));
    assert!(game.game().touch_controls(), "back");
    assert!(game.frame().text().contains("SPECIAL"));
    assert!(!game.frame().text().contains("Punch:"), "again");

    // While a finger holds the toggle button (hidden or shown), it lights up.
    game.game_mut().set_touches(&[at(Screen::Fight, "TOGGLE")]);
    assert!(
        game.frame().rects.iter().any(|r| r.color == HELD
            && (r.y == 14.0 || r.y > 14.0 && r.y < 14.0 + 36.0 && r.x < 14.0 + 78.0)),
        "the toggle under a finger"
    );
    game.game_mut().toggle_touch_controls();
    game.game_mut().set_touches(&[at(Screen::Fight, "TOGGLE")]);
    assert!(
        game.frame()
            .rects
            .iter()
            .any(|r| r.color == HELD && r.x < 14.0 + 78.0),
        "the lone toggle under a finger"
    );
}

#[test]
fn the_toggle_key_hides_the_controls_like_the_button() {
    let mut game = Headless::practice();
    game.skip_intro();
    game.touch(&[(640.0, 300.0)]);
    assert!(game.game().touch_controls());
    game.tap(&[srr::game::Key::X]);
    assert!(!game.game().touch_controls());
    game.tap(&[srr::game::Key::X]);
    assert!(game.game().touch_controls());
}

#[test]
fn the_toggle_leaves_the_keyboard_player_alone() {
    let mut game = Headless::practice();
    game.skip_intro();
    assert!(!game.game().touch_mode());
    game.tap(&[srr::game::Key::X]);
    assert!(game.game().touch_controls(), "nothing to hide");
}

#[test]
fn a_tap_on_a_card_selects_the_character_a_second_tap_picks_it() {
    let mut game = Headless::practice();
    while !game.game().intro().can_skip() {
        game.idle(1);
    }
    game.touch(&[(640.0, 300.0)]);
    game.tap_at(at(Screen::Intro, "START"));
    assert_eq!(game.game().screen(), Screen::Select);

    // A tap on Brick's card selects it; a tap on the same card again picks it.
    let card = brick_card();
    game.tap_at(card);
    assert_eq!(game.game().select().selected(), Character::Brick);
    assert_eq!(game.game().screen(), Screen::Select, "still choosing");
    game.tap_at(card);
    assert_eq!(game.game().screen(), Screen::Fight);
    assert_eq!(game.game().character(), Character::Brick);
}

#[test]
fn a_tap_next_to_the_cards_changes_nothing() {
    let mut game = Headless::practice();
    while !game.game().intro().can_skip() {
        game.idle(1);
    }
    game.tap(&[srr::game::Key::Enter]);
    assert_eq!(game.game().screen(), Screen::Select);
    game.tap_at((10.0, 10.0));
    assert_eq!(
        game.game().select().selected(),
        Character::Chalk,
        "no card there"
    );
    assert_eq!(game.game().screen(), Screen::Select);
}

#[test]
fn a_window_of_no_size_maps_every_touch_to_the_corner() {
    assert_eq!(Viewport::fit(0.0, 0.0).to_virtual(300.0, 200.0), (0.0, 0.0));
}
