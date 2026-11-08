//! The 3D fighters of the third-person view, as the engine-agnostic front end sees them: which
//! animation clip a fighter plays and where in it, where the models stand and which texture
//! they wear ([`Stage`]), the tuning of the cel shading, and the layout of `assets/models/`.
//!
//! The models themselves — rigged glTF binaries made by the image-to-3D pipeline of
//! `docs/3d-asset-pipeline.md` — are loaded and drawn by the engine (the Bevy backend with the
//! `mode3d` feature); the front end only describes the stage, like it describes a
//! [`Frame`](super::Frame). Nothing here feeds back into the rules: the clip follows the
//! fighter's state and the frame data of its strike, never the other way round, so replays
//! and online matches stay exactly as deterministic without the models as with them.
//!
//! Every fighter whose character has a model ([`Models`]) is an [`Actor`] of the stage and is
//! left out of the software mannequins of [`tps`](super::tps); the others keep their
//! mannequins, so a missing or broken model never costs more than its looks.

use super::arena::{ARENA_LEFT, FLOOR_Y, feet, rules_of};
use super::skin::SPRITE_NAMES;
use super::space::{Camera, Fx, Vec3};
use super::tps::depth;
use crate::game::{
    Character, Facing, Fighter, FighterState, GrapplePosition, GrappleRole, Limb, MatchConfig,
    PlayerId, Variant,
};
use crate::net::Client;

// ---------------------------------------------------------------------------------------------
// Tuning
// ---------------------------------------------------------------------------------------------

/// The height of a standing model in world units (the pixels of the side view): the model is
/// scaled so that its rest pose is this tall, whatever units it was exported in.
pub const MODEL_HEIGHT: f32 = 168.0;
/// How long a change of clip blends from the old one to the new one, in seconds.
pub const CROSSFADE_SECONDS: f32 = 0.1;
/// Where a strike's clip is sampled over its start-up: from the start of the clip to this
/// fraction of it (the wind-up).
pub const STRIKE_WINDUP: f32 = 0.4;
/// Where a strike's clip is sampled over its active ticks: from [`STRIKE_WINDUP`] to this
/// fraction of the clip (the contact). The recovery takes the rest.
pub const STRIKE_CONTACT: f32 = 0.6;
/// The pose of a hold (a clinch): the middle of the contact window of a strike.
pub const HOLD_POSE: f32 = (STRIKE_WINDUP + STRIKE_CONTACT) / 2.0;
/// Hits of a combo from which a stunned fighter plays the heavy reaction instead of the light
/// one (an airborne hit always plays it).
pub const HEAVY_HIT_COMBO: u32 = 2;
/// Where the cel shading steps, on the half-Lambert scale (the cosine between the normal and
/// the light brought into `[0, 1]`, so the terminator sits at `0.5` like in the anime games):
/// below the first edge the deep shadow, below the second the shadow, above it the light.
pub const SHADOW_EDGES: [f32; 2] = [0.2, 0.5];
/// What the albedo is multiplied by in the deep shadow and in the shadow, in linear RGB: cool
/// and a touch purple, like the shadow ramps of the anime games, never a grey. The light keeps
/// the texture exactly as painted.
pub const SHADOW_TINTS: [[f32; 3]; 2] = [[0.33, 0.27, 0.46], [0.58, 0.50, 0.70]];
/// The half-width over which every edge of the shading is feathered (half-Lambert units for
/// the shadows, cosines for the rim and the highlight): crisp, not aliased. Must be above
/// zero: the shader's `smoothstep` divides by it.
pub const FEATHER: f32 = 0.02;
/// The stepped highlight of the lit side: the Blinn-Phong exponent, the value of
/// `n_dot_h ^ exponent` above which it shows, and its strength (`0.0` switches it off).
pub const SPECULAR: (f32, f32, f32) = (32.0, 0.5, 0.35);
/// The rim light of the lit side: the cosine between the normal and the eye under which it
/// shows, and its strength (`0.0` switches it off).
pub const RIM: (f32, f32) = (0.3, 0.4);
/// The thickness of the ink outline around a model, in pixels of the virtual screen, whatever
/// the distance to the camera.
pub const OUTLINE_WIDTH: f32 = 1.8;
/// What the albedo is multiplied by in the ink outline, in linear RGB: a dark, slightly
/// purple version of the colour under it (brown-red on skin, deep blue on blue cloth), not
/// a black line.
pub const OUTLINE_SHADE: [f32; 3] = [0.15, 0.10, 0.16];
/// The direction the light comes from, in world units: left, above, from the street (the
/// light of the mannequins and the alley planes of [`tps`](super::tps)).
pub const LIGHT: [f32; 3] = [-2.0, 3.0, -1.0];

