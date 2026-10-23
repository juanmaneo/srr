//! The engine-agnostic front end, part 2: the fight screen — arena (`srr::front::arena`),
//! versus HUD (`srr::front::hud`) — and the client-side [`Game`] state machine with its music
//! cues.
//!
//! Duels are driven through an online [`Client`] with no input delay: every tick the test hands
//! it the server's frame (both fighters' inputs), then ticks it with the local input.
//! That way a test controls both fighters and can reach any state: strikes, grabs, every
//! grapple position, knockouts, time-ups, next rounds, stalls and every connection status.
//! Bigger matches (up to ten players), the lobby and practice (the dummy, the computer, and
//! practising while the online room stays open) are covered too.
//!
//! Numbers of [`cfg`]: the arena is 300 wide, so the fighters spawn at 100 and 200, inside the
//! reach (150) of every strike and of the grab. Strikes: 2 start-up, 2 active, 2 recovery ticks,
//! 10 damage, no knockback, 3 ticks of hit-stun. Grab: 2 start-up, 1 active (it connects on the
//! third tick), 3 recovery ticks; a position advances after 3 ticks; escaping is out of reach.

use std::collections::HashSet;

use srr::audio::Mood;
use srr::front::arena::{self, ARENA_LEFT, ARENA_PIXELS, FLOOR_Y, LYING_TAG_Y, TAG_ABOVE};
use srr::front::controls::{Practice, Row, controls_lines, technique_keys};
use srr::front::hud::{
    self, COMBO_LINGER, COMBO_MIN_HITS, CONTROLS_LINE_HEIGHT, CONTROLS_Y, DANGER_PERCENT, NOTICE_Y,
    STALL_TEXT, STALL_WARNING_TICKS, TRAIL_HOLD, TRAIL_SPEED, bar_slots, lobby_text, position_name,
    ready_text,
};
use srr::front::intro::{self, SKIP_AFTER};
use srr::front::stick::{
    self, Accessory, Bone, Figure, GRAB, JUDO, LINE_WIDTH, MAX_REACH, Mark, Point, SPARK,
    SUBMISSION, SWEEP_LIFT, THROW_LIFT,
};
use srr::front::{
    Align, CONTROLS_HINT, Frame, Game, HealthBar, Label, PRACTICE_KEY, Rect, Rgba, SCREEN_WIDTH,
    Screen, SoundState, intensity, seconds,
};
use srr::game::{
    Attack, AttackData, AttackPhase, Character, FighterState, GlitchSet, GrappleData,
    GrapplePosition, GrappleRole, Input, Key, KeyBindings, Match, MatchConfig, Outcome, Phase,
    PlayerId, Rival, Roster, Strength, TICKS_PER_SECOND, Technique,
};
use srr::net::{
    Client, ClientError, ClientMessage, MAX_PLAYERS, MAX_ROLLBACK, PlayerSet, ServerMessage, Status,
};

// ---------------------------------------------------------------------------------------------
// Helpers: configuration and inputs
// ---------------------------------------------------------------------------------------------

/// The session number the fake server hands out.
const SESSION: u8 = 7;

const STRIKE: AttackData = AttackData {
    startup: 2,
    active: 2,
    recovery: 2,
    range: 150,
    damage: 10,
    knockback: 0,
    hitstun: 3,
};

/// See the module documentation.
fn cfg() -> MatchConfig {
    MatchConfig {
        arena_width: 300,
        max_health: 100,
        punches: [STRIKE; 3],
        kicks: [STRIKE; 3],
        round_time_ticks: 20 * TICKS_PER_SECOND,
        countdown_ticks: 0,
        glitches: GlitchSet::none(),
        grapple: GrappleData {
            startup: 2,
            active: 1,
            recovery: 3,
            range: 150,
            advance_ticks: 3,
            max_hold_ticks: 10_000,
            escape_presses: [1000; 3],
            ..GrappleData::default()
        },
        ..MatchConfig::default()
    }
}

/// [`cfg`] where a single strike knocks out.
fn fragile() -> MatchConfig {
    MatchConfig {
        max_health: 10,
        ..cfg()
    }
}

/// [`cfg`] where a punch stuns for 6 ticks: long enough for a chained punch (which lands 3
/// ticks after the chain) to combo.
fn stunning() -> MatchConfig {
    MatchConfig {
        punches: [AttackData {
            hitstun: 6,
            ..STRIKE
        }; 3],
        ..cfg()
    }
}

/// [`cfg`] with rounds of `ticks` fighting ticks.
fn short_rounds(ticks: u32) -> MatchConfig {
    MatchConfig {
        round_time_ticks: ticks,
        ..cfg()
    }
}

fn hold(input: Input) -> Input {
    input
}

fn idle() -> Input {
    Input::NONE
}

fn punch() -> Input {
    hold(Input::pressing(Attack::punch(Strength::Low)))
}

fn kick() -> Input {
    hold(Input::pressing(Attack::kick(Strength::High)))
}

fn grab() -> Input {
    hold(Input {
        grab: true,
        ..Input::NONE
    })
}

fn jump() -> Input {
    hold(Input {
        jump: true,
        ..Input::NONE
    })
}

fn crouch() -> Input {
    hold(Input {
        down: true,
        ..Input::NONE
    })
}

fn left() -> Input {
    hold(Input {
        left: true,
        ..Input::NONE
    })
}

fn right() -> Input {
    hold(Input {
        right: true,
        ..Input::NONE
    })
}

fn keys(list: &[Key]) -> HashSet<Key> {
    list.iter().copied().collect()
}

// ---------------------------------------------------------------------------------------------
// Helpers: driving duels
// ---------------------------------------------------------------------------------------------

/// An online client controlling `local` in a match between `players`, just started.
fn brawl(config: MatchConfig, local: u8, players: PlayerSet) -> Client {
    let mut client = Client::online(config, 0);
    client.handle(ServerMessage::Welcome {
        player: PlayerId(local),
    });
    client.handle(ServerMessage::Lobby {
        seated: players,
        ready: PlayerSet::EMPTY,
    });
    client.handle(ServerMessage::Start {
        session: SESSION,
        players,
        characters: [Character::Chalk; 10],
    });
    assert_eq!(client.status(), Status::Playing);
    client
}

/// An online client controlling `local`, whose duel has just started.
fn duel(config: MatchConfig, local: u8) -> Client {
    brawl(config, local, PlayerSet::first(2))
}

/// Hands the client the server's next frame: what each player of `held` holds (the others
/// hold nothing), with the players of `late` replaced by the server.
fn relay_frame(client: &mut Client, held: &[(PlayerId, Input)], late: PlayerSet) {
    let session = client.session().expect("a match runs");
    let mut inputs = [Input::NONE; PlayerSet::CAPACITY];
    for &(player, input) in held {
        inputs[usize::from(player.0)] = input;
    }
    client.handle(ServerMessage::Frame {
        session: session.id(),
        tick: session.confirmed_tick(),
        players: session.players(),
        late,
        inputs,
    });
}

/// One tick of the duel with what players 0 and 1 hold. The server's frame comes first, so
/// the round shown is exact (nothing is guessed, and the local input is not sent again).
fn play(client: &mut Client, p0: Input, p1: Input) {
    let local = client.session().expect("a duel runs").local_player();
    let mine = if local == PlayerId(0) { p0 } else { p1 };
    relay_frame(
        client,
        &[(PlayerId(0), p0), (PlayerId(1), p1)],
        PlayerSet::EMPTY,
    );
    let before = client.session().unwrap().tick();
    client.tick(mine);
    assert_eq!(client.session().unwrap().tick(), before + 1, "the tick ran");
}

/// The server starts the next match between the same players.
fn next_round(client: &mut Client) {
    let session = client.session().expect("a match ran");
    let (id, players) = (session.id().wrapping_add(1), session.players());
    client.handle(ServerMessage::Start {
        session: id,
        players,
        characters: [Character::Chalk; 10],
    });
}

/// One tick where only `player` holds `input`.
fn play_as(client: &mut Client, player: u8, input: Input) {
    if player == 0 {
        play(client, input, idle());
    } else {
        play(client, idle(), input);
    }
}

fn idle_for(client: &mut Client, ticks: u32) {
    for _ in 0..ticks {
        play(client, idle(), idle());
    }
}

fn current(client: &Client) -> &Match {
    client.session().expect("a duel runs").current()
}

fn state(client: &Client, player: u8) -> FighterState {
    current(client).fighter(PlayerId(player)).unwrap().state()
}

fn health(client: &Client, player: u8) -> i32 {
    current(client).fighter(PlayerId(player)).unwrap().health()
}

const fn grappling(role: GrappleRole, position: GrapplePosition) -> FighterState {
    FighterState::Grappling(role, position)
}

/// `top` grabs the other fighter: two start-up ticks, then the grab connects.
fn clinch(client: &mut Client, top: u8) {
    for _ in 0..3 {
        play_as(client, top, grab());
    }
    assert_eq!(
        state(client, top),
        grappling(GrappleRole::Top, GrapplePosition::Clinch)
    );
    assert_eq!(
        state(client, 1 - top),
        grappling(GrappleRole::Bottom, GrapplePosition::Clinch)
    );
}

/// Waits three ticks, then `top` freshly presses grab: one rung up the ladder.
fn climb(client: &mut Client, top: u8) {
    idle_for(client, 3);
    play_as(client, top, grab());
}

/// `winner` wins `rounds` rounds by knockout (with [`fragile`]); the last one stays finished.
fn win_rounds(client: &mut Client, winner: u8, rounds: u32) {
    for round in 0..rounds {
        if round > 0 {
            next_round(client);
            assert_eq!(current(client).outcome(), None, "a new round started");
        }
        while current(client).outcome().is_none() {
            play_as(client, winner, punch());
        }
    }
}

/// The health bars of the fighters as they are now.
fn bars_of(client: &Client) -> Vec<HealthBar> {
    client.session().map_or_else(Vec::new, |session| {
        session
            .current()
            .fighters()
            .iter()
            .map(|f| HealthBar::new(f.player(), f.health()))
            .collect()
    })
}

/// The health bars as they are now, counting the combos of the fighters.
fn counted_bars(client: &Client) -> Vec<HealthBar> {
    let mut bars = bars_of(client);
    for (bar, fighter) in bars.iter_mut().zip(current(client).fighters()) {
        bar.count(fighter.combo_hits());
    }
    bars
}

fn hud_frame(client: &Client, bars: &[HealthBar], fight_ticks: u32) -> Frame {
    let mut frame = Frame::new(arena::BACKGROUND);
    hud::draw(
        &mut frame,
        client,
        bars,
        fight_ticks,
        &KeyBindings::STANDARD,
    );
    frame
}

fn arena_frame(client: &Client, fight_ticks: u32) -> Frame {
    let mut frame = Frame::new(arena::BACKGROUND);
    arena::draw(&mut frame, client, fight_ticks);
    frame
}

fn find_label<'a>(frame: &'a Frame, text: &str) -> Option<&'a Label> {
    frame.labels.iter().find(|l| l.text == text)
}

fn rects_of(frame: &Frame, color: Rgba) -> Vec<Rect> {
    frame
        .rects
        .iter()
        .copied()
        .filter(|r| r.color == color)
        .collect()
}

// Colours of the HUD and of the arena (private in their modules).
const HEALTH: Rgba = Rgba::hex(0xffd400);
const HEALTH_LOW: Rgba = Rgba::hex(0xff7a00);
const TRAIL: Rgba = Rgba::hex(0xe8202a);
const LOCAL_NAME: Rgba = Rgba::hex(0xfff08a);
const NAME: Rgba = Rgba::hex(0xf4f0ff);
const WIN_ON: Rgba = Rgba::hex(0xffd400);
const WIN_OFF: Rgba = Rgba::hex(0x3a3448);
const TIMER: Rgba = Rgba::hex(0xffffff);
const TIMER_LOW: Rgba = Rgba::hex(0xff4a3a);
const GRAPPLE_ON: Rgba = Rgba::hex(0x6cff8a);
const ESCAPE: Rgba = Rgba::hex(0x3fc8ff);
const BANNER: Rgba = Rgba::hex(0xffd84a);
const BANNER_SMALL: Rgba = Rgba::hex(0xf4f0ff);

const MARKER: Rgba = Rgba::hex(0xfff299);
const STUNNED: Rgba = Rgba::hex(0xffffff);
const KNOCKED_OUT: Rgba = Rgba::hex(0x595959);
const NEON: Rgba = Rgba::hex(0xff3ab4);
const SHADOW: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.45,
};

// ---------------------------------------------------------------------------------------------
// HUD: health bars
// ---------------------------------------------------------------------------------------------

#[test]
fn a_new_health_bar_is_full_without_trail() {
    let bar = HealthBar::new(PlayerId(1), 80);
    assert_eq!(bar.player(), PlayerId(1));
    assert_eq!((bar.health(), bar.trail()), (80, 80));
}

#[test]
fn the_trail_holds_after_a_hit_then_drains_down_to_the_health() {
    let mut bar = HealthBar::new(PlayerId(0), 100);
    bar.update(95);
    assert_eq!((bar.health(), bar.trail()), (95, 100));
    for _ in 0..TRAIL_HOLD {
        bar.update(95);
        assert_eq!(bar.trail(), 100, "holding");
    }
    for drained in 1..=5 {
        bar.update(95);
        assert_eq!(bar.trail(), 100 - drained * TRAIL_SPEED);
    }
    for _ in 0..10 {
        bar.update(95);
    }
    assert_eq!(bar.trail(), 95, "the trail stops at the health");
    bar.update(95);
    assert_eq!((bar.health(), bar.trail()), (95, 95));
}

#[test]
fn a_new_hit_restarts_the_hold() {
    let mut bar = HealthBar::new(PlayerId(0), 100);
    bar.update(90);
    for _ in 0..TRAIL_HOLD - 1 {
        bar.update(90);
    }
    bar.update(80);
    for _ in 0..TRAIL_HOLD {
        bar.update(80);
    }
    assert_eq!(bar.trail(), 100, "the combo kept the whole trail");
    bar.update(80);
    assert_eq!(bar.trail(), 100 - TRAIL_SPEED);
}

#[test]
fn the_combo_counter_follows_the_hits_and_lingers_once_the_fighter_recovers() {
    let mut bar = HealthBar::new(PlayerId(0), 100);
    assert_eq!(bar.hits(), 0);
    bar.count(1);
    bar.count(2);
    assert_eq!(bar.hits(), 2);
    for _ in 0..COMBO_LINGER {
        bar.count(0);
        assert_eq!(bar.hits(), 2, "lingering");
    }
    bar.count(0);
    assert_eq!(bar.hits(), 0);
    bar.count(3);
    bar.count(0);
    bar.count(1);
    assert_eq!(bar.hits(), 1, "a new combo replaces the old count at once");
}

#[test]
fn health_going_up_resets_the_trail() {
    let mut bar = HealthBar::new(PlayerId(0), 100);
    bar.update(40);
    bar.update(100);
    assert_eq!((bar.health(), bar.trail()), (100, 100));
    bar.update(40);
    bar.update(70);
    assert_eq!(
        (bar.health(), bar.trail()),
        (70, 100),
        "still below the trail"
    );
    for _ in 0..TRAIL_HOLD + 30 {
        bar.update(70);
    }
    assert_eq!(bar.trail(), 70);
}

// ---------------------------------------------------------------------------------------------
// HUD: layout, names, texts
// ---------------------------------------------------------------------------------------------

#[test]
fn a_duel_gets_two_big_mirrored_bars() {
    let slots = bar_slots(2);
    assert_eq!(slots.len(), 2);
    let (left, right) = (slots[0], slots[1]);
    assert!(left.left_side && !right.left_side);
    assert_eq!((left.y, left.height), (right.y, right.height));
    assert_eq!(left.width, right.width);
    assert!(left.x < 640.0 && right.x > 640.0);
    assert_eq!(
        left.x,
        1280.0 - right.x - right.width,
        "mirrored around the centre"
    );
}

#[test]
fn a_single_bar_goes_on_the_left() {
    let slots = bar_slots(1);
    assert_eq!(slots.len(), 1);
    assert!(slots[0].left_side);
    assert_eq!(slots[0].height, bar_slots(2)[0].height);
    assert!(bar_slots(0).is_empty());
}

