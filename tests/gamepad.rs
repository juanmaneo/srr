//! The gamepad controls: the buttons and sticks of a PS4 / PS5 controller (or any standard
//! pad) translated into keys, and a whole practice game played with a pad only.

use std::collections::HashSet;

use srr::front::Screen;
use srr::front::gamepad::{GamepadState, PadButton, STICK_DEAD_ZONE};
use srr::game::{Character, FighterState, Key, KeyBindings, MatchConfig, Technique};

fn keys(list: &[Key]) -> HashSet<Key> {
    list.iter().copied().collect()
}

/// The pad of the whole fight mapping, one button at a time: the button, then the keys it
/// presses with the standard layout, on the fight screen.
fn fight_button(button: PadButton, standard: &[Key]) {
    let pad = GamepadState::new().with_button(button);
    assert_eq!(
        pad.keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(standard),
        "{button:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// The state
// ---------------------------------------------------------------------------------------------

#[test]
fn a_new_pad_presses_nothing() {
    let pad = GamepadState::new();
    assert_eq!(pad.buttons(), Vec::<PadButton>::new());
    assert_eq!(pad.left_stick(), (0.0, 0.0));
    assert!(pad.keys(&KeyBindings::STANDARD, Screen::Fight).is_empty());
    assert!(pad.keys(&KeyBindings::WASD, Screen::Select).is_empty());
}

#[test]
fn the_buttons_can_come_in_one_by_one() {
    let mut pad = GamepadState::new();
    pad.press(PadButton::West);
    pad.press(PadButton::Start);
    pad.set_stick(0.9, -0.9);
    assert_eq!(
        pad.buttons(),
        vec![PadButton::West, PadButton::Start],
        "in `PadButton::ALL` order"
    );
    pad.release();
    assert!(pad.buttons().is_empty());
    assert_eq!(pad.left_stick(), (0.9, -0.9), "but the stick stays");
}

#[test]
fn two_pads_steer_the_same_fighter() {
    let square = GamepadState::new()
        .with_button(PadButton::West)
        .with_stick(0.2, 0.0);
    let stick = GamepadState::new()
        .with_button(PadButton::Start)
        .with_stick(0.9, -0.8);
    let merged = square.merge(stick);
    assert_eq!(merged.buttons(), vec![PadButton::West, PadButton::Start]);
    assert_eq!(merged.left_stick(), (0.9, -0.8));
    // Square (the low punch), Options (the pause), and the stick walking left and crouching.
    assert_eq!(
        merged.keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[Key::Numpad1, Key::P, Key::ArrowRight, Key::ArrowDown])
    );
}

// ---------------------------------------------------------------------------------------------
// The fight mapping
// ---------------------------------------------------------------------------------------------

#[test]
fn the_attacks_sit_on_the_shapes_and_the_shoulders() {
    fight_button(PadButton::West, &[Key::Numpad1]); // Square: low punch
    fight_button(PadButton::North, &[Key::Numpad5]); // Triangle: medium punch
    fight_button(PadButton::RightTrigger2, &[Key::Numpad6]); // R1: high punch
    fight_button(PadButton::South, &[Key::Numpad4, Key::Enter]); // Cross: low kick + ready
    fight_button(PadButton::East, &[Key::Numpad8]); // Circle: medium kick
    fight_button(PadButton::RightTrigger, &[Key::Numpad9]); // R2: high kick
    fight_button(PadButton::LeftTrigger2, &[Key::Numpad0]); // L1: grab
    fight_button(PadButton::LeftTrigger, &[Key::Numpad3]); // L2: special move
    fight_button(PadButton::LeftThumb, &[Key::Numpad7]); // L3: sidestep to the wall
    fight_button(PadButton::RightThumb, &[Key::Numpad2]); // R3: sidestep to the street
}

#[test]
fn the_menu_buttons_of_the_fight_screen() {
    // Cross readies between the rounds (its low kick aside), Share leaves an online practice
    // fight, Options pauses.
    let cross = GamepadState::new().with_button(PadButton::South);
    assert_eq!(
        cross.keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[Key::Numpad4, Key::Enter])
    );
    fight_button(PadButton::Select, &[Key::Escape]);
    fight_button(PadButton::Start, &[Key::P]);
    // The PS button does nothing, anywhere.
    fight_button(PadButton::Mode, &[]);
    let mode = GamepadState::new().with_button(PadButton::Mode);
    assert!(mode.keys(&KeyBindings::STANDARD, Screen::Intro).is_empty());
}

#[test]
fn the_buttons_follow_the_layout_played_with() {
    let pad = GamepadState::new().with_button(PadButton::West); // Square
    assert_eq!(
        pad.keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[Key::Numpad1]),
        "the standard layout"
    );
    assert_eq!(
        pad.keys(&KeyBindings::WASD, Screen::Fight),
        keys(&[Key::B]),
        "the WASD layout"
    );
}

