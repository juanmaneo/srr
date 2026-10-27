//! The engine-agnostic front end: the screens, the HUD and the music cues.
//!
//! [`Game`] is the whole client-side game as a pure state machine. It is fed the keys held on
//! each fixed tick, drives the [`Client`] (rules + network session), steers the music through a
//! [`MusicControl`], and describes what to show as a [`Frame`] of rectangles and monospace
//! labels. Nothing here knows about Bevy, Fyrox, sockets or sound cards, so every screen is
//! unit-tested, and an engine backend only has to draw frames (see [`crate::engine`] with the
//! `client` feature).
//!
//! Screens:
//!
//! 1. [`Screen::Intro`]: the graffiti title ([`intro`]), until Enter or Escape is pressed;
//!    `C` opens the controls page, `T` ([`PRACTICE_KEY`]) goes practising;
//! 2. [`Screen::Select`]: choose your [`Character`] ([`select`]), then Enter goes to the
//!    fight (practising, after `T`) and Escape back to the title;
//! 3. [`Screen::Controls`]: choose a keyboard preset, remap any action, change character or
//!    choose who to practise against ([`controls`]); it is also the pause menu of the fight
//!    screen (`P`), going back to the fight;
//! 4. [`Screen::Fight`]: the arena ([`arena`]) under the versus HUD ([`hud`]): health bars with
//!    the red "recoverable damage" trail and the combo counter, round wins, timer, grapple
//!    meter and banners. `Tab`
//!    ([`VIEW_KEY`]) switches the arena between the side view and the third-person 3D view
//!    ([`tps`]), even in the middle of a fight.
//!
//! On the fight screen, Enter means "ready": online, the server starts the next match (from a
//! 1v1 to a brawl of ten) once every seated player is ready; alone, it starts the next round.
//! `P` pauses: a practice fight freezes until the pause menu is left; an online match cannot
//! wait for one player, so it goes on with the paused player standing still.
//!
//! Practice is a fight against a [`Rival`]: the training dummy or the computer (the
//! [bot](crate::game::Bot)). A practice game (natively, or with `?practice`) is nothing else;
//! online, a practice fight is played locally while the room stays open, `T` starts it between
//! matches and Escape leaves it, and a real match that starts takes over. `T` during a
//! practice fight switches the rival, and so does the practice row of the pause menu
//! ([`Practice`]).
//!
//! A spectator ([`Client::spectator`], the page opened with `?spectate`, say on a TV) skips
//! the intro and the select screen and shows the fights of the room on the fight screen: the
//! frames of the server play them exactly, and everything the players are told about the room
//! is shown too. Nothing is ever sent, no fighter is played, and any number of spectators may
//! watch.
//!
//! On a touch screen the engine hands the game its touches ([`Game::set_touches`]): from the
//! first one on, on-screen controls ([`touch`]) are drawn over every screen and press keys like
//! a keyboard would.
//!
//! [`Headless`] plays the [`Game`] without any engine: scripted keys in, frames rendered in
//! memory by the software renderer of [`render`] (pixels, PNG files, text snapshots). That is
//! how the tests look at the screens without opening a window.

use std::collections::HashSet;

use crate::audio::{Mood, MusicControl};
use crate::game::{
    Character, GrapplePosition, Input, Key, KeyBindings, Match, Phase, PlayerId, Rival, Skin,
    TICKS_PER_SECOND,
};
use crate::net::{Client, MAX_PLAYERS, Status};
use space::Camera;

pub mod arena;
pub mod ascii;
pub mod controls;
pub mod gamepad;
pub mod headless;
pub mod hud;
pub mod intro;
pub mod manga;
pub mod render;
mod scene;
pub mod select;
pub mod skin;
pub mod space;
pub mod stick;
pub mod touch;
pub mod tps;

pub use controls::{ControlsPage, PAUSE_KEY, Practice};
pub use gamepad::{GamepadState, PadButton};
pub use headless::Headless;
pub use hud::HealthBar;
pub use intro::Intro;
pub use scene::{
    Align, Frame, Label, MONO_ADVANCE, Rect, Rgba, SCREEN_HEIGHT, SCREEN_WIDTH, Viewport,
};
pub use select::{Choice, SelectPage};
pub use touch::TouchPad;

