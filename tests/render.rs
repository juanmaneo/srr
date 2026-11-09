//! The engine booted headlessly: the ECS wiring and the rendering the browser runs, without
//! a window, a GPU context or a sound card.
//!
//! Commit `203c1b6` shipped a black screen: it added the `PadTaps` resource and two systems
//! reading it, but never called `.init_resource::<PadTaps>()`, and Bevy's system-param
//! validation only panics when the app actually runs — `cargo check`, clippy and
//! `tests/project_layout.rs` (which forbids tests touching `srr::engine`) could not see it.
//! These tests boot the real app (the very construction the browser's `Engine::run` uses,
//! see the deliberate exception in `tests/project_layout.rs`) and assert it survives
//! startup and draws:
//!
//! 1. `booting_the_real_app_survives_startup` (fast, no GPU): several frames of the whole
//!    ECS — every system param validated, every resource a system reads present. This
//!    alone fails on the PadTaps bug, on any machine.
//! 2. `the_intro_renders_and_animates_headlessly` (SSR): real frames through the real wgpu
//!    renderer on whatever adapter the machine has (llvmpipe in CI), captured offscreen —
//!    not one flat colour, and moving. Skips loudly, not silently, when no adapter exists.
//!
//! 3. With the `mode3d` feature (`cargo test --features mode3d --test render`), the
//!    third-person view with its glTF fighters: the puppet of `assets/models/chalk/`
//!    (generated, committed, no download) loads, gets its `AnimationPlayer` and its toon
//!    materials, renders, and plays the clip of the fighter's state. The 2D tests above run
//!    unchanged with the feature on.
//!
//! The shell plays in practice mode (`Client::practice`, what `Shell::new` wires
//! natively): no network. Audio may fail to start headlessly — the game handles
//! `SoundState::Unavailable`.

#![cfg(feature = "bevy")]

use std::sync::{Mutex, MutexGuard};

use srr::engine::{Capture, Pixels, ServerError, boot_app, server_app};
use srr::front::{SCREEN_HEIGHT, SCREEN_WIDTH};
use srr::platform::Shell;

/// One rendering server at a time. WHY: the test harness runs the tests in parallel, and
/// each rendering test creates its own wgpu device; on CI's software rasterizer (lavapipe)
/// two devices initialising at once in one process crashed the whole binary (SIGSEGV, not a
/// failure: run 43c447b of `ci.yml`, while `test.yml` passed the same step). Mesa shares its
/// JIT state per process, so the devices are created and dropped one after the other. Taken
/// first in a test, so the server is dropped before it is released.
fn gpu() -> MutexGuard<'static, ()> {
    static GPU: Mutex<()> = Mutex::new(());
    // A test that panicked while holding it must not fail the next ones too.
    GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Frames to boot before asserting: enough for Startup, several fixed ticks and several
/// draws, few enough to stay fast.
const FRAMES: u32 = 8;

/// One frame of virtual time: the browser runs at 60 fps.
const FRAME_SECONDS: f64 = 1.0 / 60.0;

/// Updates to wait for the screenshot readback: it completes on a task pool, a few frames
/// after the screenshot was taken.
const READBACK_BUDGET: u32 = 16;

/// The capture size: the virtual screen, 1:1 (the whole frame visible, letterbox-free).
fn capture() -> Capture {
    Capture {
        width: SCREEN_WIDTH as u32,
        height: SCREEN_HEIGHT as u32,
    }
}

// ---------------------------------------------------------------------------------------------
// 1. ECS boot: fast, no GPU — runs everywhere
// ---------------------------------------------------------------------------------------------

/// The regression test for the black screen: the same ECS the browser runs boots, ticks
/// its shell and draws, frame after frame.
///
/// The ECS validates every system's params when the first frame runs, so a resource some
/// system reads but nothing initialises (the `PadTaps` bug of commit `203c1b6`) panics
/// here, loudly — this is the test that would have caught the black screen before it
/// shipped.
#[test]
fn booting_the_real_app_survives_startup() {
    let mut app = boot_app(Shell::new());
    for _ in 0..FRAMES {
        app.advance(FRAME_SECONDS);
    }
    // The shell's pools grew: the drawing systems ran (Startup spawned the camera, the
    // fixed timestep ticked the shell, `draw` mapped its frame onto the screen).
    assert!(app.frame_drawn(), "nothing was drawn in {FRAMES} frames");
}

// ---------------------------------------------------------------------------------------------
// 2. SSR: real frames, headless GPU
// ---------------------------------------------------------------------------------------------

