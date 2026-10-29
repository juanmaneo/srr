//! The [Fyrox] 1.0 backend: `cargo run --no-default-features --features fyrox`.
//!
//! Fyrox runs the window and the loop through its [`Executor`]; the whole game lives in one
//! [`Plugin`], [`Game`], which owns the [`Shell`] (so the non-`Send` audio stream never leaves
//! the main thread: Fyrox plugins only need `Visit + Reflect`, not `Send`).
//!
//! * **Time.** The executor calls [`Plugin::update`] at a fixed
//!   [`TICKS_PER_SECOND`] rate, but when it lags it may call it once with a longer `dt` to
//!   catch up. The plugin therefore keeps its own accumulator and runs [`Shell::tick`] once per
//!   elapsed 1/60 s, so the game always advances exactly 60 ticks per second of real time
//!   (with a cap on catching up after a long stall).
//! * **Keys.** Keyboard events (physical keys, so the keypad works whatever the NumLock state
//!   and layout) are folded into the set of [`Key`]s held. A key pressed and released between
//!   two ticks is still held for one tick, so quick taps are never lost; losing the window focus
//!   releases everything.
//! * **Drawing.** Each [`Frame`] is mirrored onto Fyrox's UI: two [`Canvas`] layers (rectangles
//!   below, labels above) placed on the letterboxed [`Viewport`] and clipping to it, holding a
//!   pool of borderless [`Border`] widgets (one per [`Rect`]) and of [`Text`] widgets (one per
//!   [`Label`], in the embedded Fira Mono). The pools only grow; widgets beyond the frame's
//!   count are hidden. Every widget remembers what it shows, so a tick only sends UI messages
//!   for what actually changed (positions, sizes, colours, text). Coordinates are converted to
//!   whole window pixels, and fonts are rasterised at their on-screen size so text stays crisp.
//!   The window is cleared with [`Frame::background`].
//!
//! Only tested natively; the web build uses Bevy.
//!
//! [Fyrox]: https://fyrox.rs

use std::collections::HashSet;

use fyrox::core::algebra::Vector2;
use fyrox::core::color::Color;
use fyrox::core::log::Log;
use fyrox::core::pool::{Handle, ObjectOrVariant};
use fyrox::core::reflect::prelude::*;
use fyrox::core::visitor::{Visit, VisitResult, Visitor};
use fyrox::dpi::LogicalSize;
use fyrox::engine::executor::Executor;
use fyrox::engine::{GraphicsContext, GraphicsContextParams};
use fyrox::event::{ElementState, Event, WindowEvent};
use fyrox::event_loop::EventLoop;
use fyrox::gui::border::{Border, BorderBuilder};
use fyrox::gui::brush::Brush;
use fyrox::gui::canvas::{Canvas, CanvasBuilder};
use fyrox::gui::font::{Font, FontResource, FontStyles};
use fyrox::gui::image::{Image, ImageBuilder};
use fyrox::gui::text::{Text, TextBuilder, TextMessage};
use fyrox::gui::texture::{
    TextureKind, TexturePixelKind, TextureResource, TextureResourceExtension,
};
use fyrox::gui::widget::{WidgetBuilder, WidgetMessage};
use fyrox::gui::{Thickness, UiNode, UserInterface};
use fyrox::keyboard::{KeyCode, PhysicalKey};
use fyrox::plugin::error::GameResult;
use fyrox::plugin::{Plugin, PluginContext};
use fyrox::resource::ResourceKind;
use fyrox::window::WindowAttributes;

use crate::front::{Frame, GamepadState, Label, Rect, Rgba, SCREEN_HEIGHT, SCREEN_WIDTH, Viewport};
use crate::game::{Key, TICKS_PER_SECOND};
use crate::platform::Shell;

use super::{Engine, WINDOW_TITLE};