#[test]
fn the_stick_and_the_dpad_move_the_fighter() {
    let stick = |x, y| GamepadState::new().with_stick(x, y);
    assert_eq!(
        stick(0.9, 0.0).keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[Key::ArrowRight])
    );
    assert_eq!(
        stick(-0.9, 0.0).keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[Key::ArrowLeft])
    );
    assert_eq!(
        stick(0.0, 0.9).keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[Key::ArrowUp])
    );
    assert_eq!(
        stick(0.0, -0.9).keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[Key::ArrowDown])
    );
    // A diagonal turns around.
    assert_eq!(
        stick(-0.9, -0.9).keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[Key::ArrowLeft, Key::ArrowDown])
    );
    // A resting thumb presses nothing, even worn sticks.
    assert_eq!(
        stick(STICK_DEAD_ZONE / 2.0, STICK_DEAD_ZONE / 2.0)
            .keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[]),
        "inside the dead zone"
    );
    assert_eq!(
        stick(0.9, 0.0)
            .merge(stick(0.0, -0.9))
            .keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[Key::ArrowRight]),
        "the stick leaning furthest wins"
    );

    let dpad = GamepadState::new().with_buttons(&[PadButton::DPadRight, PadButton::DPadDown]);
    assert_eq!(
        dpad.keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[Key::ArrowRight, Key::ArrowDown])
    );
    // The D-pad works with the layout too.
    assert_eq!(
        dpad.keys(&KeyBindings::WASD, Screen::Fight),
        keys(&[Key::D, Key::S])
    );
}

// ---------------------------------------------------------------------------------------------
// The menus
// ---------------------------------------------------------------------------------------------

#[test]
fn the_menus_read_the_arrow_keys_and_their_own_buttons() {
    let pad = GamepadState::new()
        .with_buttons(&[PadButton::DPadLeft])
        .with_stick(0.0, 0.9);
    // The stick and the D-pad press the arrow keys, whatever the layout.
    assert_eq!(
        pad.keys(&KeyBindings::WASD, Screen::Select),
        keys(&[Key::ArrowLeft, Key::ArrowUp])
    );
    // Cross confirms, Circle goes back.
    let cross = GamepadState::new().with_button(PadButton::South);
    assert_eq!(
        cross.keys(&KeyBindings::STANDARD, Screen::Select),
        keys(&[Key::Enter])
    );
    let circle = GamepadState::new().with_button(PadButton::East);
    assert_eq!(
        circle.keys(&KeyBindings::STANDARD, Screen::Controls),
        keys(&[Key::Escape])
    );
}

#[test]
fn the_title_and_the_pause_menu_have_their_shortcuts() {
    // Triangle opens the controls page from the title, Square goes practising.
    let triangle = GamepadState::new().with_button(PadButton::North);
    assert_eq!(
        triangle.keys(&KeyBindings::STANDARD, Screen::Intro),
        keys(&[Key::C])
    );
    let square = GamepadState::new().with_button(PadButton::West);
    assert_eq!(
        square.keys(&KeyBindings::STANDARD, Screen::Intro),
        keys(&[Key::T])
    );
    // Options resumes the pause menu, like P does; Circle goes back, like Esc.
    let options = GamepadState::new().with_button(PadButton::Start);
    assert_eq!(
        options.keys(&KeyBindings::STANDARD, Screen::Controls),
        keys(&[Key::P]),
        "Options is also the pause key on the pause menu"
    );
}

// ---------------------------------------------------------------------------------------------
// The game
// ---------------------------------------------------------------------------------------------

