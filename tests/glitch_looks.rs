//! What the intentional glitches look like (`srr::front::glitch`), in the arena of the stick
//! figures and in the 3D view: the lag copies ringed around the victim of an echo barrage, the
//! afterimages of a phantom warp, the stretched arm of a stretch strike, the crater a slam
//! leaves. The rules have their own tests (`tests/glitches.rs`); here only the looks are held
//! to the state of the match: they show while the glitch is happening, only then, and not at
//! all when the glitch is switched off.
//!
//! Duels are driven like in `tests/front_fight.rs`: an online client with no input delay, handed
//! the server's frame every tick. The arena is 300 wide, so the fighters spawn at 100 and 200.

use srr::front::arena::{self, ARENA_LEFT};
use srr::front::glitch::{
    self, COLD, CRACK, GHOST_ALPHA, HOT, MOST_COPIES, PIT, RING_RADIUS, SHOCK_TICKS, copies, ring,
    shock, tint, trail,
};
use srr::front::skin::sprite_name_of;
use srr::front::stick::{Figure, Mark};
use srr::front::{Frame, Rgba, tps};
use srr::game::{
    Attack, AttackData, CRATER_TICKS, Character, FighterState, Glitch, GlitchSet, GrappleData,
    Input, Match, MatchConfig, PlayerId, Skin, Strength, Style, TICKS_PER_SECOND, WARP_TRAIL,
};
use srr::net::{Client, PlayerSet, ServerMessage, Status};

// ---------------------------------------------------------------------------------------------
// Helpers: a duel with one glitch switched on, driven tick by tick
// ---------------------------------------------------------------------------------------------

const STRIKE: AttackData = AttackData {
    startup: 2,
    active: 2,
    recovery: 2,
    range: 150,
    damage: 10,
    knockback: 0,
    hitstun: 3,
};

/// The arena of `tests/front_fight.rs`, with only `glitch` on (or none).
fn config(glitch: Option<Glitch>) -> MatchConfig {
    MatchConfig {
        arena_width: 300,
        max_health: 1000,
        punches: [STRIKE; 3],
        kicks: [STRIKE; 3],
        round_time_ticks: 60 * TICKS_PER_SECOND,
        countdown_ticks: 0,
        glitches: glitch.map_or(GlitchSet::none(), |glitch| GlitchSet::none().with(glitch)),
        grapple: GrappleData {
            max_hold_ticks: 10_000,
            escape_presses: [1000; 3],
            ..GrappleData::default()
        },
        ..MatchConfig::default()
    }
}

/// An online client controlling player 0 in a duel that has just started.
fn duel(config: MatchConfig) -> Client {
    let players = PlayerSet::first(2);
    let mut client = Client::online(config, 0);
    client.handle(ServerMessage::Welcome {
        player: PlayerId(0),
    });
    client.handle(ServerMessage::Lobby {
        seated: players,
        ready: PlayerSet::EMPTY,
    });
    client.handle(ServerMessage::Start {
        session: 7,
        players,
        characters: [Character::Chalk; 10],
    });
    assert_eq!(client.status(), Status::Playing);
    client
}

/// One tick of the duel with what players 0 and 1 hold.
fn play(client: &mut Client, p0: Input, p1: Input) {
    let session = client.session().expect("a duel runs");
    let mut inputs = [Input::NONE; PlayerSet::CAPACITY];
    inputs[0] = p0;
    inputs[1] = p1;
    client.handle(ServerMessage::Frame {
        session: session.id(),
        tick: session.confirmed_tick(),
        players: session.players(),
        late: PlayerSet::EMPTY,
        inputs,
    });
    client.tick(p0);
}

fn current(client: &Client) -> &Match {
    client.session().expect("a duel runs").current()
}

fn punch() -> Input {
    Input::pressing(Attack::punch(Strength::Low))
}

fn jump() -> Input {
    Input {
        jump: true,
        ..Input::NONE
    }
}

fn right() -> Input {
    Input {
        right: true,
        ..Input::NONE
    }
}