/// Renders the first seconds of the intro through the real wgpu renderer and captures the
/// framebuffer: the app draws (not the one flat colour of a dead canvas) and animates
/// (consecutive captures differ).
#[test]
fn the_intro_renders_and_animates_headlessly() {
    let _gpu = gpu();
    let mut server = match server_app(Shell::new(), capture()) {
        Ok(server) => server,
        // Loudly, not silently: a test that never runs protects nothing. CI has mesa (the
        // llvmpipe software rasterizer) installed and runs this for real on every push —
        // see the `render` step of the workflow. Locally without a rasterizer, this is a
        // documented skip, and the ECS boot test above still ran.
        Err(error @ (ServerError::NoAdapter(_) | ServerError::NoDevice(_))) => {
            eprintln!(
                "SKIP the_intro_renders_and_animates_headlessly: rendering impossible here: {error}"
            );
            return;
        }
    };
    // One second of the intro, then another half: enough for the title to appear and to
    // move between the two captures.
    let first = server.capture(60, FRAME_SECONDS, READBACK_BUDGET);
    let second = server.capture(30, FRAME_SECONDS, READBACK_BUDGET);

    assert_eq!(first.size(), (SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32));
    // Not a dead canvas: the frame is not one flat colour (the black screen behind a
    // crashed app would be — the CSS background is all the browser ever painted).
    assert_flat(&first, "the first second of the intro");
    // The intro animates: consecutive captures differ.
    assert!(
        first.differences(&second) > 0,
        "the intro did not animate: two captures half a second apart are identical"
    );
}

// ---------------------------------------------------------------------------------------------
// 3. The 3D view: glTF fighters, cel-shaded (`mode3d`)
// ---------------------------------------------------------------------------------------------

#[cfg(feature = "mode3d")]
mod mode3d {
    use super::*;
    use srr::engine::{Server, StageReport};
    use srr::front::model::{Body, Clip};
    use srr::front::{Game, Headless, Screen, VIEW_KEY, View};
    use srr::game::{Character, Key, MatchConfig, PlayerId, Skin, Style, Variant};
    use srr::net::Client;

    /// Frames to let the model load from disk, the scene spawn, the actors dress and fit:
    /// a bound, not a wait (it takes a handful).
    const LOAD_BUDGET: u32 = 240;

    /// A practice fight against the dummy, on the fight screen, in the third-person view, P1
    /// wearing `skin`.
    fn fight_in_3d_as(skin: Skin) -> Shell {
        let mut client = Client::practice(MatchConfig::default());
        let _ = client.set_skin(skin);
        let mut game = Headless::new(Game::new(client));
        game.skip_intro();
        game.tap(&[VIEW_KEY]);
        assert_eq!(game.game().screen(), Screen::Fight);
        assert_eq!(game.game().view(), View::ThirdPerson);
        Shell::with_game(game.game().clone())
    }

    /// The rendering server on the 3D view, or `None` when rendering is impossible here
    /// (printed, like the SSR test above).
    fn server(what: &str) -> Option<Server> {
        server_as(what, Skin::STICK)
    }

    /// [`server`], P1 wearing `skin`.
    fn server_as(what: &str, skin: Skin) -> Option<Server> {
        match server_app(fight_in_3d_as(skin), capture()) {
            Ok(server) => Some(server),
            Err(error @ (ServerError::NoAdapter(_) | ServerError::NoDevice(_))) => {
                eprintln!("SKIP {what}: rendering impossible here: {error}");
                None
            }
        }
    }

    /// Runs frames until every actor of the stage is dressed, fitted and playing a clip,
    /// within [`LOAD_BUDGET`].
    fn settle(server: &mut Server) -> StageReport {
        for _ in 0..LOAD_BUDGET {
            server.run(1, FRAME_SECONDS);
            let report = server.stage();
            if report.models.is_some()
                && report.actors.len() == 2
                && report
                    .actors
                    .iter()
                    .all(|actor| actor.fitted && actor.clip.is_some())
            {
                return report;
            }
        }
        panic!(
            "the 3D stage did not settle in {LOAD_BUDGET} frames: {:?}",
            server.stage()
        );
    }