#[test]
fn bigger_matches_stack_thinner_bars_on_both_sides() {
    let three = bar_slots(3);
    let sides: Vec<bool> = three.iter().map(|s| s.left_side).collect();
    assert_eq!(
        sides,
        vec![true, true, false],
        "the first half rounded up on the left"
    );
    assert!(three[0].height < bar_slots(2)[0].height);
    assert!(three[1].y > three[0].y, "stacked");
    assert_eq!(three[2].y, three[0].y, "the right column starts at the top");

    let five = bar_slots(5);
    let sides: Vec<bool> = five.iter().map(|s| s.left_side).collect();
    assert_eq!(sides, vec![true, true, true, false, false]);
    assert_eq!(five[4].y, five[1].y);
    assert!(five[0].height < three[0].height, "tinier beyond four");

    let four = bar_slots(4);
    assert_eq!(four.iter().filter(|s| s.left_side).count(), 2);
    assert!(four.iter().all(|s| s.height == three[0].height));

    let ten = bar_slots(10);
    assert_eq!(ten.iter().filter(|s| s.left_side).count(), 5);
    assert!(ten.iter().all(|s| s.height == five[0].height));
    assert_eq!(ten[9].y, ten[4].y, "five rows a side");
}

#[test]
fn names_mark_the_local_player_and_the_computer() {
    let client = duel(cfg(), 1);
    assert_eq!(hud::name(&client, PlayerId(0)), "P1");
    assert_eq!(hud::name(&client, PlayerId(1)), "P2 (you)");
    assert_eq!(hud::tag(&client, PlayerId(1)), "P2");

    let lobby = Client::online(cfg(), 0);
    assert_eq!(
        hud::name(&lobby, PlayerId(0)),
        "P1",
        "no match: nobody is you"
    );

    let versus = Client::versus(cfg(), Rival::Brawler);
    assert_eq!(hud::name(&versus, PlayerId(0)), "P1 (you)");
    assert_eq!(hud::name(&versus, PlayerId(1)), "CPU");
    assert_eq!(hud::tag(&versus, PlayerId(1)), "CPU");
    let practice = Client::practice(cfg());
    assert_eq!(hud::name(&practice, PlayerId(1)), "P2", "the dummy");
}

#[test]
fn grapple_positions_have_upper_case_names() {
    let names: Vec<&str> = GrapplePosition::ALL
        .into_iter()
        .map(position_name)
        .collect();
    assert_eq!(
        names,
        vec![
            "CLINCH",
            "MOUNT",
            "SUBMISSION",
            "JUJI SWEEP",
            "HON-GESA-GATAME",
            "IPPON-SEOI-NAGE",
            "JUJI-GATAME"
        ]
    );
    let standard = KeyBindings::STANDARD;
    assert_eq!(technique_keys(Technique::Sweep, &standard), "5+9");
    assert_eq!(technique_keys(Technique::Throw, &standard), "8+9");
}

#[test]
fn the_banner_tells_where_the_connection_is() {
    let mut client = Client::online(cfg(), 0);
    assert_eq!(
        hud::banner(&client),
        (None, "Connecting to the server...".into())
    );

    client.handle(ServerMessage::Welcome {
        player: PlayerId(0),
    });
    assert_eq!(
        hud::banner(&client),
        (
            None,
            "Nobody else is here yet\nOpen this page on other machines (up to 10 players)".into()
        )
    );

    client.handle(ServerMessage::Start {
        session: SESSION,
        players: PlayerSet::first(2),
        characters: [Character::Chalk; 10],
    });
    client.handle(ServerMessage::PlayerLeft {
        player: PlayerId(1),
    });
    client.handle(ServerMessage::Lobby {
        seated: PlayerSet::first(1),
        ready: PlayerSet::EMPTY,
    });
    assert_eq!(
        hud::banner(&client),
        (
            None,
            "P2 left\nNobody else is here yet\nOpen this page on other machines (up to 10 players)"
                .into()
        )
    );

    client.handle(ServerMessage::RoomFull);
    assert_eq!(
        hud::banner(&client),
        (
            None,
            "The room is full (10 players)\nTry again later".into()
        )
    );

    client.disconnected();
    assert_eq!(client.status(), Status::RoomFull, "room full stays");
    let mut lost = Client::online(cfg(), 0);
    lost.disconnected();
    assert_eq!(
        hud::banner(&lost),
        (None, "Disconnected from the server\nReload the page".into())
    );
}

#[test]
fn the_banner_reports_a_failed_connection() {
    let mut client = Client::online(cfg(), 0);
    client.handle(ServerMessage::Start {
        session: SESSION,
        players: PlayerSet::first(2),
        characters: [Character::Chalk; 10],
    });
    assert_eq!(
        client.status(),
        Status::Failed(ClientError::StartBeforeWelcome)
    );
    let (big, lines) = hud::banner(&client);
    assert_eq!(big, None);
    assert_eq!(
        lines,
        format!(
            "Error: {}\nReload the page",
            ClientError::StartBeforeWelcome
        )
    );
}

#[test]
fn the_countdown_shows_ready_and_the_seconds_left() {
    let config = MatchConfig {
        countdown_ticks: 2 * TICKS_PER_SECOND,
        ..cfg()
    };
    let mut client = duel(config, 0);
    assert_eq!(hud::banner(&client), (Some("READY"), "2".into()));
    idle_for(&mut client, TICKS_PER_SECOND + 1);
    assert_eq!(hud::banner(&client), (Some("READY"), "1".into()));
}

#[test]
fn the_first_second_of_the_fight_shows_fight_then_nothing() {
    let mut client = duel(cfg(), 0);
    assert_eq!(hud::banner(&client), (Some("FIGHT!"), String::new()));
    idle_for(&mut client, TICKS_PER_SECOND - 1);
    assert_eq!(hud::banner(&client), (Some("FIGHT!"), String::new()));
    idle_for(&mut client, 1);
    assert_eq!(hud::banner(&client), (None, String::new()));
}

#[test]
fn a_stalled_session_tells_that_the_server_is_not_heard_from() {
    let mut client = duel(cfg(), 0);
    idle_for(&mut client, TICKS_PER_SECOND);
    // The match goes on, guessed, as far as it may; then it waits.
    for _ in 0..MAX_ROLLBACK + STALL_WARNING_TICKS {
        client.tick(idle());
    }
    assert_eq!(client.stalled_ticks(), STALL_WARNING_TICKS);
    assert_eq!(hud::banner(&client), (None, String::new()), "not yet");
    client.tick(idle());
    assert_eq!(hud::banner(&client), (None, STALL_TEXT.into()));
}

#[test]
fn the_bars_mark_who_lags_and_who_left() {
    let mut client = brawl(cfg(), 1, PlayerSet::first(4));
    relay_frame(&mut client, &[], set(&[2]));
    client.tick(idle());
    client.handle(ServerMessage::PlayerLeft {
        player: PlayerId(3),
    });
    assert_eq!(client.status(), Status::Playing, "three still fight");
    let session = client.session().unwrap();
    let marks: Vec<&str> = (0..4)
        .map(|player| hud::network_mark(&client, session, PlayerId(player)))
        .collect();
    assert_eq!(marks, ["", "", "  LAG", "  LEFT"]);

    let frame = hud_frame(&client, &bars_of(&client), TICKS_PER_SECOND);
    assert!(find_label(&frame, "P3 Chalk  100 HP  LAG").is_some());
    assert!(find_label(&frame, "P4 Chalk  100 HP  LEFT").is_some());
}

#[test]
fn a_knockout_names_the_winner() {
    let mut client = duel(fragile(), 0);
    win_rounds(&mut client, 0, 1);
    assert_eq!(
        hud::banner(&client),
        (Some("KO"), "You win!\nEnter for the next round".into())
    );

    let mut client = duel(fragile(), 0);
    win_rounds(&mut client, 1, 1);
    assert_eq!(
        hud::banner(&client),
        (Some("KO"), "P2 wins!\nEnter for the next round".into())
    );

    let mut client = duel(fragile(), 1);
    win_rounds(&mut client, 1, 1);
    assert_eq!(hud::banner(&client).1, "You win!\nEnter for the next round");
}

#[test]
fn a_time_up_shows_time_and_the_healthier_fighter_wins() {
    let mut client = duel(short_rounds(30), 0);
    play_as(&mut client, 1, punch());
    while current(&client).outcome().is_none() {
        play(&mut client, idle(), idle());
    }
    assert_eq!(
        current(&client).outcome(),
        Some(Outcome::Winner(srr::game::TeamId(1)))
    );
    assert_eq!(
        hud::banner(&client),
        (Some("TIME"), "P2 wins!\nEnter for the next round".into())
    );
}

#[test]
fn a_time_up_with_equal_health_is_a_draw() {
    let mut client = duel(short_rounds(30), 0);
    idle_for(&mut client, 30);
    assert_eq!(current(&client).outcome(), Some(Outcome::Draw));
    assert_eq!(
        hud::banner(&client),
        (Some("TIME"), "Draw!\nEnter for the next round".into())
    );
}

/// What the server says after a change of the room.
fn lobby(client: &mut Client, seated: PlayerSet, ready: PlayerSet) {
    client.handle(ServerMessage::Lobby { seated, ready });
}

fn set(players: &[u8]) -> PlayerSet {
    players.iter().map(|&p| PlayerId(p)).collect()
}

#[test]
fn the_ready_text_follows_who_is_ready_and_who_joins() {
    let mut client = brawl(short_rounds(1), 0, PlayerSet::first(3));
    relay_frame(&mut client, &[], PlayerSet::EMPTY);
    client.tick(idle());
    let session = client.session().unwrap().clone();
    assert!(session.current().outcome().is_some());
    assert_eq!(ready_text(&client, &session), "Enter for the next round");

    lobby(&mut client, PlayerSet::first(3), set(&[0, 2]));
    assert_eq!(
        ready_text(&client, &session),
        "Waiting for P2 to get ready..."
    );
    lobby(&mut client, PlayerSet::first(4), set(&[0]));
    assert_eq!(
        ready_text(&client, &session),
        "Waiting for P2, P3, P4 to get ready...\nP4 joins the next round"
    );
    lobby(&mut client, set(&[0, 3]), set(&[0]));
    assert_eq!(
        ready_text(&client, &session),
        "Waiting for P4 to get ready...\nP4 joins the next round"
    );
    lobby(&mut client, set(&[0]), set(&[0]));
    assert_eq!(
        ready_text(&client, &session),
        "Waiting for another player..."
    );

    // Two newcomers while the duel ends; the local player is not ready yet.
    let mut client = duel(short_rounds(1), 1);
    idle_for(&mut client, 1);
    lobby(&mut client, PlayerSet::first(4), PlayerSet::EMPTY);
    let session = client.session().unwrap().clone();
    assert_eq!(
        ready_text(&client, &session),
        "Enter for the next round\nP3, P4 join the next round"
    );
    assert_eq!(
        hud::banner(&client).1,
        "Draw!\nEnter for the next round\nP3, P4 join the next round"
    );

    // Alone, Enter replays at once.
    let practice = Client::practice(cfg());
    let session = practice.session().unwrap().clone();
    assert_eq!(ready_text(&practice, &session), "Enter for a rematch");
}

#[test]
fn the_lobby_text_counts_who_is_here_and_ready() {
    let mut client = Client::online(cfg(), 0);
    client.handle(ServerMessage::Welcome {
        player: PlayerId(1),
    });
    lobby(&mut client, set(&[1]), PlayerSet::EMPTY);
    assert_eq!(
        lobby_text(&client),
        "Nobody else is here yet\nOpen this page on other machines (up to 10 players)"
    );
    lobby(&mut client, PlayerSet::first(3), set(&[0]));
    assert_eq!(
        lobby_text(&client),
        "3 players here, 1 ready - Enter to fight"
    );
    lobby(&mut client, PlayerSet::first(3), set(&[0, 1]));
    assert_eq!(lobby_text(&client), "Waiting for P3 to get ready...");
    assert_eq!(hud::banner(&client), (None, lobby_text(&client)));
}

// ---------------------------------------------------------------------------------------------
// HUD: drawing
// ---------------------------------------------------------------------------------------------

#[test]
fn without_a_duel_the_hud_shows_the_banner_and_the_controls() {
    let client = Client::online(cfg(), 0);
    let frame = hud_frame(&client, &[], 0);
    assert!(frame.rects.is_empty());
    let banner = find_label(&frame, "Connecting to the server...").unwrap();
    assert_eq!((banner.size, banner.color), (36.0, BANNER));
    let lines = &frame.labels[frame.labels.len() - 2..];
    let expected = controls_lines(&KeyBindings::STANDARD);
    for (index, (line, text)) in lines.iter().zip(&expected).enumerate() {
        assert_eq!(&line.text, text);
        assert_eq!(line.align, Align::Center);
        assert_eq!(line.x, SCREEN_WIDTH / 2.0);
        assert_eq!(line.y, CONTROLS_Y + index as f32 * CONTROLS_LINE_HEIGHT);
    }
    assert_eq!(
        expected[0],
        "Move: Left / Right   Sidestep: 7 / 2   Jump: Up   Crouch: Down   \
         Turn: Down+Left / Down+Right   Block: step back"
    );
    assert_eq!(
        expected[1],
        "Punch: 1 5 6   Kick: 4 8 9   Grab: 0   Judo: 5+9, 8+9   Special: 3   Enter: ready   \
         P: pause   Tab: view"
    );

    let mut waiting = Client::online(cfg(), 0);
    waiting.handle(ServerMessage::Welcome {
        player: PlayerId(0),
    });
    let frame = hud_frame(&waiting, &[], 0);
    assert!(find_label(&frame, "Nobody else is here yet").is_some());
    assert!(
        find_label(
            &frame,
            "Open this page on other machines (up to 10 players)"
        )
        .is_some()
    );
}

#[test]
fn the_hud_draws_both_bars_names_and_the_timer() {
    let client = duel(cfg(), 0);
    let frame = hud_frame(&client, &bars_of(&client), 1);
    let p1 = find_label(&frame, "P1 (you) Chalk  100 HP").unwrap();
    assert_eq!(
        (p1.align, p1.color, p1.size),
        (Align::Left, LOCAL_NAME, 18.0)
    );
    let p2 = find_label(&frame, "P2 Chalk  100 HP").unwrap();
    assert_eq!((p2.align, p2.color), (Align::Right, NAME));
    let timer = find_label(&frame, "20").unwrap();
    assert_eq!(timer.color, TIMER, "plenty of time left");
    assert_eq!(rects_of(&frame, HEALTH).len(), 2);
    assert_eq!(rects_of(&frame, TRAIL).len(), 2);
    assert_eq!(
        rects_of(&frame, WIN_OFF).len(),
        6,
        "three empty pips per bar"
    );
    assert!(
        frame
            .labels
            .iter()
            .any(|l| l.text.contains('@') && l.color != BANNER),
        "FIGHT! in the block font"
    );
}

#[test]
fn the_trail_shows_recent_damage_on_the_right_side_of_the_left_bar() {
    let mut client = duel(cfg(), 0);
    let mut bars = bars_of(&client);
    for _ in 0..3 {
        play_as(&mut client, 1, punch());
    }
    assert_eq!(health(&client, 0), 90);
    bars[0].update(90);
    let frame = hud_frame(&client, &bars, 0);
    let slot = bar_slots(2)[0];
    let trail = rects_of(&frame, TRAIL)
        .into_iter()
        .find(|r| r.x == slot.x)
        .unwrap();
    let health = rects_of(&frame, HEALTH)
        .into_iter()
        .find(|r| r.x == slot.x && r.height == slot.height)
        .unwrap();
    assert_eq!(trail.width, slot.width);
    assert!((health.width - slot.width * 0.9).abs() < 1e-3);
    assert!(find_label(&frame, "P1 (you) Chalk   90 HP").is_some());
}

#[test]
fn a_bar_in_danger_blinks() {
    let mut client = duel(cfg(), 0);
    while health(&client, 1) * 100 > 100 * DANGER_PERCENT {
        play_as(&mut client, 0, punch());
    }
    let bars = bars_of(&client);
    let on = hud_frame(&client, &bars, 0);
    let off = hud_frame(&client, &bars, 8);
    let slot = bar_slots(2)[1];
    let bar_color = |frame: &Frame| {
        frame
            .rects
            .iter()
            .find(|r| {
                r.y == slot.y
                    && r.height == slot.height
                    && r.color != TRAIL
                    && r.x > 640.0
                    && r.width < slot.width
            })
            .map(|r| r.color)
    };
    assert_eq!(bar_color(&on), Some(HEALTH_LOW));
    assert_eq!(bar_color(&off), Some(HEALTH));
    assert_eq!(rects_of(&on, HEALTH_LOW).len(), 1, "only the bar in danger");
}