/// Whether the music reaches the speakers, as reported by the platform layer (the [`Game`]
/// itself never touches a sound card).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SoundState {
    /// Not started yet: browsers only allow sound after a key press, click or touch.
    #[default]
    Waiting,
    /// The music is playing.
    Playing,
    /// No audio output could be opened (the reason is in the console / on stderr).
    Unavailable,
}

impl SoundState {
    /// The note shown in the corner of the intro.
    ///
    /// ```
    /// use srr::front::SoundState;
    ///
    /// assert_eq!(SoundState::Playing.note(), "sound: on");
    /// ```
    pub fn note(self) -> &'static str {
        match self {
            SoundState::Waiting => "sound: press a key or click",
            SoundState::Playing => "sound: on",
            SoundState::Unavailable => "sound: no audio output",
        }
    }
}

// Where the sound note sits on the intro: top-right corner, right-aligned.
const SOUND_NOTE_X: f32 = SCREEN_WIDTH - 24.0;
const SOUND_NOTE_Y: f32 = 14.0;
const SOUND_NOTE_SIZE: f32 = 16.0;
const SOUND_WAITING: Rgba = Rgba::hex(0xffe14a);
const SOUND_OTHER: Rgba = Rgba::hex(0x8a8098);

/// The hints to the controls page and to practice, in the top-left corner of the intro.
pub const CONTROLS_HINT: &str = "C: controls / keyboard layout   T: practice";
const CONTROLS_HINT_X: f32 = 24.0;
const CONTROLS_HINT_COLOR: Rgba = Rgba::hex(0xd8d0e8);

/// The key opening the controls page from the intro (see [`crate::front::controls`]). It
/// cannot be bound to an action.
pub const CONTROLS_KEY: Key = Key::C;

/// The key going practising: on the intro it opens the fight screen and, online, starts a
/// practice fight; on the fight screen it starts one between online matches, and switches the
/// rival of a practice fight ([`Practice::next`]). It cannot be bound to an action.
pub const PRACTICE_KEY: Key = Key::T;

/// The key flipping the on-screen touch controls between shown and hidden (what the TOGGLE
/// button of the touch controls does). It cannot be bound to an action.
pub const TOGGLE_KEY: Key = Key::X;

/// What the game is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    /// The graffiti title.
    Intro,
    /// The character select screen.
    Select,
    /// The keyboard layout.
    Controls,
    /// The arena and the HUD.
    Fight,
}

/// How the fight is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum View {
    /// The side view: the chalk stick figures on the alley wall ([`arena`]).
    #[default]
    Side,
    /// The third-person 3D view: wooden mannequins seen over the local fighter's shoulder
    /// ([`tps`]).
    ThirdPerson,
}

impl View {
    /// The other view.
    pub fn next(self) -> View {
        match self {
            View::Side => View::ThirdPerson,
            View::ThirdPerson => View::Side,
        }
    }
}

/// The key switching the [`View`] on the fight screen.
pub const VIEW_KEY: Key = Key::Tab;

/// Seconds left on the round timer from fighting ticks (rounded up).
pub fn seconds(ticks: u32) -> u32 {
    ticks.div_ceil(TICKS_PER_SECOND)
}