    /// The puppet loads for Chalk (both fighters of the practice fight), each fighter
    /// gets an animated, cel-shaded, fitted actor standing in guard, the game is told its
    /// mannequins are replaced, and the stage renders them.
    #[test]
    fn the_3d_view_dresses_the_fighters_in_toon_models() {
        let _gpu = gpu();
        let Some(mut server) = server("the_3d_view_dresses_the_fighters_in_toon_models") else {
            return;
        };
        let report = settle(&mut server);
        let models = report.models.expect("settled");
        assert_eq!(models.characters(), [Character::Chalk], "{report:?}");
        let players: Vec<PlayerId> = report.actors.iter().map(|actor| actor.player).collect();
        assert_eq!(players, [PlayerId(0), PlayerId(1)]);
        for actor in &report.actors {
            assert_eq!(actor.character, Character::Chalk);
            assert_eq!(
                actor.body,
                Body::Puppet(Character::Chalk),
                "chalk: the puppet"
            );
            assert!(
                actor.animated,
                "no AnimationPlayer with the clips: {actor:?}"
            );
            assert!(actor.toon_materials > 0, "no toon material: {actor:?}");
            assert!(actor.fitted, "{actor:?}");
            assert_eq!(actor.clip, Some(Clip::Idle), "{actor:?}");
        }
        assert_eq!(
            server.game().models(),
            models,
            "the game was told which mannequins the engine replaces"
        );

        // The stage image has the models (opaque pixels over a transparent background,
        // a small share of the screen: two fighters), and the frame is not flat. Half a
        // second first: the materials reach the GPU a few frames after the actors fit
        // (their textures upload, their pipelines specialise).
        server.run(30, FRAME_SECONDS);
        let stage = server.capture_stage(FRAME_SECONDS, READBACK_BUDGET);
        assert_eq!(stage.size(), (SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32));
        let covered = 1.0 - stage.fraction([0, 0, 0, 0]);
        assert!(
            covered > 0.002 && covered < 0.5,
            "the models cover {covered} of the stage image"
        );
        let frame = server.capture(1, FRAME_SECONDS, READBACK_BUDGET);
        assert_flat(&frame, "the 3D view");
    }

    /// The committed girls load from `assets/models/manga/`: the redhead (the first the
    /// image-to-3D pipeline made) is P1's model in the manga style, in her own texture, rigged
    /// with the puppet's movements; the dummy keeps the chalk puppet.
    #[test]
    fn a_manga_girl_is_drawn_as_her_own_model() {
        let _gpu = gpu();
        let skin = Skin::styled(Style::Manga, Variant::Redhead);
        let Some(mut server) = server_as("a_manga_girl_is_drawn_as_her_own_model", skin) else {
            return;
        };
        let report = settle(&mut server);
        let models = report.models.expect("settled");
        assert!(models.has_girl(Variant::Redhead), "{report:?}");
        assert!(models.contains(Character::Chalk), "{report:?}");
        let [girl, dummy] = &report.actors[..] else {
            panic!("two actors: {report:?}");
        };
        assert_eq!(girl.body, Body::Girl(Variant::Redhead), "{girl:?}");
        assert_eq!(dummy.body, Body::Puppet(Character::Chalk), "{dummy:?}");
        for actor in [girl, dummy] {
            assert!(
                actor.animated,
                "no AnimationPlayer with the clips: {actor:?}"
            );
            assert!(actor.toon_materials > 0, "no toon material: {actor:?}");
            assert!(actor.fitted && actor.clip == Some(Clip::Idle), "{actor:?}");
        }
        server.run(30, FRAME_SECONDS);
        let stage = server.capture_stage(FRAME_SECONDS, READBACK_BUDGET);
        let covered = 1.0 - stage.fraction([0, 0, 0, 0]);
        assert!(
            covered > 0.002 && covered < 0.5,
            "the models cover {covered} of the stage image"
        );
    }

    /// The clips follow the fighter's state through the engine: walking plays the walk,
    /// a punch its punch, standing still the idle.
    #[test]
    fn the_clips_follow_the_fighter() {
        let _gpu = gpu();
        let Some(mut server) = server("the_clips_follow_the_fighter") else {
            return;
        };
        settle(&mut server);
        let clip_of = |server: &mut Server, player: u8| {
            server
                .stage()
                .actors
                .iter()
                .find(|actor| actor.player == PlayerId(player))
                .and_then(|actor| actor.clip)
        };
        // The countdown, then P1 walks right (towards the dummy).
        server.hold(&[Key::D]);
        let walked = (0..LOAD_BUDGET).any(|_| {
            server.run(1, FRAME_SECONDS);
            clip_of(&mut server, 0) == Some(Clip::WalkF)
        });
        assert!(walked, "P1 never walked: {:?}", server.stage());
        // A low punch.
        server.hold(&[Key::B]);
        let punched = (0..30).any(|_| {
            server.run(1, FRAME_SECONDS);
            clip_of(&mut server, 0) == Some(Clip::Punch)
        });
        assert!(punched, "P1 never punched: {:?}", server.stage());
        // Hands off: back to the guard.
        server.hold(&[]);
        let idle = (0..120).any(|_| {
            server.run(1, FRAME_SECONDS);
            clip_of(&mut server, 0) == Some(Clip::Idle)
        });
        assert!(idle, "P1 never stood still again: {:?}", server.stage());
        // The dummy stood in guard throughout.
        assert_eq!(clip_of(&mut server, 1), Some(Clip::Idle));
    }