#[test]
fn round_wins_light_pips_and_more_than_three_are_counted() {
    let mut client = duel(fragile(), 0);
    win_rounds(&mut client, 0, 4);
    assert_eq!(client.session().unwrap().wins(PlayerId(0)), 4);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    let count = find_label(&frame, "x4").unwrap();
    assert_eq!(
        count.align,
        Align::Right,
        "towards the timer from the left bar"
    );
    assert_eq!(rects_of(&frame, WIN_OFF).len(), 3, "P2 has no win");
    assert!(rects_of(&frame, WIN_ON).len() >= 3);

    let mut client = duel(fragile(), 0);
    win_rounds(&mut client, 1, 5);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    let count = find_label(&frame, "x5").unwrap();
    assert_eq!(
        count.align,
        Align::Left,
        "towards the timer from the right bar"
    );
    assert!(count.x > 640.0);
}

#[test]
fn three_pips_light_up_without_a_counter() {
    let mut client = duel(fragile(), 0);
    win_rounds(&mut client, 0, 3);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    assert!(!frame.labels.iter().any(|l| l.text.starts_with('x')));
    assert_eq!(rects_of(&frame, WIN_OFF).len(), 3);
}

#[test]
fn the_combo_counter_shows_from_the_second_hit_under_the_bar_of_the_fighter_hit() {
    assert_eq!(COMBO_MIN_HITS, 2);
    let mut client = duel(stunning(), 0);
    for _ in 0..3 {
        play_as(&mut client, 0, punch()); // lands on the third tick
    }
    assert_eq!(health(&client, 1), 90);
    let frame = hud_frame(&client, &counted_bars(&client), 0);
    assert!(
        frame
            .labels
            .iter()
            .all(|label| !label.text.ends_with("HITS")),
        "one hit is no combo"
    );

    let medium = hold(Input::pressing(Attack::punch(Strength::Medium)));
    for _ in 0..3 {
        play_as(&mut client, 0, medium); // chained at once, lands on the sixth tick
    }
    assert_eq!(health(&client, 1), 80);
    let frame = hud_frame(&client, &counted_bars(&client), 0);
    let counter = find_label(&frame, "2 HITS").unwrap();
    assert_eq!(
        (counter.align, counter.color),
        (Align::Right, Rgba::hex(0xfff35c))
    );
    let slot = bar_slots(2)[1];
    assert_eq!(counter.x, slot.x + slot.width, "under player 2's bar");
    let name = find_label(&frame, "P2 Chalk   80 HP").unwrap();
    assert!(counter.y > name.y, "below the name");

    let high = hold(Input::pressing(Attack::punch(Strength::High)));
    for _ in 0..3 {
        play_as(&mut client, 0, high);
    }
    assert_eq!(health(&client, 1), 70);
    let frame = hud_frame(&client, &counted_bars(&client), 0);
    let hotter = find_label(&frame, "3 HITS").unwrap();
    assert_eq!(hotter.color, Rgba::hex(0xffc93a));
    assert!(find_label(&frame, "2 HITS").is_none());
}

#[test]
fn the_grapple_meter_shows_the_ladder_on_top_and_the_escape_gauge_below() {
    let mut client = duel(cfg(), 0);
    clinch(&mut client, 0);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    let ladder = find_label(&frame, "[CLINCH] > mount > submission").unwrap();
    assert_eq!((ladder.align, ladder.color), (Align::Left, GRAPPLE_ON));
    let escape = find_label(&frame, "ESCAPE CLINCH").unwrap();
    assert_eq!((escape.align, escape.color), (Align::Right, ESCAPE));
    let gauge = rects_of(&frame, ESCAPE);
    assert_eq!(gauge.len(), 1);
    assert_eq!(gauge[0].width, 0.0, "no escape point yet");

    play(&mut client, idle(), punch());
    let frame = hud_frame(&client, &bars_of(&client), 0);
    let gauge = rects_of(&frame, ESCAPE);
    assert!(gauge[0].width > 0.0, "one escape point");
    let slot = bar_slots(2)[1];
    assert!(
        (gauge[0].x + gauge[0].width - (slot.x + slot.width)).abs() < 1e-3,
        "filled from the outer edge"
    );

    climb(&mut client, 0);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    assert!(find_label(&frame, "clinch > [MOUNT] > submission").is_some());
    assert!(find_label(&frame, "ESCAPE MOUNT").is_some());
    climb(&mut client, 0);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    assert!(find_label(&frame, "clinch > mount > [SUBMISSION]").is_some());
    assert!(find_label(&frame, "ESCAPE SUBMISSION").is_some());
}

#[test]
fn the_grapple_meter_mirrors_when_the_right_fighter_is_on_top() {
    let mut client = duel(cfg(), 0);
    clinch(&mut client, 1);
    play(&mut client, punch(), idle());
    let frame = hud_frame(&client, &bars_of(&client), 0);
    let ladder = find_label(&frame, "[CLINCH] > mount > submission").unwrap();
    assert_eq!(ladder.align, Align::Right);
    let escape = find_label(&frame, "ESCAPE CLINCH").unwrap();
    assert_eq!(escape.align, Align::Left);
    let gauge = rects_of(&frame, ESCAPE);
    let slot = bar_slots(2)[0];
    assert_eq!(gauge[0].x, slot.x, "filled from the left edge");
    assert!(gauge[0].width > 0.0);
}

#[test]
fn the_timer_turns_red_in_the_last_ten_seconds() {
    let mut client = duel(short_rounds(10 * TICKS_PER_SECOND + 1), 0);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    assert_eq!(find_label(&frame, "11").unwrap().color, TIMER);
    idle_for(&mut client, 1);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    assert_eq!(find_label(&frame, "10").unwrap().color, TIMER_LOW);

    idle_for(&mut client, 10 * TICKS_PER_SECOND);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    assert_eq!(
        find_label(&frame, "00").unwrap().color,
        TIMER,
        "the round is over"
    );
}

#[test]
fn the_countdown_timer_stays_white() {
    let config = MatchConfig {
        countdown_ticks: 60,
        round_time_ticks: 5 * TICKS_PER_SECOND,
        ..cfg()
    };
    let client = duel(config, 0);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    assert_eq!(find_label(&frame, "05").unwrap().color, TIMER);
    assert!(find_label(&frame, "1").is_some(), "the countdown second");
}

#[test]
fn fight_shakes_and_the_result_lines_are_smaller_under_a_big_word() {
    let client = duel(cfg(), 0);
    let top = |frame: &Frame| {
        frame
            .labels
            .iter()
            .filter(|l| l.text.contains('@'))
            .map(|l| l.y)
            .fold(f32::MAX, f32::min)
    };
    let still = hud_frame(&client, &bars_of(&client), 2);
    let shaken = hud_frame(&client, &bars_of(&client), 0);
    assert_eq!(top(&shaken) - top(&still), 3.0);

    let mut client = duel(fragile(), 0);
    win_rounds(&mut client, 0, 1);
    let ko_a = hud_frame(&client, &bars_of(&client), 0);
    let ko_b = hud_frame(&client, &bars_of(&client), 2);
    assert_eq!(top(&ko_a), top(&ko_b), "only FIGHT! shakes");
    let result = find_label(&ko_a, "You win!").unwrap();
    assert_eq!((result.size, result.color), (30.0, BANNER_SMALL));
    assert!(find_label(&ko_a, "Enter for the next round").is_some());
}

#[test]
fn small_bars_get_small_labels() {
    let client = duel(cfg(), 0);
    let mut bars = bars_of(&client);
    bars.push(HealthBar::new(PlayerId(2), 100));
    let frame = hud_frame(&client, &bars, 0);
    let p1 = find_label(&frame, "P1 (you) Chalk  100 HP").unwrap();
    assert_eq!(p1.size, 13.0);
    let p2 = find_label(&frame, "P2 Chalk  100 HP").unwrap();
    assert_eq!(
        p2.align,
        Align::Left,
        "the second of three bars is on the left"
    );
}

// ---------------------------------------------------------------------------------------------
// Arena: helpers
// ---------------------------------------------------------------------------------------------

#[test]
fn every_player_has_a_colour_and_they_cycle() {
    let colors: Vec<Rgba> = (0..MAX_PLAYERS as u8)
        .map(|p| arena::player_color(PlayerId(p)))
        .collect();
    for (i, a) in colors.iter().enumerate() {
        for b in &colors[i + 1..] {
            assert_ne!(a, b);
        }
    }
    assert_eq!(arena::player_color(PlayerId(10)), colors[0]);
    assert_eq!(arena::player_color(PlayerId(13)), colors[3]);
}

#[test]
fn the_whole_arena_fits_the_screen() {
    let config = cfg();
    assert_eq!(arena::scale(&config), ARENA_PIXELS / 300.0);
    let broken = MatchConfig {
        arena_width: 0,
        ..cfg()
    };
    assert_eq!(arena::scale(&broken), ARENA_PIXELS, "never divides by zero");
}

#[test]
fn feet_follow_the_fighter_on_the_floor_and_in_the_air() {
    let mut client = duel(cfg(), 0);
    let config = cfg();
    let scale = arena::scale(&config);
    let fighter = current(&client).fighter(PlayerId(1)).unwrap();
    assert_eq!(
        arena::feet(fighter, &config),
        (ARENA_LEFT + 200.0 * scale, FLOOR_Y)
    );
    play_as(&mut client, 0, jump());
    let jumper = current(&client).fighter(PlayerId(0)).unwrap();
    assert!(jumper.y() > 0);
    let (x, y) = arena::feet(jumper, &config);
    assert_eq!(x, ARENA_LEFT + 100.0 * scale);
    assert_eq!(y, FLOOR_Y - jumper.y() as f32 * scale);
}

#[test]
fn the_body_colour_shows_the_state() {
    let base = Rgba::rgb(0.8, 0.4, 0.2);
    assert_eq!(arena::body_color(base, FighterState::Knockout), KNOCKED_OUT);
    assert_eq!(arena::body_color(base, FighterState::HitStun), STUNNED);
    for state in [
        FighterState::Blocking,
        FighterState::Idle,
        FighterState::Walking,
        FighterState::Airborne,
        FighterState::Attacking(AttackPhase::Active),
        FighterState::Grabbing(AttackPhase::Startup),
        grappling(GrappleRole::Top, GrapplePosition::Mount),
    ] {
        assert_eq!(arena::body_color(base, state), base, "{state:?}");
    }
}

#[test]
fn stronger_attacks_are_hotter() {
    let colors: Vec<Rgba> = Strength::ALL
        .into_iter()
        .map(arena::strength_color)
        .collect();
    assert_eq!(colors[0], Rgba::hex(0xf2f28c));
    assert_eq!(colors[1], Rgba::hex(0xffa633));
    assert_eq!(colors[2], Rgba::hex(0xff4026));
    assert!(colors[0].g > colors[1].g && colors[1].g > colors[2].g);
}

// ---------------------------------------------------------------------------------------------
// Arena: drawing
// ---------------------------------------------------------------------------------------------

#[test]
fn without_a_duel_only_the_alley_is_drawn() {
    let mut frame = Frame::new(arena::BACKGROUND);
    arena::draw(&mut frame, &Client::online(cfg(), 0), 0);
    assert!(frame.labels.is_empty());
    assert!(frame.rects.len() > 100, "bricks, tag, neon and street");
    assert!(
        frame
            .rects
            .iter()
            .all(|r| r.x >= 0.0 && r.x + r.width <= 1280.0 + 1e-3),
        "every brick is cut at the screen edges"
    );
    assert!(rects_of(&frame, SHADOW).is_empty());
}

#[test]
fn the_neon_strip_flickers_now_and_then() {
    let neon_at = |ticks: u32| {
        let mut frame = Frame::new(arena::BACKGROUND);
        arena::draw(&mut frame, &Client::online(cfg(), 0), ticks);
        frame
            .rects
            .iter()
            .find(|r| r.x == 120.0 && r.width == 220.0 && r.height == 5.0)
            .unwrap()
            .color
    };
    let colors: Vec<Rgba> = (0..1200).step_by(6).map(neon_at).collect();
    let bright = colors.iter().filter(|&&c| c == NEON).count();
    let dim = colors.iter().filter(|&&c| c == NEON.scaled(0.3)).count();
    assert_eq!(bright + dim, colors.len());
    assert!(
        dim > 0 && bright > dim * 3,
        "mostly on: {bright} on, {dim} off"
    );
}

#[test]
fn idle_fighters_stand_with_shadows_name_tags_and_the_local_marker() {
    let client = duel(cfg(), 0);
    let frame = arena_frame(&client, 0);
    let shadows = rects_of(&frame, SHADOW);
    assert_eq!(shadows.len(), 2);
    assert!(shadows.iter().all(|s| s.width == 64.0));
    let tags: Vec<&str> = frame.labels.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(tags, vec!["P1 Chalk", "P2 Chalk"]);
    assert!(
        frame
            .labels
            .iter()
            .all(|l| l.align == Align::Center && l.y < FLOOR_Y)
    );
    let markers = rects_of(&frame, MARKER);
    assert_eq!(markers.len(), 1);
    let (x, _) = arena::feet(current(&client).fighter(PlayerId(0)).unwrap(), &cfg());
    assert_eq!(
        markers[0].x + markers[0].width / 2.0,
        x,
        "over the local fighter"
    );

    let guest = duel(cfg(), 1);
    let frame = arena_frame(&guest, 0);
    let (x, _) = arena::feet(current(&guest).fighter(PlayerId(1)).unwrap(), &cfg());
    let marker = rects_of(&frame, MARKER)[0];
    assert_eq!(marker.x + marker.width / 2.0, x);
}

#[test]
fn an_airborne_fighter_casts_a_smaller_shadow() {
    let mut client = duel(cfg(), 0);
    play_as(&mut client, 1, jump());
    play(&mut client, idle(), idle());
    assert_eq!(state(&client, 1), FighterState::Airborne);
    let frame = arena_frame(&client, 0);
    let widths: Vec<f32> = rects_of(&frame, SHADOW).iter().map(|s| s.width).collect();
    assert_eq!(widths, vec![64.0, 36.0]);
    let tag = find_label(&frame, "P2 Chalk").unwrap();
    assert!(
        tag.y < find_label(&frame, "P1 Chalk").unwrap().y,
        "the tag rises with the jump"
    );
}

#[test]
fn a_fighter_further_back_stands_higher_and_is_drawn_first() {
    let mut client = duel(cfg(), 0);
    let sidestep_in = Input {
        sidestep_in: true,
        ..Input::NONE
    };
    for _ in 0..9 {
        play_as(&mut client, 1, hold(sidestep_in));
    }
    play(&mut client, idle(), idle());
    let m = current(&client);
    let back = m.fighter(PlayerId(1)).unwrap();
    assert_eq!(back.z(), 45);
    let rise = arena::rise(back, m.config());
    assert_eq!(rise, 45.0 * arena::scale(m.config()) * arena::DEPTH_RISE);
    assert_eq!(
        arena::rise(m.fighter(PlayerId(0)).unwrap(), m.config()),
        0.0
    );

    let figure = Figure::of(back, m.config(), 0);
    let raised = figure.raised(rise);
    for (moved, still) in [
        (raised.head, figure.head),
        (raised.neck, figure.neck),
        (raised.shoulder, figure.shoulder),
        (raised.hip, figure.hip),
        (raised.arms[1].joint, figure.arms[1].joint),
        (raised.legs[0].end, figure.legs[0].end),
    ] {
        assert_eq!((moved.x, moved.y), (still.x, still.y - rise));
    }

    let frame = arena_frame(&client, 0);
    let shadows = rects_of(&frame, SHADOW);
    assert_eq!(shadows[1].y, shadows[0].y - rise, "the shadow rises too");
    assert!(find_label(&frame, "P2 Chalk").unwrap().y < find_label(&frame, "P1 Chalk").unwrap().y);
    // After the shadows, the accessory of the one further back comes first.
    let fighters = &frame.rects[frame.rects.iter().rposition(|r| r.color == SHADOW).unwrap()..];
    let first = |player: u8| {
        let color = arena::player_color(PlayerId(player));
        fighters.iter().position(|r| r.color == color).unwrap()
    };
    assert!(first(1) < first(0));
}