/// How tense the round is, `0.0..=1.0`, for the music: the lowest health share among the
/// fighters still standing, a grapple heading for a submission, and the last ten seconds.
///
/// ```
/// use srr::front::intensity;
/// use srr::game::{Match, MatchConfig, Roster};
///
/// let fresh = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
/// assert_eq!(intensity(&fresh), 0.0);
/// ```
pub fn intensity(current: &Match) -> f32 {
    let danger = current
        .fighters()
        .iter()
        .filter(|f| !f.is_knocked_out())
        .map(|f| {
            let max = arena::rules_of(current, f).max_health.max(1) as f32;
            1.0 - f.health() as f32 / max
        })
        .fold(0.0_f32, f32::max);
    let grapple = current
        .fighters()
        .iter()
        .filter_map(|f| f.grapple())
        .map(|g| match g.position {
            GrapplePosition::Clinch => 0.3,
            GrapplePosition::Mount => 0.6,
            GrapplePosition::Sweep | GrapplePosition::Throw => 0.8,
            GrapplePosition::Submission | GrapplePosition::ScarfHold | GrapplePosition::Armbar => {
                1.0
            }
        })
        .fold(0.0_f32, f32::max);
    let clock = if current.phase() == Phase::Fighting
        && current.remaining_ticks() <= 10 * TICKS_PER_SECOND
    {
        0.5
    } else {
        0.0
    };
    danger.max(grapple).max(clock).clamp(0.0, 1.0)
}

/// The client-side game: screens, input, HUD state and music cues around a [`Client`].
#[derive(Debug, Clone)]
pub struct Game {
    client: Client,
    /// A practice fight played locally while the online room stays open.
    practice: Option<Client>,
    screen: Screen,
    intro: Intro,
    music: MusicControl,
    /// Keys held on the previous tick, to detect fresh presses.
    held: HashSet<Key>,
    bars: Vec<HealthBar>,
    /// Ticks spent on the fight screen.
    fight_ticks: u32,
    sound: SoundState,
    bindings: KeyBindings,
    controls: ControlsPage,
    view: View,
    /// The camera of the third-person view, gliding towards [`tps::aim`] on every fight tick.
    camera: Camera,
    select: SelectPage,
    /// Whether the select screen was opened with [`PRACTICE_KEY`]: the fight then starts with
    /// a practice fight, online.
    practise_after_select: bool,
    /// The fingers on the screen, in virtual screen coordinates.
    touches: Vec<(f32, f32)>,
    /// Whether the screen was ever touched: the touch controls are shown from then on.
    touch_mode: bool,
    /// Whether the touch controls are drawn right now, or only the TOGGLE button that brings
    /// them back: they are hidden after the TOGGLE button (or [`TOGGLE_KEY`]) and shown the
    /// same way.
    touch_controls: bool,
    /// The character whose card a finger tapped on the select screen, to pick on the next
    /// tick.
    tapped_character: Option<Character>,
    /// The buttons held on the gamepads, and where their left sticks lean (merged).
    gamepads: gamepad::GamepadState,
}

impl Game {
    /// A game starting on the intro, playing with `client` — unless `client` is a spectator
    /// ([`Client::spectator`]), whose page goes straight to the fight screen: a TV set up to
    /// watch the room shows the fights without anybody pressing anything on it.
    pub fn new(client: Client) -> Self {
        let character = client.character();
        let spectating = client.is_spectator();
        let game = Game {
            practice: None,
            screen: if spectating {
                Screen::Fight
            } else {
                Screen::Intro
            },
            intro: Intro::new(),
            music: MusicControl::new(),
            held: HashSet::new(),
            bars: Vec::new(),
            fight_ticks: 0,
            sound: SoundState::Waiting,
            bindings: KeyBindings::WASD,
            controls: ControlsPage::new(),
            view: View::Side,
            camera: tps::overview(),
            select: SelectPage::new(character),
            practise_after_select: false,
            touches: Vec::new(),
            touch_mode: false,
            touch_controls: true,
            tapped_character: None,
            gamepads: gamepad::GamepadState::new(),
            client,
        };
        game.music.set_mood(Mood::Intro);
        game
    }

    /// The primary client: the online one, or the local one of a practice game.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// The primary client, to hand it what the server sent.
    pub fn client_mut(&mut self) -> &mut Client {
        &mut self.client
    }

    /// The client played on the fight screen: the practice fight started while online,
    /// otherwise the primary client.
    pub fn active_client(&self) -> &Client {
        self.practice.as_ref().unwrap_or(&self.client)
    }

    fn active_mut(&mut self) -> &mut Client {
        self.practice.as_mut().unwrap_or(&mut self.client)
    }

    /// Whether an online player is practising while the room stays open.
    pub fn is_practising_online(&self) -> bool {
        self.practice.is_some()
    }