/// `x` stepped at `edge`, feathered over [`FEATHER`] on each side: `0.0` below the feather,
/// `1.0` above it, the smooth S of the shader's `smoothstep` in between.
///
/// ```
/// use srr::front::model::step;
///
/// assert_eq!(step(0.5, 0.4), 0.0, "below the feather");
/// assert!((step(0.5, 0.5) - 0.5).abs() < 1e-5, "on the edge: half way");
/// assert_eq!(step(0.5, 0.6), 1.0, "above the feather");
/// ```
pub fn step(edge: f32, x: f32) -> f32 {
    let t = ((x - (edge - FEATHER)) / (2.0 * FEATHER)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How lit a surface whose normal makes the cosine `n_dot_l` with the light is: `0.0` in the
/// shadow, `1.0` in the light, stepping at [`SHADOW_EDGES`]`[1]` of the half-Lambert.
pub fn lit(n_dot_l: f32) -> f32 {
    step(SHADOW_EDGES[1], n_dot_l * 0.5 + 0.5)
}

/// What the albedo is multiplied by at `n_dot_l`: the stepped diffuse of the cel shading,
/// [`SHADOW_TINTS`]`[0]` facing away from the light, `[1]` in the shadow, white in the light.
/// The engine's shader steps exactly like this.
///
/// ```
/// use srr::front::model::{SHADOW_TINTS, shade};
///
/// assert_eq!(shade(-1.0), SHADOW_TINTS[0], "facing away: the deep shadow");
/// assert_eq!(shade(-0.3), SHADOW_TINTS[1], "past the terminator: the shadow");
/// assert_eq!(shade(1.0), [1.0, 1.0, 1.0], "facing the light: the texture as painted");
/// ```
pub fn shade(n_dot_l: f32) -> [f32; 3] {
    let half = n_dot_l * 0.5 + 0.5;
    let shadow = step(SHADOW_EDGES[0], half);
    let lit = step(SHADOW_EDGES[1], half);
    let mut tint = [0.0; 3];
    for (channel, (deep, light)) in tint
        .iter_mut()
        .zip(SHADOW_TINTS[0].iter().zip(SHADOW_TINTS[1]))
    {
        let shaded = deep * (1.0 - shadow) + light * shadow;
        *channel = shaded * (1.0 - lit) + lit;
    }
    tint
}

/// The stepped highlight on a surface whose normal makes the cosine `n_dot_h` with the half
/// vector between the light and the eye, and `n_dot_l` with the light: [`SPECULAR`]`.2` where
/// `n_dot_h ^ `[`SPECULAR`]`.0` passes [`SPECULAR`]`.1`, on the lit side only.
pub fn specular(n_dot_h: f32, n_dot_l: f32) -> f32 {
    let (exponent, edge, strength) = SPECULAR;
    step(edge, n_dot_h.max(0.0).powf(exponent)) * lit(n_dot_l) * strength
}

/// The rim light on a surface whose normal makes the cosine `n_dot_v` with the eye and
/// `n_dot_l` with the light: [`RIM`]`.1` on the silhouette (under [`RIM`]`.0`), on the lit side
/// only, nothing elsewhere.
pub fn rim(n_dot_v: f32, n_dot_l: f32) -> f32 {
    (1.0 - step(RIM.0, n_dot_v)) * lit(n_dot_l) * RIM.1
}

/// How far the ink hull is pushed out along the normal, in world units, for a vertex at the
/// clip-space depth `clip_w` seen through a projection scaling heights by `projection_scale`
/// (`clip_from_view[1][1]`) onto a viewport `viewport_height` pixels tall: the world length
/// that [`OUTLINE_WIDTH`] pixels cover at that depth, so the line stays as thick on screen
/// near and far.
pub fn outline_offset(clip_w: f32, projection_scale: f32, viewport_height: f32) -> f32 {
    OUTLINE_WIDTH * 2.0 * clip_w / (projection_scale * viewport_height)
}

// ---------------------------------------------------------------------------------------------
// Clips
// ---------------------------------------------------------------------------------------------

/// The named animation clips every model carries (see `docs/3d-asset-pipeline.md`): a model
/// missing one plays its idle instead, and the engine logs the gap when it loads the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Clip {
    /// Standing in guard.
    Idle,
    /// Walking forward.
    WalkF,
    /// Stepping back.
    WalkB,
    /// Crouching.
    Crouch,
    /// Jumping, in place.
    Jump,
    /// The guard up, standing its ground.
    Block,
    /// Flinching from a hit.
    HitLight,
    /// Thrown back by a hit.
    HitHeavy,
    /// Going down, and lying.
    Ko,
    /// A punch, scrubbed through the frame data.
    Punch,
    /// A kick, scrubbed through the frame data.
    Kick,
    /// A sweep: the takedowns.
    Sweep,
    /// The character's special move.
    Special,
}

impl Clip {
    /// Every clip, in the order of their [`index`](Clip::index).
    pub const ALL: [Clip; 13] = [
        Clip::Idle,
        Clip::WalkF,
        Clip::WalkB,
        Clip::Crouch,
        Clip::Jump,
        Clip::Block,
        Clip::HitLight,
        Clip::HitHeavy,
        Clip::Ko,
        Clip::Punch,
        Clip::Kick,
        Clip::Sweep,
        Clip::Special,
    ];

    /// The name of the clip inside the glTF file.
    ///
    /// ```
    /// use srr::front::model::Clip;
    ///
    /// assert_eq!(Clip::WalkF.name(), "walk_f");
    /// assert_eq!(Clip::by_name("hit_heavy"), Some(Clip::HitHeavy));
    /// assert_eq!(Clip::by_name("moonwalk"), None);
    /// ```
    pub const fn name(self) -> &'static str {
        match self {
            Clip::Idle => "idle",
            Clip::WalkF => "walk_f",
            Clip::WalkB => "walk_b",
            Clip::Crouch => "crouch",
            Clip::Jump => "jump",
            Clip::Block => "block",
            Clip::HitLight => "hit_light",
            Clip::HitHeavy => "hit_heavy",
            Clip::Ko => "ko",
            Clip::Punch => "punch",
            Clip::Kick => "kick",
            Clip::Sweep => "sweep",
            Clip::Special => "special",
        }
    }

    /// The clip named `name`, if any.
    pub fn by_name(name: &str) -> Option<Clip> {
        Clip::ALL.iter().copied().find(|clip| clip.name() == name)
    }

    /// Position in [`Clip::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// How a clip runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Playback {
    /// Repeating on the engine's clock: the stances and the walks.
    Loop,
    /// Once on the engine's clock, holding its last frame: the jump, the reactions, the KO.
    Once,
    /// Scrubbed by the rules: sampled at this fraction of the clip (`0.0..=1.0`), so the swing
    /// and the contact of a strike land on its start-up and active ticks exactly.
    Scrub(f32),
}

