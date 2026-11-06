//! The 3D stage of the third-person view (the `mode3d` feature): the glTF fighters of
//! `assets/models/`, cel-shaded and inked, rendered by a `Camera3d` into an offscreen image
//! that the 2D drawing of [`super::draw`] shows as one sprite — over the alley and the
//! mannequins of the software renderer, under the HUD ([`Stage::layer`]).
//!
//! * The models are loaded once per character ([`load_library`]): the `Gltf` asset, whose
//!   named animations become one [`AnimationGraph`] per actor. A model that fails to load
//!   (missing, broken) is logged and its character keeps its mannequin: the game tells the
//!   front end which characters it draws ([`Game::set_models`]) and never crashes on assets.
//!   A clip the model lacks is logged too; the fighter plays its idle instead.
//! * One actor per fighter ([`cast`]): a root entity (where the fighter stands, which way it
//!   faces) holding a fit node (the model scaled to [`MODEL_HEIGHT`], its feet on the root)
//!   holding the scene. Once the scene is spawned its materials are swapped for
//!   [`ToonMaterial`] and every mesh gets an [`OutlineMaterial`] twin ([`dress`]), then the
//!   model is measured and fitted ([`fit`]).
//! * The animation follows the [`Stage`](crate::front::model::Stage) the front end describes: the clip of the fighter's
//!   state ([`AnimationTransitions`] cross-fades the change over [`CROSSFADE_SECONDS`]), looping,
//!   once, or scrubbed to the fraction the frame data dictates ([`Playback::Scrub`]) — the
//!   contact of a punch lands on its active ticks whatever the frame rate. Nothing flows back:
//!   the rules never read this module.
//! * A manga variant is a skin texture ([`skin_path`]) swapped into the toon materials;
//!   without it (or when it is missing) the model wears its own texture.

use std::f32::consts::FRAC_PI_2;
use std::time::Duration;

use bevy::animation::graph::AnimationNodeIndex;
use bevy::animation::{AnimationClip, RepeatAnimation};
use bevy::asset::{LoadState, RenderAssetUsages, load_internal_asset};
use bevy::color::ColorToComponents;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::gltf::{Gltf, GltfAssetLabel};
use bevy::pbr::{
    Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin, MeshMaterial3d,
    StandardMaterial,
};
use bevy::prelude::*;
use bevy::render::camera::RenderTarget;
use bevy::render::mesh::skinning::SkinnedMesh;
use bevy::render::mesh::{MeshAabb, MeshVertexBufferLayoutRef};