    /// Dumps PNGs of the 3D view to `target/screenshots3d/`, to eyeball the models (the
    /// puppet now, a real model later): guard, walk, punch and kick of P1 against the dummy,
    /// plus the raw stage layer. Not run by CI (it needs a rasterizer and writes files):
    ///
    /// ```sh
    /// cargo test -p srr --features mode3d --test render -- --ignored --nocapture dump_3d_screenshots
    /// ```
    #[test]
    #[ignore]
    fn dump_3d_screenshots() {
        use srr::front::render::{adler32, crc32};

        fn png(px: &Pixels) -> Vec<u8> {
            let (w, h) = (px.size().0 as usize, px.size().1 as usize);
            let mut raw = Vec::with_capacity(h * (1 + 3 * w));
            for row in px.rgba().chunks(w * 4) {
                raw.push(0);
                for p in row.chunks(4) {
                    raw.extend_from_slice(&p[..3]);
                }
            }
            let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
            let chunk = |out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
                out.extend((data.len() as u32).to_be_bytes());
                let start = out.len();
                out.extend(kind);
                out.extend(data);
                out.extend(crc32(&out[start..]).to_be_bytes());
            };
            let mut ihdr = Vec::new();
            ihdr.extend((w as u32).to_be_bytes());
            ihdr.extend((h as u32).to_be_bytes());
            ihdr.extend([8, 2, 0, 0, 0]);
            let mut z = vec![0x78, 0x01];
            let blocks: Vec<&[u8]> = raw.chunks(0xffff).collect();
            let last = blocks.len() - 1;
            for (i, block) in blocks.iter().enumerate() {
                z.push(u8::from(i == last));
                let len = block.len() as u16;
                z.extend(len.to_le_bytes());
                z.extend((!len).to_le_bytes());
                z.extend(*block);
            }
            z.extend(adler32(&raw).to_be_bytes());
            chunk(&mut out, b"IHDR", &ihdr);
            chunk(&mut out, b"IDAT", &z);
            chunk(&mut out, b"IEND", &[]);
            out
        }

        let _gpu = gpu();
        let Some(mut server) = server("dump_3d_screenshots") else {
            return;
        };
        settle(&mut server);
        let dir = std::path::Path::new("target/screenshots3d");
        std::fs::create_dir_all(dir).unwrap();
        let shot = |server: &mut Server, name: &str, updates: u32| {
            let px = server.capture(updates, FRAME_SECONDS, READBACK_BUDGET);
            let path = dir.join(format!("{name}.png"));
            std::fs::write(&path, png(&px)).unwrap();
            eprintln!("wrote {}", path.display());
        };
        let clip_of = |server: &mut Server, player: u8| {
            server
                .stage()
                .actors
                .iter()
                .find(|actor| actor.player == PlayerId(player))
                .and_then(|actor| actor.clip)
        };

        shot(&mut server, "1_idle", 2);
        server.hold(&[Key::D]);
        for _ in 0..LOAD_BUDGET {
            server.run(1, FRAME_SECONDS);
            if clip_of(&mut server, 0) == Some(Clip::WalkF) {
                break;
            }
        }
        shot(&mut server, "2_walk", 3);
        server.hold(&[Key::B]);
        for _ in 0..30 {
            server.run(1, FRAME_SECONDS);
            if clip_of(&mut server, 0) == Some(Clip::Punch) {
                break;
            }
        }
        // A few frames into the strike: the scrub is in its active window, not the wind-up.
        shot(&mut server, "3_punch", 6);
        server.hold(&[]);
        for _ in 0..120 {
            server.run(1, FRAME_SECONDS);
            if clip_of(&mut server, 0) == Some(Clip::Idle) {
                break;
            }
        }
        server.hold(&[Key::G]);
        for _ in 0..30 {
            server.run(1, FRAME_SECONDS);
            if clip_of(&mut server, 0) == Some(Clip::Kick) {
                break;
            }
        }
        shot(&mut server, "4_kick", 8);
        server.hold(&[]);
        let stage = server.capture_stage(FRAME_SECONDS, READBACK_BUDGET);
        std::fs::write(dir.join("5_stage_only.png"), png(&stage)).unwrap();
        eprintln!("wrote stage_only");
    }
}

/// Asserts the capture is not one flat colour, with that colour listed when it is (a dead
/// canvas is exactly one).
fn assert_flat(pixels: &Pixels, what: &str) {
    let mut distinct = 0;
    let mut sample = [0, 0, 0, 0];
    for pixel in pixels.rgba().as_chunks::<4>().0 {
        let pixel = *pixel;
        if distinct == 0 {
            sample = pixel;
            distinct = 1;
        } else if pixel != sample {
            distinct += 1;
            break;
        }
    }
    assert!(
        distinct > 1,
        "{what} rendered as one flat colour (rgba {sample:?}): a dead canvas"
    );
}