/// The monospace font of every label (0.6 em advance, all printable ASCII).
const FONT: &[u8] = include_bytes!("../../assets/fonts/FiraMono-subset.ttf");
/// Side of the font atlas pages, in pixels: room for glyphs of the largest labels on a 4K screen.
const FONT_PAGE_SIZE: usize = 1024;
/// Duration of one game tick, in seconds.
const TICK: f32 = 1.0 / TICKS_PER_SECOND as f32;
/// After a stall, at most this many ticks are replayed at once; the rest of the time is dropped.
const MAX_CATCH_UP_TICKS: u32 = 6;

/// The Fyrox engine.
pub struct Fyrox;

impl Engine for Fyrox {
    const NAME: &'static str = "Fyrox";

    fn run(shell: Shell) {
        // Fyrox is chatty at the info level (GPU, shaders, resources); keep warnings and errors.
        Log::set_log_info(false);

        let event_loop = EventLoop::new().expect("cannot create the window event loop");
        let window_attributes = WindowAttributes::default()
            .with_title(WINDOW_TITLE)
            .with_inner_size(LogicalSize::new(SCREEN_WIDTH, SCREEN_HEIGHT))
            .with_resizable(true);
        let mut executor = Executor::from_params(
            Some(event_loop),
            GraphicsContextParams {
                window_attributes,
                ..GraphicsContextParams::default()
            },
        );
        executor.set_desired_update_rate(TICKS_PER_SECOND as f32);
        // The game has no asset files to watch or index: everything is embedded.
        executor.set_resource_hot_reloading_enabled(false);
        // Fyrox indexes the resources of the folder holding its registry and writes the registry
        // there (`./data/` by default). Point it to a scratch folder instead of littering the
        // working directory, and empty it: outside the working directory, the scan warns about
        // every file it finds, including the registry of the previous run.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let registry = std::env::temp_dir()
                .join("srr-fyrox")
                .join("resources.registry");
            // Missing on the first run; any other failure only costs a warning.
            let _ = std::fs::remove_file(&registry);
            executor
                .resource_manager
                .state()
                .resource_registry
                .lock()
                .set_path(registry);
        }
        executor.add_plugin(Game::new(shell));
        executor.run();
    }
}

/// The plugin running the game: the shell, the keys held and the widgets mirroring the frame.
#[derive(Debug, Reflect)]
#[reflect(non_cloneable)]
struct Game {
    #[reflect(hidden)]
    shell: Shell,
    #[reflect(hidden)]
    keys: HeldKeys,
    /// Real time not yet consumed by ticks, in seconds.
    #[reflect(hidden)]
    lag: f32,
    /// The UI the frames are drawn on and its widgets, created by [`Plugin::init`].
    #[reflect(hidden)]
    view: Option<(Handle<UserInterface>, View)>,
}

/// Nothing to save: the game state is not serialised through Fyrox.
impl Visit for Game {
    fn visit(&mut self, _name: &str, _visitor: &mut Visitor) -> VisitResult {
        Ok(())
    }
}

impl Game {
    fn new(shell: Shell) -> Self {
        Game {
            shell,
            keys: HeldKeys::default(),
            lag: 0.0,
            view: None,
        }
    }
}

impl Plugin for Game {
    fn init(&mut self, _scene_path: Option<&str>, context: PluginContext) -> GameResult {
        let font = Font::from_memory(FONT, FONT_PAGE_SIZE, FontStyles::default(), Vec::new())
            .expect("the embedded Fira Mono is a valid font");
        let font = FontResource::new_embedded(font);
        // The engine starts without any UI; it renders (and resizes) every one registered.
        let mut ui = UserInterface::new(Vector2::new(SCREEN_WIDTH, SCREEN_HEIGHT));
        let view = View::new(&mut ui, font);
        let ui = context.user_interfaces.add(ui);
        self.view = Some((ui, view));
        Ok(())
    }