use bevy::render::render_resource::{
    AsBindGroup, Extent3d, Face, RenderPipelineDescriptor, Shader, ShaderRef, ShaderType,
    SpecializedMeshPipelineError, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::view::NoFrustumCulling;
use bevy::scene::SceneInstanceReady;
use bevy::sprite::Anchor;
use bevy::window::PrimaryWindow;

use super::{CurrentStage, ShellResource, depth, world};
use crate::front::model::{
    BAND_EDGES, BAND_LEVELS, CROSSFADE_SECONDS, Clip, LIGHT, MODEL_HEIGHT, Models, OUTLINE_COLOR,
    OUTLINE_WIDTH, Playback, RIM, model_path, skin_path,
};
use crate::front::space::{CENTER_Y, FOCAL};
use crate::front::{SCREEN_HEIGHT, SCREEN_WIDTH, Viewport};
use crate::game::{Character, Facing, PlayerId, Variant};

const TOON_SHADER: Handle<Shader> =
    Handle::weak_from_u128(0x5a7e_2d3b_9c41_4f8e_b6d0_1c2a_3e4f_5061);
const OUTLINE_SHADER: Handle<Shader> =
    Handle::weak_from_u128(0x7b1c_4e6a_0d2f_4a9b_8c3e_5f60_7182_93a4);

/// How far ahead of the eye the near plane sits, in world units (the software renderer's
/// `NEAR` is 16).
const NEAR: f32 = 8.0;
/// Nothing farther than this is drawn (the alley is 4300 units wide).
const FAR: f32 = 6000.0;
/// The white flash of a stunned fighter: how much white is mixed into its albedo.
const FLASH: f32 = 0.7;

/// Registers the materials, the shaders, the camera and the systems. Must come after the
/// render plugins (the materials register render commands in the render app).
pub(super) fn plugin(app: &mut App) {
    load_internal_asset!(app, TOON_SHADER, "toon.wgsl", Shader::from_wgsl);
    load_internal_asset!(app, OUTLINE_SHADER, "outline.wgsl", Shader::from_wgsl);
    app.add_plugins((
        MaterialPlugin::<ToonMaterial> {
            prepass_enabled: false,
            shadows_enabled: false,
            ..default()
        },
        MaterialPlugin::<OutlineMaterial> {
            prepass_enabled: false,
            shadows_enabled: false,
            ..default()
        },
    ))
    .init_resource::<Library>()
    .init_resource::<Cast>()
    .add_systems(Startup, (spawn_stage, open_library))
    .add_systems(
        Update,
        (load_library, cast, dress, fit, wear, composite)
            .chain()
            .after(super::draw),
    );
}

// ---------------------------------------------------------------------------------------------
// Materials
// ---------------------------------------------------------------------------------------------

pub use uniforms::{OutlineSettings, ToonSettings};

/// The uniforms of the materials. Their own module: the `ShaderType` derive emits a layout
/// check per field as a function it never calls, which the compiler reports as dead code.
mod uniforms {
    #![allow(dead_code)]

    use super::*;

    /// The uniform of [`ToonMaterial`] (see `toon.wgsl`).
    #[derive(ShaderType, Debug, Clone, Copy)]
    pub struct ToonSettings {
        /// The material's colour, used when there is no texture.
        pub color: Vec4,
        /// `xyz`: the direction towards the light (unit, world space).
        pub light: Vec4,
        /// `xy`: the band edges ([`BAND_EDGES`]); `z`, `w`: the rim threshold and strength
        /// ([`RIM`]).
        pub edges: Vec4,
        /// `xyz`: the band levels ([`BAND_LEVELS`]); `w`: 1 when the texture is bound.
        pub levels: Vec4,
        /// `rgb` mixed into the albedo by `a`: the flash of a stun.
        pub tint: Vec4,
    }

    /// The uniform of [`OutlineMaterial`] (see `outline.wgsl`).
    #[derive(ShaderType, Debug, Clone, Copy)]
    pub struct OutlineSettings {
        /// The ink.
        pub color: Vec4,
        /// `x`: the thickness of the hull, in world units.
        pub width: Vec4,
    }
}

/// The cel-shaded material of the fighters: stepped diffuse bands, no gloss, an optional rim
/// light, in the colours of the skin texture (or the material's own).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct ToonMaterial {
    #[uniform(0)]
    pub settings: ToonSettings,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Option<Handle<Image>>,
}

impl ToonMaterial {
    /// The toon look of `color` and `texture` (the model's own, or a skin).
    fn new(color: LinearRgba, texture: Option<Handle<Image>>) -> Self {
        // The light of the front end's world, with its `z` mirrored into Bevy's.
        let light = Vec3::new(LIGHT[0], LIGHT[1], -LIGHT[2]).normalize();
        ToonMaterial {
            settings: ToonSettings {
                color: color.to_vec4(),
                light: light.extend(0.0),
                edges: Vec4::new(BAND_EDGES[0], BAND_EDGES[1], RIM.0, RIM.1),
                levels: Vec4::new(
                    BAND_LEVELS[0],
                    BAND_LEVELS[1],
                    BAND_LEVELS[2],
                    if texture.is_some() { 1.0 } else { 0.0 },
                ),
                tint: Vec4::new(1.0, 1.0, 1.0, 0.0),
            },
            texture,
        }
    }

    /// Wears `texture` (`None`: the colour alone).
    fn set_texture(&mut self, texture: Option<Handle<Image>>) {
        self.settings.levels.w = if texture.is_some() { 1.0 } else { 0.0 };
        self.texture = texture;
    }
}