// ---------------------------------------------------------------------------------------------
// Judo techniques
// ---------------------------------------------------------------------------------------------

/// [`cfg`] where both techniques reach across the arena, can always be countered in the air,
/// and their projections last 20 ticks.
fn judo() -> MatchConfig {
    let mut config = cfg();
    for data in &mut config.techniques {
        data.range = 150;
        data.projection_ticks = 20;
        data.counter_window = 20;
        data.escape_presses = 1000;
    }
    config
}

fn keys_of(technique: Technique) -> Input {
    let [first, second] = technique.buttons();
    hold(Input::pressing(first).with(second))
}

/// `top` performs `technique` on the other fighter, until it is caught in the projection.
fn project(client: &mut Client, top: u8, technique: Technique) {
    play_as(client, top, keys_of(technique));
    for _ in 0..12 {
        if matches!(state(client, top), FighterState::Grappling(..)) {
            break;
        }
        idle_for(client, 1);
    }
    assert_eq!(
        state(client, top),
        grappling(GrappleRole::Top, technique.projection())
    );
}

#[test]
fn the_grapple_meter_follows_the_technique_and_prompts_the_counter() {
    let mut client = duel(judo(), 0);
    project(&mut client, 0, Technique::Sweep);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    assert!(find_label(&frame, "[JUJI SWEEP] > hon-gesa-gatame").is_some());
    let counter = find_label(&frame, "JUJI SWEEP!  COUNTER 5+9").unwrap();
    assert_eq!((counter.align, counter.color), (Align::Right, ESCAPE));
    assert!(rects_of(&frame, ESCAPE).is_empty(), "no escape in the air");
    idle_for(&mut client, 20);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    assert!(find_label(&frame, "juji sweep > [HON-GESA-GATAME]").is_some());
    assert!(find_label(&frame, "ESCAPE HON-GESA-GATAME").is_some());

    let mut client = duel(judo(), 0);
    project(&mut client, 1, Technique::Throw);
    let frame = hud_frame(&client, &bars_of(&client), 0);
    let ladder = find_label(&frame, "[IPPON-SEOI-NAGE] > juji-gatame").unwrap();
    assert_eq!(ladder.align, Align::Right);
    let counter = find_label(&frame, "IPPON-SEOI-NAGE!  COUNTER 8+9").unwrap();
    assert_eq!(counter.align, Align::Left);
}

#[test]
fn the_hud_names_the_local_keys() {
    let mut client = duel(judo(), 0);
    project(&mut client, 1, Technique::Throw);
    let mut frame = Frame::new(arena::BACKGROUND);
    hud::draw(
        &mut frame,
        &client,
        &bars_of(&client),
        0,
        &KeyBindings::WASD,
    );
    assert!(find_label(&frame, "IPPON-SEOI-NAGE!  COUNTER Y+U").is_some());
    let text = frame.text();
    assert!(
        text.contains(
            "Move: A / D   Sidestep: Q / E   Jump: W   Crouch: S   Turn: S+A / S+D   \
             Block: step back"
        ),
        "{text}"
    );
    assert!(text.contains("Punch: B H J   Kick: G Y U   Grab: Space   Judo: H+U, Y+U"));
}

#[test]
fn intensity_peaks_with_the_techniques() {
    let mut client = duel(judo(), 0);
    project(&mut client, 0, Technique::Throw);
    assert_eq!(intensity(current(&client)), 0.8);
    idle_for(&mut client, 20);
    assert_eq!(intensity(current(&client)), 1.0);
}

#[test]
fn every_tick_of_a_brawl_draws_inside_the_screen() {
    let mut client = duel(cfg(), 0);
    let script = [punch(), kick(), grab(), jump(), left(), right(), idle()];
    for tick in 0..400u32 {
        let p0 = script[(tick as usize / 3) % script.len()];
        let p1 = script[(tick as usize / 5 + 2) % script.len()];
        play(&mut client, p0, p1);
        let frame = arena_frame(&client, tick);
        for r in &frame.rects {
            assert!(r.width >= 0.0 && r.height >= 0.0);
            assert!(r.x > -200.0 && r.x < 1480.0, "tick {tick}: {r:?}");
        }
        assert_eq!(frame.labels.len(), 2);
    }
}

// ---------------------------------------------------------------------------------------------
// Stick figures: the pen
// ---------------------------------------------------------------------------------------------

const CHALK: Rgba = Rgba::hex(0x123456);

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

fn assert_at(point: Point, x: f32, y: f32) {
    assert!(
        near(point.x, x) && near(point.y, y),
        "{point:?} is not at ({x}, {y})"
    );
}

fn stroked(a: Point, b: Point, width: f32) -> Vec<Rect> {
    let mut frame = Frame::new(arena::BACKGROUND);
    stick::stroke(&mut frame, a, b, width, CHALK);
    assert!(frame.rects.iter().all(|r| r.color == CHALK));
    frame.rects
}

const fn pt(x: f32, y: f32) -> Point {
    Point { x, y }
}

#[test]
fn a_straight_line_is_a_single_rectangle_with_overlapping_caps() {
    let horizontal = stroked(pt(0.0, 10.0), pt(100.0, 10.0), 3.0);
    assert_eq!(horizontal.len(), 1);
    let r = horizontal[0];
    assert!(near(r.x, -1.5) && near(r.width, 103.0), "{r:?}");
    assert!(near(r.y, 8.5) && near(r.height, 3.0), "{r:?}");

    let vertical = stroked(pt(5.0, 50.0), pt(5.0, 0.0), 3.0);
    assert_eq!(vertical.len(), 1);
    let r = vertical[0];
    assert!(near(r.x, 3.5) && near(r.width, 3.0), "{r:?}");
    assert!(near(r.y, -1.5) && near(r.height, 53.0), "{r:?}");
}

#[test]
fn a_slanted_line_is_a_staircase_of_thin_runs() {
    let runs = stroked(pt(0.0, 0.0), pt(30.0, 40.0), LINE_WIDTH);
    assert!(runs.len() > 15, "{} runs", runs.len());
    // Mostly vertical: each run is a thin column, and together they go all the way down.
    assert!(runs.iter().all(|r| r.height < 4.0 && r.width > LINE_WIDTH));
    let top = runs.iter().map(|r| r.y).fold(f32::MAX, f32::min);
    let bottom = runs.iter().map(|r| r.y + r.height).fold(f32::MIN, f32::max);
    assert!(near(top, -1.4) && near(bottom, 41.4), "{top}..{bottom}");
    let flat = stroked(pt(0.0, 0.0), pt(40.0, -30.0), LINE_WIDTH);
    assert!(flat.iter().all(|r| r.width < 4.0 && r.height > LINE_WIDTH));
}

#[test]
fn a_thick_stroke_keeps_its_length_and_a_dot_has_no_area() {
    let thick = stroked(pt(0.0, 0.0), pt(100.0, 0.0), 10.0);
    assert_eq!(thick.len(), 1);
    let r = thick[0];
    let cap = LINE_WIDTH / 2.0;
    assert!(near(r.x, -cap) && near(r.width, 100.0 + 2.0 * cap), "{r:?}");
    assert!(near(r.height, 10.0));

    let dot = stroked(pt(7.0, 7.0), pt(7.0, 7.0), 3.0);
    assert_eq!(dot.len(), 1);
    assert_eq!(dot[0].width, 0.0, "never divides by zero, never paints");
}

#[test]
fn a_ring_is_hollow_and_as_wide_as_its_outline() {
    let mut frame = Frame::new(arena::BACKGROUND);
    let center = pt(100.0, 100.0);
    stick::ring(&mut frame, center, 12.0, 3.5, CHALK);
    let outer = 12.0 + 1.75;
    assert!(frame.rects.len() > 20);
    let covers = |x: f32, y: f32| {
        frame
            .rects
            .iter()
            .any(|r| r.x <= x && x <= r.x + r.width && r.y <= y && y <= r.y + r.height)
    };
    assert!(!covers(100.0, 100.0), "the head is hollow");
    assert!(!covers(105.0, 95.0));
    assert!(covers(100.0 - 12.0, 100.0) && covers(100.0 + 12.0, 100.0));
    assert!(covers(100.0, 100.0 - 12.0) && covers(100.0, 100.0 + 12.0));
    let left = frame.rects.iter().map(|r| r.x).fold(f32::MAX, f32::min);
    let right = frame
        .rects
        .iter()
        .map(|r| r.x + r.width)
        .fold(f32::MIN, f32::max);
    let top = frame.rects.iter().map(|r| r.y).fold(f32::MAX, f32::min);
    let bottom = frame
        .rects
        .iter()
        .map(|r| r.y + r.height)
        .fold(f32::MIN, f32::max);
    assert!(near(left, 100.0 - outer) && near(right, 100.0 + outer));
    assert!(near(top, 100.0 - outer) && near(bottom, 100.0 + outer));
}

// ---------------------------------------------------------------------------------------------
// Stick figures: poses
// ---------------------------------------------------------------------------------------------

fn figure(client: &Client, player: u8, fight_ticks: u32) -> Figure {
    let m = current(client);
    Figure::of(
        m.fighter(PlayerId(player)).unwrap(),
        m.config(),
        fight_ticks,
    )
}

fn drawn(figure: &Figure) -> Frame {
    let mut frame = Frame::new(arena::BACKGROUND);
    figure.draw(&mut frame);
    frame
}

fn feet_of(client: &Client, player: u8) -> (f32, f32) {
    let m = current(client);
    arena::feet(m.fighter(PlayerId(player)).unwrap(), m.config())
}

/// The screen point `forward` ahead of and `up` above `player`'s feet.
fn local(client: &Client, player: u8, forward: f32, up: f32) -> (f32, f32) {
    let (x, y) = feet_of(client, player);
    let facing = if player == 0 { 1.0 } else { -1.0 };
    (x + facing * forward, y - up)
}

fn assert_local(client: &Client, player: u8, point: Point, forward: f32, up: f32) {
    let (x, y) = local(client, player, forward, up);
    assert_at(point, x, y);
}

#[test]
fn every_player_wears_an_accessory_in_their_colour() {
    let m = Match::new(Roster::free_for_all(4).unwrap(), cfg()).unwrap();
    let worn: Vec<Accessory> = m
        .fighters()
        .iter()
        .map(|f| Figure::of(f, m.config(), 0).accessory)
        .collect();
    use Accessory::*;
    assert_eq!(worn, [Hat, Beret, Ponytail, Headband]);
    assert_eq!(stick::accessory(PlayerId(6)), Ponytail, "they cycle");
    for fighter in m.fighters() {
        let figure = Figure::of(fighter, m.config(), 0);
        let color = arena::player_color(fighter.player());
        assert_eq!(figure.accent, color);
        let frame = drawn(&figure);
        assert!(
            !rects_of(&frame, color).is_empty(),
            "{:?}",
            figure.accessory
        );
        assert!(!rects_of(&frame, stick::INK).is_empty(), "the chalk lines");
        assert!(rects_of(&frame, SPARK).is_empty());
    }
}

#[test]
fn an_idle_figure_stands_in_guard_and_breathes() {
    let client = duel(cfg(), 0);
    for player in [0, 1] {
        let still = figure(&client, player, 0);
        let (x, _) = feet_of(&client, player);
        assert_eq!(still.facing, if player == 0 { 1.0 } else { -1.0 });
        assert_eq!(
            (still.ink, still.mark, still.lying),
            (stick::INK, Mark::None, false)
        );
        assert!(still.legs.iter().all(|leg| near(leg.end.y, FLOOR_Y)));
        assert!(still.head.y < still.neck.y && still.neck.y < still.hip.y);
        assert!(
            (still.arms[0].end.x - x) * still.facing > 0.0,
            "the fists in front"
        );
        let r = stick::HEAD_RADIUS;
        assert_eq!(still.top(), still.head.y - r - 12.0, "room for the hat");
        let breathing = figure(&client, player, 24);
        assert!(breathing.head.y > still.head.y, "the shoulders drop");
        assert_eq!(
            breathing.legs.map(|leg| leg.end),
            still.legs.map(|leg| leg.end),
            "the feet stay put"
        );
    }
}

#[test]
fn the_back_limbs_are_dimmer_unless_they_have_their_own_colour() {
    let client = duel(cfg(), 0);
    let figure = figure(&client, 0, 0);
    let plain = figure.arms[0];
    assert_eq!(figure.bone_color(&plain, true), stick::INK);
    assert_eq!(figure.bone_color(&plain, false), stick::INK.scaled(0.75));
    let lit = Bone {
        ink: Some(GRAB),
        ..plain
    };
    assert_eq!(figure.bone_color(&lit, false), GRAB);
}

#[test]
fn walking_scissors_the_legs_and_blocking_raises_the_hands() {
    let mut client = duel(cfg(), 0);
    play(&mut client, right(), right());
    assert_eq!(state(&client, 0), FighterState::Walking);
    assert_eq!(state(&client, 1), FighterState::Blocking);
    let first = figure(&client, 0, 0);
    let guard = figure(&client, 1, 0);
    play(&mut client, right(), idle());
    let second = figure(&client, 0, 0);
    assert_ne!(first.legs, second.legs, "the legs move with the steps");
    for walker in [first, second] {
        assert!(walker.legs.iter().all(|leg| leg.end.y <= FLOOR_Y + 1e-3));
        assert!(walker.legs.iter().all(|leg| leg.end.y >= FLOOR_Y - 5.0));
    }
    assert!(guard.arms.iter().all(|arm| arm.end.y < guard.neck.y));
    assert!(
        guard.arms.iter().all(|arm| arm.end.x < guard.hip.x),
        "in front of a fighter facing left"
    );
    assert_eq!(
        guard.ink,
        stick::INK,
        "the guard is in the pose, not the ink"
    );
}

#[test]
fn a_crouching_figure_squats_with_its_fists_up() {
    let mut client = duel(cfg(), 0);
    let standing = figure(&client, 1, 0);
    play_as(&mut client, 1, crouch());
    assert_eq!(state(&client, 1), FighterState::Crouching);
    let squat = figure(&client, 1, 0);
    assert_local(&client, 1, squat.hip, -6.0, 26.0);
    assert_local(&client, 1, squat.neck, 4.0, 56.0);
    assert_local(&client, 1, squat.arms[0].end, 20.0, 54.0);
    assert_local(&client, 1, squat.arms[1].end, 12.0, 50.0);
    assert_local(&client, 1, squat.legs[0].end, 18.0, 0.0);
    assert_local(&client, 1, squat.legs[1].end, -20.0, 0.0);
    assert!(
        squat.head.y > standing.head.y + 20.0 && squat.top() > standing.top() + 20.0,
        "well below a standing head"
    );
    assert!(
        squat
            .arms
            .iter()
            .all(|arm| (arm.end.y - squat.neck.y).abs() < 8.0 && arm.end.y < squat.hip.y),
        "the fists at chin level"
    );
    assert_eq!((squat.ink, squat.lying), (stick::INK, false));
}

#[test]
fn a_jumping_figure_tucks_its_legs() {
    let mut client = duel(cfg(), 0);
    play_as(&mut client, 1, jump());
    play(&mut client, idle(), idle());
    assert_eq!(state(&client, 1), FighterState::Airborne);
    let jumper = figure(&client, 1, 0);
    assert!(jumper.legs.iter().all(|leg| leg.end.y < FLOOR_Y - 24.0));
    assert_local(&client, 1, jumper.legs[0].end, 14.0, 24.0);
}

