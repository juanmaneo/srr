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
//!    `C` opens the controls page;
//! 2. [`Screen::Controls`]: choose a keyboard preset or remap any action ([`controls`]); it is
//!    also the pause menu of the fight screen (`P`), going back to the fight;
//! 3. [`Screen::Fight`]: the arena ([`arena`]) under the versus HUD ([`hud`]): health bars with
//!    the red "recoverable damage" trail, round wins, timer, grapple meter and banners.
//!
//! On the fight screen, Enter means "ready": online, the server starts the next match (1v1,
//! 1v1v1 or 1v1v1v1) once every seated player is ready; alone, it starts the next round.
//! `P` pauses: a local match (the dummy or the bot) freezes until the pause menu is left; an
//! online match cannot wait for one player, so it goes on with the paused player standing
//! still.
//!
//! A player who is alone (practice mode, or nobody else in the online room) is *offered* a
//! match against the computer and has to confirm it: Y fights the [bot](crate::game::Bot), N
//! keeps the training dummy (or keeps waiting online, where Y still accepts later). Online, the
//! bot match is played locally while the room stays open, and Escape leaves it.
//!
//! [`Headless`] plays the [`Game`] without any engine: scripted keys in, frames rendered in
//! memory by the software renderer of [`render`] (pixels, PNG files, text snapshots). That is
//! how the tests look at the screens without opening a window.

use std::collections::HashSet;

use crate::audio::{Mood, MusicControl};
use crate::game::{
    GrapplePosition, Input, Key, KeyBindings, Match, Phase, PlayerId, TICKS_PER_SECOND,
};
use crate::net::{Client, MAX_PLAYERS, Rival, Status};

pub mod arena;
pub mod ascii;
pub mod controls;
pub mod headless;
pub mod hud;
pub mod intro;
pub mod render;
mod scene;
pub mod stick;

pub use controls::{ControlsPage, PAUSE_KEY};
pub use headless::Headless;
pub use hud::HealthBar;
pub use intro::Intro;
pub use scene::{
    Align, Frame, Label, MONO_ADVANCE, Rect, Rgba, SCREEN_HEIGHT, SCREEN_WIDTH, Viewport,
};

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

/// The hint to the controls page, in the top-left corner of the intro.
pub const CONTROLS_HINT: &str = "C: controls / keyboard layout";
const CONTROLS_HINT_X: f32 = 24.0;
const CONTROLS_HINT_COLOR: Rgba = Rgba::hex(0xd8d0e8);

/// The question asked on the fight screen to a player who is alone.
pub const BOT_PROMPT: &str = "Nobody else is here. Fight the bot?  Y: yes  N: no";

/// Whether the player who is alone has answered the offer of a match against the computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Offer {
    /// Not answered: the question is asked while the player is alone.
    Open,
    /// Answered no; online, Y still accepts while the player is alone.
    Declined,
}

/// What the game is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    /// The graffiti title.
    Intro,
    /// The keyboard layout.
    Controls,
    /// The arena and the HUD.
    Fight,
}

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
    let max = current.config().max_health.max(1) as f32;
    let danger = current
        .fighters()
        .iter()
        .filter(|f| !f.is_knocked_out())
        .map(|f| 1.0 - f.health() as f32 / max)
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
    /// A match against the computer, accepted while alone in the online room.
    bot: Option<Client>,
    offer: Offer,
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
}

