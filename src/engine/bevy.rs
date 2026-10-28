//! The [Bevy](https://bevyengine.org) 0.15 backend (default; native and browser).
//!
//! * The [`Shell`] lives in a non-send resource (it owns the audio stream) and is ticked in
//!   `FixedUpdate`, at [`TICKS_PER_SECOND`], with the keys held and the fingers on a touch
//!   screen (mapped back onto the virtual screen, for the touch controls).
//! * Each `Update`, its [`Frame`] is mapped onto the window with a letterboxing [`Viewport`]:
//!   rectangles become top-left anchored sprites, labels become `Text2d` in the default font
//!   (Fira Mono, a monospace font, as the ASCII art requires). Sizes are scaled in window
//!   pixels rather than through the camera so that text is rasterised sharp at any size.
//! * Entities are pooled: the pools grow to the largest frame seen, unused entities are hidden,
//!   and components are only written when they change (text layout is the expensive part).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use bevy::MinimalPlugins;
use bevy::asset::RenderAssetUsages;
use bevy::core_pipeline::CorePipelinePlugin;
use bevy::input::InputPlugin;
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::RenderPlugin;
use bevy::render::camera::{ClearColor, RenderTarget};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::renderer::{
    RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue, WgpuWrapper,
};
use bevy::render::settings::{Backends, RenderCreation};
use bevy::render::texture::ImagePlugin;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::sprite::{Anchor, SpritePlugin};
use bevy::text::TextPlugin;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformPlugin;
use bevy::window::PrimaryWindow;

use super::{Engine, WEB_CANVAS, WINDOW_TITLE};
use crate::front::gamepad::PadButton;
use crate::front::{Align, Frame, GamepadState, Rgba, Viewport};
use crate::game::{Key, TICKS_PER_SECOND};
use crate::platform::Shell;

/// The Bevy engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct Bevy;

impl Engine for Bevy {
    const NAME: &'static str = "Bevy";

    fn run(shell: Shell) {
        build_app(shell, Headless::Windowed).run();
    }
}

/// Which plugins to wire the systems into: the windowed game, or a windowless variant whose
/// time and rendering are driven by hand — the tests use it to boot the very same ECS, with
/// the same systems and the same resources, without a GPU or a window.
#[derive(Debug, Clone, Copy, Default)]
pub enum Headless {
    /// The browser / desktop game: `DefaultPlugins`, a window, real time.
    #[default]
    Windowed,
    /// No window, no input, no clock, no renderer at all: the caller advances time and
    /// pumps `app.update()` on the bare ECS (the fast boot test).
    Server,
    /// The same, plus the render plugins writing into the offscreen capture of [`Capture`]
    /// (the SSR test).
    Render,
}

/// How the headless [`App`] renders: the capture the renderer writes into, spawned as an
/// `Image` asset and a camera render target, and the `Screenshot` that copies it to the CPU.
#[derive(Debug, Clone, Copy)]
pub struct Capture {
    /// The virtual screen the frame maps onto: 1 window pixel per virtual pixel.
    pub width: u32,
    pub height: u32,
}

/// The systems, resources and their registration, shared verbatim between the windowed game
/// and the headless variant: this is the wiring the browser runs, and what the boot test
/// checks (the `PadTaps` panic of commit 203c1b6 — a system reading a resource nothing
/// initialises — only surfaces when the ECS actually validates system params).
fn build_app(shell: Shell, headless: Headless) -> App {
    let mut app = App::new();
    match headless {
        Headless::Windowed => {
            app.add_plugins(DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: WINDOW_TITLE.into(),
                    // On the web, render into the canvas of web/index.html.
                    canvas: Some(WEB_CANVAS.into()),
                    fit_canvas_to_parent: true,
                    ..default()
                }),
                ..default()
            }));
        }
        Headless::Server | Headless::Render => {
            app.add_plugins(MinimalPlugins);
        }
    }
    app.insert_non_send_resource(ShellResource(shell))
        .insert_resource(Time::<Fixed>::from_hz(f64::from(TICKS_PER_SECOND)))
        .init_resource::<Pools>()
        .init_resource::<Taps>()
        .init_resource::<TouchTaps>()
        .init_resource::<PadTaps>()
        .add_systems(Startup, spawn_camera)
        .add_systems(FixedUpdate, tick)
        .add_systems(Update, (latch_taps, latch_pad_taps, draw));
    app
}