#[test]
fn a_flying_kick_shoots_the_front_leg_out_and_down() {
    let mut client = duel(cfg(), 0);
    play_as(&mut client, 0, jump());
    play_as(&mut client, 0, kick()); // in the air: the first start-up tick
    assert!(current(&client).fighter(PlayerId(0)).unwrap().is_airborne());
    assert_eq!(
        state(&client, 0),
        FighterState::Attacking(AttackPhase::Startup)
    );
    let wind_up = figure(&client, 0, 0);
    assert_eq!(
        wind_up.legs[0].ink,
        Some(arena::strength_color(Strength::High))
    );
    assert_local(&client, 0, wind_up.legs[0].end, 16.0, 36.0);

    idle_for(&mut client, 2); // the third tick of the kick is active
    assert_eq!(
        state(&client, 0),
        FighterState::Attacking(AttackPhase::Active)
    );
    let flying = figure(&client, 0, 0);
    assert_local(&client, 0, flying.legs[0].end, MAX_REACH, 20.0);
    assert_local(&client, 0, flying.legs[1].end, -12.0, 30.0);
    assert!(
        flying
            .legs
            .iter()
            .all(|leg| leg.end.y < FLOOR_Y - 20.0 + 1e-3),
        "both feet off the floor"
    );
    assert!(
        flying
            .arms
            .iter()
            .all(|arm| (arm.end.x - flying.hip.x) * flying.facing < 0.0),
        "the arms trail behind"
    );
    assert!(
        (flying.neck.x - flying.hip.x) * flying.facing < 0.0,
        "leaning back"
    );

    idle_for(&mut client, 2); // recovery
    assert_eq!(
        state(&client, 0),
        FighterState::Attacking(AttackPhase::Recovery)
    );
    assert_local(
        &client,
        0,
        figure(&client, 0, 0).legs[0].end,
        MAX_REACH * 0.4,
        30.0,
    );
}

/// Plays `attack` for player 0 (pressed on the first tick) and returns its figure on each of
/// the six ticks the attack lasts, with its state.
fn strike_figures(client: &mut Client, attack: Input) -> Vec<(FighterState, Figure)> {
    (0..6)
        .map(|tick| {
            play_as(client, 0, if tick == 0 { attack } else { idle() });
            (state(client, 0), figure(client, 0, tick))
        })
        .collect()
}

#[test]
fn a_punch_winds_up_stretches_to_its_range_and_recovers() {
    let mut client = duel(cfg(), 0);
    let frames = strike_figures(&mut client, punch());
    use AttackPhase::*;
    let states: Vec<FighterState> = frames.iter().map(|(s, _)| *s).collect();
    assert_eq!(
        states,
        [Startup, Startup, Active, Active, Recovery, Recovery].map(FighterState::Attacking)
    );
    let color = Some(arena::strength_color(Strength::Low));
    assert!(frames.iter().all(|(_, f)| f.arms[0].ink == color));
    assert!(frames.iter().all(|(_, f)| f.arms[1].ink.is_none()));
    // The range (150 × 3.67 pixels) is longer than any arm may stretch.
    assert_local(&client, 0, frames[0].1.arms[0].end, 6.0, 74.0);
    assert_local(&client, 0, frames[2].1.arms[0].end, MAX_REACH, 76.0);
    assert_local(&client, 0, frames[4].1.arms[0].end, MAX_REACH * 0.5, 76.0);
    assert_eq!(health(&client, 1), 90);
}

#[test]
fn a_kick_stretches_the_front_leg_in_its_colour() {
    let mut client = duel(cfg(), 0);
    let frames = strike_figures(&mut client, kick());
    let color = Some(arena::strength_color(Strength::High));
    assert!(frames.iter().all(|(_, f)| f.legs[0].ink == color));
    assert!(frames.iter().all(|(_, f)| f.arms[0].ink.is_none()));
    assert_local(&client, 0, frames[0].1.legs[0].end, 10.0, 30.0);
    assert_local(&client, 0, frames[2].1.legs[0].end, MAX_REACH, 84.0);
    assert_eq!(frames[2].1.legs[0].end.y, FLOOR_Y - 84.0, "a head kick");
    assert_local(&client, 0, frames[4].1.legs[0].end, MAX_REACH * 0.4, 14.0);
}

#[test]
fn every_strike_is_aimed_at_its_own_height() {
    let punches = [76.0, 80.0, 92.0];
    let kicks = [18.0, 48.0, 84.0];
    for (index, strength) in Strength::ALL.into_iter().enumerate() {
        for (attack, height) in [
            (Attack::punch(strength), punches[index]),
            (Attack::kick(strength), kicks[index]),
        ] {
            let mut client = duel(cfg(), 0);
            let frames = strike_figures(&mut client, hold(Input::pressing(attack)));
            let active = frames[2].1;
            let end = if attack == Attack::punch(strength) {
                active.arms[0].end
            } else {
                active.legs[0].end
            };
            assert!(near(end.y, FLOOR_Y - height), "{attack:?}: {end:?}");
        }
    }
}

#[test]
fn a_stunned_figure_flashes_white_and_sees_stars() {
    let mut client = duel(cfg(), 0);
    for tick in 0..3 {
        play_as(&mut client, 0, if tick == 0 { punch() } else { idle() });
    }
    assert_eq!(state(&client, 1), FighterState::HitStun);
    let hit = figure(&client, 1, 0);
    assert_eq!((hit.ink, hit.mark), (STUNNED, Mark::Stunned));
    assert!(
        hit.neck.x > hit.hip.x,
        "knocked backwards, away from the left"
    );
    let frame = drawn(&hit);
    assert!(!rects_of(&frame, SPARK).is_empty());
    assert!(!rects_of(&frame, STUNNED).is_empty());
}

#[test]
fn a_whiffed_grab_reaches_out_with_both_arms_then_recovers() {
    let mut client = duel(cfg(), 0);
    let mut seen = Vec::new();
    for tick in 0..6 {
        play(
            &mut client,
            if tick == 0 { grab() } else { idle() },
            if tick == 0 { jump() } else { idle() },
        );
        let reacher = figure(&client, 0, tick);
        assert_eq!(reacher.arms[0].ink, Some(GRAB));
        assert_eq!(reacher.arms[1].ink, Some(GRAB.scaled(0.7)));
        seen.push((state(&client, 0), reacher));
    }
    use AttackPhase::*;
    let states: Vec<FighterState> = seen.iter().map(|s| s.0).collect();
    assert_eq!(
        states,
        [Startup, Startup, Active, Recovery, Recovery, Recovery].map(FighterState::Grabbing)
    );
    assert_local(&client, 0, seen[0].1.arms[0].end, 16.0, 80.0);
    assert_local(&client, 0, seen[2].1.arms[0].end, MAX_REACH, 80.0);
    assert_local(&client, 0, seen[3].1.arms[0].end, 14.0, 74.0);
}

/// Player 0 tries `technique` with a range too short to catch anybody; its figure on every
/// tick of the attempt.
fn whiffed(technique: Technique) -> Vec<(AttackPhase, Figure)> {
    let mut config = judo();
    for data in &mut config.techniques {
        data.range = 1;
    }
    let mut client = duel(config, 0);
    play_as(&mut client, 0, keys_of(technique));
    let mut seen = Vec::new();
    while let FighterState::Technique(_, phase) = state(&client, 0) {
        seen.push((phase, figure(&client, 0, 0)));
        assert!(seen.len() < 100, "the technique ends");
        idle_for(&mut client, 1);
    }
    for phase in [
        AttackPhase::Startup,
        AttackPhase::Active,
        AttackPhase::Recovery,
    ] {
        assert!(seen.iter().any(|s| s.0 == phase), "{technique}: {phase:?}");
    }
    seen
}

#[test]
fn the_techniques_reach_in_with_their_judo_limbs() {
    for (_, throw) in whiffed(Technique::Throw) {
        assert_eq!(throw.arms[0].ink, Some(JUDO));
        assert_eq!(throw.arms[1].ink, Some(JUDO.scaled(0.7)));
        assert_eq!(throw.legs[0].ink, None);
    }
    let sweep = whiffed(Technique::Sweep);
    for (_, figure) in &sweep {
        assert_eq!(figure.legs[0].ink, Some(JUDO), "the reaping leg");
        assert_eq!(
            figure.arms[1].ink,
            Some(JUDO.scaled(0.7)),
            "the gripping arm"
        );
    }
    let (_, active) = sweep.iter().find(|s| s.0 == AttackPhase::Active).unwrap();
    assert!(active.legs[0].end.y > FLOOR_Y - 10.0, "low, at the ankles");
}

#[test]
fn a_clinch_wraps_both_figures_arms_around_each_other() {
    let mut client = duel(cfg(), 0);
    clinch(&mut client, 0);
    let (x0, _) = feet_of(&client, 0);
    let (x1, _) = feet_of(&client, 1);
    for player in [0, 1] {
        let hugger = figure(&client, player, 0);
        assert_eq!(hugger.arms[0].ink, Some(GRAB));
        assert_eq!(hugger.arms[1].ink, Some(GRAB.scaled(0.7)));
        assert!(!hugger.lying);
        let hand = hugger.arms[0].end.x;
        assert!(x0.min(x1) < hand && hand < x0.max(x1), "between the two");
    }
    let frame = arena_frame(&client, 0);
    assert!(frame.labels.iter().all(|l| l.y < FLOOR_Y), "both stand");
}

/// The tag of `player` in `frame`.
fn tag_y(frame: &Frame, player: u8) -> f32 {
    find_label(frame, &format!("P{} Chalk", player + 1))
        .unwrap()
        .y
}

/// Asserts that the lying `player` has its head away from `other`, farther than its hips.
fn assert_fallen_away(client: &Client, player: u8, other: u8) {
    let lying = figure(client, player, 0);
    assert!(lying.lying, "P{} lies down", player + 1);
    assert!(lying.head.y > FLOOR_Y - 30.0, "the head on the floor");
    let (from, _) = feet_of(client, other);
    assert!(
        (lying.head.x - from).abs() > (lying.hip.x - from).abs(),
        "fallen backwards, away from P{}",
        other + 1
    );
}

#[test]
fn the_mount_and_the_submission_pin_the_bottom_down() {
    for top in [0, 1] {
        let bottom = 1 - top;
        let mut client = duel(cfg(), 0);
        clinch(&mut client, top);
        climb(&mut client, top);
        assert_eq!(
            state(&client, top),
            grappling(GrappleRole::Top, GrapplePosition::Mount)
        );
        let kneeling = figure(&client, top, 0);
        assert!(!kneeling.lying);
        assert!(kneeling.hip.y > FLOOR_Y - 40.0, "kneeling");
        assert_eq!(kneeling.arms[0].ink, Some(GRAB), "the striking arm");
        assert_fallen_away(&client, bottom, top);
        assert_eq!(figure(&client, bottom, 0).arms[0].ink, None);
        let frame = arena_frame(&client, 0);
        assert_eq!(tag_y(&frame, bottom), LYING_TAG_Y);
        assert_eq!(tag_y(&frame, top), kneeling.top() - TAG_ABOVE);

        climb(&mut client, top);
        assert_eq!(
            state(&client, bottom),
            grappling(GrappleRole::Bottom, GrapplePosition::Submission)
        );
        assert_eq!(figure(&client, top, 0).arms[0].ink, Some(SUBMISSION));
        assert_eq!(figure(&client, bottom, 0).arms[0].ink, Some(SUBMISSION));
        assert_eq!(figure(&client, bottom, 6).arms[0].ink, None, "it flashes");
        assert_fallen_away(&client, bottom, top);
    }
}

#[test]
fn the_projections_lift_the_figures_off_the_floor() {
    for top in [0, 1] {
        let bottom = 1 - top;
        let mut client = duel(judo(), 0);
        project(&mut client, top, Technique::Sweep);
        let flying = figure(&client, top, 0);
        assert_eq!(flying.legs[0].ink, Some(JUDO), "the scissoring legs");
        assert_eq!(flying.legs[1].ink, Some(JUDO.scaled(0.7)));
        assert_local(&client, top, flying.hip, 0.0, 40.0 + SWEEP_LIFT);
        let swept = figure(&client, bottom, 0);
        assert_local(&client, bottom, swept.hip, -4.0, 44.0 + SWEEP_LIFT * 0.5);
        assert!(!swept.lying && !flying.lying);

        let mut client = duel(judo(), 0);
        project(&mut client, top, Technique::Throw);
        let thrower = figure(&client, top, 0);
        assert_eq!(thrower.arms[0].ink, Some(JUDO), "pulling the arm");
        let thrown = figure(&client, bottom, 0);
        assert_local(&client, bottom, thrown.hip, -12.0, 18.0 + THROW_LIFT);
        assert!(
            thrown.legs.iter().all(|leg| leg.end.y < thrown.neck.y),
            "head first, legs in the air"
        );
        let frame = arena_frame(&client, 0);
        assert_eq!(tag_y(&frame, bottom), thrown.top() - TAG_ABOVE);
    }
}

#[test]
fn the_finishes_hold_the_bottom_on_the_floor() {
    for top in [0, 1] {
        let bottom = 1 - top;
        let mut client = duel(judo(), 0);
        project(&mut client, top, Technique::Sweep);
        idle_for(&mut client, 20);
        assert_eq!(
            state(&client, top),
            grappling(GrappleRole::Top, GrapplePosition::ScarfHold)
        );
        let sitting = figure(&client, top, 0);
        assert!(!sitting.lying);
        assert!(sitting.hip.y > FLOOR_Y - 20.0, "sitting at the side");
        assert_eq!(sitting.arms[0].ink, Some(JUDO), "the arm around the neck");
        assert_eq!(figure(&client, bottom, 0).arms[0].ink, Some(SUBMISSION));
        assert_eq!(figure(&client, bottom, 6).arms[0].ink, None);
        assert_fallen_away(&client, bottom, top);

        let mut client = duel(judo(), 0);
        project(&mut client, top, Technique::Throw);
        idle_for(&mut client, 20);
        assert_eq!(
            state(&client, bottom),
            grappling(GrappleRole::Bottom, GrapplePosition::Armbar)
        );
        let locking = figure(&client, top, 0);
        assert!(locking.lying, "on the back, across the opponent");
        assert_eq!(locking.arms[0].ink, Some(SUBMISSION));
        assert!(
            locking.legs.iter().all(|leg| leg.ink.is_none()),
            "the legs hold"
        );
        assert_fallen_away(&client, bottom, top);
        let lit = arena_frame(&client, 0);
        let dark = arena_frame(&client, 6);
        assert!(rects_of(&lit, SUBMISSION).len() > rects_of(&dark, SUBMISSION).len());
        assert_eq!((tag_y(&lit, 0), tag_y(&lit, 1)), (LYING_TAG_Y, LYING_TAG_Y));
    }
}

#[test]
fn a_knocked_out_figure_lies_grey_with_crossed_out_eyes() {
    for winner in [0, 1] {
        let loser = 1 - winner;
        let mut client = duel(fragile(), 0);
        win_rounds(&mut client, winner, 1);
        assert_eq!(state(&client, loser), FighterState::Knockout);
        let out = figure(&client, loser, 0);
        assert_eq!((out.ink, out.mark), (KNOCKED_OUT, Mark::KnockedOut));
        assert_eq!(out.accent, arena::player_color(PlayerId(loser)).scaled(0.5));
        assert_fallen_away(&client, loser, winner);
        let frame = drawn(&out);
        assert!(rects_of(&frame, stick::INK).is_empty());
        assert!(rects_of(&frame, KNOCKED_OUT).len() > rects_of(&frame, SPARK).len());
        assert_eq!(tag_y(&arena_frame(&client, 0), loser), LYING_TAG_Y);
    }
}

#[test]
fn the_local_marker_floats_over_the_figure() {
    for local in [0, 1] {
        let mut client = duel(cfg(), local);
        play_as(&mut client, local, jump());
        let frame = arena_frame(&client, 0);
        let marker = rects_of(&frame, MARKER)[0];
        let top = figure(&client, local, 0).top();
        assert_eq!(marker.y, top - 8.0 - marker.height);
        assert_eq!(marker.height, 10.0);
        assert_eq!(tag_y(&frame, local), top - TAG_ABOVE);
    }
}

// ---------------------------------------------------------------------------------------------
// Music intensity and timer
// ---------------------------------------------------------------------------------------------

#[test]
fn seconds_are_rounded_up() {
    assert_eq!(seconds(0), 0);
    assert_eq!(seconds(1), 1);
    assert_eq!(seconds(TICKS_PER_SECOND), 1);
    assert_eq!(seconds(TICKS_PER_SECOND + 1), 2);
    assert_eq!(seconds(99 * TICKS_PER_SECOND), 99);
}

#[test]
fn a_fresh_round_is_not_tense() {
    let fresh = Match::new(Roster::duel(), cfg()).unwrap();
    assert_eq!(intensity(&fresh), 0.0);
}