    fn update(&mut self, context: &mut PluginContext) -> GameResult {
        self.lag += context.dt;
        let mut ticks = 0;
        while self.lag >= TICK {
            self.lag -= TICK;
            self.shell.tick(self.keys.held(), &[], GamepadState::new());
            self.keys.ticked();
            ticks += 1;
            if ticks == MAX_CATCH_UP_TICKS {
                self.lag = 0.0;
                break;
            }
        }

        let GraphicsContext::Initialized(graphics) = &mut *context.graphics_context else {
            return Ok(());
        };
        let Some((ui, view)) = &mut self.view else {
            return Ok(());
        };
        let ui = context.user_interfaces.try_get_mut(*ui)?;
        let size = graphics.window.inner_size();
        let viewport = Viewport::fit(size.width as f32, size.height as f32);
        if ticks > 0 || view.viewport != Some(viewport) {
            let frame = self.shell.frame();
            graphics
                .renderer
                .set_backbuffer_clear_color(color(frame.background));
            view.show(ui, viewport, &frame);
        }
        Ok(())
    }

    fn on_os_event(&mut self, event: &Event<()>, _context: PluginContext) -> GameResult {
        let Event::WindowEvent { event, .. } = event else {
            return Ok(());
        };
        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key
                    && let Some(key) = map_key(code)
                {
                    match event.state {
                        ElementState::Pressed => self.keys.press(key),
                        ElementState::Released => self.keys.release(key),
                    }
                }
            }
            WindowEvent::Focused(false) => self.keys.clear(),
            _ => {}
        }
        Ok(())
    }
}

