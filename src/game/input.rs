//! Player input, the six attacks and the keyboard layout.
//!
//! [`Input`] is what the simulation consumes; [`KeyBindings`] translates the set of keys held on
//! the keyboard into the `Input` of the local player. Every player plays on their own machine,
//! so there is a single layout at a time: [`KeyBindings::STANDARD`] (the arrows move, the
//! numeric keypad attacks and grabs), [`KeyBindings::WASD`], or any remapping of them.

use std::collections::HashSet;
use std::fmt;

/// The limb an attack is thrown with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Limb {
    /// A fist attack.
    Punch,
    /// A leg attack.
    Kick,
}

/// How hard an attack hits. Stronger attacks deal more damage but are slower to start and to
/// recover from (see [`MatchConfig::default`](super::MatchConfig::default)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Strength {
    /// Weak and fast.
    Low,
    /// The middle ground.
    Medium,
    /// Strong and slow.
    High,
}

impl Strength {
    /// Every strength, from the weakest to the strongest.
    pub const ALL: [Strength; 3] = [Strength::Low, Strength::Medium, Strength::High];

    /// Position in [`Strength::ALL`]: 0 for low, 1 for medium, 2 for high.
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// One of the six attacks: a limb thrown at a strength.
///
/// ```
/// use srr::game::{Attack, Limb, Strength};
///
/// let attack = Attack::kick(Strength::High);
/// assert_eq!(attack.limb, Limb::Kick);
/// assert_eq!(attack.to_string(), "high kick");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Attack {
    /// The limb thrown.
    pub limb: Limb,
    /// How hard it hits.
    pub strength: Strength,
}

impl Attack {
    /// Every attack: the punches from low to high, then the kicks from low to high.
    pub const ALL: [Attack; 6] = [
        Attack::punch(Strength::Low),
        Attack::punch(Strength::Medium),
        Attack::punch(Strength::High),
        Attack::kick(Strength::Low),
        Attack::kick(Strength::Medium),
        Attack::kick(Strength::High),
    ];

    /// A punch of the given strength.
    pub const fn punch(strength: Strength) -> Self {
        Attack {
            limb: Limb::Punch,
            strength,
        }
    }

    /// A kick of the given strength.
    pub const fn kick(strength: Strength) -> Self {
        Attack {
            limb: Limb::Kick,
            strength,
        }
    }
}

impl fmt::Display for Attack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let strength = match self.strength {
            Strength::Low => "low",
            Strength::Medium => "medium",
            Strength::High => "high",
        };
        let limb = match self.limb {
            Limb::Punch => "punch",
            Limb::Kick => "kick",
        };
        write!(f, "{strength} {limb}")
    }
}

/// What one player holds down during one tick. A level, not an edge: holding a button keeps
/// triggering it. Grappling reacts to fresh presses only, computed with
/// [`Input::pressed_since`].
///
/// There is no block button: a fighter blocks by stepping away from the attack (see
/// [`FighterState::Blocking`](super::FighterState::Blocking)). Grabs cannot be blocked.
///
/// `Input::default()` and [`Input::NONE`] press nothing.
///
/// ```
/// use srr::game::{Attack, Input, Strength};
///
/// let walk_right = Input { right: true, ..Input::NONE };
/// assert_eq!(walk_right.horizontal(), 1);
///
/// let jab = Input::pressing(Attack::punch(Strength::Low));
/// assert!(jab.punch_low);
/// assert_eq!(jab.attack(), Some(Attack::punch(Strength::Low)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Input {
    /// Move left.
    pub left: bool,
    /// Move right.
    pub right: bool,
    /// Jump.
    pub jump: bool,
    /// Low punch.
    pub punch_low: bool,
    /// Medium punch.
    pub punch_medium: bool,
    /// High punch.
    pub punch_high: bool,
    /// Low kick.
    pub kick_low: bool,
    /// Medium kick.
    pub kick_medium: bool,
    /// High kick.
    pub kick_high: bool,
    /// Grab; in a grapple, advance the position (top) or reverse it
    /// ([`Glitch::Reversal`](super::Glitch::Reversal)).
    pub grab: bool,
    /// Down: crouch on the floor (high attacks pass over). Together with a direction, turn to
    /// face that direction. Fighters never turn by themselves: after jumping over the opponent
    /// (or being walked around), the player has to turn to face it again.
    pub down: bool,
}

impl Input {
    /// No button held.
    pub const NONE: Input = Input {
        left: false,
        right: false,
        jump: false,
        punch_low: false,
        punch_medium: false,
        punch_high: false,
        kick_low: false,
        kick_medium: false,
        kick_high: false,
        grab: false,
        down: false,
    };

    /// Number of bits used by [`Input::to_bits`].
    pub const BITS: u32 = 11;