struct ShellResource(Shell);

/// Every sprite and text entity ever needed, reused frame after frame in order.
#[derive(Resource, Default)]
struct Pools {
    rects: Vec<Entity>,
    labels: Vec<Entity>,
}

/// Keys pressed since the last tick: a tap shorter than a tick still reaches the game.
#[derive(Resource, Default)]
struct Taps(HashSet<Key>);

/// Where fingers touched the screen since the last tick (window coordinates): a tap shorter
/// than a tick still reaches the game.
#[derive(Resource, Default)]
struct TouchTaps(Vec<Vec2>);

/// Pad buttons pressed since the last tick: a tap shorter than a tick still reaches the game.
#[derive(Resource, Default)]
struct PadTaps(HashSet<PadButton>);

#[derive(Component)]
struct RectNode;

#[derive(Component)]
struct LabelNode;

/// Depth of the `index`-th rectangle; labels go above every rectangle.
fn depth(index: usize, labels: bool) -> f32 {
    let base = if labels { 500.0 } else { 0.0 };
    base + index as f32 * 0.01
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn key(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::ArrowLeft => Key::ArrowLeft,
        KeyCode::ArrowRight => Key::ArrowRight,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::Numpad0 => Key::Numpad0,
        KeyCode::Numpad1 => Key::Numpad1,
        KeyCode::Numpad2 => Key::Numpad2,
        KeyCode::Numpad3 => Key::Numpad3,
        KeyCode::Numpad4 => Key::Numpad4,
        KeyCode::Numpad5 => Key::Numpad5,
        KeyCode::Numpad6 => Key::Numpad6,
        KeyCode::Numpad7 => Key::Numpad7,
        KeyCode::Numpad8 => Key::Numpad8,
        KeyCode::Numpad9 => Key::Numpad9,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::Space => Key::Space,
        KeyCode::KeyA => Key::A,
        KeyCode::KeyB => Key::B,
        KeyCode::KeyC => Key::C,
        KeyCode::KeyD => Key::D,
        KeyCode::KeyE => Key::E,
        KeyCode::KeyF => Key::F,
        KeyCode::KeyG => Key::G,
        KeyCode::KeyH => Key::H,
        KeyCode::KeyI => Key::I,
        KeyCode::KeyJ => Key::J,
        KeyCode::KeyK => Key::K,
        KeyCode::KeyL => Key::L,
        KeyCode::KeyM => Key::M,
        KeyCode::KeyN => Key::N,
        KeyCode::KeyO => Key::O,
        KeyCode::KeyP => Key::P,
        KeyCode::KeyQ => Key::Q,
        KeyCode::KeyR => Key::R,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyT => Key::T,
        KeyCode::KeyU => Key::U,
        KeyCode::KeyV => Key::V,
        KeyCode::KeyW => Key::W,
        KeyCode::KeyX => Key::X,
        KeyCode::KeyY => Key::Y,
        KeyCode::KeyZ => Key::Z,
        _ => return None,
    })
}

fn latch_taps(
    keyboard: Res<ButtonInput<KeyCode>>,
    touches: Res<Touches>,
    mut taps: ResMut<Taps>,
    mut touch_taps: ResMut<TouchTaps>,
) {
    taps.0
        .extend(keyboard.get_just_pressed().copied().filter_map(key));
    touch_taps
        .0
        .extend(touches.iter_just_pressed().map(|touch| touch.position()));
}

/// The Bevy button of a standard pad button.
fn pad_button(button: PadButton) -> GamepadButton {
    match button {
        PadButton::South => GamepadButton::South,
        PadButton::East => GamepadButton::East,
        PadButton::North => GamepadButton::North,
        PadButton::West => GamepadButton::West,
        PadButton::LeftTrigger => GamepadButton::LeftTrigger,
        PadButton::LeftTrigger2 => GamepadButton::LeftTrigger2,
        PadButton::RightTrigger => GamepadButton::RightTrigger,
        PadButton::RightTrigger2 => GamepadButton::RightTrigger2,
        PadButton::Select => GamepadButton::Select,
        PadButton::Start => GamepadButton::Start,
        PadButton::Mode => GamepadButton::Mode,
        PadButton::LeftThumb => GamepadButton::LeftThumb,
        PadButton::RightThumb => GamepadButton::RightThumb,
        PadButton::DPadUp => GamepadButton::DPadUp,
        PadButton::DPadDown => GamepadButton::DPadDown,
        PadButton::DPadLeft => GamepadButton::DPadLeft,
        PadButton::DPadRight => GamepadButton::DPadRight,
    }
}