/// The game key a physical keyboard key stands for, if any.
fn map_key(code: KeyCode) -> Option<Key> {
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

/// The keys held, as seen by the next tick.
#[derive(Debug, Default)]
struct HeldKeys {
    held: HashSet<Key>,
    /// Pressed since the last tick.
    fresh: HashSet<Key>,
    /// Released since the last tick although pressed since then too: kept held for one tick.
    released_late: HashSet<Key>,
}

impl HeldKeys {
    fn held(&self) -> &HashSet<Key> {
        &self.held
    }

    fn press(&mut self, key: Key) {
        if self.held.insert(key) {
            self.fresh.insert(key);
        }
        self.released_late.remove(&key);
    }

    fn release(&mut self, key: Key) {
        if self.fresh.contains(&key) {
            self.released_late.insert(key);
        } else {
            self.held.remove(&key);
        }
    }

    /// A tick saw the keys: apply the deferred releases.
    fn ticked(&mut self) {
        for key in self.released_late.drain() {
            self.held.remove(&key);
        }
        self.fresh.clear();
    }

    fn clear(&mut self) {
        self.held.clear();
        self.fresh.clear();
        self.released_late.clear();
    }
}

/// The UI widgets mirroring the last frame shown.
#[derive(Debug)]
struct View {
    font: FontResource,
    /// Holds the rectangles; drawn first.
    rect_layer: Handle<Canvas>,
    /// Holds the sprites; drawn above every rectangle.
    image_layer: Handle<Canvas>,
    /// Holds the labels; drawn above everything.
    label_layer: Handle<Canvas>,
    /// The viewport the layers are placed on, once shown.
    viewport: Option<Viewport>,
    rects: Vec<Slot<Border, RectLook>>,
    images: Vec<Slot<Image, ImageLook>>,
    labels: Vec<Slot<Text, LabelLook>>,
    /// The sprite textures created so far, by name and pose.
    textures: Vec<(&'static str, crate::front::skin::Pose, TextureResource)>,
}

/// A pooled widget and what it currently shows (`None` when hidden).
#[derive(Debug)]
struct Slot<W, L> {
    handle: Handle<W>,
    look: Option<L>,
}

/// What a rectangle widget shows, in window pixels relative to the layer.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RectLook {
    position: Vector2<f32>,
    size: Vector2<f32>,
    color: Color,
}

/// What an image widget shows, in window pixels relative to the layer.
#[derive(Debug, Clone)]
struct ImageLook {
    position: Vector2<f32>,
    size: Vector2<f32>,
    texture: TextureResource,
}

/// What a text widget shows, in window pixels relative to the layer.
#[derive(Debug, Clone, PartialEq)]
struct LabelLook {
    text: String,
    position: Vector2<f32>,
    font_size: f32,
    color: Color,
}

impl View {
    fn new(ui: &mut UserInterface, font: FontResource) -> Self {
        // Built in this order under the root, so each layer is drawn above the last.
        let rect_layer = layer(ui);
        let image_layer = layer(ui);
        let label_layer = layer(ui);
        View {
            font,
            rect_layer,
            image_layer,
            label_layer,
            viewport: None,
            rects: Vec::new(),
            images: Vec::new(),
            labels: Vec::new(),
            textures: Vec::new(),
        }
    }

    /// Updates the widgets to show `frame` letterboxed on `viewport`.
    fn show(&mut self, ui: &mut UserInterface, viewport: Viewport, frame: &Frame) {
        if self.viewport != Some(viewport) {
            self.viewport = Some(viewport);
            let position = Vector2::new(viewport.offset_x.round(), viewport.offset_y.round());
            let size = Vector2::new(
                (SCREEN_WIDTH * viewport.scale).round(),
                (SCREEN_HEIGHT * viewport.scale).round(),
            );
            for layer in [self.rect_layer, self.image_layer, self.label_layer] {
                ui.send(layer, WidgetMessage::DesiredPosition(position));
                ui.send(layer, WidgetMessage::Width(size.x));
                ui.send(layer, WidgetMessage::Height(size.y));
            }
        }
        if viewport.scale <= 0.0 {
            // Minimised: nothing visible, and fonts cannot be rasterised at size zero.
            return;
        }

        for (index, rect) in frame.rects.iter().enumerate() {
            let look = rect_look(rect, viewport.scale);
            match self.rects.get_mut(index) {
                Some(slot) => update_rect(ui, slot, look),
                None => {
                    let slot = self.new_rect(ui, look);
                    self.rects.push(slot);
                }
            }
        }
        for slot in self.rects.iter_mut().skip(frame.rects.len()) {
            hide(ui, slot);
        }

        // The sprites: the manga fighters, between the rectangles and the labels.
        for (index, image) in frame.images.iter().enumerate() {
            let Some(look) = image_look(image, viewport.scale, &mut self.textures) else {
                continue;
            };
            match self.images.get_mut(index) {
                Some(slot) => update_image(ui, slot, look),
                None => {
                    let slot = self.new_image(ui, look);
                    self.images.push(slot);
                }
            }
        }
        for slot in self.images.iter_mut().skip(frame.images.len()) {
            hide(ui, slot);
        }

        for (index, label) in frame.labels.iter().enumerate() {
            let look = label_look(label, viewport.scale);
            match self.labels.get_mut(index) {
                Some(slot) => update_label(ui, slot, look),
                None => {
                    let slot = self.new_label(ui, look);
                    self.labels.push(slot);
                }
            }
        }
        for slot in self.labels.iter_mut().skip(frame.labels.len()) {
            hide(ui, slot);
        }
    }

    fn new_rect(&self, ui: &mut UserInterface, look: RectLook) -> Slot<Border, RectLook> {
        let widget = WidgetBuilder::new()
            .with_hit_test_visibility(false)
            .with_desired_position(look.position)
            .with_width(look.size.x)
            .with_height(look.size.y)
            .with_background(Brush::Solid(look.color).into())
            .with_foreground(Brush::Solid(Color::TRANSPARENT).into());
        let handle = BorderBuilder::new(widget)
            .with_stroke_thickness(Thickness::uniform(0.0).into())
            .build(&mut ui.build_ctx());
        ui.link_nodes(handle, self.rect_layer, false);
        Slot {
            handle,
            look: Some(look),
        }
    }

    fn new_image(&self, ui: &mut UserInterface, look: ImageLook) -> Slot<Image, ImageLook> {
        let widget = WidgetBuilder::new()
            .with_hit_test_visibility(false)
            .with_desired_position(look.position)
            .with_width(look.size.x)
            .with_height(look.size.y);
        let handle = ImageBuilder::new(widget)
            .with_texture(look.texture.clone())
            .build(&mut ui.build_ctx());
        ui.link_nodes(handle, self.image_layer, false);
        Slot {
            handle,
            look: Some(look),
        }
    }

    fn new_label(&self, ui: &mut UserInterface, look: LabelLook) -> Slot<Text, LabelLook> {
        let widget = WidgetBuilder::new()
            .with_hit_test_visibility(false)
            .with_desired_position(look.position)
            .with_foreground(Brush::Solid(look.color).into());
        let handle = TextBuilder::new(widget)
            .with_font(self.font.clone())
            .with_font_size(look.font_size.into())
            .with_text(look.text.clone())
            .build(&mut ui.build_ctx());
        ui.link_nodes(handle, self.label_layer, false);
        Slot {
            handle,
            look: Some(look),
        }
    }
}

/// An empty canvas under the UI root, clipping its children to the virtual screen.
fn layer(ui: &mut UserInterface) -> Handle<Canvas> {
    let widget = WidgetBuilder::new()
        .with_hit_test_visibility(false)
        .with_clip_to_bounds(true);
    CanvasBuilder::new(widget).build(&mut ui.build_ctx())
}

/// `rect` in window pixels, its edges snapped to whole pixels so neighbours never leave seams.
fn rect_look(rect: &Rect, scale: f32) -> RectLook {
    let left = (rect.x * scale).round();
    let top = (rect.y * scale).round();
    let right = ((rect.x + rect.width) * scale).round();
    let bottom = ((rect.y + rect.height) * scale).round();
    RectLook {
        position: Vector2::new(left, top),
        size: Vector2::new(right - left, bottom - top),
        color: color(rect.color),
    }
}

/// `label` in window pixels. The text keeps its spaces: with a monospace font they are what
/// lines the stacked labels of the ASCII art up on the character grid.
fn label_look(label: &Label, scale: f32) -> LabelLook {
    LabelLook {
        text: label.text.clone(),
        position: Vector2::new((label.left() * scale).round(), (label.y * scale).round()),
        font_size: label.size * scale,
        color: color(label.color),
    }
}

fn update_rect(ui: &UserInterface, slot: &mut Slot<Border, RectLook>, look: RectLook) {
    let handle = slot.handle;
    let old = show_slot(ui, slot, look);
    if old.is_none_or(|old| old.position != look.position) {
        ui.send(handle, WidgetMessage::DesiredPosition(look.position));
    }
    if old.is_none_or(|old| old.size.x != look.size.x) {
        ui.send(handle, WidgetMessage::Width(look.size.x));
    }
    if old.is_none_or(|old| old.size.y != look.size.y) {
        ui.send(handle, WidgetMessage::Height(look.size.y));
    }
    if old.is_none_or(|old| old.color != look.color) {
        ui.send(
            handle,
            WidgetMessage::Background(Brush::Solid(look.color).into()),
        );
    }
}

/// `image` in window pixels: its feet on its `(x, y)`, `height` tall at the viewport's
/// scale. The texture is created once per name, pose and facing (the left-facing cells are
/// pre-mirrored in the decoded sheet, so nothing is flipped here).
fn image_look(
    image: &crate::front::scene::Image,
    scale: f32,
    textures: &mut Vec<(
        &'static str,
        crate::front::skin::Pose,
        crate::front::skin::Facing,
        TextureResource,
    )>,
) -> Option<ImageLook> {
    let sprite = crate::front::skin::sprite_by_name(image.name, image.pose, image.facing)?;
    let texture = match textures
        .iter()
        .find(|(n, p, f, _)| *n == image.name && *p == image.pose && *f == image.facing)
    {
        Some((_, _, _, texture)) => texture.clone(),
        None => {
            let texture = sprite_texture(&sprite)?;
            textures.push((
                sprite_name(image.name)?,
                image.pose,
                image.facing,
                texture.clone(),
            ));
            texture
        }
    };
    let height = image.height * scale;
    let width = height * sprite.width as f32 / sprite.height.max(1) as f32;
    // The feet at the bottom-centre of the box.
    let left = image.x * scale - width / 2.0;
    let top = image.y * scale - height;
    Some(ImageLook {
        position: Vector2::new(left.round(), top.round()),
        size: Vector2::new(width, height),
        texture,
    })
}

/// The static name of the sprite `name` (a `&'static str` for the cache).
fn sprite_name(name: &str) -> Option<&'static str> {
    crate::front::skin::SPRITE_NAMES
        .iter()
        .copied()
        .find(|n| *n == name)
}