    /// Only the button of `attack` is held.
    pub fn pressing(attack: Attack) -> Input {
        Input::NONE.with(attack)
    }

    /// This input with the button of `attack` held as well.
    pub fn with(mut self, attack: Attack) -> Input {
        *self.button_mut(attack) = true;
        self
    }

    /// Whether the button of `attack` is held.
    pub fn is_pressed(&self, attack: Attack) -> bool {
        let mut copy = *self;
        *copy.button_mut(attack)
    }

    fn button_mut(&mut self, attack: Attack) -> &mut bool {
        match (attack.limb, attack.strength) {
            (Limb::Punch, Strength::Low) => &mut self.punch_low,
            (Limb::Punch, Strength::Medium) => &mut self.punch_medium,
            (Limb::Punch, Strength::High) => &mut self.punch_high,
            (Limb::Kick, Strength::Low) => &mut self.kick_low,
            (Limb::Kick, Strength::Medium) => &mut self.kick_medium,
            (Limb::Kick, Strength::High) => &mut self.kick_high,
        }
    }

    /// -1 for left, 1 for right, 0 for neither or both.
    pub fn horizontal(&self) -> i32 {
        i32::from(self.right) - i32::from(self.left)
    }

    /// The attack requested by this input, if any attack button is held.
    ///
    /// When several are held the strongest wins, and a kick wins over a punch of the same
    /// strength.
    ///
    /// ```
    /// use srr::game::{Attack, Input, Strength};
    ///
    /// let mash = Input::pressing(Attack::punch(Strength::High)).with(Attack::kick(Strength::Low));
    /// assert_eq!(mash.attack(), Some(Attack::punch(Strength::High)));
    /// assert_eq!(Input::NONE.attack(), None);
    /// ```
    pub fn attack(&self) -> Option<Attack> {
        Strength::ALL
            .into_iter()
            .rev()
            .flat_map(|strength| [Attack::kick(strength), Attack::punch(strength)])
            .find(|attack| self.is_pressed(*attack))
    }

    /// The buttons held in this input but not in `previous`: the fresh presses (rising edges)
    /// when `previous` is the input of the tick before.
    ///
    /// ```
    /// use srr::game::Input;
    ///
    /// let before = Input { left: true, ..Input::NONE };
    /// let now = Input { left: true, grab: true, ..Input::NONE };
    /// assert_eq!(now.pressed_since(before), Input { grab: true, ..Input::NONE });
    /// assert_eq!(now.pressed_since(now), Input::NONE);
    /// ```
    pub fn pressed_since(self, previous: Input) -> Input {
        Input::from_bits(self.to_bits() & !previous.to_bits())
    }

    /// Packs the input into the low [`Input::BITS`] bits: left, right, jump, the attack
    /// buttons in [`Attack::ALL`] order, grab, then down.
    ///
    /// ```
    /// use srr::game::Input;
    ///
    /// let input = Input { right: true, kick_high: true, ..Input::NONE };
    /// assert_eq!(Input::from_bits(input.to_bits()), input);
    /// ```
    pub fn to_bits(self) -> u16 {
        self.flags()
            .into_iter()
            .enumerate()
            .fold(0, |bits, (index, held)| bits | (u16::from(held) << index))
    }

    /// Unpacks [`Input::to_bits`]; bits above [`Input::BITS`] are ignored.
    pub fn from_bits(bits: u16) -> Input {
        let held = |index: usize| bits & (1 << index) != 0;
        Input {
            left: held(0),
            right: held(1),
            jump: held(2),
            punch_low: held(3),
            punch_medium: held(4),
            punch_high: held(5),
            kick_low: held(6),
            kick_medium: held(7),
            kick_high: held(8),
            grab: held(9),
            down: held(10),
        }
    }

    fn flags(self) -> [bool; Input::BITS as usize] {
        [
            self.left,
            self.right,
            self.jump,
            self.punch_low,
            self.punch_medium,
            self.punch_high,
            self.kick_low,
            self.kick_medium,
            self.kick_high,
            self.grab,
            self.down,
        ]
    }
}

/// Physical keys the layout can use, independent of any windowing library. They are ordered
/// as declared, so keys pressed on the same tick are always handled in the same order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    /// The left arrow key.
    ArrowLeft,
    /// The right arrow key.
    ArrowRight,
    /// The up arrow key.
    ArrowUp,
    /// The down arrow key.
    ArrowDown,
    /// Keypad `0`.
    Numpad0,
    /// Keypad `1`.
    Numpad1,
    /// Keypad `2`.
    Numpad2,
    /// Keypad `3`.
    Numpad3,
    /// Keypad `4`.
    Numpad4,
    /// Keypad `5`.
    Numpad5,
    /// Keypad `6`.
    Numpad6,
    /// Keypad `7`.
    Numpad7,
    /// Keypad `8`.
    Numpad8,
    /// Keypad `9`.
    Numpad9,
    /// `Enter` (either the main one or the keypad's): confirms, skips the intro, ready for the
    /// next match.
    Enter,
    /// `Escape`: skips the intro, leaves the controls page, leaves an online match against the
    /// computer.
    Escape,
    /// The space bar.
    Space,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
}