/// The fraction of a strike's clip to show on step `frame` (1-based, the step just completed)
/// of a timeline of `startup`, `active` and `recovery` ticks: the start-up runs the wind-up
/// (up to [`STRIKE_WINDUP`]), the active ticks the contact (up to [`STRIKE_CONTACT`]), the
/// recovery the rest.
///
/// ```
/// use srr::front::model::{STRIKE_CONTACT, STRIKE_WINDUP, strike_fraction};
///
/// assert!(strike_fraction(1, 4, 2, 6) < STRIKE_WINDUP, "winding up");
/// assert_eq!(strike_fraction(4, 4, 2, 6), STRIKE_WINDUP, "the last start-up tick");
/// assert_eq!(strike_fraction(6, 4, 2, 6), STRIKE_CONTACT, "the last active tick");
/// assert_eq!(strike_fraction(12, 4, 2, 6), 1.0, "the last recovery tick");
/// assert_eq!(strike_fraction(99, 4, 2, 6), 1.0, "never past the end");
/// ```
pub fn strike_fraction(frame: u32, startup: u32, active: u32, recovery: u32) -> f32 {
    let share = |done: u32, of: u32| (done as f32 / of.max(1) as f32).clamp(0.0, 1.0);
    if frame <= startup {
        STRIKE_WINDUP * share(frame, startup)
    } else if frame <= startup.saturating_add(active) {
        STRIKE_WINDUP + (STRIKE_CONTACT - STRIKE_WINDUP) * share(frame - startup, active)
    } else {
        let done = frame - startup.saturating_add(active);
        STRIKE_CONTACT + (1.0 - STRIKE_CONTACT) * share(done, recovery)
    }
}