#[test]
fn intensity_follows_the_lowest_health() {
    let mut client = duel(cfg(), 0);
    for _ in 0..3 {
        play_as(&mut client, 0, punch());
    }
    assert_eq!(health(&client, 1), 90);
    assert!((intensity(current(&client)) - 0.1).abs() < 1e-6);
    idle_for(&mut client, 3);
    for _ in 0..3 {
        play_as(&mut client, 0, punch());
    }
    assert_eq!(health(&client, 1), 80);
    assert!((intensity(current(&client)) - 0.2).abs() < 1e-6);
}

#[test]
fn intensity_rises_along_the_grapple_ladder() {
    let mut client = duel(cfg(), 0);
    clinch(&mut client, 0);
    assert_eq!(intensity(current(&client)), 0.3);
    climb(&mut client, 0);
    assert_eq!(intensity(current(&client)), 0.6);
    climb(&mut client, 0);
    assert_eq!(intensity(current(&client)), 1.0);
}

#[test]
fn the_last_ten_seconds_are_tense() {
    let client = duel(short_rounds(10 * TICKS_PER_SECOND), 0);
    assert_eq!(intensity(current(&client)), 0.5);
    let client = duel(short_rounds(10 * TICKS_PER_SECOND + 1), 0);
    assert_eq!(intensity(current(&client)), 0.0);
    let counting = MatchConfig {
        countdown_ticks: 10,
        ..short_rounds(60)
    };
    let client = duel(counting, 0);
    assert_eq!(intensity(current(&client)), 0.0, "not during the countdown");
}

#[test]
fn knocked_out_fighters_do_not_count() {
    let mut client = duel(fragile(), 0);
    win_rounds(&mut client, 0, 1);
    assert_eq!(health(&client, 1), 0);
    assert_eq!(intensity(current(&client)), 0.0);
}

// ---------------------------------------------------------------------------------------------
// Game: screens and input
// ---------------------------------------------------------------------------------------------

fn practice() -> Game {
    Game::new(Client::practice(MatchConfig::default()))
}

/// An online game whose duel (as `local`) has started, still on the intro.
fn online_game(config: MatchConfig, local: u8) -> Game {
    Game::new(duel(config, local))
}

/// Waits until the intro can be skipped, then presses Enter on the title and on the select
/// screen (keeping the character): the fight screen is up, Enter still held.
fn skip_intro(game: &mut Game) {
    while !game.intro().can_skip() {
        assert!(game.tick(&keys(&[])).is_empty());
    }
    game.tick(&keys(&[Key::Enter]));
    assert_eq!(game.screen(), Screen::Select);
    game.tick(&keys(&[]));
    game.tick(&keys(&[Key::Enter]));
    assert_eq!(game.screen(), Screen::Fight);
}

/// Skips the intro of a practice game: the training dummy is waiting.
fn practice_fight(game: &mut Game) {
    skip_intro(game);
    assert_eq!(game.practice(), Practice::Alone(Rival::Dummy));
}

/// One fight tick of an online game with the keys held, then the server's frame of that tick:
/// the input the game sent, and the opponent's `remote` one. As on a perfect network, the round
/// shown guesses the opponent's newest input (their previous one, again) until the next tick.
fn fight_tick(game: &mut Game, pressed: &[Key], remote: Input) -> Vec<Vec<u8>> {
    let outgoing = game.tick(&keys(pressed));
    let sent = outgoing
        .iter()
        .find_map(|frame| match ClientMessage::decode(frame) {
            Ok(ClientMessage::Input { input, .. }) => Some(input),
            _ => None,
        });
    if let Some(mine) = sent {
        let client = game.client_mut();
        let local = client.session().expect("a duel runs").local_player();
        let theirs = PlayerId(1 - local.0);
        relay_frame(client, &[(local, mine), (theirs, remote)], PlayerSet::EMPTY);
    }
    outgoing
}

#[test]
fn the_game_starts_on_the_intro_with_its_theme() {
    let game = practice();
    assert_eq!(game.screen(), Screen::Intro);
    assert_eq!(game.intro().ticks(), 0);
    assert_eq!(game.music().mood(), Mood::Intro);
    assert_eq!(game.music().intensity(), 0.0);
    assert!(game.client().is_local());
    assert_eq!(game.practice(), Practice::Alone(Rival::Dummy));
    assert!(!game.is_practising_online());
    assert!(game.health_bars().is_empty(), "nothing ticked yet");
}

#[test]
fn enter_cannot_skip_the_intro_too_early() {
    let mut game = practice();
    for _ in 0..SKIP_AFTER - 1 {
        game.tick(&keys(&[Key::Enter]));
        assert_eq!(game.screen(), Screen::Intro);
    }
    game.tick(&keys(&[]));
    assert_eq!(game.screen(), Screen::Intro);
    assert!(game.intro().can_skip());
    game.tick(&keys(&[Key::Enter]));
    assert_eq!(game.screen(), Screen::Select, "choose the character first");
    game.tick(&keys(&[]));
    game.tick(&keys(&[Key::Enter]));
    assert_eq!(game.screen(), Screen::Fight);
    assert_eq!(game.music().mood(), Mood::Fight, "the countdown runs");
}

#[test]
fn a_key_held_since_before_the_skip_delay_does_not_skip() {
    let mut game = practice();
    for _ in 0..SKIP_AFTER + 30 {
        game.tick(&keys(&[Key::Escape, Key::ArrowLeft]));
    }
    assert_eq!(
        game.screen(),
        Screen::Intro,
        "Escape was never pressed afresh"
    );
    game.tick(&keys(&[Key::ArrowLeft]));
    game.tick(&keys(&[Key::Escape, Key::ArrowLeft]));
    assert_eq!(game.screen(), Screen::Select);
}

#[test]
fn other_keys_do_not_skip_the_intro() {
    let mut game = practice();
    for tick in 0..100 {
        let pressed = if tick % 2 == 0 {
            keys(&[Key::Numpad0, Key::ArrowUp])
        } else {
            keys(&[])
        };
        game.tick(&pressed);
    }
    assert_eq!(game.screen(), Screen::Intro);
    assert_eq!(game.intro().ticks(), 100);
}

#[test]
fn the_intro_points_to_the_controls_page() {
    let game = practice();
    let frame = game.frame();
    let hint = find_label(&frame, CONTROLS_HINT).unwrap();
    assert_eq!((hint.x, hint.align), (24.0, Align::Left));
    assert!(hint.text.is_ascii());
}

#[test]
fn c_opens_the_controls_page_and_escape_comes_back_to_the_intro() {
    let mut game = practice();
    game.tick(&keys(&[]));
    game.tick(&keys(&[Key::C]));
    assert_eq!(game.screen(), Screen::Controls);
    assert_eq!(game.controls().selected(), Row::Preset);
    assert_eq!(game.music().mood(), Mood::Intro, "the intro theme goes on");
    let frame = game.frame();
    assert_eq!(frame.background, intro::BACKGROUND);
    assert!(find_label(&frame, "CONTROLS").is_some());
    assert!(find_label(&frame, "< Standard (arrows + keypad) >").is_some());
    let ticks = game.intro().ticks();
    for _ in 0..10 {
        game.tick(&keys(&[Key::C]));
    }
    assert_eq!(game.intro().ticks(), ticks, "the title waits");
    assert_eq!(game.screen(), Screen::Controls);

    game.tick(&keys(&[Key::Escape]));
    assert_eq!(game.screen(), Screen::Intro);
    game.tick(&keys(&[Key::Escape]));
    assert_eq!(
        game.screen(),
        Screen::Intro,
        "the same Escape does not skip too"
    );
}

#[test]
fn the_controls_page_opens_on_the_preset_row_every_time() {
    let mut game = practice();
    game.tick(&keys(&[Key::C]));
    game.tick(&keys(&[Key::ArrowDown]));
    assert_eq!(game.controls().selected(), Row::Action(0));
    game.tick(&keys(&[Key::Escape]));
    game.tick(&keys(&[Key::C]));
    assert_eq!(game.screen(), Screen::Controls);
    assert_eq!(game.controls().selected(), Row::Preset);
}

#[test]
fn the_layout_chosen_on_the_controls_page_drives_the_fight() {
    let mut game = online_game(cfg(), 0);
    assert_eq!(game.bindings(), &KeyBindings::STANDARD);
    game.tick(&keys(&[Key::C]));
    game.tick(&keys(&[Key::ArrowRight]));
    assert_eq!(game.bindings(), &KeyBindings::WASD);
    // Remap the grab to K: down to "Grab", Enter, K.
    for _ in 0..10 {
        game.tick(&keys(&[Key::ArrowDown]));
        game.tick(&keys(&[]));
    }
    assert_eq!(game.controls().selected(), Row::Action(9));
    game.tick(&keys(&[Key::Enter]));
    assert!(game.controls().is_waiting());
    game.tick(&keys(&[Key::K]));
    assert!(!game.controls().is_waiting());
    assert_eq!(game.bindings().grab, Key::K);
    game.tick(&keys(&[Key::Escape]));
    assert_eq!(game.screen(), Screen::Intro);

    skip_intro(&mut game);
    fight_tick(&mut game, &[Key::ArrowDown], idle());
    assert_eq!(
        state(game.client(), 0),
        FighterState::Idle,
        "the arrows do nothing"
    );
    fight_tick(&mut game, &[Key::S], idle());
    assert_eq!(state(game.client(), 0), FighterState::Crouching);
    fight_tick(&mut game, &[Key::K], idle());
    assert_eq!(
        state(game.client(), 0),
        FighterState::Grabbing(AttackPhase::Startup)
    );
    let text = game.frame().text();
    assert!(text.contains("Grab: K"), "{text}");
}

#[test]
fn the_platform_can_set_the_layout() {
    let mut game = practice();
    game.set_bindings(KeyBindings::WASD);
    assert_eq!(game.bindings(), &KeyBindings::WASD);
    skip_intro(&mut game);
    game.tick(&keys(&[]));
    let text = game.frame().text();
    for line in controls_lines(&KeyBindings::WASD) {
        assert!(text.contains(&line), "{text}");
    }
}

// ---------------------------------------------------------------------------------------------
// Game: pausing
// ---------------------------------------------------------------------------------------------

/// A practice game (with [`cfg`]: no countdown) fighting the dummy.
fn practice_duel() -> Game {
    let mut game = Game::new(Client::practice(cfg()));
    practice_fight(&mut game);
    game
}

fn session_tick(game: &Game) -> u32 {
    game.active_client().session().unwrap().tick()
}

#[test]
fn p_pauses_a_practice_fight_and_opens_the_pause_menu() {
    let mut game = practice_duel();
    game.tick(&keys(&[Key::ArrowRight]));
    assert_eq!(session_tick(&game), 1);
    assert!(!game.is_paused());

    assert!(game.tick(&keys(&[Key::P])).is_empty());
    assert_eq!(game.screen(), Screen::Controls);
    assert!(game.is_paused());
    assert!(game.controls().is_paused());
    assert_eq!(game.controls().selected(), Row::Preset);
    assert_eq!(game.controls().practice(), Practice::Alone(Rival::Dummy));
    assert_eq!(game.music().mood(), Mood::Breakdown, "not the intro theme");
    assert_eq!(game.music().intensity(), 0.0);
    let frame = game.frame();
    assert_eq!(frame.background, intro::BACKGROUND);
    assert!(find_label(&frame, "PAUSED").is_some());
    assert!(find_label(&frame, "Resume the fight").is_some());

    let frozen = game.shown_match().unwrap().clone();
    for _ in 0..20 {
        game.tick(&keys(&[Key::ArrowRight, Key::Numpad5]));
        game.tick(&keys(&[]));
    }
    assert_eq!(session_tick(&game), 1, "a local match freezes");
    assert_eq!(game.shown_match().unwrap(), &frozen);
    assert_eq!(game.screen(), Screen::Controls, "held keys do not leave");

    game.tick(&keys(&[Key::Escape]));
    assert_eq!(
        game.screen(),
        Screen::Fight,
        "back to the fight, not the title"
    );
    assert!(!game.is_paused());
    assert_eq!(session_tick(&game), 1, "the leaving tick is not played");
    assert_eq!(game.music().mood(), Mood::Fight);
    game.tick(&keys(&[Key::ArrowRight]));
    assert_eq!(session_tick(&game), 2);
    assert_eq!(state(game.client(), 0), FighterState::Walking);
}

#[test]
fn p_resumes_too_and_the_layout_changed_while_paused_drives_the_fight() {
    let mut game = practice_duel();
    game.tick(&keys(&[Key::P]));
    game.tick(&keys(&[Key::ArrowRight]));
    assert_eq!(game.bindings(), &KeyBindings::WASD);
    // Remap the grab to K: down to "Grab", Enter, K.
    for _ in 0..10 {
        game.tick(&keys(&[Key::ArrowDown]));
        game.tick(&keys(&[]));
    }
    game.tick(&keys(&[Key::Enter]));
    assert!(game.controls().is_waiting());
    game.tick(&keys(&[Key::P]));
    assert!(
        game.controls().is_waiting(),
        "P cannot be bound, nor resumes then"
    );
    assert_eq!(game.screen(), Screen::Controls);
    game.tick(&keys(&[Key::K]));
    assert_eq!(game.bindings().grab, Key::K);
    game.tick(&keys(&[]));
    game.tick(&keys(&[Key::P]));
    assert_eq!(game.screen(), Screen::Fight);
    assert!(!game.is_paused());
    assert_eq!(session_tick(&game), 0);

    game.tick(&keys(&[Key::ArrowDown]));
    assert_eq!(
        state(game.client(), 0),
        FighterState::Idle,
        "the arrows do nothing"
    );
    game.tick(&keys(&[Key::S]));
    assert_eq!(state(game.client(), 0), FighterState::Crouching);
    game.tick(&keys(&[Key::K]));
    assert_eq!(
        state(game.client(), 0),
        FighterState::Grabbing(AttackPhase::Startup)
    );
    assert!(game.frame().text().contains("Grab: K"));

    game.tick(&keys(&[Key::P]));
    assert_eq!(game.screen(), Screen::Controls, "a fresh P pauses again");
    assert_eq!(
        game.controls().selected(),
        Row::Preset,
        "on the preset row again"
    );
}

/// Up from the preset row: back, then the practice row.
fn up_to_the_practice_row(game: &mut Game) {
    for _ in 0..2 {
        game.tick(&keys(&[Key::ArrowUp]));
        game.tick(&keys(&[]));
    }
    assert_eq!(game.controls().selected(), Row::Practice);
}

#[test]
fn the_pause_menu_switches_the_rival_of_a_practice_game() {
    let mut game = practice_duel();
    game.tick(&keys(&[Key::ArrowRight]));
    game.tick(&keys(&[Key::P]));
    assert!(game.is_paused());
    let text = game.frame().text();
    assert!(text.contains("Practice"), "{text}");
    assert!(text.contains("< the training dummy >"), "{text}");
    game.tick(&keys(&[PRACTICE_KEY]));
    assert_eq!(
        game.client().rival(),
        Some(Rival::Dummy),
        "T is for the fight screen"
    );

    up_to_the_practice_row(&mut game);
    game.tick(&keys(&[Key::ArrowRight]));
    assert_eq!(game.controls().practice(), Practice::Alone(Rival::Brawler));
    assert_eq!(
        game.client().rival(),
        Some(Rival::Brawler),
        "carried out at once"
    );
    assert_eq!(game.practice(), Practice::Alone(Rival::Brawler));
    assert!(
        !game.is_practising_online(),
        "a practice game is not online"
    );
    assert_eq!(session_tick(&game), 1, "still frozen");
    assert!(game.frame().text().contains("< the computer >"));

    game.tick(&keys(&[Key::Escape]));
    assert_eq!(game.screen(), Screen::Fight);
    game.tick(&keys(&[]));
    assert_eq!(session_tick(&game), 2);
    let text = game.frame().text();
    assert!(text.contains("CPU Chalk  100 HP"), "{text}");
    assert!(
        text.contains("Practice: the computer   T: switch to the patient computer"),
        "{text}"
    );
}