impl Key {
    /// How the key is written on screen (ASCII only). The keypad digits are written as bare
    /// digits: they are the only digit keys.
    ///
    /// ```
    /// use srr::game::Key;
    ///
    /// assert_eq!(Key::Numpad5.label(), "5");
    /// assert_eq!(Key::ArrowDown.label(), "Down");
    /// assert_eq!(Key::W.label(), "W");
    /// ```
    pub fn label(self) -> &'static str {
        match self {
            Key::ArrowLeft => "Left",
            Key::ArrowRight => "Right",
            Key::ArrowUp => "Up",
            Key::ArrowDown => "Down",
            Key::Numpad0 => "0",
            Key::Numpad1 => "1",
            Key::Numpad2 => "2",
            Key::Numpad3 => "3",
            Key::Numpad4 => "4",
            Key::Numpad5 => "5",
            Key::Numpad6 => "6",
            Key::Numpad7 => "7",
            Key::Numpad8 => "8",
            Key::Numpad9 => "9",
            Key::Enter => "Enter",
            Key::Escape => "Esc",
            Key::Space => "Space",
            Key::A => "A",
            Key::B => "B",
            Key::C => "C",
            Key::D => "D",
            Key::E => "E",
            Key::F => "F",
            Key::G => "G",
            Key::H => "H",
            Key::I => "I",
            Key::J => "J",
            Key::K => "K",
            Key::L => "L",
            Key::M => "M",
            Key::N => "N",
            Key::O => "O",
            Key::P => "P",
            Key::Q => "Q",
            Key::R => "R",
            Key::S => "S",
            Key::T => "T",
            Key::U => "U",
            Key::V => "V",
            Key::W => "W",
            Key::X => "X",
            Key::Y => "Y",
            Key::Z => "Z",
        }
    }

    /// Whether the key can be bound to an action: every key but Enter and Escape, which drive
    /// the menus (and the readiness for the next match).
    pub fn is_bindable(self) -> bool {
        !matches!(self, Key::Enter | Key::Escape)
    }
}

/// The keys of the local player.
///
/// [`KeyBindings::STANDARD`] (also the `Default`) is the game's layout: arrows to move and
/// jump, the down arrow to crouch (down + left / right to turn around), keypad `1`/`5`/`6`
/// for the low/medium/high punch, keypad `4`/`8`/`9` for the low/medium/high kick and keypad
/// `0` to grab. The judo techniques are two keys pressed together: `5` + `9` and `8` + `9`
/// (see [`Technique`](super::Technique)).
///
/// [`KeyBindings::WASD`] is the same layout for keyboards without a keypad, and
/// [`KeyBindings::bind`] remaps one action at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBindings {
    /// Key bound to moving left.
    pub left: Key,
    /// Key bound to moving right.
    pub right: Key,
    /// Key bound to jumping.
    pub jump: Key,
    /// Key bound to the low punch.
    pub punch_low: Key,
    /// Key bound to the medium punch.
    pub punch_medium: Key,
    /// Key bound to the high punch.
    pub punch_high: Key,
    /// Key bound to the low kick.
    pub kick_low: Key,
    /// Key bound to the medium kick.
    pub kick_medium: Key,
    /// Key bound to the high kick.
    pub kick_high: Key,
    /// Key bound to the grab.
    pub grab: Key,
    /// Key bound to down: crouch, and turn around together with a direction.
    pub down: Key,
}

impl KeyBindings {
    /// The game's layout.
    pub const STANDARD: KeyBindings = KeyBindings {
        left: Key::ArrowLeft,
        right: Key::ArrowRight,
        jump: Key::ArrowUp,
        punch_low: Key::Numpad1,
        punch_medium: Key::Numpad5,
        punch_high: Key::Numpad6,
        kick_low: Key::Numpad4,
        kick_medium: Key::Numpad8,
        kick_high: Key::Numpad9,
        grab: Key::Numpad0,
        down: Key::ArrowDown,
    };

    /// The standard layout on the left hand and the letters: `W` `A` `S` `D` for the arrows,
    /// `B` `H` `J` for the punches (keypad `1` `5` `6`), `G` `Y` `U` for the kicks (keypad
    /// `4` `8` `9`) and `Space` to grab (keypad `0`). The judo techniques become `H` + `U`
    /// and `Y` + `U`.
    pub const WASD: KeyBindings = KeyBindings {
        left: Key::A,
        right: Key::D,
        jump: Key::W,
        punch_low: Key::B,
        punch_medium: Key::H,
        punch_high: Key::J,
        kick_low: Key::G,
        kick_medium: Key::Y,
        kick_high: Key::U,
        grab: Key::Space,
        down: Key::S,
    };