/// The clip `fighter` plays. `moved` tells whether it changed place since the last tick: a
/// walk or a step back that the arena's bound stops shows the stance instead of a treadmill.
///
/// The mapping mirrors the poses of the sprites ([`pose_of`](super::skin::pose_of)): a strike
/// plays its limb's clip, the takedowns and their projections the sweep, the bottom of a
/// grapple lies like a knockout, the top of a ground position crouches over it, and a clinch
/// holds the contact pose of a punch (see [`playback_of`]).
pub fn clip_of(fighter: &Fighter, moved: bool) -> Clip {
    match fighter.state() {
        FighterState::Idle => Clip::Idle,
        FighterState::Walking if moved => Clip::WalkF,
        FighterState::Walking => Clip::Idle,
        FighterState::Blocking if moved => Clip::WalkB,
        FighterState::Blocking => Clip::Block,
        FighterState::Crouching => Clip::Crouch,
        FighterState::Airborne => Clip::Jump,
        FighterState::HitStun => {
            if fighter.is_airborne() || fighter.combo_hits() >= HEAVY_HIT_COMBO {
                Clip::HitHeavy
            } else {
                Clip::HitLight
            }
        }
        FighterState::Knockout => Clip::Ko,
        FighterState::Attacking(_) => match fighter.attack() {
            Some(attack) if attack.limb == Limb::Kick => Clip::Kick,
            _ => Clip::Punch,
        },
        FighterState::Special(_) => Clip::Special,
        // A grab reaches out like a punch; both techniques are takedowns.
        FighterState::Grabbing(_) => Clip::Punch,
        FighterState::Technique(_, _) => Clip::Sweep,
        FighterState::Grappling(GrappleRole::Bottom, GrapplePosition::Clinch) => Clip::HitLight,
        FighterState::Grappling(GrappleRole::Bottom, _) => Clip::Ko,
        FighterState::Grappling(GrappleRole::Top, GrapplePosition::Clinch) => Clip::Punch,
        FighterState::Grappling(
            GrappleRole::Top,
            GrapplePosition::Sweep | GrapplePosition::Throw,
        ) => Clip::Sweep,
        FighterState::Grappling(GrappleRole::Top, _) => Clip::Crouch,
    }
}

/// How `fighter`'s clip runs under `rules` (its character's, see
/// [`rules_of`](super::arena::rules_of)): the strikes, the special move, the grab and the
/// techniques are scrubbed through their frame data ([`strike_fraction`]), a clinch holds
/// [`HOLD_POSE`], the stances loop, and the jump, the reactions, the KO and the projections play
/// once.
pub fn playback_of(fighter: &Fighter, rules: &MatchConfig) -> Playback {
    let frame = fighter.move_frame().unwrap_or(1);
    let scrub = |startup: u32, active: u32, recovery: u32| {
        Playback::Scrub(strike_fraction(frame, startup, active, recovery))
    };
    match fighter.state() {
        // A strike runs on its attack's frame data, the special move (no attack) on its own.
        FighterState::Attacking(_) | FighterState::Special(_) => {
            let data = fighter
                .attack()
                .map_or(rules.special.attack, |attack| *rules.attack(attack));
            scrub(data.startup, data.active, data.recovery)
        }
        FighterState::Grabbing(_) => {
            let data = rules.grapple;
            scrub(data.startup, data.active, data.recovery)
        }
        FighterState::Technique(technique, _) => {
            let data = rules.technique(technique);
            scrub(data.startup, data.active, data.recovery)
        }
        FighterState::Grappling(_, GrapplePosition::Clinch) => Playback::Scrub(HOLD_POSE),
        FighterState::Idle
        | FighterState::Walking
        | FighterState::Blocking
        | FighterState::Crouching
        | FighterState::Grappling(GrappleRole::Top, _) => Playback::Loop,
        FighterState::Airborne
        | FighterState::HitStun
        | FighterState::Knockout
        | FighterState::Grappling(GrappleRole::Bottom, _) => Playback::Once,
    }
}

