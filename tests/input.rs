//! Player input, the six attacks and the keyboard layout.
//!
//! `Input` is a *level* (what is held right now), not an edge: holding a button keeps
//! triggering it. Every player plays on their own machine with their own layout. The default
//! (`KeyBindings::STANDARD`): the arrows move, jump and crouch (down), keypad 1/5/6 punch
//! (low/medium/high), keypad 4/8/9 kick (low/medium/high) and keypad 0 grabs.
//! `KeyBindings::WASD` is the same on the letters: W A S D, B H J punch, G Y U kick, Space
//! grabs. `KeyBindings::bind` remaps one action, swapping keys so no key does two things.
//! Grappling reads fresh presses (rising edges) through `Input::pressed_since`.

use std::collections::HashSet;

use srr::game::{Attack, Input, Key, KeyBindings, Limb, Strength};

fn pressed(keys: &[Key]) -> HashSet<Key> {
    keys.iter().copied().collect()
}

// ---------------------------------------------------------------------------------------------
// Attacks
// ---------------------------------------------------------------------------------------------

#[test]
fn strengths_are_ordered_from_low_to_high_and_indexed_accordingly() {
    assert_eq!(
        Strength::ALL,
        [Strength::Low, Strength::Medium, Strength::High]
    );
    assert!(Strength::Low < Strength::Medium && Strength::Medium < Strength::High);
    for (index, strength) in Strength::ALL.into_iter().enumerate() {
        assert_eq!(strength.index(), index);
    }
}

#[test]
fn there_are_six_distinct_attacks_punches_first() {
    let distinct: HashSet<Attack> = Attack::ALL.into_iter().collect();
    assert_eq!(distinct.len(), 6);
    assert_eq!(
        Attack::ALL,
        [
            Attack::punch(Strength::Low),
            Attack::punch(Strength::Medium),
            Attack::punch(Strength::High),
            Attack::kick(Strength::Low),
            Attack::kick(Strength::Medium),
            Attack::kick(Strength::High),
        ]
    );
}

#[test]
fn constructors_set_the_limb_and_the_strength() {
    let punch = Attack::punch(Strength::Medium);
    assert_eq!(
        (punch.limb, punch.strength),
        (Limb::Punch, Strength::Medium)
    );
    let kick = Attack::kick(Strength::Low);
    assert_eq!((kick.limb, kick.strength), (Limb::Kick, Strength::Low));
}