fn latch_pad_taps(pads: Query<&Gamepad>, mut taps: ResMut<PadTaps>) {
    for pad in &pads {
        taps.0.extend(
            PadButton::ALL
                .iter()
                .copied()
                .filter(|&button| pad.just_pressed(pad_button(button))),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn tick(
    keyboard: Res<ButtonInput<KeyCode>>,
    touches: Res<Touches>,
    pads: Query<&Gamepad>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut taps: ResMut<Taps>,
    mut touch_taps: ResMut<TouchTaps>,
    mut pad_taps: ResMut<PadTaps>,
    mut shell: NonSendMut<ShellResource>,
) {
    let mut pressed: HashSet<Key> = keyboard.get_pressed().copied().filter_map(key).collect();
    pressed.extend(taps.0.drain());
    let mut gamepads = GamepadState::new();
    let taps = pad_taps.0.drain();
    for pad in &pads {
        let mut state = GamepadState::new();
        for button in PadButton::ALL {
            if pad.pressed(pad_button(button)) {
                state.press(button);
            }
        }
        let stick = pad.left_stick();
        state.set_stick(stick.x, stick.y);
        gamepads = gamepads.merge(state);
    }
    for button in taps {
        gamepads.press(button);
    }
    let viewport = windows
        .get_single()
        .map_or(Viewport::fit(0.0, 0.0), |window| {
            Viewport::fit(window.width(), window.height())
        });
    let fingers: Vec<(f32, f32)> = touches
        .iter()
        .map(|touch| touch.position())
        .chain(touch_taps.0.drain(..))
        .map(|position| viewport.to_virtual(position.x, position.y))
        .collect();
    shell.0.tick(&pressed, &fingers, gamepads);
}

fn color(rgba: Rgba) -> Color {
    Color::srgba(rgba.r, rgba.g, rgba.b, rgba.a)
}

/// World position (origin at the window centre, y up) of the virtual point `(x, y)`, rounded
/// to whole window pixels.
fn world(viewport: &Viewport, window: Vec2, x: f32, y: f32) -> Vec2 {
    let (wx, wy) = viewport.to_window(x, y);
    Vec2::new((wx - window.x / 2.0).round(), (window.y / 2.0 - wy).round())
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    shell: NonSend<ShellResource>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut clear: ResMut<ClearColor>,
    mut pools: ResMut<Pools>,
    mut rects: Query<(&mut Sprite, &mut Transform, &mut Visibility), With<RectNode>>,
    mut labels: Query<
        (
            &mut Text2d,
            &mut TextFont,
            &mut TextColor,
            &mut Anchor,
            &mut Transform,
            &mut Visibility,
        ),
        (With<LabelNode>, Without<RectNode>),
    >,
) {
    let Ok(window) = windows.get_single() else {
        return;
    };
    let size = Vec2::new(window.width(), window.height());
    let viewport = Viewport::fit(size.x, size.y);
    let frame: Frame = shell.0.frame();
    let background = color(frame.background);
    if clear.0 != background {
        clear.0 = background;
    }

    for (index, rect) in frame.rects.iter().enumerate() {
        let position = world(&viewport, size, rect.x, rect.y).extend(depth(index, false));
        let custom_size = Some(Vec2::new(rect.width, rect.height) * viewport.scale);
        let sprite_color = color(rect.color);
        match pools.rects.get(index).copied() {
            Some(entity) => {
                let Ok((mut sprite, mut transform, mut visibility)) = rects.get_mut(entity) else {
                    continue;
                };
                if sprite.custom_size != custom_size || sprite.color != sprite_color {
                    sprite.custom_size = custom_size;
                    sprite.color = sprite_color;
                }
                if transform.translation != position {
                    transform.translation = position;
                }
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                let entity = commands
                    .spawn((
                        Sprite {
                            color: sprite_color,
                            custom_size,
                            anchor: Anchor::TopLeft,
                            ..default()
                        },
                        Transform::from_translation(position),
                        RectNode,
                    ))
                    .id();
                pools.rects.push(entity);
            }
        }
    }
    for &entity in pools.rects.iter().skip(frame.rects.len()) {
        if let Ok((_, _, mut visibility)) = rects.get_mut(entity) {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }

    for (index, label) in frame.labels.iter().enumerate() {
        let position = world(&viewport, size, label.x, label.y).extend(depth(index, true));
        let font_size = (label.size * viewport.scale).max(1.0);
        let text_color = color(label.color);
        let anchor = match label.align {
            Align::Left => Anchor::TopLeft,
            Align::Center => Anchor::TopCenter,
            Align::Right => Anchor::TopRight,
        };
        match pools.labels.get(index).copied() {
            Some(entity) => {
                let Ok((mut text, mut font, mut tint, mut current, mut transform, mut visibility)) =
                    labels.get_mut(entity)
                else {
                    continue;
                };
                if text.0 != label.text {
                    text.0.clone_from(&label.text);
                }
                if font.font_size != font_size {
                    font.font_size = font_size;
                }
                if tint.0 != text_color {
                    tint.0 = text_color;
                }
                current.set_if_neq(anchor);
                if transform.translation != position {
                    transform.translation = position;
                }
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                let entity = commands
                    .spawn((
                        Text2d::new(label.text.clone()),
                        TextFont {
                            font_size,
                            ..default()
                        },
                        TextColor(text_color),
                        TextLayout::new_with_no_wrap(),
                        anchor,
                        Transform::from_translation(position),
                        LabelNode,
                    ))
                    .id();
                pools.labels.push(entity);
            }
        }
    }
    for &entity in pools.labels.iter().skip(frame.labels.len()) {
        if let Ok((_, _, _, _, _, mut visibility)) = labels.get_mut(entity) {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

// -----------------------------------------------------------------------------------------
// Server-side rendering: the headless variant the tests boot
// -----------------------------------------------------------------------------------------

/// The offscreen framebuffer the headless [`App`] renders into, and how to read it back.
///
/// There is no window and no swapchain to present: a `Camera2d` targets an `Image` asset
/// instead, and a [`Screenshot`] of that image copies it to the CPU. [`Server::capture`]
/// waits for the readback and hands the bytes back. Rendering still goes through the real
/// render graph, the real sprite and text pipelines, on whatever adapter `wgpu` finds —
/// llvmpipe in CI, a real GPU anywhere else.
pub struct Server {
    /// The whole app, ready for `update()`: same resources, same systems as the browser's,
    /// with the window and input plugins swapped for the render plugins and a manual clock.
    pub app: App,
    capture: Handle<Image>,
}

/// One rendered frame, as the screenshot read it back: RGBA8, row-major from the top-left.
pub struct Pixels {
    data: Vec<u8>,
    width: u32,
    height: u32,
}

impl Pixels {
    /// The `(r, g, b, a)` bytes of the whole framebuffer, row-major from the top-left.
    pub fn rgba(&self) -> &[u8] {
        &self.data
    }

    /// The fraction of pixels that are exactly `rgba`: how flat the frame is (the dead
    /// canvas behind a crashed app is one flat colour).
    pub fn fraction(&self, rgba: [u8; 4]) -> f32 {
        if self.data.is_empty() {
            return 0.0;
        }
        self.data
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|&pixel| *pixel == rgba)
            .count() as f32
            / (self.data.len() / 4) as f32
    }

    /// How many pixels differ between two captures of the same size: 0 means the frames
    /// are identical, i.e. nothing animates.
    pub fn differences(&self, other: &Pixels) -> usize {
        self.data
            .iter()
            .zip(other.data.iter())
            .filter(|(a, b)| a != b)
            .count()
    }

    /// The capture size, for assertions about what came back.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Why a headless app could not be built. Rendering is *impossible* on this machine, not
/// broken: the caller should skip loudly (print why), not fail.
#[derive(Debug)]
pub enum ServerError {
    /// `wgpu` found no adapter on any of the pinned backends (Vulkan, then GL): the machine
    /// has no GPU driver at all — a container or runner without mesa/llvmpipe.
    NoAdapter(String),
    /// The adapter was found but `request_device` failed: its limits do not satisfy the
    /// renderer's.
    NoDevice(String),
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerError::NoAdapter(message) => write!(f, "no wgpu adapter: {message}"),
            ServerError::NoDevice(message) => write!(f, "wgpu device request failed: {message}"),
        }
    }
}

impl std::error::Error for ServerError {}

/// The empty RGBA image the renderer fills in. It stays on the CPU side (`MAIN_WORLD`):
/// the screenshot is what brings the rendered bytes back.
fn capture_image(capture: Capture) -> Image {
    Image::new(
        Extent3d {
            width: capture.width,
            height: capture.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0; capture.width as usize * capture.height as usize * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD,
    )
}

/// The screenshot images that have come back, newest last: the observer of
/// [`ScreenshotCaptured`] pushes into this.
#[derive(Resource, Default)]
struct Captures(Vec<Pixels>);

/// Builds the app for booting it headlessly with no GPU at all: the same plugins, resources
/// and systems as the browser's, minus the renderer — `MinimalPlugins` and the input
/// resources the systems read (a window-less `App` has no `ButtonInput<KeyCode>` or
/// `Touches` of its own).
///
/// `Boot::advance` then runs whole main-schedule frames (Startup, the fixed timestep
/// ticking the shell, the drawing systems — which draw into the pools), the ECS validating
/// every system's params on the first one. This is the fast, GPU-less layer of
/// `tests/render.rs`: the regression test for the black screen (a resource some system
/// reads but nothing initialises panics on the very first frame), and it runs everywhere,
/// adapter or not.
pub fn boot_app(shell: Shell) -> Boot {
    let mut app = build_app(shell, Headless::Server);
    app.add_plugins(InputPlugin)
        // The one render resource `draw` touches: the colour the window would be cleared
        // with. There is no renderer here (the drawing stops at the pools), but the system
        // still reads it, so it must exist exactly like it does in the browser's app.
        .init_resource::<ClearColor>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    // The `Window` entity `WindowPlugin` would have spawned: `draw` sizes the letterboxed
    // viewport from it (`windows.get_single()`), so the headless boot must give it one or
    // the whole drawing path is skipped. Same component, same default 1280x720 — only the
    // OS window behind it is missing.
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    Boot { app }
}

/// The booted app without a renderer: whole frames of the ECS, driven by hand.
pub struct Boot {
    app: App,
}

impl Boot {
    /// One frame of `seconds` of virtual time: exactly what a browser frame does.
    pub fn advance(&mut self, seconds: f64) {
        let mut strategy = self.app.world_mut().resource_mut::<TimeUpdateStrategy>();
        *strategy = TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(seconds));
        self.app.update();
    }

    /// Whether the drawing systems have mapped anything onto the screen yet: the pools
    /// hold at least one sprite or label (the shell drew a non-empty frame).
    pub fn frame_drawn(&self) -> bool {
        let pools = self.app.world().resource::<Pools>();
        !pools.rects.is_empty() || !pools.labels.is_empty()
    }
}

/// Builds the real app for rendering without a window, a GPU context or a sound card — on
/// a software rasterizer when no GPU is present. The construction is the one the browser
/// runs ([`build_app`]); on top of it:
///
/// * the renderer is initialised manually (`RenderCreation::Manual`): `Automatic` would try
///   to create a surface for a window that does not exist. The adapter is requested on
///   Vulkan and GL, so llvmpipe (or lavapipe/zink/llvmpipe-GL) is enough;
/// * the render plugins the sprite and text drawing needs are added by hand
///   (`MinimalPlugins` includes none of them), and the clock is driven by hand
///   ([`TimeUpdateStrategy::ManualDuration`]) so the tests are deterministic;
/// * a `Camera2d` targets the capture `Image` (the screen camera of `spawn_camera` draws
///   nothing: its window target is never extracted, no `RawHandleWrapper` backs the
///   `Window` entity), and [`Server::capture`] takes a [`Screenshot`] of the image to read
///   the frame back — the readback is asynchronous, so it lands a few `update()`s after
///   the frame was rendered.
///
/// `app.update()` runs whole frames: the main schedule (Startup, the fixed timestep, the
/// drawing systems), then extraction and the render schedule, into the capture image.
///
/// # Errors
///
/// [`ServerError`] when no adapter is available on any pinned backend, or the adapter
/// cannot provide a device: rendering is impossible, not broken — the caller should skip
/// loudly (print why) rather than fail.
pub fn server_app(shell: Shell, capture: Capture) -> Result<Server, ServerError> {
    let mut app = build_app(shell, Headless::Render);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: Backends::VULKAN | Backends::GL,
        ..default()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::None,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok_or_else(|| {
        ServerError::NoAdapter("no adapter on VULKAN | GL (no GPU driver / rasterizer)".into())
    })?;
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("srr-server"),
            required_features: adapter.features(),
            required_limits: adapter.limits(),
            ..default()
        },
        None,
    ))
    .map_err(|error| ServerError::NoDevice(error.to_string()))?;

    app.add_plugins(RenderPlugin {
        render_creation: RenderCreation::Manual(
            RenderDevice::from(device),
            RenderQueue(Arc::new(WgpuWrapper::new(queue))),
            RenderAdapterInfo(WgpuWrapper::new(adapter.get_info())),
            RenderAdapter(Arc::new(WgpuWrapper::new(adapter))),
            RenderInstance(Arc::new(WgpuWrapper::new(instance))),
        ),
        // The tests wait on pipelines: nothing compiles in the background to race against.
        synchronous_pipeline_compilation: true,
    })
    .add_plugins((
        ImagePlugin::default_linear(),
        CorePipelinePlugin,
        TransformPlugin,
        InputPlugin,
        TextPlugin,
        SpritePlugin::default(),
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
    .init_resource::<Captures>();

    // The capture image: an asset (the asset store exists — `ImagePlugin` just built it) the
    // headless camera targets instead of a window.
    let capture_handle = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(capture_image(capture));
    // The `Window` entity `WindowPlugin` would have spawned: `draw` sizes the letterboxed
    // viewport from it (`windows.get_single()`), so the headless app must give it one or
    // nothing would be drawn into the capture. Same component, same default 1280x720 —
    // only the OS window behind it is missing. `spawn_camera` (shared with the browser)
    // has already spawned the screen camera; its target does not exist here, so it draws
    // nothing and stays harmless: the headless camera below is the one that renders.
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    let camera_target = capture_handle.clone();
    app.add_systems(Startup, move |mut commands: Commands| {
        commands.spawn((
            Camera2d,
            Camera {
                target: RenderTarget::Image(camera_target.clone()),
                clear_color: ClearColorConfig::Default,
                ..default()
            },
        ));
    });
    Ok(Server {
        app,
        capture: capture_handle,
    })
}

impl Server {
    /// Runs `updates` frames, advancing the virtual clock by `seconds` per frame, then takes
    /// a [`Screenshot`] of the capture image and waits (a bounded number of further
    /// `update()`s, the readback is asynchronous) for the pixels to come back.
    ///
    /// # Panics
    ///
    /// Panics if the screenshot never completes within `budget` updates: the renderer would
    /// be broken, not slow (the capture is tiny).
    pub fn capture(&mut self, updates: u32, seconds: f64, budget: u32) -> Pixels {
        for _ in 0..updates {
            self.advance(seconds);
        }
        // A fresh screenshot entity per capture: the plugin despawns it once the image has
        // come back (see `Captured`), and the observer of the event stashes it.
        let entity = self
            .app
            .world_mut()
            .spawn(Screenshot::image(self.capture.clone()))
            .observe(
                |trigger: Trigger<ScreenshotCaptured>, mut captures: ResMut<Captures>| {
                    captures.0.push(Pixels {
                        data: trigger.event().0.data.clone(),
                        width: trigger.event().0.texture_descriptor.size.width,
                        height: trigger.event().0.texture_descriptor.size.height,
                    });
                },
            )
            .id();
        for _ in 0..budget {
            self.advance(seconds);
            let mut captures = self.app.world_mut().resource_mut::<Captures>();
            if let Some(pixels) = captures.0.pop() {
                return pixels;
            }
        }
        panic!("the screenshot of the capture image never came back (entity {entity:?})");
    }

    /// One frame at `seconds` of virtual time: exactly what a browser frame does at that
    /// frame time. The device is polled after the frame: without an event loop driving it,
    /// nothing else would complete the screenshot's buffer readback.
    fn advance(&mut self, seconds: f64) {
        let mut strategy = self.app.world_mut().resource_mut::<TimeUpdateStrategy>();
        *strategy = TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(seconds));
        self.app.update();
        if let Some(render_app) = self.app.get_sub_app_mut(RenderApp) {
            render_app
                .world()
                .resource::<RenderDevice>()
                .poll(wgpu::Maintain::Poll);
        }
    }
}