// ---------------------------------------------------------------------------------------------
// The stage
// ---------------------------------------------------------------------------------------------

/// The characters the engine has a model for: the fighters playing them are drawn by the
/// engine, the others stay software mannequins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Models(u8);

impl Models {
    /// No model at all: everything is drawn by the software renderer.
    pub const NONE: Models = Models(0);

    /// This set plus `character`.
    ///
    /// ```
    /// use srr::front::model::Models;
    /// use srr::game::Character;
    ///
    /// let models = Models::NONE.with(Character::Zip);
    /// assert!(models.contains(Character::Zip));
    /// assert!(!models.contains(Character::Chalk));
    /// assert_eq!(models.characters(), vec![Character::Zip]);
    /// assert!(Models::NONE.is_empty() && !models.is_empty());
    /// ```
    pub const fn with(self, character: Character) -> Models {
        Models(self.0 | (1 << character.index()))
    }

    /// Whether `character` has a model.
    pub const fn contains(self, character: Character) -> bool {
        self.0 & (1 << character.index()) != 0
    }

    /// Whether no character has a model.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The characters with a model, in [`Character::ALL`] order.
    pub fn characters(self) -> Vec<Character> {
        Character::ALL
            .into_iter()
            .filter(|&character| self.contains(character))
            .collect()
    }
}

/// One fighter drawn as a model.
#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    /// The player it fights for.
    pub player: PlayerId,
    /// The character, hence the model.
    pub character: Character,
    /// The manga variant it wears, the skin texture of the model (`None`: the model's own
    /// texture, for the chalk skin).
    pub variant: Option<Variant>,
    /// Where its feet stand, in the world units of [`tps`](super::tps): `x` along the arena
    /// from the left bound, `y` above the pavement, `z` towards the wall.
    pub position: [f32; 3],
    /// The way it faces: the model looks along `+x` or `-x`.
    pub facing: Facing,
    /// The clip it plays.
    pub clip: Clip,
    /// How the clip runs.
    pub playback: Playback,
    /// A cue to restart a clip that plays once: the hits of the combo the fighter is caught
    /// in, so every hit of a combo flinches again.
    pub cue: u32,
    /// Whether it flashes white (the stun of a hit).
    pub flash: bool,
}

/// The third-person view as the engine renders it: the camera and the actors, drawn over the
/// first `layer` rectangles of the frame (the alley and the mannequins) and under the rest
/// (the HUD, the touch controls).
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    /// The eye of the camera, in world units.
    pub eye: [f32; 3],
    /// A point the camera looks at, in world units.
    pub target: [f32; 3],
    /// How many rectangles of the frame are under the stage.
    pub layer: usize,
    /// The fighters whose character has a model.
    pub actors: Vec<Actor>,
}

fn to_array(v: Vec3) -> [f32; 3] {
    [v.x.to_f32(), v.y.to_f32(), v.z.to_f32()]
}

/// The stage of `client`'s fight seen by `camera`, over `layer` rectangles: one actor per
/// fighter whose character is in `models`. `previous` holds the `x` of every fighter on the
/// previous tick, to tell a walk from a stance against the bound ([`clip_of`]).
pub fn stage(
    client: &Client,
    camera: &Camera,
    models: Models,
    previous: &[(PlayerId, i32)],
    layer: usize,
) -> Stage {
    let forward = camera.forward() * Fx::int(100);
    let mut stage = Stage {
        eye: to_array(camera.eye),
        target: to_array(camera.eye + forward),
        layer,
        actors: Vec::new(),
    };
    let Some(session) = client.session() else {
        return stage;
    };
    let current = session.current();
    let config = current.config();
    for fighter in current.fighters() {
        let character = config.character(fighter.player());
        if !models.contains(character) {
            continue;
        }
        let (x, y) = feet(fighter, config);
        let moved = previous
            .iter()
            .find(|(player, _)| *player == fighter.player())
            .is_some_and(|&(_, x)| x != fighter.x());
        stage.actors.push(Actor {
            player: fighter.player(),
            character,
            variant: client.skin(fighter.player()).variant,
            position: [x - ARENA_LEFT, FLOOR_Y - y, depth(fighter, config) as f32],
            facing: fighter.facing(),
            clip: clip_of(fighter, moved),
            playback: playback_of(fighter, rules_of(current, fighter)),
            cue: fighter.combo_hits(),
            flash: fighter.state() == FighterState::HitStun,
        });
    }
    stage
}