impl Material for ToonMaterial {
    fn fragment_shader() -> ShaderRef {
        TOON_SHADER.into()
    }
}

/// The ink outline: the mesh drawn again as an inverted hull (pushed out along its normals,
/// front faces culled) in a flat dark colour.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct OutlineMaterial {
    #[uniform(0)]
    pub settings: OutlineSettings,
}

impl Default for OutlineMaterial {
    fn default() -> Self {
        OutlineMaterial {
            settings: OutlineSettings {
                color: Vec4::new(OUTLINE_COLOR[0], OUTLINE_COLOR[1], OUTLINE_COLOR[2], 1.0),
                width: Vec4::new(OUTLINE_WIDTH, 0.0, 0.0, 0.0),
            },
        }
    }
}

impl Material for OutlineMaterial {
    fn vertex_shader() -> ShaderRef {
        OUTLINE_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        OUTLINE_SHADER.into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = Some(Face::Front);
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// The library: one model per character
// ---------------------------------------------------------------------------------------------

/// The clips of a loaded model, by [`Clip::index`]: `None` where the model lacks the clip.
type Clips = [Option<Handle<AnimationClip>>; 13];

/// Where a character's model stands.
#[derive(Debug, Clone)]
enum Model {
    /// The `Gltf` is loading.
    Loading(Handle<Gltf>),
    /// Loaded, with its clips (the handle keeps the asset alive for the actors' scenes).
    Ready {
        #[allow(dead_code)]
        gltf: Handle<Gltf>,
        clips: Box<Clips>,
    },
    /// Failed to load (logged): the character keeps its mannequin.
    Missing,
}

/// The models of every character ([`Character::ALL`] order), and whether the game was told
/// which ones it has.
#[derive(Resource, Default)]
struct Library {
    models: Vec<(Character, Model)>,
    reported: bool,
}

impl Library {
    fn model(&self, character: Character) -> Option<&Model> {
        self.models
            .iter()
            .find(|(c, _)| *c == character)
            .map(|(_, model)| model)
    }
}

/// Starts loading every character's model.
fn open_library(asset_server: Res<AssetServer>, mut library: ResMut<Library>) {
    library.models = Character::ALL
        .into_iter()
        .map(|character| {
            let path = model_path(character);
            (character, Model::Loading(asset_server.load::<Gltf>(path)))
        })
        .collect();
}

/// Settles the models as they load (or fail), and tells the game which characters are drawn
/// in 3D once every model has settled.
fn load_library(
    asset_server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut library: ResMut<Library>,
    mut shell: NonSendMut<ShellResource>,
) {
    for (character, model) in &mut library.models {
        let Model::Loading(handle) = model else {
            continue;
        };
        let path = model_path(*character);
        match asset_server.load_state(handle.id()) {
            LoadState::Loaded => {
                let Some(gltf) = gltfs.get(&*handle) else {
                    continue;
                };
                let mut clips: Clips = Default::default();
                for clip in Clip::ALL {
                    match gltf.named_animations.get(clip.name()) {
                        Some(animation) => clips[clip.index()] = Some(animation.clone()),
                        None => error!(
                            "3D model {path} has no `{}` animation clip: {} plays its idle instead",
                            clip.name(),
                            character.name()
                        ),
                    }
                }
                info!(
                    "3D model {path} loaded: {} of {} clips",
                    clips.iter().flatten().count(),
                    Clip::ALL.len()
                );
                *model = Model::Ready {
                    gltf: handle.clone(),
                    clips: Box::new(clips),
                };
            }
            LoadState::Failed(error) => {
                warn!(
                    "no 3D model for {}: {path}: {error}; its mannequin is drawn instead",
                    character.name()
                );
                *model = Model::Missing;
            }
            LoadState::NotLoaded | LoadState::Loading => {}
        }
    }
    if library.reported
        || library
            .models
            .iter()
            .any(|(_, model)| matches!(model, Model::Loading(_)))
    {
        return;
    }
    library.reported = true;
    let models = library
        .models
        .iter()
        .filter(|(_, model)| matches!(model, Model::Ready { .. }))
        .fold(Models::NONE, |models, (character, _)| {
            models.with(*character)
        });
    shell.0.game_mut().set_models(models);
}

// ---------------------------------------------------------------------------------------------
// The stage: the camera, the offscreen image and its sprite
// ---------------------------------------------------------------------------------------------

/// The image the 3D camera renders into: the virtual screen, with a transparent background.
#[derive(Resource)]
struct StageImage(Handle<Image>);

#[derive(Component)]
struct StageCamera;

#[derive(Component)]
struct StageNode;

/// Spawns the offscreen image, the 3D camera targeting it (inactive until a stage shows) and
/// the sprite the 2D camera draws it with.
fn spawn_stage(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new(
        Extent3d {
            width: SCREEN_WIDTH as u32,
            height: SCREEN_HEIGHT as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0; SCREEN_WIDTH as usize * SCREEN_HEIGHT as usize * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::RENDER_ATTACHMENT;
    let handle = images.add(image);
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            target: RenderTarget::Image(handle.clone()),
            clear_color: ClearColorConfig::Custom(Color::NONE),
            is_active: false,
            ..default()
        },
        // The pinhole of the software renderer: `FOCAL` pixels from the eye to the screen.
        Projection::Perspective(PerspectiveProjection {
            fov: 2.0 * (CENTER_Y as f32 / FOCAL as f32).atan(),
            aspect_ratio: SCREEN_WIDTH / SCREEN_HEIGHT,
            near: NEAR,
            far: FAR,
        }),
        Tonemapping::None,
        Msaa::Off,
        StageCamera,
    ));
    commands.spawn((
        Sprite {
            image: handle.clone(),
            anchor: Anchor::TopLeft,
            ..default()
        },
        Visibility::Hidden,
        StageNode,
    ));
    commands.insert_resource(StageImage(handle));
}

/// Shows the stage image over the frame's first `layer` rectangles, letterboxed like them.
fn composite(
    stage: Res<CurrentStage>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut nodes: Query<(&mut Sprite, &mut Transform, &mut Visibility), With<StageNode>>,
) {
    let Ok((mut sprite, mut transform, mut visibility)) = nodes.get_single_mut() else {
        return;
    };
    let (Some(stage), Ok(window)) = (&stage.0, windows.get_single()) else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let size = Vec2::new(window.width(), window.height());
    let viewport = Viewport::fit(size.x, size.y);
    let position = world(&viewport, size, 0.0, 0.0).extend(depth(stage.layer, false) - 0.005);
    let custom_size = Some(Vec2::new(SCREEN_WIDTH, SCREEN_HEIGHT) * viewport.scale);
    if sprite.custom_size != custom_size {
        sprite.custom_size = custom_size;
    }
    if transform.translation != position {
        transform.translation = position;
    }
    visibility.set_if_neq(Visibility::Inherited);
}

/// The front end's world point (x along the arena, y up, z towards the wall) in Bevy's
/// right-handed world: `z` mirrored, so the camera looks down `-z` like Bevy's does.
fn to_bevy(point: [f32; 3]) -> Vec3 {
    Vec3::new(point[0], point[1], -point[2])
}

// ---------------------------------------------------------------------------------------------
// The actors
// ---------------------------------------------------------------------------------------------

/// The actor entity of every player on stage.
#[derive(Resource, Default)]
struct Cast(Vec<(PlayerId, Entity)>);

/// The animation of a dressed actor.
#[derive(Debug, Clone)]
struct Animation {
    /// The entity carrying the [`AnimationPlayer`] (the model's root node).
    player: Entity,
    /// The graph node and the length of every clip the model has, by [`Clip::index`].
    nodes: [Option<(AnimationNodeIndex, f32)>; 13],
}

/// One fighter's model: the root entity (position, facing) of a fit node (scale, feet on the
/// floor) holding the glTF scene.
#[derive(Component, Debug)]
struct Actor {
    player: PlayerId,
    character: Character,
    /// The fit node, holding the scene.
    fit: Entity,
    /// The scene has been spawned under the fit node.
    ready: bool,
    /// The materials are toon, the outlines spawned, the animation graph attached.
    dressed: bool,
    /// The model was measured and scaled to [`MODEL_HEIGHT`].
    fitted: bool,
    animation: Option<Animation>,
    /// The toon materials of the model's meshes, and the look each one had in the glTF.
    materials: Vec<(Handle<ToonMaterial>, Option<Handle<Image>>)>,
    /// The clip playing, how, and the cue it was started on ([`Stage`] actors restart a
    /// one-shot clip when their cue changes).
    playing: Option<(Clip, Playback)>,
    cue: u32,
    /// The variant asked for, its texture being loaded.
    skin: Option<(Variant, Handle<Image>)>,
    /// The variant the materials wear (`Some(None)`: the model's own texture).
    worn: Option<Option<Variant>>,
    flash: bool,
}

/// The animation state of an actor on stage.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Loop,
    Once,
    Scrub,
}