    /// The presets offered on the controls page, with their names; the first is the default.
    pub const PRESETS: [(&'static str, KeyBindings); 2] = [
        ("Standard (arrows + keypad)", KeyBindings::STANDARD),
        ("WASD (letters + space)", KeyBindings::WASD),
    ];

    /// What each key of [`KeyBindings::keys`] does, in the same order.
    pub const ACTIONS: [&'static str; Input::BITS as usize] = [
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
    ];

    /// The layout binding [`KeyBindings::keys`] order: the inverse of `keys`.
    ///
    /// ```
    /// use srr::game::KeyBindings;
    ///
    /// let wasd = KeyBindings::WASD;
    /// assert_eq!(KeyBindings::from_keys(wasd.keys()), wasd);
    /// ```
    pub const fn from_keys(keys: [Key; Input::BITS as usize]) -> Self {
        let [
            left,
            right,
            jump,
            punch_low,
            punch_medium,
            punch_high,
            kick_low,
            kick_medium,
            kick_high,
            grab,
            down,
        ] = keys;
        KeyBindings {
            left,
            right,
            jump,
            punch_low,
            punch_medium,
            punch_high,
            kick_low,
            kick_medium,
            kick_high,
            grab,
            down,
        }
    }

    /// Binds `key` to the action at `action` in [`KeyBindings::ACTIONS`] order. When another
    /// action already used `key`, the two actions swap keys, so no key ever does two things.
    /// Returns false (and changes nothing) when `action` is out of range or `key` is not
    /// [bindable](Key::is_bindable).
    ///
    /// ```
    /// use srr::game::{Key, KeyBindings};
    ///
    /// let mut layout = KeyBindings::STANDARD;
    /// assert!(layout.bind(9, Key::Numpad5)); // grab on keypad 5...
    /// assert_eq!((layout.grab, layout.punch_medium), (Key::Numpad5, Key::Numpad0)); // ...swapped
    /// ```
    pub fn bind(&mut self, action: usize, key: Key) -> bool {
        let mut keys = self.keys();
        if action >= keys.len() || !key.is_bindable() {
            return false;
        }
        if let Some(other) = keys.iter().position(|&bound| bound == key) {
            keys[other] = keys[action];
        }
        keys[action] = key;
        *self = KeyBindings::from_keys(keys);
        true
    }

    /// The key of `attack`'s button.
    pub fn key_of(&self, attack: Attack) -> Key {
        // The attacks follow left, right and jump, in `Attack::ALL` order.
        let limb = match attack.limb {
            Limb::Punch => 0,
            Limb::Kick => 3,
        };
        self.keys()[3 + limb + attack.strength.index()]
    }

    /// The bound keys in the bit order of [`Input::to_bits`]: left, right, jump, the attacks
    /// in [`Attack::ALL`] order, grab, then down.
    pub fn keys(&self) -> [Key; Input::BITS as usize] {
        [
            self.left,
            self.right,
            self.jump,
            self.punch_low,
            self.punch_medium,
            self.punch_high,
            self.kick_low,
            self.kick_medium,
            self.kick_high,
            self.grab,
            self.down,
        ]
    }

    /// The input of the local player given every key currently held on the keyboard.
    ///
    /// Unbound keys are ignored. Pressing both directions reports both, and
    /// [`Input::horizontal`] then cancels them out.
    ///
    /// Complexity: O(1) (eleven hash lookups).
    ///
    /// ```
    /// use std::collections::HashSet;
    /// use srr::game::{Key, KeyBindings};
    ///
    /// let held: HashSet<Key> = [Key::ArrowRight, Key::Numpad9, Key::Numpad2].into_iter().collect();
    /// let input = KeyBindings::STANDARD.to_input(&held);
    /// assert!(input.right && input.kick_high);
    /// assert!(!input.jump);
    /// ```
    pub fn to_input(&self, pressed: &HashSet<Key>) -> Input {
        let [
            left,
            right,
            jump,
            punch_low,
            punch_medium,
            punch_high,
            kick_low,
            kick_medium,
            kick_high,
            grab,
            down,
        ] = self.keys().map(|key| pressed.contains(&key));
        Input {
            left,
            right,
            jump,
            punch_low,
            punch_medium,
            punch_high,
            kick_low,
            kick_medium,
            kick_high,
            grab,
            down,
        }
    }
}

impl Default for KeyBindings {
    fn default() -> Self {
        KeyBindings::STANDARD
    }
}