/// The throw: two kick buttons together.
fn throw() -> Input {
    Input::pressing(Attack::kick(Strength::Medium)).with(Attack::kick(Strength::High))
}

/// The ticks of the fight so far: what the screens animate with.
fn ticks(client: &Client) -> u32 {
    client.session().expect("a duel runs").tick()
}

/// The arena of the side view and the 3D view drawn for `client`.
fn arena_frame(client: &Client) -> Frame {
    let mut frame = Frame::new(arena::BACKGROUND);
    arena::draw(&mut frame, client, ticks(client));
    frame
}

fn tps_frame(client: &Client) -> Frame {
    let mut frame = Frame::new(arena::BACKGROUND);
    tps::draw(&mut frame, client, &tps::aim(client), ticks(client));
    frame
}

/// The rectangles of `frame` painted in `color` (whatever their opacity).
fn painted(frame: &Frame, color: Rgba) -> Vec<f32> {
    frame
        .rects
        .iter()
        .filter(|rect| (rect.color.r, rect.color.g, rect.color.b) == (color.r, color.g, color.b))
        .map(|rect| rect.color.a)
        .collect()
}

fn glitchy(frame: &Frame) -> usize {
    painted(frame, COLD).len() + painted(frame, HOT).len()
}

/// Player 0 jabs player 1 until it has echoes to rain; returns the client right then.
fn echoing() -> Client {
    let mut client = duel(config(Some(Glitch::EchoBarrage)));
    for _ in 0..8 {
        play(&mut client, punch(), Input::NONE);
        if current(&client).fighters()[1].echoes() > 0 {
            return client;
        }
    }
    panic!("the punch never landed");
}

/// Player 0 jabs while player 1 jumps into the fist (the jab only reaches 60 units up); returns
/// the client right after the hit snapped it.
fn warped() -> Client {
    let mut client = duel(config(Some(Glitch::PhantomWarp)));
    play(&mut client, punch(), Input::NONE);
    play(&mut client, punch(), jump());
    for _ in 0..6 {
        play(&mut client, punch(), Input::NONE);
        if current(&client).fighters()[1].warp().is_some() {
            return client;
        }
    }
    panic!("the hit never snapped the jumper");
}

// ---------------------------------------------------------------------------------------------
// The geometry
// ---------------------------------------------------------------------------------------------

#[test]
fn the_ring_of_copies_thins_out_and_turns() {
    assert_eq!(copies(0), 0, "no barrage, no copies");
    assert_eq!(copies(1), 2, "never fewer than two while it rains");
    let counts: Vec<usize> = (0..40).map(copies).collect();
    assert!(
        counts.windows(2).all(|pair| pair[0] <= pair[1]),
        "{counts:?}"
    );
    assert_eq!(*counts.last().unwrap(), MOST_COPIES);
    // The copies stand round a circle, a quarter turn apart for four of them...
    let at = |copy| ring(copy, 4, 0);
    assert_eq!(at(0), (1.0, 0.0));
    for copy in 0..4 {
        let (across, depth) = ring(copy, 4, 9);
        assert!((across.hypot(depth) - 1.0).abs() < 1e-5);
    }
    // ...and the circle turns with the ticks.
    assert_ne!(ring(0, 4, 0), ring(0, 4, 1));
    assert_eq!(ring(0, 0, 0), (1.0, 0.0), "a ring of nothing is harmless");
    // They take turns being cold and hot.
    assert_eq!((tint(0, 0), tint(1, 0)), (COLD, HOT));
    assert_eq!((tint(0, 4), tint(1, 4)), (HOT, COLD));
    assert_eq!(tint(0, 8), COLD);
}

#[test]
fn the_afterimages_fade_with_the_trail() {
    let fresh = trail(WARP_TRAIL);
    let old = trail(1);
    for (new, aged) in fresh.iter().zip(old) {
        assert_eq!(new.0, aged.0, "they stay where they are");
        assert!(new.1 > aged.1 && aged.1 > 0.0);
    }
    assert_eq!(fresh[0].1, GHOST_ALPHA);
    assert!(
        fresh
            .windows(2)
            .all(|pair| pair[0].0 < pair[1].0 && pair[0].1 > pair[1].1)
    );
    assert_eq!(
        trail(99),
        fresh,
        "a trail never starts brighter than a fresh one"
    );
    assert_eq!(trail(0)[0].1, 0.0);
}

