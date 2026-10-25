//! The controls page (`srr::front::controls`): the keyboard presets, the remapping and the
//! practice choice.
//!
//! Rows, top to bottom: the preset chooser, one row per action (in `KeyBindings::ACTIONS`
//! order), the practice row, then "back". The page reads the keys freshly pressed this tick,
//! in `Key` order, and only the first one counts:
//! * `Up` / `Down` move the selection and wrap around;
//! * on the preset row, `Right` or `Enter` pick the next preset, `Left` the previous one (both
//!   wrap); a customised layout goes back to the first preset;
//! * on an action row, `Enter` waits for a key: a bindable key is bound (swapping with the
//!   action that had it), `Esc` cancels, Enter is ignored and it keeps waiting;
//! * on the practice row, `Right` or `Enter` go to the next practice choice, `Left` to the
//!   previous one (see `Practice`);
//! * `Esc`, or `Enter` on the last row, leaves the page; opened as the pause menu of a fight
//!   (`ControlsPage::paused`), `P` leaves it too.

use srr::front::controls::{
    COLUMN_X, ControlsPage, FIRST_ROW_Y, PAUSE_KEY, Practice, ROW_HEIGHT, ROWS, Row,
    controls_lines, preset_name, technique_keys,
};
use srr::front::{Align, Frame, Rgba, SCREEN_HEIGHT};
use srr::game::{Character, Input, Key, KeyBindings, Rival, Technique};

const WAITING: Rgba = Rgba::hex(0xffe14a);
const HIGHLIGHT: Rgba = Rgba::hex(0x3a1430);

fn draw(page: &ControlsPage, bindings: &KeyBindings) -> Frame {
    let mut frame = Frame::new(Rgba::hex(0));
    page.draw(&mut frame, bindings);
    frame
}

/// Presses `key` once; returns whether the page was left.
fn press(page: &mut ControlsPage, bindings: &mut KeyBindings, key: Key) -> bool {
    page.tick(&[key], bindings)
}

/// A page with `row` selected.
fn page_on(row: usize) -> ControlsPage {
    let mut page = ControlsPage::new();
    let mut bindings = KeyBindings::STANDARD;
    for _ in 0..row {
        press(&mut page, &mut bindings, Key::ArrowDown);
    }
    page
}

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

#[test]
fn the_rows_are_the_preset_the_actions_the_character_the_practice_and_back() {
    assert_eq!(ROWS, Input::BITS as usize + 5);
    assert_eq!(Row::at(0), Row::Preset);
    for action in 0..KeyBindings::ACTIONS.len() {
        assert_eq!(Row::at(action + 1), Row::Action(action));
    }
    assert_eq!(Row::at(ROWS - 4), Row::Character);
    assert_eq!(Row::at(ROWS - 3), Row::Skin);
    assert_eq!(Row::at(ROWS - 2), Row::Practice);
    assert_eq!(Row::at(ROWS - 1), Row::Back);
}

#[test]
fn presets_are_named_and_anything_else_is_custom() {
    for (name, preset) in KeyBindings::PRESETS {
        assert_eq!(preset_name(&preset), name);
    }
    let mut mine = KeyBindings::STANDARD;
    mine.bind(0, Key::Z);
    assert_eq!(preset_name(&mine), "Custom");
}

#[test]
fn techniques_name_the_two_keys_of_the_layout() {
    let cases = [
        (Technique::Sweep, KeyBindings::STANDARD, "5+9"),
        (Technique::Throw, KeyBindings::STANDARD, "8+9"),
        (Technique::Sweep, KeyBindings::WASD, "H+U"),
        (Technique::Throw, KeyBindings::WASD, "Y+U"),
    ];
    for (technique, bindings, expected) in cases {
        assert_eq!(technique_keys(technique, &bindings), expected);
    }
}