#[test]
fn an_online_match_goes_on_while_paused_with_the_player_standing_still() {
    let mut game = online_game(cfg(), 0);
    skip_intro(&mut game);
    fight_tick(&mut game, &[Key::ArrowRight], idle());
    assert_eq!(state(game.client(), 0), FighterState::Walking);
    assert_eq!(session_tick(&game), 1);

    let frames = fight_tick(&mut game, &[Key::P, Key::ArrowRight], right());
    assert!(game.is_paused());
    assert_eq!(frames.len(), 1, "the pausing tick is still played");
    assert_eq!(session_tick(&game), 2, "an online match cannot wait");
    let x_of = |game: &Game, player: u8| {
        current(game.client())
            .fighter(PlayerId(player))
            .unwrap()
            .x()
    };
    let (mine, theirs) = (x_of(&game, 0), x_of(&game, 1));
    for _ in 0..5 {
        let frames = fight_tick(&mut game, &[Key::ArrowRight, Key::Numpad5], right());
        assert_eq!(frames.len(), 1, "an idle input goes out every tick");
        let Ok(ClientMessage::Input { input, .. }) = ClientMessage::decode(&frames[0]) else {
            panic!("not an input");
        };
        assert_eq!(
            input,
            Input::NONE,
            "the keys held drive the menu, not the fighter"
        );
    }
    assert_eq!(session_tick(&game), 7);
    assert_eq!(
        state(game.client(), 0),
        FighterState::Idle,
        "standing still"
    );
    assert_eq!(x_of(&game, 0), mine);
    assert!(x_of(&game, 1) > theirs, "the others go on");
    assert_eq!(game.screen(), Screen::Controls);

    let frames = fight_tick(&mut game, &[Key::Escape], idle());
    assert_eq!(game.screen(), Screen::Fight);
    assert_eq!(frames.len(), 1, "the resuming tick is played too");
    assert_eq!(session_tick(&game), 8);
    fight_tick(&mut game, &[Key::ArrowRight], idle());
    assert_eq!(state(game.client(), 0), FighterState::Walking);
}

#[test]
fn the_practice_fight_played_online_freezes_while_paused() {
    let mut game = alone_online(0);
    game.tick(&keys(&[PRACTICE_KEY]));
    assert!(game.is_practising_online());
    game.tick(&keys(&[]));
    assert_eq!(session_tick(&game), 2);
    assert!(
        game.tick(&keys(&[Key::P])).is_empty(),
        "nothing to send: the room is idle"
    );
    assert!(game.is_paused());
    assert_eq!(
        game.controls().practice(),
        Practice::WhileWaiting(Rival::Brawler)
    );
    for _ in 0..10 {
        assert!(game.tick(&keys(&[Key::ArrowLeft])).is_empty());
    }
    assert_eq!(
        session_tick(&game),
        2,
        "the practice fight is local: it waits"
    );
    assert!(game.is_practising_online());
    game.tick(&keys(&[Key::Escape]));
    assert_eq!(game.screen(), Screen::Fight);
    assert!(
        game.is_practising_online(),
        "Escape left the menu, not the practice"
    );
    game.tick(&keys(&[]));
    assert_eq!(session_tick(&game), 3);
}

#[test]
fn the_pause_menu_starts_and_ends_a_practice_fight_while_online() {
    let mut game = alone_online(0);
    game.tick(&keys(&[Key::P]));
    assert_eq!(game.controls().practice(), Practice::Off);
    let text = game.frame().text();
    assert!(
        text.contains("Practice") && text.contains("< off >"),
        "{text}"
    );
    up_to_the_practice_row(&mut game);
    assert!(
        game.tick(&keys(&[Key::Enter])).is_empty(),
        "Enter: the next choice"
    );
    assert_eq!(
        game.controls().practice(),
        Practice::WhileWaiting(Rival::Brawler)
    );
    assert!(game.is_practising_online(), "started from the menu");
    assert_eq!(game.practice(), Practice::WhileWaiting(Rival::Brawler));
    assert_eq!(game.active_client().rival(), Some(Rival::Brawler));
    assert_eq!(game.client().status(), Status::Lobby, "the room stays open");
    assert!(
        game.frame()
            .text()
            .contains("< the computer - the room stays open >")
    );
    game.tick(&keys(&[]));
    game.tick(&keys(&[Key::ArrowRight]));
    assert_eq!(game.practice(), Practice::WhileWaiting(Rival::Patient));
    assert_eq!(game.active_client().rival(), Some(Rival::Patient));
    game.tick(&keys(&[]));
    game.tick(&keys(&[Key::ArrowRight]));
    assert_eq!(game.practice(), Practice::WhileWaiting(Rival::Aggressive));
    game.tick(&keys(&[]));
    game.tick(&keys(&[Key::ArrowRight]));
    assert_eq!(game.practice(), Practice::WhileWaiting(Rival::Dummy));
    game.tick(&keys(&[]));
    game.tick(&keys(&[Key::ArrowRight]));
    assert_eq!(game.controls().practice(), Practice::Off, "round and round");
    assert!(!game.is_practising_online(), "ended from the menu");
    game.tick(&keys(&[]));
    game.tick(&keys(&[Key::ArrowLeft]));
    assert_eq!(game.practice(), Practice::WhileWaiting(Rival::Dummy));
    game.tick(&keys(&[Key::P]));
    assert_eq!(game.screen(), Screen::Fight);
    assert!(game.is_practising_online());
    assert_eq!(game.local_player(), Some(PlayerId(0)));
}

#[test]
fn the_intro_does_not_play_the_duel() {
    let mut game = online_game(cfg(), 0);
    for _ in 0..50 {
        assert!(game.tick(&keys(&[Key::ArrowRight])).is_empty());
    }
    assert_eq!(game.client().session().unwrap().tick(), 0);
    assert_eq!(game.music().mood(), Mood::Intro);
    assert_eq!(
        game.health_bars().len(),
        2,
        "the bars already follow the duel"
    );
}

#[test]
fn the_fight_screen_plays_the_practice_duel() {
    let mut game = practice();
    practice_fight(&mut game);
    assert_eq!(
        game.client().session().unwrap().tick(),
        0,
        "the intro does not play the round"
    );
    assert!(game.tick(&keys(&[])).is_empty(), "practice sends nothing");
    assert_eq!(game.client().session().unwrap().tick(), 1);
    let countdown = MatchConfig::default().countdown_ticks;
    for _ in 0..countdown + 5 {
        game.tick(&keys(&[Key::ArrowRight]));
    }
    let shown = game.shown_match().unwrap();
    assert_eq!(shown.phase(), Phase::Fighting);
    let p0 = shown.fighter(PlayerId(0)).unwrap();
    assert!(p0.x() > 400, "the arrow walked player 1: {}", p0.x());
    assert_eq!(game.local_player(), Some(PlayerId(0)));
}

#[test]
fn the_fight_screen_sends_the_local_input_online() {
    let mut game = online_game(cfg(), 1);
    skip_intro(&mut game);
    let frames = fight_tick(&mut game, &[Key::Numpad0], idle());
    assert_eq!(frames.len(), 1, "an input frame");
    let Ok(ClientMessage::Input {
        session,
        tick,
        input,
    }) = ClientMessage::decode(&frames[0])
    else {
        panic!("not an input");
    };
    assert_eq!((session, tick), (SESSION, 0));
    assert!(input.grab);
    assert_eq!(game.local_player(), Some(PlayerId(1)));

    let frames = fight_tick(&mut game, &[Key::Enter], idle());
    assert_eq!(frames.len(), 1, "Enter means nothing during the round");
}

#[test]
fn enter_after_the_round_tells_the_server_the_player_is_ready() {
    let mut game = online_game(short_rounds(2), 0);
    skip_intro(&mut game);
    fight_tick(&mut game, &[], idle());
    fight_tick(&mut game, &[], idle());
    assert!(game.shown_match().unwrap().outcome().is_some());
    let frames = fight_tick(&mut game, &[Key::Enter], idle());
    assert_eq!(
        frames[0],
        ClientMessage::Ready {
            character: Character::Chalk
        }
        .encode(),
        "ready first"
    );
    assert_eq!(frames.len(), 2, "then the (ignored) input of the tick");
    assert!(
        fight_tick(&mut game, &[Key::Enter], idle()).len() == 1,
        "held, not pressed afresh"
    );
}

#[test]
fn health_bars_follow_the_fighters() {
    let mut game = online_game(cfg(), 0);
    skip_intro(&mut game);
    for _ in 0..3 {
        fight_tick(&mut game, &[], punch());
    }
    let bars = game.health_bars();
    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].player(), PlayerId(0));
    assert_eq!(bars[1].player(), PlayerId(1));
    assert_eq!((bars[0].health(), bars[0].trail()), (90, 100));
    assert_eq!((bars[1].health(), bars[1].trail()), (100, 100));
    for _ in 0..TRAIL_HOLD + 20 {
        fight_tick(&mut game, &[], idle());
    }
    assert_eq!(game.health_bars()[0].trail(), 90, "drained");
}

#[test]
fn health_bars_count_the_combos() {
    let mut game = online_game(stunning(), 0);
    skip_intro(&mut game);
    for _ in 0..3 {
        fight_tick(&mut game, &[], punch()); // the jab lands on the third tick
    }
    assert_eq!(game.health_bars()[0].hits(), 1);
    let medium = hold(Input::pressing(Attack::punch(Strength::Medium)));
    for _ in 0..3 {
        fight_tick(&mut game, &[], medium); // chained at once, lands on the sixth tick
    }
    assert_eq!(game.health_bars()[0].health(), 80);
    assert_eq!(game.health_bars()[0].hits(), 2);
    assert_eq!(game.health_bars()[1].hits(), 0);
}

#[test]
fn without_a_duel_there_are_no_health_bars() {
    let mut game = Game::new(Client::online(cfg(), 0));
    skip_intro(&mut game);
    assert!(game.tick(&keys(&[])).is_empty());
    assert!(game.health_bars().is_empty());
    assert!(game.shown_match().is_none());
    assert_eq!(game.local_player(), None);
    assert_eq!(game.music().mood(), Mood::Breakdown, "waiting");

    game.client_mut().handle(ServerMessage::Welcome {
        player: PlayerId(0),
    });
    game.client_mut().handle(ServerMessage::Start {
        session: SESSION,
        players: PlayerSet::first(2),
        characters: [Character::Chalk; 10],
    });
    fight_tick(&mut game, &[], idle());
    assert_eq!(game.health_bars().len(), 2, "built for the new duel");
}

#[test]
fn a_new_round_refills_the_bars() {
    let mut game = online_game(fragile(), 0);
    skip_intro(&mut game);
    while game.shown_match().unwrap().outcome().is_none() {
        fight_tick(&mut game, &[], punch());
    }
    assert_eq!(game.health_bars()[0].health(), 0);
    fight_tick(&mut game, &[Key::Enter], idle());
    next_round(game.client_mut());
    fight_tick(&mut game, &[], idle());
    let bars = game.health_bars();
    assert_eq!((bars[0].health(), bars[0].trail()), (10, 10));
}

// ---------------------------------------------------------------------------------------------
// Game: music
// ---------------------------------------------------------------------------------------------

#[test]
fn the_music_follows_the_fight() {
    let mut game = online_game(cfg(), 0);
    skip_intro(&mut game);
    fight_tick(&mut game, &[], idle());
    assert_eq!(game.music().mood(), Mood::Fight);
    assert_eq!(game.music().intensity(), 0.0);
    for _ in 0..3 {
        fight_tick(&mut game, &[], punch());
    }
    let expected = intensity(game.shown_match().unwrap());
    assert!(expected > 0.0);
    assert_eq!(game.music().intensity(), expected);
}

#[test]
fn the_countdown_plays_the_groove_without_tension() {
    let config = MatchConfig {
        countdown_ticks: 30,
        round_time_ticks: 5 * TICKS_PER_SECOND,
        ..cfg()
    };
    let mut game = online_game(config, 0);
    skip_intro(&mut game);
    fight_tick(&mut game, &[], idle());
    assert!(matches!(
        game.shown_match().unwrap().phase(),
        Phase::Countdown { .. }
    ));
    assert_eq!(game.music().mood(), Mood::Fight);
    assert_eq!(
        game.music().intensity(),
        0.0,
        "the last seconds only count while fighting"
    );
}

#[test]
fn the_music_breaks_down_between_rounds_and_when_the_opponent_leaves() {
    let mut game = online_game(fragile(), 0);
    skip_intro(&mut game);
    while game.shown_match().unwrap().outcome().is_none() {
        fight_tick(&mut game, &[Key::Numpad1], idle());
    }
    assert_eq!(game.music().mood(), Mood::Breakdown);
    assert_eq!(game.music().intensity(), 0.0);

    next_round(game.client_mut());
    fight_tick(&mut game, &[], idle());
    assert_eq!(game.music().mood(), Mood::Fight, "a new round");

    game.client_mut().handle(ServerMessage::PlayerLeft {
        player: PlayerId(1),
    });
    game.tick(&keys(&[]));
    assert_eq!(game.music().mood(), Mood::Breakdown);
    assert!(game.shown_match().is_some(), "the last round stays visible");
}

// ---------------------------------------------------------------------------------------------
// Game: status line and frames
// ---------------------------------------------------------------------------------------------

#[test]
fn the_status_line_describes_practice_and_every_connection_state() {
    assert_eq!(
        practice().status_line(),
        "practice mode: versus the training dummy - T or the pause menu: change the rival"
    );
    assert_eq!(
        Game::new(Client::versus(cfg(), Rival::Brawler)).status_line(),
        "practice mode: versus the computer - T or the pause menu: change the rival"
    );

    let mut game = Game::new(Client::online(cfg(), 0));
    assert_eq!(game.status_line(), "connecting to the server...");
    game.client_mut().handle(ServerMessage::Welcome {
        player: PlayerId(0),
    });
    assert_eq!(game.status_line(), "connected - nobody else is here yet");
    lobby(game.client_mut(), PlayerSet::first(3), PlayerSet::EMPTY);
    assert_eq!(
        game.status_line(),
        "3 players in the room - press Enter to fight"
    );
    game.client_mut().handle(ServerMessage::Start {
        session: SESSION,
        players: PlayerSet::first(3),
        characters: [Character::Chalk; 10],
    });
    assert_eq!(game.status_line(), "a match is running - press Enter");
    game.client_mut().handle(ServerMessage::RoomFull);
    assert_eq!(
        game.status_line(),
        "the room is full (10 players) - try again later"
    );

    let mut lost = Game::new(Client::online(cfg(), 0));
    lost.client_mut().disconnected();
    assert_eq!(
        lost.status_line(),
        "disconnected from the server - reload the page"
    );

    let mut broken = Game::new(Client::online(cfg(), 0));
    broken.client_mut().receive(&[]);
    assert_eq!(
        broken.status_line(),
        "error: bad message from the server: empty message"
    );
}

#[test]
fn the_intro_frame_shows_the_status_line() {
    let mut game = Game::new(Client::online(cfg(), 0));
    for _ in 0..400 {
        game.tick(&keys(&[]));
    }
    let frame = game.frame();
    assert_eq!(frame.background, srr::front::intro::BACKGROUND);
    let status = frame.labels.last().unwrap();
    assert_eq!(status.text, "connecting to the server...");
    assert_eq!(status.align, Align::Center);
    assert!(frame.text().contains(srr::front::intro::PROMPT));
}

#[test]
fn the_intro_tells_whether_the_music_plays() {
    let mut game = practice();
    assert_eq!(
        game.sound(),
        SoundState::Waiting,
        "browsers need a gesture first"
    );
    let waiting = game.frame();
    let note = find_label(&waiting, "sound: press a key or click").expect("the invitation");
    assert_eq!(note.align, Align::Right);
    assert!(note.x < SCREEN_WIDTH && note.y < 40.0, "top-right corner");
    let invitation = note.color;

    game.set_sound(SoundState::Playing);
    assert_eq!(game.sound(), SoundState::Playing);
    let playing = game.frame();
    let note = find_label(&playing, "sound: on").expect("the confirmation");
    assert_ne!(note.color, invitation, "only the invitation stands out");
    assert!(find_label(&playing, "sound: press a key or click").is_none());

    game.set_sound(SoundState::Unavailable);
    assert!(find_label(&game.frame(), "sound: no audio output").is_some());
    assert_eq!(SoundState::default(), SoundState::Waiting);

    skip_intro(&mut game);
    game.tick(&keys(&[]));
    let fight = game.frame();
    assert!(
        fight.labels.iter().all(|l| !l.text.starts_with("sound:")),
        "the HUD stays clean"
    );
}