/// The texture of a sprite, built directly from the embedded RGBA bytes.
fn sprite_texture(sprite: &crate::front::skin::Sprite) -> Option<TextureResource> {
    let mut bytes = Vec::with_capacity(sprite.pixels.len());
    bytes.extend_from_slice(sprite.pixels);
    Some(TextureResource::from_bytes(
        fyrox::core::uuid::Uuid::new_v4(),
        TextureKind::Rectangle {
            width: sprite.width as u32,
            height: sprite.height as u32,
        },
        TexturePixelKind::RGBA8,
        bytes,
        ResourceKind::Embedded,
    )?)
}

fn update_image(ui: &UserInterface, slot: &mut Slot<Image, ImageLook>, look: ImageLook) {
    let handle = slot.handle;
    let old = show_slot(ui, slot, look.clone());
    if old.is_none_or(|old| old.position != look.position) {
        ui.send(handle, WidgetMessage::DesiredPosition(look.position));
    }
    if old.is_none_or(|old| old.size.x != look.size.x) {
        ui.send(handle, WidgetMessage::Width(look.size.x));
    }
    if old.is_none_or(|old| old.size.y != look.size.y) {
        ui.send(handle, WidgetMessage::Height(look.size.y));
    }
}

fn update_label(ui: &UserInterface, slot: &mut Slot<Text, LabelLook>, look: LabelLook) {
    if slot.look.as_ref() == Some(&look) {
        return;
    }
    let handle = slot.handle;
    let new = look.clone();
    let old = show_slot(ui, slot, look);
    if old.as_ref().is_none_or(|old| old.text != new.text) {
        ui.send(handle, TextMessage::Text(new.text));
    }
    if old.as_ref().is_none_or(|old| old.position != new.position) {
        ui.send(handle, WidgetMessage::DesiredPosition(new.position));
    }
    if old
        .as_ref()
        .is_none_or(|old| old.font_size != new.font_size)
    {
        ui.send(handle, TextMessage::FontSize(new.font_size.into()));
    }
    if old.as_ref().is_none_or(|old| old.color != new.color) {
        ui.send(
            handle,
            WidgetMessage::Foreground(Brush::Solid(new.color).into()),
        );
    }
}

/// Makes the slot's widget visible if it was hidden and records `look` as shown; returns the
/// previous look, `None` if the widget was hidden (its properties must then all be resent,
/// as they may be stale).
fn show_slot<W: ObjectOrVariant<UiNode>, L>(
    ui: &UserInterface,
    slot: &mut Slot<W, L>,
    look: L,
) -> Option<L> {
    let old = slot.look.replace(look);
    if old.is_none() {
        ui.send(slot.handle, WidgetMessage::Visibility(true));
    }
    old
}

fn hide<W: ObjectOrVariant<UiNode>, L>(ui: &UserInterface, slot: &mut Slot<W, L>) {
    if slot.look.take().is_some() {
        ui.send(slot.handle, WidgetMessage::Visibility(false));
    }
}

/// The Fyrox colour (8-bit sRGB with alpha) of `rgba`.
fn color(rgba: Rgba) -> Color {
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::from_rgba(byte(rgba.r), byte(rgba.g), byte(rgba.b), byte(rgba.a))
}