#[test]
fn the_controls_lines_follow_the_layout() {
    assert_eq!(
        controls_lines(&KeyBindings::STANDARD),
        [
            "Move: Left / Right   Sidestep: 7 / 2   Jump: Up   Crouch: Down   \
             Turn: Down+Left / Down+Right   Block: step back",
            "Punch: 1 5 6   Kick: 4 8 9   Grab: 0   Judo: 5+9, 8+9   Special: 3   Enter: ready   \
             P: pause   Tab: view",
        ]
    );
    assert_eq!(
        controls_lines(&KeyBindings::WASD),
        [
            "Move: A / D   Sidestep: Q / E   Jump: W   Crouch: S   Turn: S+A / S+D   \
             Block: step back",
            "Punch: B H J   Kick: G Y U   Grab: Space   Judo: H+U, Y+U   Special: F   \
             Enter: ready   P: pause   Tab: view",
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------------------------

#[test]
fn a_new_page_selects_the_preset_and_waits_for_nothing() {
    let page = ControlsPage::new();
    assert_eq!(page, ControlsPage::default());
    assert_eq!(page.selected(), Row::Preset);
    assert!(!page.is_waiting());
    assert!(!page.is_paused());
}

#[test]
fn the_pause_menu_is_the_page_opened_from_the_fight() {
    assert_eq!(PAUSE_KEY, Key::P);
    assert!(!PAUSE_KEY.is_bindable());
    let page = ControlsPage::paused();
    assert!(page.is_paused());
    assert_eq!(page.selected(), Row::Preset);
    assert!(!page.is_waiting());
    assert_ne!(page, ControlsPage::new());
}

#[test]
fn the_pause_key_leaves_the_pause_menu_only() {
    let mut bindings = KeyBindings::STANDARD;
    let mut page = ControlsPage::new();
    assert!(
        !press(&mut page, &mut bindings, PAUSE_KEY),
        "from the title: ignored"
    );
    assert_eq!(page, ControlsPage::new());

    let mut paused = ControlsPage::paused();
    press(&mut paused, &mut bindings, Key::ArrowDown);
    assert!(
        press(&mut paused, &mut bindings, PAUSE_KEY),
        "from the fight: resumes"
    );
    assert!(paused.is_paused(), "the page itself does not change");
    assert_eq!(bindings, KeyBindings::STANDARD);

    let mut paused = ControlsPage::paused();
    press(&mut paused, &mut bindings, Key::ArrowDown);
    press(&mut paused, &mut bindings, Key::Enter);
    assert!(paused.is_waiting());
    assert!(
        !press(&mut paused, &mut bindings, PAUSE_KEY),
        "while waiting for a key, P is just not bindable"
    );
    assert!(paused.is_waiting());
    assert_eq!(bindings, KeyBindings::STANDARD);
}

#[test]
fn the_pause_menu_remaps_and_leaves_like_the_controls_page() {
    let mut bindings = KeyBindings::STANDARD;
    let mut page = ControlsPage::paused();
    press(&mut page, &mut bindings, Key::ArrowRight);
    assert_eq!(bindings, KeyBindings::WASD);
    press(&mut page, &mut bindings, Key::ArrowDown);
    press(&mut page, &mut bindings, Key::Enter);
    press(&mut page, &mut bindings, Key::Z);
    assert_eq!(bindings.left, Key::Z);
    assert!(press(&mut page, &mut bindings, Key::Escape));
    let mut page = ControlsPage::paused();
    press(&mut page, &mut bindings, Key::ArrowUp);
    assert_eq!(page.selected(), Row::Back);
    assert!(press(&mut page, &mut bindings, Key::Enter));
}

#[test]
fn no_fresh_key_changes_nothing() {
    let mut page = page_on(3);
    let mut bindings = KeyBindings::WASD;
    assert!(!page.tick(&[], &mut bindings));
    assert_eq!(page, page_on(3));
    assert_eq!(bindings, KeyBindings::WASD);
}

#[test]
fn up_and_down_move_the_selection_and_wrap() {
    let mut page = ControlsPage::new();
    let mut bindings = KeyBindings::STANDARD;
    assert!(!press(&mut page, &mut bindings, Key::ArrowUp));
    assert_eq!(page.selected(), Row::Back);
    press(&mut page, &mut bindings, Key::ArrowUp);
    assert_eq!(page.selected(), Row::Practice);
    press(&mut page, &mut bindings, Key::ArrowUp);
    assert_eq!(page.selected(), Row::Skin);
    press(&mut page, &mut bindings, Key::ArrowUp);
    assert_eq!(page.selected(), Row::Character);
    press(&mut page, &mut bindings, Key::ArrowUp);
    assert_eq!(page.selected(), Row::Action(Input::BITS as usize - 1));
    for _ in 0..5 {
        press(&mut page, &mut bindings, Key::ArrowDown);
    }
    assert_eq!(page.selected(), Row::Preset);
    press(&mut page, &mut bindings, Key::ArrowDown);
    assert_eq!(page.selected(), Row::Action(0));
    assert_eq!(bindings, KeyBindings::STANDARD, "moving changes no key");
}

#[test]
fn only_the_first_fresh_key_counts() {
    let mut page = ControlsPage::new();
    let mut bindings = KeyBindings::STANDARD;
    // Key order: the down arrow comes before Escape.
    assert!(!page.tick(&[Key::ArrowDown, Key::Escape], &mut bindings));
    assert_eq!(page.selected(), Row::Action(0));
}

#[test]
fn the_preset_row_cycles_through_the_presets() {
    let mut page = ControlsPage::new();
    let mut bindings = KeyBindings::WASD;
    press(&mut page, &mut bindings, Key::ArrowRight);
    assert_eq!(bindings, KeyBindings::STANDARD);
    press(&mut page, &mut bindings, Key::ArrowRight);
    assert_eq!(bindings, KeyBindings::WASD, "wraps");
    press(&mut page, &mut bindings, Key::Enter);
    assert_eq!(bindings, KeyBindings::STANDARD, "Enter is next too");
    press(&mut page, &mut bindings, Key::ArrowLeft);
    assert_eq!(bindings, KeyBindings::WASD);
    press(&mut page, &mut bindings, Key::ArrowLeft);
    assert_eq!(bindings, KeyBindings::STANDARD, "wraps backwards");
    assert!(!page.is_waiting());
}

#[test]
fn a_custom_layout_goes_back_to_the_first_preset() {
    for key in [Key::ArrowRight, Key::ArrowLeft, Key::Enter] {
        let mut page = ControlsPage::new();
        let mut bindings = KeyBindings::STANDARD;
        bindings.bind(2, Key::Z);
        press(&mut page, &mut bindings, key);
        assert_eq!(bindings, KeyBindings::WASD, "{key:?}");
    }
}

#[test]
fn left_and_right_do_nothing_on_the_other_rows() {
    for row in [1, ROWS - 1] {
        let mut page = page_on(row);
        let mut bindings = KeyBindings::STANDARD;
        for key in [Key::ArrowLeft, Key::ArrowRight, Key::A, Key::Space] {
            assert!(!press(&mut page, &mut bindings, key));
        }
        assert_eq!(bindings, KeyBindings::STANDARD);
        assert_eq!(page, page_on(row));
    }
}

#[test]
fn the_practice_row_goes_round_the_choices() {
    let mut bindings = KeyBindings::STANDARD;
    let mut page = page_on(ROWS - 2).with_practice(Practice::Alone(Rival::Dummy));
    assert_eq!(page.selected(), Row::Practice);
    assert_eq!(page.practice(), Practice::Alone(Rival::Dummy));
    assert!(!press(&mut page, &mut bindings, Key::ArrowRight));
    assert_eq!(page.practice(), Practice::Alone(Rival::Brawler));
    press(&mut page, &mut bindings, Key::Enter);
    assert_eq!(
        page.practice(),
        Practice::Alone(Rival::Patient),
        "Enter is next too"
    );
    assert!(!page.is_waiting());
    press(&mut page, &mut bindings, Key::ArrowLeft);
    assert_eq!(page.practice(), Practice::Alone(Rival::Brawler));
    assert_eq!(bindings, KeyBindings::STANDARD, "no key changes");

    let mut online = page_on(ROWS - 2).with_practice(Practice::Off);
    for wanted in [
        Practice::WhileWaiting(Rival::Brawler),
        Practice::WhileWaiting(Rival::Patient),
        Practice::WhileWaiting(Rival::Aggressive),
        Practice::WhileWaiting(Rival::Dummy),
        Practice::Off,
    ] {
        press(&mut online, &mut bindings, Key::ArrowRight);
        assert_eq!(online.practice(), wanted);
    }
    press(&mut online, &mut bindings, Key::ArrowLeft);
    assert_eq!(online.practice(), Practice::WhileWaiting(Rival::Dummy));

    let mut playing = page_on(ROWS - 2);
    assert_eq!(playing.practice(), Practice::Unavailable, "the default");
    for key in [Key::ArrowLeft, Key::ArrowRight, Key::Enter] {
        press(&mut playing, &mut bindings, key);
        assert_eq!(playing.practice(), Practice::Unavailable);
    }
}

#[test]
fn the_practice_choice_knows_its_rival() {
    assert_eq!(Practice::Alone(Rival::Dummy).rival(), Some(Rival::Dummy));
    assert_eq!(
        Practice::WhileWaiting(Rival::Brawler).rival(),
        Some(Rival::Brawler)
    );
    assert_eq!(Practice::Off.rival(), None);
    assert_eq!(Practice::Unavailable.rival(), None);
    assert_eq!(Practice::Unavailable.previous(), Practice::Unavailable);
    assert_eq!(
        Practice::Alone(Rival::Dummy).previous(),
        Practice::Alone(Rival::Aggressive)
    );
    assert_eq!(
        Practice::WhileWaiting(Rival::Dummy).previous(),
        Practice::WhileWaiting(Rival::Aggressive)
    );
    assert_eq!(
        Practice::WhileWaiting(Rival::Brawler).previous(),
        Practice::Off,
        "before the first rival: off"
    );
    for practice in [
        Practice::Off,
        Practice::WhileWaiting(Rival::Dummy),
        Practice::Alone(Rival::Brawler),
    ] {
        assert_eq!(practice.next().previous(), practice);
        assert_eq!(practice.previous().next(), practice);
    }
    assert_eq!(ControlsPage::paused().practice(), Practice::Unavailable);
}

#[test]
fn the_practice_row_reads_the_choice() {
    let text_of = |practice: Practice| {
        draw(
            &ControlsPage::new().with_practice(practice),
            &KeyBindings::STANDARD,
        )
        .text()
    };
    let plain = text_of(Practice::Unavailable);
    assert!(plain.contains("Practice"), "{plain}");
    assert!(plain.contains("- (not during a match)"), "{plain}");
    assert!(text_of(Practice::Off).contains("< off >"));
    assert!(
        text_of(Practice::WhileWaiting(Rival::Brawler))
            .contains("< the computer - the room stays open >")
    );
    assert!(text_of(Practice::Alone(Rival::Dummy)).contains("< the training dummy >"));
    assert!(
        plain.contains("Left / Right: preset, character, practice"),
        "{plain}"
    );
}

#[test]
fn the_character_row_goes_round_the_characters() {
    let mut bindings = KeyBindings::STANDARD;
    let mut page = page_on(ROWS - 4);
    assert_eq!(page.selected(), Row::Character);
    assert_eq!(page.character(), Character::Chalk);
    press(&mut page, &mut bindings, Key::ArrowRight);
    assert_eq!(page.character(), Character::Torch);
    press(&mut page, &mut bindings, Key::Enter);
    assert_eq!(page.character(), Character::Brick);
    press(&mut page, &mut bindings, Key::ArrowLeft);
    press(&mut page, &mut bindings, Key::ArrowLeft);
    press(&mut page, &mut bindings, Key::ArrowLeft);
    assert_eq!(page.character(), Character::Zip, "wraps around");
    assert_eq!(bindings, KeyBindings::STANDARD);
    assert!(!page.is_waiting(), "no key to bind");

    let text = draw(&page, &bindings).text();
    assert!(text.contains("Character"), "{text}");
    assert!(text.contains("< Zip - Slide Tackle >"), "{text}");
    let torch = ControlsPage::new().with_character(Character::Torch);
    assert!(
        draw(&torch, &bindings)
            .text()
            .contains("< Torch - Fireball >")
    );
}

#[test]
fn escape_or_enter_on_back_leaves_the_page() {
    let mut bindings = KeyBindings::STANDARD;
    for row in [0, 4, ROWS - 1] {
        let mut page = page_on(row);
        assert!(press(&mut page, &mut bindings, Key::Escape));
    }
    let mut page = page_on(ROWS - 1);
    assert_eq!(page.selected(), Row::Back);
    assert!(press(&mut page, &mut bindings, Key::Enter));
    assert_eq!(bindings, KeyBindings::STANDARD);
}

// ---------------------------------------------------------------------------------------------
// Remapping
// ---------------------------------------------------------------------------------------------

#[test]
fn enter_on_an_action_waits_for_the_next_key_and_binds_it() {
    let mut page = page_on(4); // the low punch
    let mut bindings = KeyBindings::STANDARD;
    assert!(!press(&mut page, &mut bindings, Key::Enter));
    assert!(page.is_waiting());
    assert_eq!(bindings, KeyBindings::STANDARD);
    assert!(!press(&mut page, &mut bindings, Key::L));
    assert!(!page.is_waiting());
    assert_eq!(bindings.punch_low, Key::L);
    assert_eq!(preset_name(&bindings), "Custom");
    assert_eq!(page.selected(), Row::Action(3));
}

#[test]
fn waiting_ignores_the_menu_keys_but_escape_cancels() {
    let mut page = page_on(1);
    let mut bindings = KeyBindings::STANDARD;
    press(&mut page, &mut bindings, Key::Enter);
    assert!(!press(&mut page, &mut bindings, Key::Enter));
    assert!(page.is_waiting(), "Enter cannot be bound");
    assert!(
        !press(&mut page, &mut bindings, Key::Escape),
        "Escape cancels, it does not leave"
    );
    assert!(!page.is_waiting());
    assert_eq!(bindings, KeyBindings::STANDARD);
}

#[test]
fn waiting_treats_the_arrows_as_keys_to_bind() {
    // The jump row: binding the down arrow swaps it with "crouch".
    let mut page = page_on(3);
    let mut bindings = KeyBindings::STANDARD;
    assert_eq!(page.selected(), Row::Action(2));
    press(&mut page, &mut bindings, Key::Enter);
    press(&mut page, &mut bindings, Key::ArrowDown);
    assert_eq!(
        (bindings.jump, bindings.down),
        (Key::ArrowDown, Key::ArrowUp)
    );
    assert_eq!(
        page.selected(),
        Row::Action(2),
        "the arrow was bound, not followed"
    );
}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

fn row_y(index: usize) -> f32 {
    FIRST_ROW_Y + index as f32 * ROW_HEIGHT
}

#[test]
fn every_row_and_the_judo_line_fit_the_screen() {
    for page in [ControlsPage::new(), ControlsPage::paused()] {
        let frame = draw(&page, &KeyBindings::STANDARD);
        let judo = frame
            .labels
            .iter()
            .find(|l| l.text.starts_with("Judo:"))
            .expect("the judo line");
        assert!(judo.y > row_y(ROWS - 1), "below the last row");
        for label in &frame.labels {
            assert!(
                label.y + label.size <= SCREEN_HEIGHT,
                "{} runs off the screen",
                label.text
            );
        }
    }
}

#[test]
fn the_page_lists_every_action_with_its_key() {
    let frame = draw(&ControlsPage::new(), &KeyBindings::WASD);
    let text = frame.text();
    assert!(text.contains("CONTROLS"));
    assert!(text.contains("Up / Down: choose"));
    assert!(text.contains("< WASD (letters + space) >"));
    assert!(text.contains("> Preset"), "the selected row is marked");
    assert!(text.contains("Back to the title"));
    assert!(text.contains("Judo: H+U juji sweep, Y+U ippon-seoi-nage"));
    let keys = KeyBindings::WASD.keys();
    for (action, name) in KeyBindings::ACTIONS.into_iter().enumerate() {
        let y = row_y(action + 1);
        let label = frame
            .labels
            .iter()
            .find(|l| l.text == name && l.y == y)
            .unwrap_or_else(|| panic!("{name}"));
        assert_eq!((label.x, label.align), (COLUMN_X - 30.0, Align::Right));
        let key = frame
            .labels
            .iter()
            .find(|l| l.y == y && l.align == Align::Left)
            .unwrap();
        assert_eq!(key.text, keys[action].label());
        assert_eq!(key.x, COLUMN_X + 30.0);
    }
    assert!(frame.labels.iter().all(|l| l.text.is_ascii()));
}

#[test]
fn the_pause_menu_says_so() {
    let frame = draw(&ControlsPage::paused(), &KeyBindings::STANDARD);
    let text = frame.text();
    assert!(text.contains("PAUSED"), "{text}");
    assert!(!text.contains("CONTROLS"), "{text}");
    assert!(text.contains("Esc / P: resume"), "{text}");
    assert!(text.contains("Resume the fight"), "{text}");
    assert!(!text.contains("Back to the title"), "{text}");
    let title = frame.labels.iter().find(|l| l.text == "PAUSED").unwrap();
    let plain = draw(&ControlsPage::new(), &KeyBindings::STANDARD);
    let controls = plain.labels.iter().find(|l| l.text == "CONTROLS").unwrap();
    assert_eq!(
        (title.y, title.size, title.color),
        (controls.y, controls.size, controls.color)
    );
    assert_eq!(frame.labels.len(), plain.labels.len(), "the same rows");
    assert!(frame.labels.iter().all(|l| l.text.is_ascii()));
}

#[test]
fn the_selected_row_is_highlighted() {
    for row in [0, 5, ROWS - 1] {
        let frame = draw(&page_on(row), &KeyBindings::STANDARD);
        let highlights: Vec<_> = frame
            .rects
            .iter()
            .filter(|r| r.color == HIGHLIGHT)
            .collect();
        assert_eq!(highlights.len(), 1);
        let rect = highlights[0];
        assert!(rect.y <= row_y(row) && row_y(row) < rect.y + rect.height);
        let marked: Vec<_> = frame
            .labels
            .iter()
            .filter(|l| l.text.starts_with("> "))
            .collect();
        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].y, row_y(row));
    }
}

#[test]
fn a_waiting_row_asks_for_a_key() {
    let mut page = page_on(10); // the grab
    let mut bindings = KeyBindings::STANDARD;
    press(&mut page, &mut bindings, Key::Enter);
    let frame = draw(&page, &KeyBindings::STANDARD);
    let prompt = frame
        .labels
        .iter()
        .find(|l| l.text == "press a key...  (Esc: cancel)")
        .unwrap();
    assert_eq!((prompt.y, prompt.color), (row_y(10), WAITING));
    assert!(
        !frame
            .labels
            .iter()
            .any(|l| l.y == row_y(10) && l.text == "0"),
        "the old key is replaced"
    );
    assert_eq!(
        frame.labels.iter().filter(|l| l.color == WAITING).count(),
        1
    );
}