fn kind(playback: Playback) -> Kind {
    match playback {
        Playback::Loop => Kind::Loop,
        Playback::Once => Kind::Once,
        Playback::Scrub(_) => Kind::Scrub,
    }
}

/// Spawns the actor of `player` playing `character`: hidden until fitted.
fn spawn_actor(
    commands: &mut Commands,
    asset_server: &AssetServer,
    player: PlayerId,
    character: Character,
) -> Entity {
    let scene = asset_server.load(GltfAssetLabel::Scene(0).from_asset(model_path(character)));
    let root = commands
        .spawn((Transform::IDENTITY, Visibility::Hidden))
        .id();
    let fit = commands
        .spawn((SceneRoot(scene), Transform::IDENTITY, Visibility::Inherited))
        .observe(
            move |_: Trigger<SceneInstanceReady>, mut actors: Query<&mut Actor>| {
                if let Ok(mut actor) = actors.get_mut(root) {
                    actor.ready = true;
                }
            },
        )
        .id();
    commands.entity(root).add_child(fit).insert(Actor {
        player,
        character,
        fit,
        ready: false,
        dressed: false,
        fitted: false,
        animation: None,
        materials: Vec::new(),
        playing: None,
        cue: 0,
        skin: None,
        worn: None,
        flash: false,
    });
    root
}