impl Game {
    /// A game starting on the intro, playing with `client`.
    pub fn new(client: Client) -> Self {
        let game = Game {
            client,
            bot: None,
            offer: Offer::Open,
            screen: Screen::Intro,
            intro: Intro::new(),
            music: MusicControl::new(),
            held: HashSet::new(),
            bars: Vec::new(),
            fight_ticks: 0,
            sound: SoundState::Waiting,
            bindings: KeyBindings::STANDARD,
            controls: ControlsPage::new(),
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

    /// The client played on the fight screen: the match against the computer accepted while
    /// alone online, otherwise the primary client.
    pub fn active_client(&self) -> &Client {
        self.bot.as_ref().unwrap_or(&self.client)
    }

    fn active_mut(&mut self) -> &mut Client {
        self.bot.as_mut().unwrap_or(&mut self.client)
    }

    /// Whether the player is alone: practising against the dummy, or online with nobody else
    /// in the room.
    pub fn is_alone(&self) -> bool {
        match self.client.rival() {
            Some(Rival::Dummy) => true,
            Some(Rival::Bot) => false,
            None => self.client.status() == Status::Lobby && self.client.seated().len() <= 1,
        }
    }

    /// Whether the fight screen asks the player who is alone to fight the computer
    /// ([`BOT_PROMPT`]); the game waits for the answer.
    pub fn is_offering_bot(&self) -> bool {
        self.screen == Screen::Fight
            && self.bot.is_none()
            && self.offer == Offer::Open
            && self.is_alone()
    }

    /// Whether an online player is playing the computer while the room stays open.
    pub fn is_playing_bot_online(&self) -> bool {
        self.bot.is_some()
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

    /// The keyboard layout played with ([`KeyBindings::STANDARD`] until changed).
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

    /// One fixed tick with the keys held now. Returns the frames to send to the server, in
    /// order.
    ///
    /// On the intro, a fresh press of Enter or Escape (once [`Intro::can_skip`]) goes to the
    /// fight screen, and a fresh `C` to the controls page (see [`ControlsPage::tick`]), which
    /// comes back to the intro.
    ///
    /// On the fight screen, a fresh `P` ([`PAUSE_KEY`]) pauses: the controls page opens as the
    /// pause menu and comes back to the fight. While paused, a local match is not ticked (it
    /// freezes), and an online one is ticked with no input (it cannot wait). Otherwise, while
    /// [the bot is offered](Game::is_offering_bot), a fresh Y accepts and a fresh N declines;
    /// nothing else happens until then. Otherwise a fresh Y accepts a declined offer while
    /// alone online, a fresh Escape leaves an online match against the computer, a fresh Enter
    /// tells [`Client::ready`], and the keys become this tick's [`Input`] through the
    /// [bindings](Game::bindings) for [`Client::tick`].
    pub fn tick(&mut self, pressed: &HashSet<Key>) -> Vec<Vec<u8>> {
        let mut fresh: Vec<Key> = pressed.difference(&self.held).copied().collect();
        fresh.sort_unstable();
        let mut outgoing = Vec::new();
        match self.screen {
            Screen::Intro => {
                self.intro.tick();
                if fresh.contains(&Key::C) {
                    self.controls = ControlsPage::new();
                    self.screen = Screen::Controls;
                } else if self.intro.can_skip()
                    && (fresh.contains(&Key::Enter) || fresh.contains(&Key::Escape))
                {
                    self.screen = Screen::Fight;
                }
            }
            Screen::Controls => {
                let paused = self.controls.is_paused();
                if self.controls.tick(&fresh, &mut self.bindings) {
                    self.screen = if paused { Screen::Fight } else { Screen::Intro };
                }
                if paused {
                    self.stand_still(&mut outgoing);
                }
            }
            Screen::Fight => {
                if fresh.contains(&PAUSE_KEY) {
                    self.controls = ControlsPage::paused();
                    self.screen = Screen::Controls;
                    self.stand_still(&mut outgoing);
                } else {
                    self.fight_ticks = self.fight_ticks.saturating_add(1);
                    self.fight(pressed, &fresh, &mut outgoing);
                }
            }
        }
        self.held.clone_from(pressed);
        self.update_bars();
        self.update_music();
        outgoing
    }

    fn fight(&mut self, pressed: &HashSet<Key>, fresh: &[Key], outgoing: &mut Vec<Vec<u8>>) {
        if !self.is_alone() {
            self.offer = Offer::Open;
        }
        if self.client.status() == Status::Playing {
            // A real match started (the player was ready before accepting the bot): it wins.
            self.bot = None;
        }
        let yes = fresh.contains(&Key::Y);
        if self.is_offering_bot() {
            if yes {
                self.accept_bot();
            } else if fresh.contains(&Key::N) {
                self.offer = Offer::Declined;
            }
            return;
        }
        if yes && self.bot.is_none() && !self.client.is_local() && self.is_alone() {
            self.accept_bot();
        }
        if fresh.contains(&Key::Escape) && self.bot.take().is_some() {
            self.offer = Offer::Declined;
        }
        if fresh.contains(&Key::Enter) {
            outgoing.extend(self.active_mut().ready());
        }
        let input = self.bindings.to_input(pressed);
        outgoing.extend(self.active_mut().tick(input));
    }

    /// One paused tick. A local match (the dummy, or the bot, online too) is simply not
    /// ticked, so it freezes; an online match cannot wait for one player, so its lockstep goes
    /// on with the paused player standing still.
    fn stand_still(&mut self, outgoing: &mut Vec<Vec<u8>>) {
        if !self.client.is_local() {
            outgoing.extend(self.client.tick(Input::NONE));
        }
    }

    /// A match against the computer: it replaces the practice dummy, or is played locally
    /// while the online room stays open.
    fn accept_bot(&mut self) {
        let versus = Client::versus_bot(self.client.config().clone());
        if self.client.is_local() {
            self.client = versus;
        } else {
            self.bot = Some(versus);
        }
    }

    /// The round on screen, if a match was started.
    pub fn shown_match(&self) -> Option<&Match> {
        self.active_client()
            .session()
            .map(|session| session.current())
    }

    fn update_bars(&mut self) {
        let active = self.bot.as_ref().unwrap_or(&self.client);
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
        }
    }

    /// The music follows the screen: intro theme, full groove while fighting (tenser as the
    /// round gets closer to a knockout), breakdown between rounds and while waiting.
    fn update_music(&self) {
        let status = self.active_client().status();
        let (mood, tension) = match (self.screen, status, self.shown_match()) {
            (Screen::Controls, _, _) if self.controls.is_paused() => (Mood::Breakdown, 0.0),
            (Screen::Intro | Screen::Controls, _, _) => (Mood::Intro, 0.0),
            (Screen::Fight, Status::Playing, Some(current)) => match current.phase() {
                Phase::Countdown { .. } => (Mood::Fight, 0.0),
                Phase::Fighting => (Mood::Fight, intensity(current)),
                Phase::Finished(_) => (Mood::Breakdown, 0.0),
            },
            (Screen::Fight, _, _) => (Mood::Breakdown, 0.0),
        };
        self.music.set_mood(mood);
        self.music.set_intensity(tension);
    }

    /// One line about the connection, shown under the intro.
    pub fn status_line(&self) -> String {
        match self.client.rival() {
            Some(Rival::Dummy) => {
                return "practice mode: a training dummy (or the computer) is waiting".to_string();
            }
            Some(Rival::Bot) => return "practice mode: versus the computer".to_string(),
            None => {}
        }
        match self.client.status() {
            Status::Connecting => "connecting to the server...".to_string(),
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

    /// The player controlled here, when a match is running.
    pub fn local_player(&self) -> Option<PlayerId> {
        self.active_client().session().map(|s| s.local_player())
    }

    /// The small line under the banner about the match against the computer, if any.
    pub fn bot_notice(&self) -> Option<String> {
        if self.bot.is_some() {
            let others: Vec<String> = self
                .client
                .seated()
                .iter()
                .filter(|&player| Some(player) != self.client.player())
                .map(|player| format!("P{}", u32::from(player.0) + 1))
                .collect();
            return Some(if others.is_empty() {
                "Versus the computer - Esc: back to the room".to_string()
            } else {
                format!("{} is here - Esc: leave the bot", others.join(", "))
            });
        }
        (self.offer == Offer::Declined && !self.client.is_local() && self.is_alone())
            .then(|| "Y: fight the bot while waiting".to_string())
    }

    /// What to draw now.
    pub fn frame(&self) -> Frame {
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
            Screen::Controls => {
                let mut frame = Frame::new(intro::BACKGROUND);
                self.controls.draw(&mut frame, &self.bindings);
                frame
            }
            Screen::Fight => {
                let mut frame = Frame::new(arena::BACKGROUND);
                let client = self.active_client();
                arena::draw(&mut frame, client, self.fight_ticks);
                hud::draw(
                    &mut frame,
                    client,
                    &self.bars,
                    self.fight_ticks,
                    &self.bindings,
                );
                if let Some(notice) = self.bot_notice() {
                    hud::draw_notice(&mut frame, &notice);
                }
                if self.is_offering_bot() {
                    hud::draw_prompt(&mut frame, BOT_PROMPT);
                }
                frame
            }
        }
    }
}
