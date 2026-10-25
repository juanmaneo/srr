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

use bevy::prelude::*;
use bevy::sprite::Anchor;
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
        App::new()
            .add_plugins(DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: WINDOW_TITLE.into(),
                    // On the web, render into the canvas of web/index.html.
                    canvas: Some(WEB_CANVAS.into()),
                    fit_canvas_to_parent: true,
                    ..default()
                }),
                ..default()
            }))
            .insert_non_send_resource(ShellResource(shell))
            .insert_resource(Time::<Fixed>::from_hz(f64::from(TICKS_PER_SECOND)))
            .init_resource::<Pools>()
            .init_resource::<Taps>()
            .init_resource::<TouchTaps>()
            .init_resource::<PadTaps>()
            .add_systems(Startup, spawn_camera)
            .add_systems(FixedUpdate, tick)
            .add_systems(Update, (latch_taps, latch_pad_taps, draw))
            .run();
    }
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