// ---------------------------------------------------------------------------------------------
// The layout of assets/models/
// ---------------------------------------------------------------------------------------------

/// The directory of `character`'s model under the asset root: its name, in lowercase.
pub fn model_dir(character: Character) -> String {
    format!("models/{}", character.name().to_lowercase())
}

/// The path of `character`'s model under the asset root.
///
/// ```
/// use srr::front::model::{model_path, skin_path};
/// use srr::game::{Character, Variant};
///
/// assert_eq!(model_path(Character::Torch), "models/torch/torch.glb");
/// assert_eq!(skin_path(Character::Torch, Variant::Raven), "models/torch/skins/raven.png");
/// ```
pub fn model_path(character: Character) -> String {
    format!(
        "{}/{}.glb",
        model_dir(character),
        character.name().to_lowercase()
    )
}

/// The path of the skin texture `variant` puts on `character`'s model under the asset root:
/// named like the sprite sheet of `assets/skins/manga/`.
pub fn skin_path(character: Character, variant: Variant) -> String {
    format!(
        "{}/skins/{}.png",
        model_dir(character),
        SPRITE_NAMES[variant.index() as usize]
    )
}

/// The weight of a line of an [`Audit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Something in order, or a fallback that works (a mannequin, the model's own texture).
    Info,
    /// Something the engine cannot use as intended.
    Problem,
}

/// What [`audit`] found in `assets/models/`: one line per model, clip and skin.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Audit {
    /// Every finding, in the order of [`Character::ALL`].
    pub lines: Vec<(Severity, String)>,
}

impl Audit {
    /// Whether nothing is wrong.
    pub fn passed(&self) -> bool {
        self.lines
            .iter()
            .all(|(severity, _)| *severity == Severity::Info)
    }

    fn info(&mut self, line: String) {
        self.lines.push((Severity::Info, line));
    }

    fn problem(&mut self, line: String) {
        self.lines.push((Severity::Problem, line));
    }
}

/// Checks the layout of the models: `files` are the paths under the asset root (`models/...`)
/// and `read` fetches the bytes of one of them. Every character may have its model
/// ([`model_path`]), a glTF binary with every [`Clip`], and the skin textures of
/// [`Variant::ALL`] ([`skin_path`]); a missing model or skin is only noted (the mannequin, or
/// the model's own texture, stands in), a broken model or a clip or a stray file in `skins/`
/// is a problem.
pub fn audit(files: &[&str], read: impl Fn(&str) -> Option<Vec<u8>>) -> Audit {
    let mut audit = Audit::default();
    for character in Character::ALL {
        let path = model_path(character);
        if !files.contains(&path.as_str()) {
            audit.info(format!(
                "{}: no model ({path}): the 3D view draws its mannequin",
                character.name()
            ));
            continue;
        }
        let bytes = read(&path).unwrap_or_default();
        match clips_of(&bytes) {
            Err(error) => {
                audit.problem(format!("{path}: {error}"));
                continue;
            }
            Ok(found) => {
                audit.info(format!(
                    "{path}: {} clips ({})",
                    found.len(),
                    generator_of(&bytes).unwrap_or_else(|| "unknown generator".into())
                ));
                for clip in Clip::ALL {
                    if !found.iter().any(|name| name == clip.name()) {
                        audit.problem(format!(
                            "{path}: no `{}` clip: the fighter plays its idle instead",
                            clip.name()
                        ));
                    }
                }
            }
        }
        for variant in Variant::ALL {
            let skin = skin_path(character, variant);
            if files.contains(&skin.as_str()) {
                audit.info(format!("{skin}: present"));
            } else {
                audit.info(format!(
                    "{skin}: missing: the {} wears the model's own texture",
                    SPRITE_NAMES[variant.index() as usize]
                ));
            }
        }
        let skins_dir = format!("{}/skins/", model_dir(character));
        for file in files.iter().filter(|file| file.starts_with(&skins_dir)) {
            let known = Variant::ALL
                .iter()
                .any(|&variant| skin_path(character, variant) == *file);
            if !known {
                audit.problem(format!(
                    "{file}: not a skin of assets/skins/manga ({})",
                    SPRITE_NAMES.join(", ")
                ));
            }
        }
    }
    audit
}