#[test]
fn the_fight_frame_shows_the_arena_and_the_hud() {
    let mut game = practice();
    skip_intro(&mut game);
    game.tick(&keys(&[]));
    let frame = game.frame();
    assert_eq!(frame.background, arena::BACKGROUND);
    let text = frame.text();
    assert!(text.contains("P1 (you) Chalk  100 HP"), "{text}");
    assert!(text.contains("P2 Chalk  100 HP"));
    let notice = find_label(
        &frame,
        "Practice: the training dummy   T: switch to the computer",
    )
    .expect("the practice notice");
    assert_eq!((notice.y, notice.align), (NOTICE_Y, Align::Center));
    for line in controls_lines(&KeyBindings::STANDARD) {
        assert!(text.contains(&line), "{text}");
    }
    assert!(text.contains("99"), "the timer");
    assert!(find_label(&frame, "2").is_some(), "the countdown");
    assert!(rects_of(&frame, MARKER).len() == 1);
    assert!(
        frame
            .labels
            .iter()
            .any(|l| l.text == "P1 Chalk" && l.align == Align::Center)
    );
}

#[test]
fn a_whole_practice_round_can_be_played_and_restarted() {
    let config = MatchConfig {
        countdown_ticks: 10,
        round_time_ticks: 2 * TICKS_PER_SECOND,
        ..cfg()
    };
    let mut game = Game::new(Client::practice(config));
    practice_fight(&mut game);
    for _ in 0..200 {
        game.tick(&keys(&[Key::Numpad6]));
    }
    let shown = game.shown_match().unwrap();
    assert!(matches!(shown.phase(), Phase::Finished(_)));
    let frame = game.frame();
    assert!(frame.text().contains("You win!"));
    assert!(frame.text().contains("Enter for a rematch"));
    game.tick(&keys(&[Key::Enter]));
    assert!(matches!(
        game.shown_match().unwrap().phase(),
        Phase::Countdown { .. }
    ));
}

// ---------------------------------------------------------------------------------------------
// Game: practice
// ---------------------------------------------------------------------------------------------

/// An online game seated as `player`, alone in the room, on the fight screen.
fn alone_online(player: u8) -> Game {
    let mut client = Client::online(cfg(), 0);
    client.handle(ServerMessage::Welcome {
        player: PlayerId(player),
    });
    lobby(&mut client, set(&[player]), PlayerSet::EMPTY);
    let mut game = Game::new(client);
    skip_intro(&mut game);
    game
}

#[test]
fn t_switches_the_rival_of_a_practice_game() {
    let mut game = practice();
    practice_fight(&mut game);
    assert_eq!(
        game.practice_notice().as_deref(),
        Some("Practice: the training dummy   T: switch to the computer")
    );
    game.tick(&keys(&[Key::ArrowRight]));
    assert_eq!(session_tick(&game), 1);

    assert!(game.tick(&keys(&[PRACTICE_KEY])).is_empty());
    assert_eq!(game.client().rival(), Some(Rival::Brawler));
    assert_eq!(game.practice(), Practice::Alone(Rival::Brawler));
    assert_eq!(session_tick(&game), 2, "the round goes on");
    assert!(!game.is_practising_online());
    assert_eq!(
        game.practice_notice().as_deref(),
        Some("Practice: the computer   T: switch to the patient computer")
    );
    for _ in 0..3 {
        game.tick(&keys(&[PRACTICE_KEY]));
    }
    assert_eq!(
        game.client().rival(),
        Some(Rival::Brawler),
        "held, not pressed"
    );
    game.tick(&keys(&[]));
    let frame = game.frame();
    let text = frame.text();
    assert!(text.contains("CPU Chalk  100 HP"), "{text}");
    assert!(text.contains("P1 (you) Chalk  100 HP"), "{text}");
    assert!(
        frame
            .labels
            .iter()
            .any(|l| l.text == "CPU Chalk" && l.size == 14.0),
        "the tag over the bot"
    );

    game.tick(&keys(&[PRACTICE_KEY]));
    assert_eq!(
        game.client().rival(),
        Some(Rival::Patient),
        "round and round"
    );
    game.tick(&keys(&[]));
    assert!(
        game.frame().text().contains("CPU Chalk  100 HP"),
        "still a computer"
    );
    game.tick(&keys(&[Key::Escape]));
    assert_eq!(
        game.practice(),
        Practice::Alone(Rival::Patient),
        "Escape leaves nothing: a practice game is nothing else"
    );
}

#[test]
fn t_on_the_title_goes_practising() {
    let mut game = practice();
    game.tick(&keys(&[PRACTICE_KEY]));
    assert_eq!(game.screen(), Screen::Intro, "too early, like Enter");
    while !game.intro().can_skip() {
        game.tick(&keys(&[]));
    }
    game.tick(&keys(&[PRACTICE_KEY]));
    assert_eq!(game.screen(), Screen::Select);
    game.tick(&keys(&[Key::Enter]));
    assert_eq!(game.screen(), Screen::Fight);
    assert_eq!(
        game.practice(),
        Practice::Alone(Rival::Dummy),
        "a practice game keeps its rival"
    );

    let mut client = Client::online(cfg(), 0);
    client.handle(ServerMessage::Welcome {
        player: PlayerId(2),
    });
    lobby(&mut client, set(&[2]), PlayerSet::EMPTY);
    let mut online = Game::new(client);
    while !online.intro().can_skip() {
        online.tick(&keys(&[]));
    }
    assert!(online.tick(&keys(&[PRACTICE_KEY])).is_empty());
    assert_eq!(online.screen(), Screen::Select);
    assert!(
        !online.is_practising_online(),
        "not before the character is picked"
    );
    assert!(online.tick(&keys(&[Key::Enter])).is_empty());
    assert_eq!(online.screen(), Screen::Fight);
    assert!(
        online.is_practising_online(),
        "online: the practice fight starts"
    );
    assert_eq!(online.practice(), Practice::WhileWaiting(Rival::Brawler));
    assert_eq!(
        online.local_player(),
        Some(PlayerId(0)),
        "player 1 of the local fight"
    );

    let mut connecting = Game::new(Client::online(cfg(), 0));
    while !connecting.intro().can_skip() {
        connecting.tick(&keys(&[]));
    }
    connecting.tick(&keys(&[PRACTICE_KEY]));
    connecting.tick(&keys(&[Key::Enter]));
    assert_eq!(connecting.screen(), Screen::Fight);
    assert!(
        !connecting.is_practising_online(),
        "not seated: nothing to start"
    );
    assert_eq!(connecting.practice(), Practice::Unavailable);
}

#[test]
fn online_the_practice_fight_is_played_locally_while_the_room_stays_open() {
    let mut game = alone_online(0);
    assert_eq!(game.practice(), Practice::Off);
    let hint = "T: practice while waiting";
    assert_eq!(game.practice_notice().as_deref(), Some(hint));
    assert_eq!(find_label(&game.frame(), hint).unwrap().y, NOTICE_Y);
    assert_eq!(game.music().mood(), Mood::Breakdown, "waiting");

    game.tick(&keys(&[PRACTICE_KEY]));
    assert!(game.is_practising_online());
    assert_eq!(game.active_client().rival(), Some(Rival::Brawler));
    assert_eq!(game.client().status(), Status::Lobby, "the room stays open");
    let alone =
        "Practice: the computer   T: switch to the patient computer   Esc: back to the room";
    assert_eq!(game.practice_notice().as_deref(), Some(alone));

    assert_eq!(
        game.active_client().session().unwrap().tick(),
        1,
        "the T tick already plays"
    );
    assert!(game.tick(&keys(&[])).is_empty(), "nothing goes online");
    assert_eq!(game.active_client().session().unwrap().tick(), 2);
    assert_eq!(game.local_player(), Some(PlayerId(0)));
    assert_eq!(game.health_bars().len(), 2);
    assert_eq!(game.music().mood(), Mood::Fight);
    let frame = game.frame();
    let notice = find_label(&frame, alone).unwrap();
    assert_eq!(notice.y, NOTICE_Y);
    assert!(frame.text().contains("CPU Chalk  100 HP"));

    game.tick(&keys(&[PRACTICE_KEY]));
    assert_eq!(
        game.active_client().rival(),
        Some(Rival::Patient),
        "T switches here too"
    );
    assert!(game.is_practising_online());
    game.tick(&keys(&[]));
    game.tick(&keys(&[PRACTICE_KEY]));
    assert_eq!(game.active_client().rival(), Some(Rival::Aggressive));
    game.tick(&keys(&[]));
    game.tick(&keys(&[PRACTICE_KEY]));
    assert_eq!(game.active_client().rival(), Some(Rival::Dummy));
    game.tick(&keys(&[]));
    game.tick(&keys(&[PRACTICE_KEY]));
    assert!(!game.is_practising_online(), "then off");
    assert_eq!(game.practice(), Practice::Off);
    game.tick(&keys(&[]));
    game.tick(&keys(&[PRACTICE_KEY]));
    assert!(game.is_practising_online());

    // Somebody joins: the notice says so.
    lobby(game.client_mut(), PlayerSet::first(3), PlayerSet::EMPTY);
    game.tick(&keys(&[]));
    assert_eq!(
        game.practice_notice().as_deref(),
        Some("P2, P3 is here - Esc: leave the practice")
    );

    // Escape goes back to the room, where Enter tells the server.
    game.tick(&keys(&[Key::Escape]));
    assert!(!game.is_practising_online());
    assert_eq!(game.active_client().status(), Status::Lobby);
    assert_eq!(
        game.practice_notice().as_deref(),
        Some(hint),
        "not alone, still waiting"
    );
    assert_eq!(
        game.tick(&keys(&[Key::Enter])),
        vec![
            ClientMessage::Ready {
                character: Character::Chalk
            }
            .encode()
        ]
    );
    game.tick(&keys(&[Key::Escape]));
    assert!(!game.is_practising_online(), "Escape again changes nothing");
}

#[test]
fn a_real_match_takes_over_from_the_practice_fight() {
    let mut game = alone_online(0);
    game.tick(&keys(&[]));
    assert_eq!(
        game.tick(&keys(&[Key::Enter])),
        vec![
            ClientMessage::Ready {
                character: Character::Chalk
            }
            .encode()
        ],
        "ready in the lobby"
    );
    game.tick(&keys(&[PRACTICE_KEY]));
    assert!(game.is_practising_online());
    // Somebody joined and got ready meanwhile: the server starts the match.
    game.client_mut().handle(ServerMessage::Start {
        session: SESSION,
        players: PlayerSet::first(2),
        characters: [Character::Chalk; 10],
    });
    let frames = fight_tick(&mut game, &[], idle());
    assert!(!game.is_practising_online());
    assert_eq!(frames.len(), 1, "the input of the real match");
    assert_eq!(game.active_client().status(), Status::Playing);
    assert_eq!(game.client().session().unwrap().tick(), 1);
    assert_eq!(game.practice(), Practice::Unavailable);
    assert_eq!(game.practice_notice(), None);
    assert!(fight_tick(&mut game, &[PRACTICE_KEY], idle()).len() == 1);
    assert!(!game.is_practising_online(), "T: nothing during a match");
}

#[test]
fn practice_cannot_start_during_an_online_match_nor_before_being_seated() {
    let connecting = Game::new(Client::online(cfg(), 0));
    assert_eq!(connecting.practice(), Practice::Unavailable);
    assert_eq!(connecting.practice_notice(), None);
    let mut playing = Game::new(duel(cfg(), 0));
    assert_eq!(playing.practice(), Practice::Unavailable);
    skip_intro(&mut playing);
    fight_tick(&mut playing, &[PRACTICE_KEY], idle());
    assert!(!playing.is_practising_online());
    playing.tick(&keys(&[Key::P]));
    assert_eq!(playing.controls().practice(), Practice::Unavailable);
    assert!(
        playing.frame().text().contains("- (not during a match)"),
        "{}",
        playing.frame().text()
    );
    up_to_the_practice_row(&mut playing);
    playing.tick(&keys(&[Key::ArrowRight]));
    assert_eq!(
        playing.controls().practice(),
        Practice::Unavailable,
        "inert"
    );
    assert!(!playing.is_practising_online());

    let versus = Game::new(Client::versus(cfg(), Rival::Brawler));
    assert_eq!(versus.practice(), Practice::Alone(Rival::Brawler));
}

#[test]
fn the_controls_page_of_the_title_chooses_the_rival_too() {
    let mut game = practice();
    game.tick(&keys(&[Key::C]));
    assert_eq!(game.controls().practice(), Practice::Alone(Rival::Dummy));
    up_to_the_practice_row(&mut game);
    game.tick(&keys(&[Key::ArrowLeft]));
    assert_eq!(game.client().rival(), Some(Rival::Aggressive));
    game.tick(&keys(&[Key::Escape]));
    assert_eq!(game.screen(), Screen::Intro);
    assert!(
        game.status_line()
            .starts_with("practice mode: versus the aggressive computer")
    );

    let mut online = alone_online(1);
    online.tick(&keys(&[Key::P]));
    up_to_the_practice_row(&mut online);
    online.tick(&keys(&[Key::ArrowLeft]));
    assert_eq!(online.practice(), Practice::WhileWaiting(Rival::Dummy));
    assert!(online.is_practising_online());
    assert!(
        online.tick(&keys(&[Key::P])).is_empty(),
        "nothing to send: the room is idle"
    );
    assert_eq!(online.screen(), Screen::Fight);
}

// ---------------------------------------------------------------------------------------------
// 1v1v1
// ---------------------------------------------------------------------------------------------

/// One tick of a match where player `i` holds `inputs[i]`.
fn play_all(client: &mut Client, inputs: &[Input]) {
    let local = client.session().expect("a match runs").local_player();
    let held: Vec<(PlayerId, Input)> = (0..).map(PlayerId).zip(inputs.iter().copied()).collect();
    relay_frame(client, &held, PlayerSet::EMPTY);
    let before = client.session().unwrap().tick();
    client.tick(inputs[usize::from(local.0)]);
    assert_eq!(client.session().unwrap().tick(), before + 1, "the tick ran");
}

#[test]
fn three_fighters_get_three_stacked_bars() {
    let client = brawl(cfg(), 1, PlayerSet::first(3));
    let frame = hud_frame(&client, &bars_of(&client), 1);
    let p1 = find_label(&frame, "P1 Chalk  100 HP").unwrap();
    let p2 = find_label(&frame, "P2 (you) Chalk  100 HP").unwrap();
    let p3 = find_label(&frame, "P3 Chalk  100 HP").unwrap();
    assert_eq!(
        (p1.align, p2.align, p3.align),
        (Align::Left, Align::Left, Align::Right)
    );
    assert_eq!(p2.color, LOCAL_NAME);
    assert_eq!(p2.y - p1.y, 64.0, "room for the grapple meter between rows");
    assert_eq!(p3.y, p1.y);
    assert_eq!(rects_of(&frame, HEALTH).len(), 3);

    let arena = arena_frame(&client, 1);
    for tag in ["P1 Chalk", "P2 Chalk", "P3 Chalk"] {
        assert!(find_label(&arena, tag).is_some(), "{tag}");
    }
    assert_eq!(rects_of(&arena, MARKER).len(), 1, "only the local fighter");
}

#[test]
fn a_three_player_round_goes_to_the_last_one_standing() {
    // Arena 300: fighters at 75, 150 and 225; player 1's punch (reach 150) catches both.
    let mut client = brawl(fragile(), 2, PlayerSet::first(3));
    let mut ticks = 0;
    while current(&client).outcome().is_none() {
        play_all(&mut client, &[punch(), idle(), idle()]);
        ticks += 1;
        assert!(ticks < 20, "the punch must land");
    }
    assert_eq!(
        current(&client).outcome(),
        Some(Outcome::Winner(srr::game::TeamId(0)))
    );
    assert_eq!(client.session().unwrap().wins(PlayerId(0)), 1);
    assert_eq!(
        hud::banner(&client),
        (Some("KO"), "P1 wins!\nEnter for the next round".into())
    );
    let frame = hud_frame(&client, &bars_of(&client), 1);
    let lit = rects_of(&frame, WIN_ON)
        .into_iter()
        .filter(|r| r.width == r.height)
        .count();
    assert_eq!(lit, 1, "one pip lit");
}