    /// Where practice stands: the rival of a practice game or of a practice fight played
    /// while online, whether one could start, or nothing of the sort for now.
    pub fn practice(&self) -> Practice {
        match (self.client.rival(), self.practice.as_ref()) {
            (Some(rival), _) => Practice::Alone(rival),
            (None, Some(client)) => {
                Practice::WhileWaiting(client.rival().expect("a practice client has a rival"))
            }
            (None, None) if self.client.status() == Status::Lobby => Practice::Off,
            (None, None) => Practice::Unavailable,
        }
    }

    /// Carries out a practice choice (what the practice row and [`PRACTICE_KEY`] do): switches
    /// the rival of a practice fight (the new bot takes over at once, see
    /// [`Client::set_rival`]), starts a practice fight while online, or ends it.
    pub fn set_practice(&mut self, practice: Practice) {
        match practice {
            Practice::Alone(rival) => self.client.set_rival(rival),
            Practice::WhileWaiting(rival) => match &mut self.practice {
                Some(client) => client.set_rival(rival),
                None => {
                    let mut client = Client::versus(self.client.config().clone(), rival);
                    client.set_character(self.client.character());
                    self.practice = Some(client);
                }
            },
            Practice::Off | Practice::Unavailable => self.practice = None,
        }
    }

    /// The character the player fights as ([`Character::Chalk`] until picked).
    pub fn character(&self) -> Character {
        self.client.character()
    }

    /// The skin the local player's fighter wears (see [`Game::set_skin`]).
    pub fn skin(&self) -> Skin {
        self.client.my_skin()
    }

    /// The local player's fighter wears `skin` from now on: locally it shows at once; online
    /// the server confirms it to everybody (including this browser) in the same tick, so the
    /// whole room sees the change together. Returns the frames to send to the server.
    pub fn set_skin(&mut self, skin: Skin) -> Vec<Vec<u8>> {
        if let Some(practice) = &mut self.practice {
            let player = practice.player().unwrap_or(PlayerId(0));
            practice.set_skin_of(player, skin);
        }
        self.client.set_skin(skin).into_iter().collect()
    }

    /// Fights as `character` from now on: alone (or in a practice fight) the round restarts
    /// with it at once; online it is used for the next match (see [`Client::set_character`]).
    /// Returns the frames to send to the server.
    pub fn set_character(&mut self, character: Character) -> Vec<Vec<u8>> {
        if let Some(practice) = &mut self.practice {
            practice.set_character(character);
        }
        self.client.set_character(character).into_iter().collect()
    }

    /// The select screen's state.
    pub fn select(&self) -> &SelectPage {
        &self.select
    }

    /// The fingers on the screen now, in virtual screen coordinates (see
    /// [`Viewport::to_virtual`]); the next [`Game::tick`] presses the keys of the touch
    /// controls under them. The first touch turns the touch controls on for good. A tap on a
    /// character's card of the select screen selects it.
    pub fn set_touches(&mut self, touches: &[(f32, f32)]) {
        self.touches.clear();
        self.touches.extend_from_slice(touches);
        self.touch_mode |= !touches.is_empty();
        self.tapped_character = None;
        if self.screen == Screen::Select && !touches.is_empty() {
            // A finger on a button or on the stick is not a tap on the card under it.
            self.tapped_character = touches
                .iter()
                .filter(|&&touch| self.touch_pad().keys(&[touch], &self.bindings).is_empty())
                .find_map(|&touch| self.select.card_at(touch));
        }
    }

    /// Whether the touch controls are shown: the screen was touched at least once.
    pub fn touch_mode(&self) -> bool {
        self.touch_mode
    }

    /// Whether the touch controls are drawn right now, or only the TOGGLE button that brings
    /// them back: they are hidden after the TOGGLE button (or [`TOGGLE_KEY`]) and shown the
    /// same way.
    pub fn touch_controls(&self) -> bool {
        self.touch_controls
    }

    /// Hides the touch controls when they are shown, brings them back when they are hidden.
    pub fn toggle_touch_controls(&mut self) {
        self.touch_controls = !self.touch_controls;
    }