#[test]
fn attacks_display_as_strength_then_limb() {
    let names: Vec<String> = Attack::ALL.iter().map(Attack::to_string).collect();
    assert_eq!(
        names,
        [
            "low punch",
            "medium punch",
            "high punch",
            "low kick",
            "medium kick",
            "high kick"
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------------------------

#[test]
fn default_input_presses_nothing() {
    let input = Input::default();
    assert_eq!(input, Input::NONE);
    assert!(!input.left && !input.right && !input.jump && !input.grab);
    for attack in Attack::ALL {
        assert!(!input.is_pressed(attack), "{attack}");
    }
    assert_eq!(input.attack(), None);
}

#[test]
fn each_attack_has_its_own_button() {
    let fields = |input: Input| {
        [
            input.punch_low,
            input.punch_medium,
            input.punch_high,
            input.kick_low,
            input.kick_medium,
            input.kick_high,
        ]
    };
    for (index, attack) in Attack::ALL.into_iter().enumerate() {
        let input = Input::pressing(attack);
        let mut expected = [false; 6];
        expected[index] = true;
        assert_eq!(fields(input), expected, "{attack}");
        for other in Attack::ALL {
            assert_eq!(
                input.is_pressed(other),
                other == attack,
                "{attack} / {other}"
            );
        }
        assert_eq!(input.attack(), Some(attack));
        assert_eq!(input.horizontal(), 0);
        assert!(!input.jump && !input.grab);
    }
}

#[test]
fn grab_is_not_an_attack() {
    let grab = Input {
        grab: true,
        ..Input::NONE
    };
    assert_eq!(grab.attack(), None);
    assert_eq!(grab.horizontal(), 0);
    for attack in Attack::ALL {
        assert!(!grab.is_pressed(attack), "{attack}");
    }
    let jab = Attack::punch(Strength::Low);
    assert_eq!(
        grab.with(jab).attack(),
        Some(jab),
        "grab + attack still attacks"
    );
}

// ---------------------------------------------------------------------------------------------
// Fresh presses
// ---------------------------------------------------------------------------------------------

#[test]
fn pressed_since_keeps_only_the_buttons_that_were_not_held_before() {
    let before = Input {
        left: true,
        grab: true,
        ..Input::NONE
    };
    let now = Input {
        left: true,
        jump: true,
        ..Input::pressing(Attack::kick(Strength::Medium))
    };
    assert_eq!(
        now.pressed_since(before),
        Input {
            jump: true,
            ..Input::pressing(Attack::kick(Strength::Medium))
        }
    );
    assert_eq!(
        before.pressed_since(now),
        Input {
            grab: true,
            ..Input::NONE
        }
    );
}

#[test]
fn holding_a_button_is_not_a_fresh_press() {
    for bits in 0..(1u16 << Input::BITS) {
        let input = Input::from_bits(bits);
        assert_eq!(input.pressed_since(input), Input::NONE);
        assert_eq!(input.pressed_since(Input::NONE), input);
        assert_eq!(Input::NONE.pressed_since(input), Input::NONE);
    }
}

#[test]
fn with_adds_a_button_and_keeps_the_others() {
    let input = Input {
        left: true,
        ..Input::NONE
    }
    .with(Attack::kick(Strength::Low))
    .with(Attack::punch(Strength::High));
    assert!(input.left);
    assert!(input.kick_low && input.punch_high);
    assert!(!input.punch_low && !input.kick_high);
    assert_eq!(input.with(Attack::kick(Strength::Low)), input, "idempotent");
}

#[test]
fn the_strongest_held_attack_wins() {
    for weaker in Attack::ALL {
        for stronger in Attack::ALL {
            if stronger.strength > weaker.strength {
                let both = Input::pressing(weaker).with(stronger);
                assert_eq!(both.attack(), Some(stronger), "{weaker} + {stronger}");
            }
        }
    }
}

#[test]
fn a_kick_wins_over_a_punch_of_the_same_strength() {
    for strength in Strength::ALL {
        let both = Input::pressing(Attack::punch(strength)).with(Attack::kick(strength));
        assert_eq!(both.attack(), Some(Attack::kick(strength)));
    }
}

#[test]
fn every_attack_button_held_throws_the_high_kick() {
    let all = Attack::ALL
        .into_iter()
        .fold(Input::NONE, |input, attack| input.with(attack));
    assert_eq!(all.attack(), Some(Attack::kick(Strength::High)));
}

#[test]
fn movement_buttons_request_no_attack() {
    let input = Input {
        left: true,
        right: true,
        jump: true,
        ..Input::NONE
    };
    assert_eq!(input.attack(), None);
    assert_eq!(input.kick(), None);
}

#[test]
fn the_strongest_kick_held_is_the_kick_whatever_punches_are_held() {
    assert_eq!(Input::NONE.kick(), None);
    for punch in [Attack::punch(Strength::Low), Attack::punch(Strength::High)] {
        assert_eq!(Input::pressing(punch).kick(), None, "{punch} is no kick");
        for strength in Strength::ALL {
            let kick = Attack::kick(strength);
            let both = Input::pressing(punch).with(kick);
            assert_eq!(both.kick(), Some(kick), "{punch} + {kick}");
        }
    }
    let two_kicks =
        Input::pressing(Attack::kick(Strength::Low)).with(Attack::kick(Strength::Medium));
    assert_eq!(two_kicks.kick(), Some(Attack::kick(Strength::Medium)));
}

#[test]
fn horizontal_is_minus_one_for_left() {
    let input = Input {
        left: true,
        ..Input::NONE
    };
    assert_eq!(input.horizontal(), -1);
}

#[test]
fn horizontal_is_plus_one_for_right() {
    let input = Input {
        right: true,
        ..Input::NONE
    };
    assert_eq!(input.horizontal(), 1);
}

#[test]
fn horizontal_is_zero_without_direction() {
    assert_eq!(Input::NONE.horizontal(), 0);
    let input = Input {
        jump: true,
        ..Input::pressing(Attack::kick(Strength::High))
    };
    assert_eq!(input.horizontal(), 0);
}

#[test]
fn opposite_directions_cancel_out() {
    let input = Input {
        left: true,
        right: true,
        ..Input::NONE
    };
    assert_eq!(input.horizontal(), 0);
}

// ---------------------------------------------------------------------------------------------
// Bit packing
// ---------------------------------------------------------------------------------------------

#[test]
fn inputs_pack_into_fourteen_bits() {
    assert_eq!(Input::BITS, 14);
    assert_eq!(Input::NONE.to_bits(), 0);
    assert_eq!(Input::from_bits(0), Input::NONE);
}

#[test]
fn bits_follow_the_documented_order() {
    let single = [
        Input {
            left: true,
            ..Input::NONE
        },
        Input {
            right: true,
            ..Input::NONE
        },
        Input {
            jump: true,
            ..Input::NONE
        },
    ]
    .into_iter()
    .chain(Attack::ALL.map(Input::pressing))
    .chain([
        Input {
            grab: true,
            ..Input::NONE
        },
        Input {
            down: true,
            ..Input::NONE
        },
        Input {
            sidestep_in: true,
            ..Input::NONE
        },
        Input {
            sidestep_out: true,
            ..Input::NONE
        },
        Input {
            special: true,
            ..Input::NONE
        },
    ]);
    let mut count = 0;
    for (bit, input) in single.enumerate() {
        assert_eq!(input.to_bits(), 1 << bit, "{input:?}");
        count += 1;
    }
    assert_eq!(count, Input::BITS);
}

#[test]
fn every_input_survives_a_round_trip_through_bits() {
    for bits in 0..(1u16 << Input::BITS) {
        let input = Input::from_bits(bits);
        assert_eq!(input.to_bits(), bits);
        assert_eq!(Input::from_bits(input.to_bits()), input);
    }
}

#[test]
fn bits_above_the_thirteenth_are_ignored() {
    assert_eq!(Input::from_bits(1 << Input::BITS), Input::NONE);
    assert_eq!(Input::from_bits(u16::MAX).to_bits(), (1 << Input::BITS) - 1);
}

// ---------------------------------------------------------------------------------------------
// Key bindings
// ---------------------------------------------------------------------------------------------

#[test]
fn the_wasd_layout_is_the_default() {
    assert_eq!(KeyBindings::default(), KeyBindings::WASD);
}

#[test]
fn the_arrows_move_jump_and_crouch() {
    let b = KeyBindings::STANDARD;
    assert_eq!(b.left, Key::ArrowLeft);
    assert_eq!(b.right, Key::ArrowRight);
    assert_eq!(b.jump, Key::ArrowUp);
    assert_eq!(b.down, Key::ArrowDown);
    assert_eq!(
        b.to_input(&pressed(&[Key::ArrowDown])),
        Input {
            down: true,
            ..Input::NONE
        }
    );
}

#[test]
fn keypad_zero_grabs() {
    let b = KeyBindings::STANDARD;
    assert_eq!(b.grab, Key::Numpad0);
    assert_eq!(
        b.to_input(&pressed(&[Key::Numpad0])),
        Input {
            grab: true,
            ..Input::NONE
        }
    );
}

#[test]
fn keypad_one_five_six_punch_and_four_eight_nine_kick() {
    let b = KeyBindings::STANDARD;
    let cases = [
        (Key::Numpad1, Attack::punch(Strength::Low)),
        (Key::Numpad5, Attack::punch(Strength::Medium)),
        (Key::Numpad6, Attack::punch(Strength::High)),
        (Key::Numpad4, Attack::kick(Strength::Low)),
        (Key::Numpad8, Attack::kick(Strength::Medium)),
        (Key::Numpad9, Attack::kick(Strength::High)),
    ];
    for (key, attack) in cases {
        assert_eq!(
            b.to_input(&pressed(&[key])),
            Input::pressing(attack),
            "{key:?}"
        );
    }
}

#[test]
fn keys_are_listed_in_bit_order() {
    let b = KeyBindings::STANDARD;
    assert_eq!(
        b.keys(),
        [
            b.left,
            b.right,
            b.jump,
            b.punch_low,
            b.punch_medium,
            b.punch_high,
            b.kick_low,
            b.kick_medium,
            b.kick_high,
            b.grab,
            b.down,
            b.sidestep_in,
            b.sidestep_out,
            b.special
        ]
    );
}

#[test]
fn the_layout_binds_fourteen_distinct_keys() {
    let distinct: HashSet<Key> = KeyBindings::STANDARD.keys().into_iter().collect();
    assert_eq!(distinct.len(), 14);
}

#[test]
fn keypad_three_is_the_special_move() {
    let b = KeyBindings::STANDARD;
    assert_eq!(b.special, Key::Numpad3);
    assert_eq!(
        b.to_input(&pressed(&[Key::Numpad3])),
        Input {
            special: true,
            ..Input::NONE
        }
    );
    let special = Input {
        special: true,
        ..Input::NONE
    };
    assert_eq!(special.to_bits(), 1 << 13, "the last bit");
    assert_eq!(Input::from_bits(1 << 13), special);
}

#[test]
fn keypad_seven_and_two_sidestep_to_the_wall_and_the_street() {
    let b = KeyBindings::STANDARD;
    assert_eq!(
        (b.sidestep_in, b.sidestep_out),
        (Key::Numpad7, Key::Numpad2)
    );
    assert_eq!(b.to_input(&pressed(&[Key::Numpad7])).sideways(), 1);
    assert_eq!(b.to_input(&pressed(&[Key::Numpad2])).sideways(), -1);
    assert_eq!(
        b.to_input(&pressed(&[Key::Numpad7, Key::Numpad2]))
            .sideways(),
        0,
        "both cancel out"
    );
}

#[test]
fn the_menu_keys_are_not_bound_to_the_game() {
    let keys = KeyBindings::STANDARD.keys();
    assert!(!keys.contains(&Key::Enter));
    assert!(!keys.contains(&Key::Escape));
}

#[test]
fn each_bound_key_sets_exactly_its_bit() {
    let b = KeyBindings::STANDARD;
    for (bit, key) in b.keys().into_iter().enumerate() {
        assert_eq!(b.to_input(&pressed(&[key])).to_bits(), 1 << bit, "{key:?}");
    }
}

#[test]
fn nothing_pressed_gives_no_input() {
    assert_eq!(KeyBindings::STANDARD.to_input(&HashSet::new()), Input::NONE);
}

#[test]
fn unbound_keys_are_ignored() {
    let unbound = pressed(&[Key::K, Key::Q, Key::E, Key::Enter, Key::Escape]);
    assert_eq!(KeyBindings::STANDARD.to_input(&unbound), Input::NONE);
}

#[test]
fn several_keys_combine_into_one_input() {
    let b = KeyBindings::STANDARD;
    let input = b.to_input(&pressed(&[
        Key::ArrowRight,
        Key::ArrowUp,
        Key::Numpad5,
        Key::Numpad2,
        Key::K,
    ]));
    assert_eq!(
        input,
        Input {
            right: true,
            jump: true,
            punch_medium: true,
            sidestep_out: true,
            ..Input::NONE
        }
    );
}

#[test]
fn every_key_held_gives_every_button() {
    let b = KeyBindings::STANDARD;
    let all: HashSet<Key> = b.keys().into_iter().collect();
    assert_eq!(b.to_input(&all), Input::from_bits(u16::MAX));
}

#[test]
fn conflicting_directions_are_reported_as_pressed_and_cancel_in_horizontal() {
    let b = KeyBindings::STANDARD;
    let input = b.to_input(&pressed(&[b.left, b.right]));
    assert!(input.left && input.right);
    assert_eq!(input.horizontal(), 0);
}

#[test]
fn bindings_can_be_customised_copied_and_compared() {
    let a = KeyBindings::STANDARD;
    let b = a;
    assert_eq!(a, b);
    let swapped = KeyBindings {
        jump: Key::K,
        ..KeyBindings::STANDARD
    };
    assert_ne!(a, swapped);
    assert_eq!(
        swapped.to_input(&pressed(&[Key::K])),
        Input {
            jump: true,
            ..Input::NONE
        }
    );
    assert_eq!(swapped.to_input(&pressed(&[Key::ArrowUp])), Input::NONE);
}

// ---------------------------------------------------------------------------------------------
// Keys, presets and remapping
// ---------------------------------------------------------------------------------------------

/// Every key, in declaration order.
const ALL_KEYS: [Key; 44] = [
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::ArrowUp,
    Key::ArrowDown,
    Key::Numpad0,
    Key::Numpad1,
    Key::Numpad2,
    Key::Numpad3,
    Key::Numpad4,
    Key::Numpad5,
    Key::Numpad6,
    Key::Numpad7,
    Key::Numpad8,
    Key::Numpad9,
    Key::Enter,
    Key::Escape,
    Key::Tab,
    Key::Space,
    Key::A,
    Key::B,
    Key::C,
    Key::D,
    Key::E,
    Key::F,
    Key::G,
    Key::H,
    Key::I,
    Key::J,
    Key::K,
    Key::L,
    Key::M,
    Key::N,
    Key::O,
    Key::P,
    Key::Q,
    Key::R,
    Key::S,
    Key::T,
    Key::U,
    Key::V,
    Key::W,
    Key::X,
    Key::Y,
    Key::Z,
];

#[test]
fn every_key_has_a_short_ascii_label() {
    let labels: Vec<&str> = ALL_KEYS.iter().map(|key| key.label()).collect();
    assert_eq!(
        labels,
        [
            "Left", "Right", "Up", "Down", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
            "Enter", "Esc", "Tab", "Space", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K",
            "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z"
        ]
    );
    let distinct: HashSet<&str> = labels.iter().copied().collect();
    assert_eq!(distinct.len(), ALL_KEYS.len());
    assert!(labels.iter().all(|label| label.is_ascii()));
}

#[test]
fn keys_are_ordered_as_declared() {
    for pair in ALL_KEYS.windows(2) {
        assert!(pair[0] < pair[1], "{:?} < {:?}", pair[0], pair[1]);
    }
    let mut shuffled = vec![Key::Z, Key::Space, Key::Numpad5, Key::ArrowLeft, Key::A];
    shuffled.sort();
    assert_eq!(
        shuffled,
        [Key::ArrowLeft, Key::Numpad5, Key::Space, Key::A, Key::Z]
    );
}

#[test]
fn only_the_menu_pause_view_practice_and_toggle_keys_cannot_be_bound() {
    for key in ALL_KEYS {
        assert_eq!(
            key.is_bindable(),
            !matches!(
                key,
                Key::Enter | Key::Escape | Key::P | Key::Tab | Key::T | Key::X
            ),
            "{key:?}"
        );
    }
    let mut layout = KeyBindings::STANDARD;
    assert!(!layout.bind(0, Key::P));
    assert!(!layout.bind(0, Key::Tab));
    assert!(!layout.bind(0, Key::T));
    assert!(!layout.bind(0, Key::X));
    assert_eq!(layout, KeyBindings::STANDARD);
}

#[test]
fn the_wasd_preset_puts_the_layout_on_the_letters() {
    let b = KeyBindings::WASD;
    assert_eq!(
        b.keys(),
        [
            Key::A,
            Key::D,
            Key::W,
            Key::B,
            Key::H,
            Key::J,
            Key::G,
            Key::Y,
            Key::U,
            Key::Space,
            Key::S,
            Key::Q,
            Key::E,
            Key::F
        ]
    );
    let distinct: HashSet<Key> = b.keys().into_iter().collect();
    assert_eq!(distinct.len(), 14);
    assert!(b.keys().into_iter().all(Key::is_bindable));
    assert_eq!(
        b.to_input(&pressed(&[Key::S, Key::A])),
        Input {
            down: true,
            left: true,
            ..Input::NONE
        }
    );
    assert_eq!(
        b.to_input(&pressed(&[Key::H, Key::U])),
        Input::pressing(Attack::punch(Strength::Medium)).with(Attack::kick(Strength::High)),
        "the sweep keys"
    );
}

#[test]
fn the_presets_are_wasd_then_standard() {
    assert_eq!(
        KeyBindings::PRESETS,
        [
            ("WASD (letters + space)", KeyBindings::WASD),
            ("Standard (arrows + keypad)", KeyBindings::STANDARD),
        ]
    );
    assert_eq!(KeyBindings::PRESETS[0].1, KeyBindings::default());
}

#[test]
fn every_action_has_a_name_in_key_order() {
    assert_eq!(
        KeyBindings::ACTIONS,
        [
            "Left",
            "Right",
            "Jump",
            "Low punch",
            "Medium punch",
            "High punch",
            "Low kick",
            "Medium kick",
            "High kick",
            "Grab",
            "Crouch / turn",
            "Sidestep to the wall",
            "Sidestep to the street",
            "Special move",
        ]
    );
}

#[test]
fn from_keys_is_the_inverse_of_keys() {
    for (_, preset) in KeyBindings::PRESETS {
        assert_eq!(KeyBindings::from_keys(preset.keys()), preset);
    }
    let mut keys = KeyBindings::STANDARD.keys();
    keys.reverse();
    let reversed = KeyBindings::from_keys(keys);
    assert_eq!(reversed.left, Key::Numpad3);
    assert_eq!(reversed.special, Key::ArrowLeft);
    assert_eq!(reversed.keys(), keys);
}

#[test]
fn key_of_names_the_key_of_each_attack() {
    for (_, preset) in KeyBindings::PRESETS {
        for (index, attack) in Attack::ALL.into_iter().enumerate() {
            assert_eq!(preset.key_of(attack), preset.keys()[3 + index], "{attack}");
            assert_eq!(
                preset.to_input(&pressed(&[preset.key_of(attack)])),
                Input::pressing(attack)
            );
        }
    }
    assert_eq!(
        KeyBindings::STANDARD.key_of(Attack::kick(Strength::High)),
        Key::Numpad9
    );
    assert_eq!(
        KeyBindings::WASD.key_of(Attack::punch(Strength::Low)),
        Key::B
    );
}

#[test]
fn binding_a_free_key_changes_only_that_action() {
    let mut b = KeyBindings::STANDARD;
    assert!(b.bind(2, Key::Space));
    assert_eq!(
        b,
        KeyBindings {
            jump: Key::Space,
            ..KeyBindings::STANDARD
        }
    );
    assert!(b.bind(10, Key::K));
    assert_eq!(b.down, Key::K);
}

#[test]
fn binding_a_used_key_swaps_the_two_actions() {
    let mut b = KeyBindings::STANDARD;
    assert!(b.bind(0, Key::ArrowRight));
    assert_eq!((b.left, b.right), (Key::ArrowRight, Key::ArrowLeft));
    let distinct: HashSet<Key> = b.keys().into_iter().collect();
    assert_eq!(distinct.len(), 14, "no key does two things");

    let mut same = KeyBindings::WASD;
    assert!(same.bind(9, Key::Space), "rebinding its own key");
    assert_eq!(same, KeyBindings::WASD);
}

#[test]
fn binding_refuses_menu_keys_and_unknown_actions() {
    let mut b = KeyBindings::STANDARD;
    assert!(!b.bind(0, Key::Enter));
    assert!(!b.bind(0, Key::Escape));
    assert!(!b.bind(Input::BITS as usize, Key::K));
    assert!(!b.bind(usize::MAX, Key::K));
    assert_eq!(b, KeyBindings::STANDARD);
}

#[test]
fn saved_layouts_round_trip_through_their_bytes() {
    for (_, preset) in KeyBindings::PRESETS {
        assert_eq!(KeyBindings::from_bytes(&preset.to_bytes()), Some(preset));
    }

    let mut mine = KeyBindings::WASD;
    assert!(mine.bind(9, Key::K));
    assert!(mine.bind(0, Key::Numpad4));
    assert_eq!(KeyBindings::from_bytes(&mine.to_bytes()), Some(mine));
}

#[test]
fn loading_layout_bytes_refuses_the_wrong_ones() {
    assert_eq!(KeyBindings::from_bytes(&[]), None, "too short");
    assert_eq!(
        KeyBindings::from_bytes(&[0; Input::BITS as usize]),
        None,
        "the same key twice"
    );
    assert_eq!(
        KeyBindings::from_bytes(&[u8::MAX; Input::BITS as usize]),
        None,
        "not the number of a key"
    );
}

#[test]
fn keys_round_trip_through_their_numbers() {
    for key in Key::ALL {
        assert_eq!(Key::at(key.index()), Some(key));
    }
    assert_eq!(Key::ALL.len(), 44, "every key is numbered");
}