/// The JSON chunk of the glTF binary `bytes` (the first chunk of a `.glb`).
///
/// # Errors
///
/// Explains what is wrong with the header or the chunk.
pub fn glb_json(bytes: &[u8]) -> Result<&str, String> {
    let word = |at: usize| -> Result<u32, String> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| format!("truncated at byte {at}"))
    };
    if word(0)? != 0x4654_6C67 {
        return Err("not a glTF binary (no `glTF` magic)".into());
    }
    let version = word(4)?;
    if version != 2 {
        return Err(format!("glTF version {version}, expected 2"));
    }
    let length = word(8)? as usize;
    if length != bytes.len() {
        return Err(format!("declares {length} bytes but has {}", bytes.len()));
    }
    let chunk_length = word(12)? as usize;
    if word(16)? != 0x4E4F_534A {
        return Err("the first chunk is not JSON".into());
    }
    let json = bytes
        .get(20..20 + chunk_length)
        .ok_or_else(|| "the JSON chunk is truncated".to_string())?;
    std::str::from_utf8(json).map_err(|error| format!("the JSON chunk is not UTF-8: {error}"))
}

/// The names of the animations of the glTF binary `bytes`, in order: the `"name"` of every
/// object of its top-level `"animations"` array.
///
/// # Errors
///
/// The reasons of [`glb_json`].
pub fn clips_of(bytes: &[u8]) -> Result<Vec<String>, String> {
    Ok(animation_names(glb_json(bytes)?))
}

/// What generated the glTF binary `bytes` (`asset.generator`), if it says.
pub fn generator_of(bytes: &[u8]) -> Option<String> {
    let json = glb_json(bytes).ok()?;
    let key = json.find("\"generator\"")?;
    let colon = key + json[key..].find(':')?;
    string_at(json, next_token(json.as_bytes(), colon + 1)).map(|(value, _)| value)
}

/// The index of the first byte of `bytes` from `at` that is not a space.
fn next_token(bytes: &[u8], at: usize) -> usize {
    (at..bytes.len())
        .find(|&i| !bytes[i].is_ascii_whitespace())
        .unwrap_or(bytes.len())
}

/// The names of the glTF animations in `json`: enough of a JSON scanner to find the
/// `"name"` of every object directly inside the top-level `"animations"` array. Strings are
/// skipped as wholes (brackets and quotes inside them do not count), nothing else is parsed.
fn animation_names(json: &str) -> Vec<String> {
    let bytes = json.as_bytes();
    let mut names = Vec::new();
    let mut depth = 0usize;
    // The depth of the animations array once its key was read, then once it was opened.
    let mut expecting = false;
    let mut animations: Option<usize> = None;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let Some((value, end)) = string_at(json, i) else {
                    break;
                };
                i = end;
                let colon = next_token(bytes, end);
                let is_key = bytes.get(colon) == Some(&b':');
                if is_key && depth == 1 && value == "animations" {
                    expecting = true;
                } else if is_key
                    && animations.is_some_and(|d| depth == d + 1)
                    && value == "name"
                    && let Some((name, end)) = string_at(json, next_token(bytes, colon + 1))
                {
                    names.push(name);
                    i = end;
                }
                continue;
            }
            b'{' | b'[' => {
                depth += 1;
                if expecting {
                    animations = Some(depth);
                    expecting = false;
                }
            }
            b'}' | b']' => {
                if animations == Some(depth) {
                    break;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        i += 1;
    }
    names
}

/// The JSON string of `text` whose opening quote is at byte `at`, unescaped, and the index
/// after its closing quote. `None` when no string starts there or it never closes.
fn string_at(text: &str, at: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'"') {
        return None;
    }
    let mut end = at + 1;
    while end < bytes.len() && bytes[end] != b'"' {
        end += if bytes[end] == b'\\' { 2 } else { 1 };
    }
    if end >= bytes.len() {
        return None;
    }
    let mut value = String::new();
    let mut chars = text[at + 1..end].chars();
    while let Some(c) = chars.next() {
        value.push(match c {
            '\\' => match chars.next().unwrap_or('\\') {
                'n' => '\n',
                't' => '\t',
                other => other,
            },
            other => other,
        });
    }
    Some((value, end + 1))
}