    /// Tells the game what is held on the gamepads connected to this machine (every pad
    /// steers the same fighter, see [`GamepadState::merge`]); the next [`Game::tick`] presses
    /// the keys those buttons and sticks stand for (see [`gamepad`]).
    pub fn set_gamepad(&mut self, state: GamepadState) {
        self.gamepads = state;
    }

    /// The touch controls of the screen shown.
    pub fn touch_pad(&self) -> TouchPad {
        TouchPad::for_screen(self.screen)
    }

    /// The handle steering the music; give a clone to the audio output.
    pub fn music(&self) -> &MusicControl {
        &self.music
    }

    /// Whether the music plays, as last reported by [`Game::set_sound`].
    pub fn sound(&self) -> SoundState {
        self.sound
    }

    /// Tells the game whether the music plays, for the note on the intro.
    pub fn set_sound(&mut self, sound: SoundState) {
        self.sound = sound;
    }

    /// The keyboard layout played with ([`KeyBindings::WASD`] until changed).
    pub fn bindings(&self) -> &KeyBindings {
        &self.bindings
    }

    /// Plays with `bindings` from now on (e.g. a layout saved by the platform).
    pub fn set_bindings(&mut self, bindings: KeyBindings) {
        self.bindings = bindings;
    }

    /// The controls page's state.
    pub fn controls(&self) -> &ControlsPage {
        &self.controls
    }

    /// Whether the fight is paused: the pause menu (the controls page) is shown over it.
    pub fn is_paused(&self) -> bool {
        self.screen == Screen::Controls && self.controls.is_paused()
    }

    /// The screen shown.
    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// The intro's animation state.
    pub fn intro(&self) -> &Intro {
        &self.intro
    }

    /// The health bars of the fighters of the round shown, in roster order.
    pub fn health_bars(&self) -> &[HealthBar] {
        &self.bars
    }

    /// How the fight screen shows the fight.
    pub fn view(&self) -> View {
        self.view
    }

    /// Shows the fight as `view` (what [`VIEW_KEY`] does).
    pub fn set_view(&mut self, view: View) {
        self.view = view;
    }