#[test]
fn a_whole_practice_game_can_be_played_with_a_pad() {
    let mut game = srr::front::Headless::practice();
    while !game.game().intro().can_skip() {
        game.idle(1);
    }
    // Cross: the select screen; the stick picks Torch; Cross: the fight.
    let cross = || GamepadState::new().with_button(PadButton::South);
    game.game_mut().set_gamepad(cross());
    game.idle(1);
    assert_eq!(game.game().screen(), Screen::Select);
    game.game_mut()
        .set_gamepad(GamepadState::new().with_stick(0.9, 0.0));
    game.idle(1);
    assert_eq!(game.game().select().selected(), Character::Torch);
    game.game_mut().set_gamepad(cross());
    game.idle(1);
    assert_eq!(game.game().screen(), Screen::Fight);
    assert_eq!(game.game().character(), Character::Torch);

    // After the countdown: walk right with the stick, then throw a fireball with L2.
    game.game_mut().set_gamepad(GamepadState::new());
    game.idle(MatchConfig::default().countdown_ticks);
    let start_x = game.game().shown_match().unwrap().fighters()[0].x();
    game.game_mut()
        .set_gamepad(GamepadState::new().with_stick(0.9, 0.0));
    game.idle(10);
    assert!(game.game().shown_match().unwrap().fighters()[0].x() > start_x);
    game.game_mut().set_gamepad(GamepadState::new());
    game.idle(1);
    game.game_mut()
        .set_gamepad(GamepadState::new().with_button(PadButton::LeftTrigger));
    game.idle(13);
    assert_eq!(game.game().shown_match().unwrap().projectiles().len(), 1);
    game.game_mut().set_gamepad(GamepadState::new());

    // Circle + R2 together: the judo throw.
    game.idle(40);
    game.game_mut().set_gamepad(
        GamepadState::new()
            .with_button(PadButton::East)
            .with_button(PadButton::RightTrigger),
    );
    game.idle(1);
    assert!(matches!(
        game.game().shown_match().unwrap().fighters()[0].state(),
        FighterState::Technique(Technique::Throw, _)
    ));
    game.game_mut().set_gamepad(GamepadState::new());

    // Options opens the pause menu; the stick tapped up twice reaches the practice row (the
    // page starts at the preset row, and up wraps to Back first), Cross on it switches the
    // rival, Options resumes, and the fighter of the computer is tagged CPU in the HUD.
    game.idle(60);
    game.game_mut()
        .set_gamepad(GamepadState::new().with_button(PadButton::Start));
    game.idle(1);
    assert_eq!(game.game().screen(), Screen::Controls);
    assert!(game.game().is_paused());
    let up = GamepadState::new().with_stick(0.0, 0.9);
    for _ in 0..2 {
        game.game_mut().set_gamepad(up);
        game.idle(1);
        game.game_mut().set_gamepad(GamepadState::new());
        game.idle(1);
    }
    game.game_mut()
        .set_gamepad(GamepadState::new().with_button(PadButton::South));
    game.idle(1);
    game.game_mut().set_gamepad(GamepadState::new());
    game.game_mut()
        .set_gamepad(GamepadState::new().with_button(PadButton::Start));
    game.idle(1);
    game.game_mut().set_gamepad(GamepadState::new());
    assert!(game.text_snapshot(160, 45).contains("CPU"));
}

#[test]
fn the_pad_leaves_an_online_practice_fight_with_share() {
    // Only the state machine: a local practice fight cannot be left with Escape (it says so
    // in the notice), so the button mapping is what is checked here, on the fight screen.
    let share = GamepadState::new().with_button(PadButton::Select);
    assert_eq!(
        share.keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[Key::Escape])
    );
}

#[test]
fn the_keyboard_still_works_with_a_pad_connected() {
    let mut game = srr::front::Headless::practice();
    while !game.game().intro().can_skip() {
        game.idle(1);
    }
    // A pad resting on the table (a worn stick leaning a little) changes nothing.
    game.game_mut()
        .set_gamepad(GamepadState::new().with_stick(0.1, -0.1));
    game.tap(&[Key::Enter]);
    assert_eq!(game.game().screen(), Screen::Select);
    game.game_mut().set_gamepad(GamepadState::new());
    game.tap(&[Key::Enter]);
    assert_eq!(game.game().screen(), Screen::Fight);
}

#[test]
fn the_pad_state_lasts_until_the_next_one_is_reported() {
    let mut game = srr::front::Headless::practice();
    game.skip_intro();
    game.idle(MatchConfig::default().countdown_ticks);
    // The stick pushed right walks the fighter for as long as the engine keeps reporting
    // it: a pad is not a keyboard, its state is read every tick, not its events.
    game.game_mut()
        .set_gamepad(GamepadState::new().with_stick(0.9, 0.0));
    game.idle(1);
    let start_x = game.game().shown_match().unwrap().fighters()[0].x();
    game.idle(10);
    assert!(
        game.game().shown_match().unwrap().fighters()[0].x() > start_x,
        "the same state keeps walking"
    );
    game.game_mut().set_gamepad(GamepadState::new());
    let stopped = game.game().shown_match().unwrap().fighters()[0].x();
    game.idle(10);
    assert_eq!(
        game.game().shown_match().unwrap().fighters()[0].x(),
        stopped,
        "a pad let go presses nothing"
    );
}

// ---------------------------------------------------------------------------------------------
// The module doc
// ---------------------------------------------------------------------------------------------

#[test]
fn the_module_doc_examples_hold() {
    // The doc examples of `gamepad.rs` run as doctests; this test only pins the fight
    // mapping the module documentation promises.
    let pad = GamepadState::new()
        .with_buttons(&[PadButton::West])
        .with_stick(0.9, 0.0);
    assert_eq!(
        pad.keys(&KeyBindings::STANDARD, Screen::Fight),
        keys(&[Key::Numpad1, Key::ArrowRight])
    );
}