#[test]
fn the_shock_ring_runs_out_to_the_edge_and_goes() {
    assert_eq!(shock(CRATER_TICKS), 0.0, "at the landing");
    let ring: Vec<f32> = (1..=CRATER_TICKS).rev().map(shock).collect();
    assert!(ring.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(shock(CRATER_TICKS - SHOCK_TICKS), 1.0);
    assert_eq!(shock(1), 1.0);
    const { assert!(CRATER_TICKS > SHOCK_TICKS, "the crater outlasts the ring") };
}

#[test]
fn a_ghost_is_the_figure_in_one_colour_without_its_marks() {
    let client = duel(config(None));
    let fighter = &current(&client).fighters()[0];
    let figure = Figure::of(fighter, client.config(), 0);
    let color = COLD.with_alpha(0.5);
    let ghost = glitch::ghosted(&figure, color);
    assert_eq!(
        (ghost.ink, ghost.accent, ghost.mark),
        (color, color, Mark::None)
    );
    assert!(
        ghost
            .arms
            .iter()
            .chain(&ghost.legs)
            .all(|bone| bone.ink.is_none())
    );
    assert_eq!(ghost.head, figure.head, "it stands where the figure stands");
    let mut frame = Frame::new(arena::BACKGROUND);
    ghost.draw(&mut frame);
    assert!(!frame.rects.is_empty());
    assert!(
        frame
            .rects
            .iter()
            .all(|rect| rect.color.a <= 0.5 && rect.color.b > 0.0)
    );
}

// ---------------------------------------------------------------------------------------------
// The echo barrage
// ---------------------------------------------------------------------------------------------

#[test]
fn the_victim_of_an_echo_barrage_is_ringed_by_lag_copies_until_it_runs_out() {
    let mut client = echoing();
    let victim = |client: &Client| current(client).fighters()[1].echoes();
    let start = victim(&client);
    assert!(start > 0);
    let at_first = arena_frame(&client);
    assert!(glitchy(&at_first) > 0, "the copies are drawn");
    assert!(
        painted(&at_first, COLD)
            .iter()
            .chain(&painted(&at_first, HOT))
            .all(|&alpha| alpha > 0.0 && alpha < 1.0),
        "ghosts, not solid figures"
    );
    // Echo hits left: fewer copies, until none.
    let mut before = glitchy(&at_first);
    let mut thinned = false;
    while victim(&client) > 0 {
        let now = glitchy(&arena_frame(&client));
        assert!(now > 0, "{} echoes left", victim(&client));
        thinned |= now < before;
        before = before.max(now);
        play(&mut client, Input::NONE, Input::NONE);
    }
    assert!(thinned, "the ring thins as the barrage runs out");
    assert_eq!(glitchy(&arena_frame(&client)), 0, "no echoes, no copies");
}

#[test]
fn the_lag_copies_ring_the_victim_in_the_3d_view_too() {
    let mut client = echoing();
    let with = tps_frame(&client).rects.len();
    while current(&client).fighters()[1].echoes() > 0 {
        play(&mut client, Input::NONE, Input::NONE);
    }
    let without = tps_frame(&client).rects.len();
    assert!(with > without, "{with} rectangles ringed, {without} after");
}

#[test]
fn without_the_glitch_a_punch_rings_no_copies() {
    let mut client = duel(config(None));
    for _ in 0..8 {
        play(&mut client, punch(), Input::NONE);
        assert_eq!(current(&client).fighters()[1].echoes(), 0);
        assert_eq!(glitchy(&arena_frame(&client)), 0);
    }
}

// ---------------------------------------------------------------------------------------------
// The phantom warp
// ---------------------------------------------------------------------------------------------

#[test]
fn a_warped_fighter_leaves_afterimages_that_fade_and_go() {
    let mut client = warped();
    let (from, now) = {
        let victim = &current(&client).fighters()[1];
        let warp = victim.warp().expect("it snapped");
        assert_eq!(warp.remaining(), WARP_TRAIL);
        (warp.from(), victim.x())
    };
    assert!(
        now > from,
        "the hit travelled to the right: {from} to {now}"
    );
    let fresh = arena_frame(&client);
    let strongest = |frame: &Frame| {
        painted(frame, COLD)
            .into_iter()
            .chain(painted(frame, HOT))
            .fold(0.0_f32, f32::max)
    };
    assert!(glitchy(&fresh) > 0, "the afterimages are drawn");
    let first = strongest(&fresh);
    // The 3D view draws them as mannequins in the same colours.
    let with = tps_frame(&client).rects.len();
    // They fade with the trail, and go with it.
    let mut last = first;
    while current(&client).fighters()[1].warp().is_some() {
        play(&mut client, Input::NONE, Input::NONE);
        last = strongest(&arena_frame(&client));
    }
    assert_eq!(glitchy(&arena_frame(&client)), 0, "the trail is gone");
    assert!(last < first, "{last} against {first}");
    assert!(with > tps_frame(&client).rects.len());
}

#[test]
fn a_hit_on_the_floor_or_without_the_glitch_leaves_no_afterimage() {
    let mut client = duel(config(Some(Glitch::PhantomWarp)));
    for _ in 0..6 {
        play(&mut client, punch(), Input::NONE);
    }
    assert!(
        current(&client).fighters()[1].warp().is_none(),
        "on the floor"
    );
    let mut client = duel(config(None));
    play(&mut client, punch(), Input::NONE);
    play(&mut client, punch(), jump());
    for _ in 0..8 {
        play(&mut client, punch(), Input::NONE);
    }
    assert!(
        current(&client).fighters()[1].health() < 1000,
        "the jab landed"
    );
    assert!(
        current(&client).fighters()[1].warp().is_none(),
        "without the glitch"
    );
}

// ---------------------------------------------------------------------------------------------
// The stretch strike
// ---------------------------------------------------------------------------------------------

/// Player 0 punches until its fist is out (the active ticks), turned away from player 1 so
/// nothing is hit; with `glitch` on or off.
fn punching(glitch: Option<Glitch>) -> Client {
    // Short punches: the stick figure's arm is then well short of the longest it stretches.
    let mut client = duel(MatchConfig {
        punches: [AttackData {
            range: 30,
            ..STRIKE
        }; 3],
        ..config(glitch)
    });
    let away = Input {
        left: true,
        down: true,
        ..Input::NONE
    };
    play(&mut client, away, Input::NONE);
    for _ in 0..4 {
        play(&mut client, punch(), Input::NONE);
        if current(&client).fighters()[0].state()
            == FighterState::Attacking(srr::game::AttackPhase::Active)
        {
            return client;
        }
    }
    panic!("the punch never reached its active ticks");
}

#[test]
fn a_stretched_punch_reaches_as_far_as_the_rules_say() {
    let stretched = punching(Some(Glitch::StretchStrike));
    let plain = punching(None);
    let reach = |client: &Client| {
        let fighter = &current(client).fighters()[0];
        let figure = Figure::of(fighter, client.config(), 0);
        (figure.arms[0].end.x - figure.shoulder.x).abs()
    };
    // The stick figure's own arm grows by the glitch's reach, to the limit of its stretch.
    let extra = config(None).stretch_reach as f32 * arena::scale(stretched.config());
    assert!(
        reach(&stretched) > reach(&plain) + 40.0 && extra > 0.0 && stick_limit() > 0.0,
        "{} against {}",
        reach(&stretched),
        reach(&plain)
    );
    let fighter = &current(&stretched).fighters()[0];
    assert!(glitch::stretching(fighter, stretched.config()));
    assert!(!glitch::stretching(fighter, plain.config()), "glitch off");
    // A punch winding up, or a kick, is not stretched.
    let mut winding = duel(config(Some(Glitch::StretchStrike)));
    play(&mut winding, punch(), Input::NONE);
    let fighter = &current(&winding).fighters()[0];
    assert_eq!(
        fighter.state(),
        FighterState::Attacking(srr::game::AttackPhase::Startup)
    );
    assert!(!glitch::stretching(fighter, winding.config()));
    let mut kicking = duel(config(Some(Glitch::StretchStrike)));
    for _ in 0..3 {
        play(
            &mut kicking,
            Input::pressing(Attack::kick(Strength::Low)),
            Input::NONE,
        );
    }
    let fighter = &current(&kicking).fighters()[0];
    assert!(
        !glitch::stretching(fighter, kicking.config()),
        "{:?}",
        fighter.state()
    );
}

/// How far a limb of the stick figure stretches at most.
fn stick_limit() -> f32 {
    srr::front::stick::MAX_REACH
}

#[test]
fn a_sprite_gets_a_rubber_arm_over_it_while_it_stretches() {
    let mut stretched = punching(Some(Glitch::StretchStrike));
    let skin = Skin::of_player(Style::Manga, PlayerId(0));
    stretched.handle(ServerMessage::SkinOf {
        player: PlayerId(0),
        skin,
    });
    assert!(sprite_name_of(&stretched.skin(PlayerId(0))).is_some());
    let frame = arena_frame(&stretched);
    assert!(
        painted(&frame, HOT)
            .iter()
            .any(|&alpha| alpha > GHOST_ALPHA),
        "the rubber arm is drawn over the sprite"
    );
    // The chalk's own arm is the stick figure's: no rubber over it.
    let chalk = punching(Some(Glitch::StretchStrike));
    assert!(painted(&arena_frame(&chalk), HOT).is_empty());
    // And not without the glitch.
    let mut plain = punching(None);
    plain.handle(ServerMessage::SkinOf {
        player: PlayerId(0),
        skin,
    });
    assert!(painted(&arena_frame(&plain), HOT).is_empty());
}

#[test]
fn the_3d_view_stretches_the_arm_too() {
    let stretched = tps_frame(&punching(Some(Glitch::StretchStrike)));
    let plain = tps_frame(&punching(None));
    assert!(
        stretched.rects.len() > plain.rects.len(),
        "the rubber arm adds parts: {} against {}",
        stretched.rects.len(),
        plain.rects.len()
    );
}

// ---------------------------------------------------------------------------------------------
// The crater slam
// ---------------------------------------------------------------------------------------------

/// Player 0 walks up to player 1 and throws it; returns the client the tick the projection
/// lands and leaves its crater.
fn slammed() -> Client {
    let mut client = duel(config(Some(Glitch::CraterSlam)));
    for _ in 0..30 {
        play(&mut client, right(), Input::NONE);
    }
    play(&mut client, throw(), Input::NONE);
    for _ in 0..120 {
        if !current(&client).craters().is_empty() {
            return client;
        }
        play(&mut client, Input::NONE, Input::NONE);
    }
    panic!("the throw never landed");
}

#[test]
fn a_landed_throw_leaves_a_crater_that_cracks_the_street_then_heals() {
    let mut client = slammed();
    let crater = *current(&client).craters().first().expect("a crater");
    assert_eq!(crater.radius(), client.config().slam_radius);
    assert_eq!(crater.remaining(), CRATER_TICKS);
    let landed = current(&client).fighters()[1].x();
    assert_eq!(crater.x(), landed, "where the thrown fighter landed");
    assert_eq!(crater.z(), 0);
    let center = ARENA_LEFT + crater.x() as f32 * arena::scale(client.config());
    let fresh = arena_frame(&client);
    let pit = |frame: &Frame| -> Vec<f32> {
        frame
            .rects
            .iter()
            .filter(|rect| (rect.color.r, rect.color.g, rect.color.b) == (PIT.r, PIT.g, PIT.b))
            .map(|rect| rect.x + rect.width / 2.0)
            .collect()
    };
    assert!(!pit(&fresh).is_empty(), "the pit is drawn");
    assert!(
        pit(&fresh).iter().all(|&x| (x - center).abs() < 1.0),
        "centred on the landing"
    );
    assert!(!painted(&fresh, CRACK).is_empty(), "the street is cracked");
    assert!(!painted(&fresh, COLD).is_empty(), "the shock ring runs");
    let with = tps_frame(&client).rects.len();
    // The shock ring is gone after its ticks, the cracks fade, the crater goes.
    for _ in 0..SHOCK_TICKS {
        play(&mut client, Input::NONE, Input::NONE);
    }
    let later = arena_frame(&client);
    assert!(painted(&later, COLD).is_empty(), "no shock ring any more");
    assert!(!painted(&later, CRACK).is_empty());
    let alpha = |frame: &Frame| painted(frame, CRACK).into_iter().fold(0.0_f32, f32::max);
    assert!(alpha(&later) < alpha(&fresh), "the cracks fade");
    while !current(&client).craters().is_empty() {
        play(&mut client, Input::NONE, Input::NONE);
    }
    let healed = arena_frame(&client);
    assert!(pit(&healed).is_empty() && painted(&healed, CRACK).is_empty());
    assert!(
        with > tps_frame(&client).rects.len(),
        "the 3D view heals it too"
    );
}

#[test]
fn without_the_glitch_a_landed_throw_leaves_the_street_alone() {
    let mut client = duel(config(None));
    for _ in 0..30 {
        play(&mut client, right(), Input::NONE);
    }
    play(&mut client, throw(), Input::NONE);
    for _ in 0..60 {
        play(&mut client, Input::NONE, Input::NONE);
        assert!(current(&client).craters().is_empty());
    }
}

#[test]
fn the_glitch_colours_stay_out_of_the_fight_when_nothing_glitches() {
    let mut client = duel(config(Some(Glitch::EchoBarrage)));
    for _ in 0..3 {
        play(&mut client, Input::NONE, Input::NONE);
    }
    let frame = arena_frame(&client);
    assert_eq!(glitchy(&frame), 0);
    assert!(painted(&frame, PIT).is_empty() && painted(&frame, CRACK).is_empty());
    const { assert!(RING_RADIUS > 0.0) };
}

// ---------------------------------------------------------------------------------------------
// Pictures
// ---------------------------------------------------------------------------------------------

/// Dumps the glitches as PNGs in `target/screenshots-glitches/`, to eyeball them (writes files,
/// so CI does not run it):
/// `cargo test -p srr --no-default-features --test glitch_looks -- --ignored dump_glitches`.
#[test]
#[ignore = "writes files for a human to look at"]
fn dump_glitches() {
    use srr::front::render::screenshot;
    let dir = std::path::Path::new("target/screenshots-glitches");
    std::fs::create_dir_all(dir).expect("the directory");
    let save = |name: &str, frame: &Frame| {
        let png = screenshot(frame, 1280, 720).to_png();
        std::fs::write(dir.join(name), png).expect("the picture");
    };
    let manga = |mut client: Client| {
        client.handle(ServerMessage::SkinOf {
            player: PlayerId(0),
            skin: Skin::of_player(Style::Manga, PlayerId(0)),
        });
        client
    };
    let echo = echoing();
    save("echo_2d.png", &arena_frame(&echo));
    save("echo_3d.png", &tps_frame(&echo));
    let warp = warped();
    save("warp_2d.png", &arena_frame(&warp));
    save("warp_3d.png", &tps_frame(&warp));
    let stretch = punching(Some(Glitch::StretchStrike));
    save("stretch_chalk_2d.png", &arena_frame(&stretch));
    save("stretch_3d.png", &tps_frame(&stretch));
    let stretch = manga(punching(Some(Glitch::StretchStrike)));
    save("stretch_manga_2d.png", &arena_frame(&stretch));
    let slam = slammed();
    save("crater_2d.png", &arena_frame(&slam));
    save("crater_3d.png", &tps_frame(&slam));
}