/// Places the actors of the current [`Stage`] and drives their animation; hides the actors
/// not on stage and the whole stage when the frame has none.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn cast(
    mut commands: Commands,
    stage: Res<CurrentStage>,
    asset_server: Res<AssetServer>,
    library: Res<Library>,
    mut cast: ResMut<Cast>,
    mut cameras: Query<(&mut Camera, &mut Transform), With<StageCamera>>,
    mut actors: Query<(&mut Actor, &mut Transform, &mut Visibility), Without<StageCamera>>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Ok((mut camera, mut camera_transform)) = cameras.get_single_mut() else {
        return;
    };
    let Some(stage) = &stage.0 else {
        camera.is_active = false;
        for (_, _, mut visibility) in &mut actors {
            visibility.set_if_neq(Visibility::Hidden);
        }
        return;
    };
    camera.is_active = true;
    *camera_transform =
        Transform::from_translation(to_bevy(stage.eye)).looking_at(to_bevy(stage.target), Vec3::Y);

    let mut on_stage = Vec::new();
    for actor in &stage.actors {
        if !matches!(library.model(actor.character), Some(Model::Ready { .. })) {
            continue;
        }
        let entity = match cast.0.iter().find(|(player, _)| *player == actor.player) {
            Some(&(_, entity))
                if actors
                    .get(entity)
                    .is_ok_and(|(a, _, _)| a.character == actor.character) =>
            {
                entity
            }
            found => {
                if let Some(&(_, old)) = found {
                    commands.entity(old).despawn_recursive();
                    cast.0.retain(|(player, _)| *player != actor.player);
                }
                let entity =
                    spawn_actor(&mut commands, &asset_server, actor.player, actor.character);
                cast.0.push((actor.player, entity));
                entity
            }
        };
        on_stage.push(entity);
        let Ok((mut state, mut transform, mut visibility)) = actors.get_mut(entity) else {
            continue;
        };
        let yaw = match actor.facing {
            Facing::Right => FRAC_PI_2,
            Facing::Left => -FRAC_PI_2,
        };
        *transform = Transform {
            translation: to_bevy(actor.position),
            rotation: Quat::from_rotation_y(yaw),
            scale: Vec3::ONE,
        };
        visibility.set_if_neq(if state.fitted {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
        if state.skin.as_ref().map(|(variant, _)| *variant) != actor.variant {
            state.skin = actor.variant.map(|variant| {
                let path = skin_path(actor.character, variant);
                (variant, asset_server.load::<Image>(path))
            });
            state.worn = None;
        }
        state.flash = actor.flash;
        let Some(animation) = state.animation.clone() else {
            continue;
        };
        let Ok((mut player, mut transitions)) = players.get_mut(animation.player) else {
            continue;
        };
        // A clip the model lacks plays the idle.
        let Some((node, duration)) =
            animation.nodes[actor.clip.index()].or(animation.nodes[Clip::Idle.index()])
        else {
            continue;
        };
        let restart = match state.playing {
            Some((clip, playback)) if clip == actor.clip => {
                kind(playback) == Kind::Once
                    && kind(actor.playback) == Kind::Once
                    && state.cue != actor.cue
            }
            _ => true,
        };
        if restart {
            let active = transitions.play(
                &mut player,
                node,
                Duration::from_secs_f32(CROSSFADE_SECONDS),
            );
            match actor.playback {
                Playback::Loop => {
                    active.repeat();
                }
                Playback::Once => {
                    active.set_repeat(RepeatAnimation::Never);
                }
                Playback::Scrub(_) => {
                    active.set_speed(0.0);
                }
            }
        } else if let Some((_, playback)) = state.playing
            && kind(playback) != kind(actor.playback)
            && let Some(active) = player.animation_mut(node)
        {
            match actor.playback {
                Playback::Loop => {
                    active.set_speed(1.0).repeat();
                }
                Playback::Once => {
                    active.set_speed(1.0).set_repeat(RepeatAnimation::Never);
                }
                Playback::Scrub(_) => {
                    active.set_speed(0.0);
                }
            }
        }
        if let Playback::Scrub(fraction) = actor.playback
            && let Some(active) = player.animation_mut(node)
        {
            active.set_seek_time((fraction.clamp(0.0, 1.0) * duration).min(duration));
        }
        state.playing = Some((actor.clip, actor.playback));
        state.cue = actor.cue;
    }
    for &(_, entity) in &cast.0 {
        if !on_stage.contains(&entity)
            && let Ok((_, _, mut visibility)) = actors.get_mut(entity)
        {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

/// Dresses the actors whose scene has just spawned: toon materials in place of the glTF's,
/// an outline twin of every mesh, and the animation graph of the model's clips on its
/// [`AnimationPlayer`].
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn dress(
    mut commands: Commands,
    library: Res<Library>,
    children: Query<&Children>,
    meshes: Query<(
        Entity,
        &Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
        Option<&SkinnedMesh>,
    )>,
    players: Query<Entity, With<AnimationPlayer>>,
    standard: Res<Assets<StandardMaterial>>,
    mut toons: ResMut<Assets<ToonMaterial>>,
    mut outlines: ResMut<Assets<OutlineMaterial>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    clips: Res<Assets<AnimationClip>>,
    mut actors: Query<(Entity, &mut Actor)>,
) {
    for (root, mut actor) in &mut actors {
        if !actor.ready || actor.dressed {
            continue;
        }
        let Some(Model::Ready {
            clips: library_clips,
            ..
        }) = library.model(actor.character)
        else {
            continue;
        };
        let outline = outlines.add(OutlineMaterial::default());
        let mut materials = Vec::new();
        let mut player = None;
        for entity in children.iter_descendants(actor.fit) {
            if players.contains(entity) {
                player = Some(entity);
            }
            let Ok((entity, mesh, material, skin)) = meshes.get(entity) else {
                continue;
            };
            let (color, texture) =
                standard
                    .get(&material.0)
                    .map_or((LinearRgba::WHITE, None), |material| {
                        (
                            material.base_color.to_linear(),
                            material.base_color_texture.clone(),
                        )
                    });
            let toon = toons.add(ToonMaterial::new(color, texture.clone()));
            materials.push((toon.clone(), texture));
            // Never culled: the bounds of a skinned mesh are its bind pose's, which a kick
            // or a sweep reaches out of (and two fighters are cheap to draw).
            commands
                .entity(entity)
                .remove::<MeshMaterial3d<StandardMaterial>>()
                .insert((MeshMaterial3d(toon), NoFrustumCulling));
            let mut twin = commands.spawn((
                Mesh3d(mesh.0.clone()),
                MeshMaterial3d(outline.clone()),
                NoFrustumCulling,
                Transform::IDENTITY,
                Visibility::Inherited,
            ));
            if let Some(skin) = skin {
                twin.insert(skin.clone());
            }
            let twin = twin.id();
            commands.entity(entity).add_child(twin);
        }
        actor.materials = materials;
        actor.worn = None;
        match player {
            Some(player) => {
                let mut graph = AnimationGraph::new();
                let mut nodes = [None; 13];
                for clip in Clip::ALL {
                    if let Some(handle) = &library_clips[clip.index()] {
                        let duration = clips.get(handle).map_or(0.0, AnimationClip::duration);
                        let node = graph.add_clip(handle.clone(), 1.0, graph.root);
                        nodes[clip.index()] = Some((node, duration));
                    }
                }
                commands.entity(player).insert((
                    AnimationGraphHandle(graphs.add(graph)),
                    AnimationTransitions::new(),
                ));
                actor.animation = Some(Animation { player, nodes });
            }
            None => error!(
                "3D model {} has no animation root: {} stands still",
                model_path(actor.character),
                actor.character.name()
            ),
        }
        actor.dressed = true;
        debug!(
            "3D actor of P{} dressed ({root:?})",
            u32::from(actor.player.0) + 1
        );
    }
}

/// Measures the dressed actors and scales them to [`MODEL_HEIGHT`], their feet on the root:
/// the bounds of every mesh in the root's frame, once the meshes are loaded. (Measured from
/// the mesh assets: the meshes are never culled, so Bevy computes no `Aabb` for them.)
fn fit(
    children: Query<&Children>,
    meshes: Query<(&Mesh3d, &GlobalTransform)>,
    assets: Res<Assets<Mesh>>,
    mut actors: Query<(&GlobalTransform, &mut Actor)>,
    mut transforms: Query<&mut Transform>,
) {
    for (root_transform, mut actor) in &mut actors {
        if !actor.dressed || actor.fitted {
            continue;
        }
        let to_root = root_transform.affine().inverse();
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        let mut measured = true;
        for entity in children.iter_descendants(actor.fit) {
            let Ok((mesh, transform)) = meshes.get(entity) else {
                continue;
            };
            let Some(aabb) = assets.get(&mesh.0).and_then(MeshAabb::compute_aabb) else {
                measured = false;
                break;
            };
            let to_root = to_root * transform.affine();
            let centre = Vec3::from(aabb.center);
            let half = Vec3::from(aabb.half_extents);
            for corner in 0..8 {
                let sign = Vec3::new(
                    if corner & 1 == 0 { -1.0 } else { 1.0 },
                    if corner & 2 == 0 { -1.0 } else { 1.0 },
                    if corner & 4 == 0 { -1.0 } else { 1.0 },
                );
                let point = to_root.transform_point3(centre + sign * half);
                min = min.min(point);
                max = max.max(point);
            }
        }
        if !measured || min.x > max.x {
            continue;
        }
        let height = (max.y - min.y).max(f32::EPSILON);
        let scale = MODEL_HEIGHT / height;
        if let Ok(mut transform) = transforms.get_mut(actor.fit) {
            let centre = (min + max) / 2.0;
            transform.scale = Vec3::splat(scale);
            transform.translation = Vec3::new(-centre.x, -min.y, -centre.z) * scale;
        }
        actor.fitted = true;
        info!(
            "3D model of {} fitted: {height:.3} units tall, scaled by {scale:.2}",
            actor.character.name()
        );
    }
}

/// Puts the skin texture of the variant on the actors' materials once it is loaded (the
/// model's own texture when there is none, or it fails), and flashes the stunned ones.
fn wear(
    asset_server: Res<AssetServer>,
    mut toons: ResMut<Assets<ToonMaterial>>,
    mut actors: Query<&mut Actor>,
) {
    for mut actor in &mut actors {
        if !actor.dressed {
            continue;
        }
        let wanted = actor.skin.as_ref().map(|(variant, _)| *variant);
        if actor.worn != Some(wanted) {
            let texture = match &actor.skin {
                None => Some(None),
                Some((variant, handle)) => match asset_server.load_state(handle.id()) {
                    LoadState::Loaded => Some(Some(handle.clone())),
                    LoadState::Failed(error) => {
                        warn!(
                            "no skin texture {} for the 3D model of {}: {error}; it wears its own",
                            skin_path(actor.character, *variant),
                            actor.character.name()
                        );
                        Some(None)
                    }
                    LoadState::NotLoaded | LoadState::Loading => None,
                },
            };
            if let Some(skin) = texture {
                for (handle, own) in &actor.materials {
                    if let Some(material) = toons.get_mut(handle) {
                        material.set_texture(skin.clone().or_else(|| own.clone()));
                    }
                }
                actor.worn = Some(wanted);
            }
        }
        let tint = Vec4::new(1.0, 1.0, 1.0, if actor.flash { FLASH } else { 0.0 });
        for (handle, _) in &actor.materials {
            if let Some(material) = toons.get_mut(handle)
                && material.settings.tint != tint
            {
                material.settings.tint = tint;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// What the tests look at
// ---------------------------------------------------------------------------------------------

/// What one actor of the stage has, for the tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorReport {
    /// The player it fights for.
    pub player: PlayerId,
    /// The character, hence the model.
    pub character: Character,
    /// Whether its model's root carries an `AnimationPlayer` with the clips of the model.
    pub animated: bool,
    /// How many of its meshes are cel-shaded.
    pub toon_materials: usize,
    /// Whether it was measured and scaled to the fighters' height.
    pub fitted: bool,
    /// The clip playing, if any.
    pub clip: Option<Clip>,
}

/// What the stage has, for the tests: which characters have a model and what each actor got.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StageReport {
    /// The characters whose model loaded; `None` while the models are still loading.
    pub models: Option<Vec<Character>>,
    /// The actors spawned, in cast order.
    pub actors: Vec<ActorReport>,
}

/// Reads the stage of `world` ([`StageReport`]).
pub(super) fn report(world: &mut World) -> StageReport {
    let library = world.resource::<Library>();
    let models = library.reported.then(|| {
        library
            .models
            .iter()
            .filter(|(_, model)| matches!(model, Model::Ready { .. }))
            .map(|(character, _)| *character)
            .collect()
    });
    let mut actors = Vec::new();
    let mut query = world.query::<&Actor>();
    for actor in query.iter(world) {
        actors.push(ActorReport {
            player: actor.player,
            character: actor.character,
            animated: actor.animation.is_some(),
            toon_materials: actor.materials.len(),
            fitted: actor.fitted,
            clip: actor.playing.map(|(clip, _)| clip),
        });
    }
    actors.sort_by_key(|actor| actor.player.0);
    StageReport { models, actors }
}

/// The offscreen image of the stage, for a test to read back.
pub(super) fn image(world: &World) -> Option<Handle<Image>> {
    world
        .get_resource::<StageImage>()
        .map(|image| image.0.clone())
}
