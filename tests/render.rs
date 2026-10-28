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
//! The shell plays in practice mode (`Client::practice`, what `Shell::new` wires
//! natively): no network. Audio may fail to start headlessly — the game handles
//! `SoundState::Unavailable`.

#![cfg(feature = "bevy")]

use srr::engine::{Capture, Pixels, ServerError, boot_app, server_app};
use srr::front::{SCREEN_HEIGHT, SCREEN_WIDTH};
use srr::platform::Shell;

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