    /// The camera of the third-person view.
    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// One fixed tick with the keys held now. Returns the frames to send to the server, in
    /// order.
    ///
    /// On the intro, a fresh press of Enter or Escape (once [`Intro::can_skip`]) goes to the
    /// select screen, a fresh `T` ([`PRACTICE_KEY`]) too but to go practising afterwards
    /// (online: a practice fight starts if one can), and a fresh `C` goes to the controls page
    /// (see [`ControlsPage::tick`]), which comes back to the intro.
    ///
    /// On the select screen (see [`SelectPage::tick`]), Enter picks the character
    /// ([`Game::set_character`]) and goes to the fight screen, Escape back to the intro.
    ///
    /// On the fight screen, a fresh `Tab` ([`VIEW_KEY`]) switches the [`View`] (the fight goes
    /// on). A fresh `P` ([`PAUSE_KEY`]) pauses: the controls page opens as the
    /// pause menu and comes back to the fight. While paused, a local match is not ticked (it
    /// freezes), and an online one is ticked with no input (it cannot wait); the practice
    /// choice made on the menu is carried out at once ([`Game::set_practice`]). Otherwise a
    /// fresh `T` goes to the next practice choice ([`Practice::next`]: starts a practice fight
    /// between online matches, or switches its rival), a fresh Escape leaves an online practice
    /// fight, a fresh Enter tells [`Client::ready`], and the keys become this tick's [`Input`]
    /// through the [bindings](Game::bindings) for [`Client::tick`].
    pub fn tick(&mut self, pressed: &HashSet<Key>) -> Vec<Vec<u8>> {
        // The fingers and the pads press keys like the keyboard does.
        let mut pressed = pressed.clone();
        pressed.extend(self.touch_pad().keys(&self.touches, &self.bindings));
        pressed.extend(self.gamepads.keys(&self.bindings, self.screen));
        let pressed = &pressed;
        let mut fresh: Vec<Key> = pressed.difference(&self.held).copied().collect();
        fresh.sort_unstable();
        let mut outgoing = Vec::new();
        // A finger on the toggle button, or a press of its key, flips the touch controls.
        if self.touch_mode
            && (self.touch_pad().toggled(&self.touches) || pressed.contains(&TOGGLE_KEY))
        {
            self.toggle_touch_controls();
        }
        match self.screen {
            Screen::Intro => {
                self.intro.tick();
                if fresh.contains(&CONTROLS_KEY) {
                    self.controls = self.controls_page(ControlsPage::new());
                    self.screen = Screen::Controls;
                } else if self.intro.can_skip() && fresh.contains(&PRACTICE_KEY) {
                    self.open_select(true);
                } else if self.intro.can_skip()
                    && (fresh.contains(&Key::Enter) || fresh.contains(&Key::Escape))
                {
                    self.open_select(false);
                }
            }
            Screen::Select => match self
                .select_tapped()
                .or_else(|| Some(self.select.tick(&fresh)))
                .unwrap_or(Choice::Browsing)
            {
                Choice::Browsing => {}
                Choice::Back => self.screen = Screen::Intro,
                Choice::Picked(character) => {
                    outgoing.extend(self.set_character(character));
                    if self.practise_after_select && self.practice() == Practice::Off {
                        self.set_practice(Practice::Off.next());
                    }
                    self.screen = Screen::Fight;
                }
            },
            Screen::Controls => {
                let paused = self.controls.is_paused();
                if self.controls.tick(&fresh, &mut self.bindings) {
                    self.screen = if paused { Screen::Fight } else { Screen::Intro };
                }
                if self.controls.character() != self.character() {
                    outgoing.extend(self.set_character(self.controls.character()));
                }
                if self.controls.practice() != self.practice() {
                    self.set_practice(self.controls.practice());
                }
                if self.controls.skin() != self.client.my_skin() {
                    outgoing.extend(self.set_skin(self.controls.skin()));
                }
                if paused {
                    self.stand_still(&mut outgoing);
                }
            }
            Screen::Fight => {
                if fresh.contains(&VIEW_KEY) {
                    self.view = self.view.next();
                }
                if fresh.contains(&PAUSE_KEY) {
                    self.controls = self.controls_page(ControlsPage::paused());
                    self.screen = Screen::Controls;
                    self.stand_still(&mut outgoing);
                } else {
                    self.fight_ticks = self.fight_ticks.saturating_add(1);
                    self.fight(pressed, &fresh, &mut outgoing);
                    if self.view == View::ThirdPerson {
                        let aim = tps::aim(self.active_client());
                        self.camera.approach(&aim, tps::CAMERA_LAG);
                    }
                }
            }
        }
        self.held.clone_from(pressed);
        self.update_bars();
        self.update_music();
        outgoing
    }

    /// The choice a tap on the select screen makes: a tap on a character's card selects it
    /// (and picks it, like Enter, if the card was already the chosen one).
    fn select_tapped(&mut self) -> Option<Choice> {
        let (tapped, keep) = (self.tapped_character, self.select.selected());
        self.tapped_character = None;
        tapped.map(|character| {
            self.select.set_selected(character);
            if character == keep {
                Choice::Picked(character)
            } else {
                Choice::Browsing
            }
        })
    }

    /// `page` showing the current character and practice choice.
    fn controls_page(&self, page: ControlsPage) -> ControlsPage {
        let player = self.client.player().unwrap_or(PlayerId(0));
        page.with_practice(self.practice())
            .with_character(self.character())
            .with_player(player)
            .with_skin(self.client.my_skin(), self.client.styles())
    }

    /// Opens the select screen on the current character; `practise` remembers that it was
    /// opened with [`PRACTICE_KEY`].
    fn open_select(&mut self, practise: bool) {
        self.select = SelectPage::new(self.character());
        self.practise_after_select = practise;
        self.screen = Screen::Select;
    }

    fn fight(&mut self, pressed: &HashSet<Key>, fresh: &[Key], outgoing: &mut Vec<Vec<u8>>) {
        if self.client.status() == Status::Playing {
            // A real match started (the player was ready before practising): it wins.
            self.practice = None;
        }
        if fresh.contains(&PRACTICE_KEY) {
            self.set_practice(self.practice().next());
        } else if fresh.contains(&Key::Escape) {
            self.practice = None;
        }
        if fresh.contains(&Key::Enter) {
            outgoing.extend(self.active_mut().ready());
        }
        let input = self.bindings.to_input(pressed);
        outgoing.extend(self.active_mut().tick(input));
    }

    /// One paused tick. A local match (a practice game, or a practice fight while online) is
    /// simply not ticked, so it freezes; an online match cannot wait for one player, so it
    /// goes on with the paused player standing still.
    fn stand_still(&mut self, outgoing: &mut Vec<Vec<u8>>) {
        if !self.client.is_local() {
            outgoing.extend(self.client.tick(Input::NONE));
        }
    }

    /// The round on screen, if a match was started.
    pub fn shown_match(&self) -> Option<&Match> {
        self.active_client()
            .session()
            .map(|session| session.current())
    }

    fn update_bars(&mut self) {
        let active = self.practice.as_ref().unwrap_or(&self.client);
        let Some(current) = active.session().map(|s| s.current()) else {
            self.bars.clear();
            return;
        };
        let fighters = current.fighters();
        let same_players = self.bars.len() == fighters.len()
            && self
                .bars
                .iter()
                .zip(fighters)
                .all(|(b, f)| b.player() == f.player());
        if !same_players {
            self.bars = fighters
                .iter()
                .map(|f| HealthBar::new(f.player(), f.health()))
                .collect();
        }
        for (bar, fighter) in self.bars.iter_mut().zip(fighters) {
            bar.update(fighter.health());
            bar.count(fighter.combo_hits());
        }
    }

    /// The music follows the screen: intro theme, full groove while fighting (tenser as the
    /// round gets closer to a knockout), breakdown between rounds and while waiting.
    fn update_music(&self) {
        let status = self.active_client().status();
        let (mood, tension) = match (self.screen, status, self.shown_match()) {
            (Screen::Controls, _, _) if self.controls.is_paused() => (Mood::Breakdown, 0.0),
            (Screen::Intro | Screen::Select | Screen::Controls, _, _) => (Mood::Intro, 0.0),
            (Screen::Fight, Status::Playing | Status::Watching, Some(current)) => {
                match current.phase() {
                    Phase::Countdown { .. } => (Mood::Fight, 0.0),
                    Phase::Fighting => (Mood::Fight, intensity(current)),
                    Phase::Finished(_) => (Mood::Breakdown, 0.0),
                }
            }
            (Screen::Fight, _, _) => (Mood::Breakdown, 0.0),
        };
        self.music.set_mood(mood);
        self.music.set_intensity(tension);
    }

    /// One line about the connection, shown under the intro.
    pub fn status_line(&self) -> String {
        if let Some(rival) = self.client.rival() {
            return format!(
                "practice mode: versus {} - T or the pause menu: change the rival",
                rival.name()
            );
        }
        match self.client.status() {
            Status::Connecting => "connecting to the server...".to_string(),
            Status::Watching => "spectating the room - the fights show up on their own".to_string(),
            Status::Lobby if self.client.seated().len() <= 1 => {
                "connected - nobody else is here yet".to_string()
            }
            Status::Lobby => format!(
                "{} players in the room - press Enter to fight",
                self.client.seated().len()
            ),
            Status::Playing => "a match is running - press Enter".to_string(),
            Status::RoomFull => {
                format!("the room is full ({MAX_PLAYERS} players) - try again later")
            }
            Status::Disconnected => "disconnected from the server - reload the page".to_string(),
            Status::Failed(error) => format!("error: {error}"),
        }
    }

    /// The player controlled here, when a match is running (`None` for a spectator).
    pub fn local_player(&self) -> Option<PlayerId> {
        self.active_client()
            .session()
            .and_then(|s| s.local_player())
    }

    /// The small line under the banner about practice, if any: the rival and how to switch it,
    /// how to start practising between online matches, or who is waiting in the room.
    ///
    /// With the touch controls on, it names their buttons (RIVAL, BACK) instead of the keys.
    pub fn practice_notice(&self) -> Option<String> {
        let (rival_key, back_key) = if self.touch_mode {
            ("RIVAL", "BACK")
        } else {
            ("T", "Esc")
        };
        let switch = |rival: Rival| {
            format!(
                "Practice: {}   {rival_key}: switch to {}",
                rival.name(),
                rival.next().name()
            )
        };
        match self.practice() {
            Practice::Unavailable => None,
            Practice::Off => Some(format!("{rival_key}: practice while waiting")),
            Practice::Alone(rival) => Some(switch(rival)),
            Practice::WhileWaiting(rival) => {
                let others: Vec<String> = self
                    .client
                    .seated()
                    .iter()
                    .filter(|&player| Some(player) != self.client.player())
                    .map(|player| format!("P{}", u32::from(player.0) + 1))
                    .collect();
                Some(if others.is_empty() {
                    format!("{}   {back_key}: back to the room", switch(rival))
                } else {
                    format!(
                        "{} is here - {back_key}: leave the practice",
                        others.join(", ")
                    )
                })
            }
        }
    }

    /// What to draw now: the screen, and over it the touch controls once the screen was
    /// touched (they replace the keyboard reminder of the fight screen).
    pub fn frame(&self) -> Frame {
        let mut frame = self.screen_frame();
        if self.touch_mode {
            if self.touch_controls {
                if self.screen == Screen::Fight {
                    frame.labels.retain(|label| label.y < hud::CONTROLS_Y);
                    // The practice notice moves up, under the top buttons, clear of the others.
                    for label in &mut frame.labels {
                        if label.y == hud::NOTICE_Y {
                            label.y = touch::NOTICE_Y;
                        }
                    }
                }
                self.touch_pad().draw(&mut frame, &self.touches);
            } else {
                // Only the TOGGLE button stays, to bring the controls back.
                self.touch_pad().draw_toggle(&mut frame, &self.touches);
            }
        }
        frame
    }

    fn screen_frame(&self) -> Frame {
        match self.screen {
            Screen::Intro => {
                let mut frame = Frame::new(intro::BACKGROUND);
                let color = if self.sound == SoundState::Waiting {
                    SOUND_WAITING
                } else {
                    SOUND_OTHER
                };
                frame.label(
                    self.sound.note(),
                    SOUND_NOTE_X,
                    SOUND_NOTE_Y,
                    SOUND_NOTE_SIZE,
                    color,
                    Align::Right,
                );
                frame.label(
                    CONTROLS_HINT,
                    CONTROLS_HINT_X,
                    SOUND_NOTE_Y,
                    SOUND_NOTE_SIZE,
                    CONTROLS_HINT_COLOR,
                    Align::Left,
                );
                self.intro.draw(&mut frame, &self.status_line());
                frame
            }
            Screen::Select => {
                let mut frame = Frame::new(intro::BACKGROUND);
                self.select.draw(&mut frame);
                frame
            }
            Screen::Controls => {
                let mut frame = Frame::new(intro::BACKGROUND);
                self.controls.draw(&mut frame, &self.bindings);
                frame
            }
            Screen::Fight => {
                let client = self.active_client();
                let mut frame = match self.view {
                    View::Side => {
                        let mut frame = Frame::new(arena::BACKGROUND);
                        arena::draw(&mut frame, client, self.fight_ticks);
                        frame
                    }
                    View::ThirdPerson => {
                        let mut frame = Frame::new(tps::BACKGROUND);
                        tps::draw(&mut frame, client, &self.camera, self.fight_ticks);
                        frame
                    }
                };
                hud::draw(
                    &mut frame,
                    client,
                    &self.bars,
                    self.fight_ticks,
                    &self.bindings,
                );
                if let Some(notice) = self.practice_notice() {
                    hud::draw_notice(&mut frame, &notice);
                }
                frame
            }
        }
    }
}
