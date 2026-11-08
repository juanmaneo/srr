//! The puppet: a wooden Pinocchio, tubes and balls, generated in code as a rigged glTF binary.
//! It is the 3D model of the chalk (`assets/models/chalk/chalk.glb`), the test fixture of the
//! 3D view (CI loads it instead of a model from the image-to-3D pipeline, which it never has
//! to download) and the placeholder a character wears until its own model is made
//! (`docs/3d-asset-pipeline.md`); `cargo xtask models` audits the models and
//! `cargo run --no-default-features --example models -- --write chalk` writes it.
//!
//! **One set of movements.** The puppet has the proportions of the arena's XKCD stick figure
//! (sixty-four pixels to the metre) and moves like it: every keyframe of every [`Clip`] is a
//! pose of the stick figure ([`clip_poses`]) solved onto the skeleton by [`solve`], so a punch
//! that reaches out in the arena reaches out in 3D, and so does everything else, the ground
//! positions of a grapple included. Only the stretch of the cartoon limbs is lost: the bones
//! keep their length.
//!
//! **The body.** Eleven joints (hips, spine, head, shoulders, elbows, hips and knees) carry
//! a mesh of ball joints and tubes: a wooden head with a long nose, painted eyes and a cap of
//! hair, a barrel torso in the outfit's cloth, wooden arms and shins, trousers, big shoes.
//! Every vertex follows exactly one joint, so the balls hide every bend. The model is
//! 1.8 m tall in its rest pose (the arms and the legs hanging down), facing `+z` like a glTF
//! character should; the gameplay moves it, the clips never do.
//!
//! **The texture.** One 3 x 2 atlas of 96-pixel cells: hair, wood, outfit, trim, eyes. Each is
//! the paint of a [`Paints`] with its own grain (streaks of lacquer, rings of wood, the weave
//! of cloth, leather), so a [`skin`] is the same atlas in the manga palette of a variant.
//!
//! [`glb`] and [`skin`] are deterministic: the committed model and skins are checked against
//! them.

use super::model::Clip;
use super::render::{Pixmap, Rgb8};
use super::scene::Rgba;
use super::skin::palette_of;
use super::stick::clip_poses;
use super::stick::{ARM, BACK_ARM, BACK_LEG, FRONT_ARM, FRONT_LEG, LEG, Point, Pose, bend};
use crate::game::{Skin, Style, Variant};

/// The `asset.generator` of the puppet, telling it apart from a real model.
pub const GENERATOR: &str = "srr puppet";

/// The generators of the placeholders this one replaced: models they made are still ours to
/// overwrite.
const PREVIOUS_GENERATORS: [&str; 1] = ["srr cube man"];

/// Whether a model whose `asset.generator` is `generator` is a placeholder made here (by the
/// puppet or by an earlier version of it), not a real model that must never be overwritten.
///
/// ```
/// use srr::front::puppet::{GENERATOR, made_here};
///
/// assert!(made_here(GENERATOR));
/// assert!(made_here("srr cube man"));
/// assert!(!made_here("Meshy"));
/// ```
pub fn made_here(generator: &str) -> bool {
    generator == GENERATOR || PREVIOUS_GENERATORS.contains(&generator)
}

/// The height of the puppet in its own units (metres), the top of the hair to the soles.
pub const HEIGHT: f32 = 1.8;

/// Metres per pixel of the stick figure: a power of two, so the maths is exact.
const K: f32 = 1.0 / 64.0;
/// The height of the ankle above the sole.
const SOLE: f32 = 0.05;

const FLOAT: u32 = 5126;
const UNSIGNED_BYTE: u32 = 5121;
const UNSIGNED_SHORT: u32 = 5123;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

// The joints, in the order of the skin (the node of a joint is its index plus one: node 0
// is the root the animations hang from).
const HIPS: usize = 0;
const SPINE: usize = 1;
const HEAD: usize = 2;
const ARM_L: usize = 3;
const ELBOW_L: usize = 4;
const ARM_R: usize = 5;
const ELBOW_R: usize = 6;
const LEG_L: usize = 7;
const KNEE_L: usize = 8;
const LEG_R: usize = 9;
const KNEE_R: usize = 10;

/// How many joints the puppet has.
pub const JOINT_COUNT: usize = 11;

/// The lengths of the bones: the stick figure's, in metres.
const TORSO: f32 = 32.0 * K;
const UPPER_ARM: f32 = ARM / 2.0 * K;
const FOREARM: f32 = ARM / 2.0 * K;
const THIGH: f32 = LEG / 2.0 * K;
const SHIN: f32 = LEG / 2.0 * K;
/// The shoulders sit at this share of the torso (the stick figure's: its shoulder is a
/// fifth of the way from the neck).
const SHOULDER_SHARE: f32 = 0.85;

/// The front arm and the back arm of the stick figure are the right and the left ones (the
/// shoulder joint and the elbow), the front leg and the back leg likewise (the hip joint and
/// the knee): the strikes of the clips use the right limbs.
const ARMS: [(usize, usize); 2] = [(ARM_R, ELBOW_R), (ARM_L, ELBOW_L)];
const LEGS: [(usize, usize); 2] = [(LEG_R, KNEE_R), (LEG_L, KNEE_L)];

/// `(name, parent joint, offset from the parent at rest)`. At rest the arms and the legs
/// hang down and the hips stand one leg and the sole above the floor.
const JOINTS: [(&str, Option<usize>, [f32; 3]); JOINT_COUNT] = [
    ("hips", None, [0.0, SOLE + LEG * K, 0.0]),
    ("spine", Some(HIPS), [0.0, 0.0, 0.0]),
    ("head", Some(SPINE), [0.0, TORSO, 0.0]),
    ("arm_l", Some(SPINE), [-0.15, TORSO * SHOULDER_SHARE, 0.0]),
    ("elbow_l", Some(ARM_L), [0.0, -UPPER_ARM, 0.0]),
    ("arm_r", Some(SPINE), [0.15, TORSO * SHOULDER_SHARE, 0.0]),
    ("elbow_r", Some(ARM_R), [0.0, -UPPER_ARM, 0.0]),
    ("leg_l", Some(HIPS), [-0.08, 0.0, 0.0]),
    ("knee_l", Some(LEG_L), [0.0, -THIGH, 0.0]),
    ("leg_r", Some(HIPS), [0.08, 0.0, 0.0]),
    ("knee_r", Some(LEG_R), [0.0, -THIGH, 0.0]),
];

/// Where `joint` stands at rest: its offsets summed up to the root.
fn joint_position(joint: usize) -> [f32; 3] {
    let (_, parent, offset) = JOINTS[joint];
    let base = parent.map_or([0.0; 3], joint_position);
    [
        base[0] + offset[0],
        base[1] + offset[1],
        base[2] + offset[2],
    ]
}

// ---------------------------------------------------------------------------------------------
// From a pose of the stick figure to a pose of the skeleton
// ---------------------------------------------------------------------------------------------

/// A pose of the skeleton: where the hips are and how every joint is pitched. Everything the
/// stick figure does happens in one plane (its own, seen from the side), so every joint only
/// turns about the `x` axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rig {
    /// The hips' translation: `(0, height, forward)` in metres, the floor at zero.
    pub hips: [f32; 3],
    /// The rotation of each joint relative to its parent, in radians about `x`: a positive
    /// angle pitches `+y` towards `+z`, so the spine leans forward and a hanging limb swings
    /// back.
    pub pitch: [f32; JOINT_COUNT],
}

/// The pitch of a bone hanging down at rest, from `a` to `b` in the plane of the stick figure
/// (`x` forward, `y` up): the angle that turns it to point at `b`.
fn hanging(a: Point, b: Point) -> f32 {
    -(b.x - a.x).atan2(a.y - b.y)
}

/// The skeleton posed like `pose`: the spine leans like the torso of the stick figure, every
/// bone of every limb points where the same bone of the stick figure points (the elbows and
/// the knees bend the way [`bend`] puts them), the hips stand where its hip stands.
///
/// ```
/// use srr::front::moves::Move;
/// use srr::front::puppet::solve;
/// use srr::front::stick::move_pose;
/// use srr::game::AttackPhase;
///
/// let rig = solve(&move_pose(Move::Stand, AttackPhase::Active));
/// assert!(rig.hips[1] > 0.8 && rig.hips[1] < 0.95);
/// ```
pub fn solve(pose: &Pose) -> Rig {
    let torso = Point {
        x: pose.neck.x - pose.hip.x,
        y: pose.neck.y - pose.hip.y,
    };
    let lean = torso.x.atan2(torso.y);
    let shoulder = Point {
        x: pose.hip.x + torso.x * SHOULDER_SHARE,
        y: pose.hip.y + torso.y * SHOULDER_SHARE,
    };
    let mut pitch = [0.0; JOINT_COUNT];
    pitch[SPINE] = lean;
    for side in [FRONT_ARM, BACK_ARM] {
        let hand = pose.hands[side];
        let elbow = bend(shoulder, hand, ARM, pose.bends[side]);
        let (upper, lower) = (hanging(shoulder, elbow), hanging(elbow, hand));
        let (shoulder_joint, elbow_joint) = ARMS[side];
        pitch[shoulder_joint] = upper - lean;
        pitch[elbow_joint] = lower - upper;
    }
    for side in [FRONT_LEG, BACK_LEG] {
        let foot = pose.feet[side - FRONT_LEG];
        let knee = bend(pose.hip, foot, LEG, pose.bends[side]);
        let (upper, lower) = (hanging(pose.hip, knee), hanging(knee, foot));
        let (hip_joint, knee_joint) = LEGS[side - FRONT_LEG];
        pitch[hip_joint] = upper;
        pitch[knee_joint] = lower - upper;
    }
    Rig {
        hips: [0.0, SOLE + pose.hip.y * K, pose.hip.x * K],
        pitch,
    }
}

/// Where the skeleton of a [`Rig`] puts its points, in the plane of the pose: `[forward, up]`
/// in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Points {
    pub hips: [f32; 2],
    pub shoulder: [f32; 2],
    pub neck: [f32; 2],
    /// The elbows, front then back, then the hands; the knees, then the feet.
    pub elbows: [[f32; 2]; 2],
    pub hands: [[f32; 2]; 2],
    pub knees: [[f32; 2]; 2],
    pub feet: [[f32; 2]; 2],
}

/// Walks the skeleton of `rig` from the hips out: the forward kinematics the clips are checked
/// against (the points of the stick figure, times [`K`], must come out).
pub fn points(rig: &Rig) -> Points {
    // Each joint's place `[forward, up]` and its pitch from the root.
    let mut world = [([0.0f32; 2], 0.0f32); JOINT_COUNT];
    for (joint, (_, parent, offset)) in JOINTS.iter().enumerate() {
        world[joint] = match parent {
            None => ([rig.hips[2], rig.hips[1]], rig.pitch[joint]),
            Some(parent) => {
                let (place, angle) = world[*parent];
                (
                    along(place, angle, (offset[1], offset[2])),
                    angle + rig.pitch[joint],
                )
            }
        };
    }
    let end = |joint: usize, length: f32| {
        let (place, angle) = world[joint];
        along(place, angle, (-length, 0.0))
    };
    Points {
        hips: world[HIPS].0,
        shoulder: world[ARM_R].0,
        neck: world[HEAD].0,
        elbows: [world[ELBOW_R].0, world[ELBOW_L].0],
        hands: [end(ELBOW_R, FOREARM), end(ELBOW_L, FOREARM)],
        knees: [world[KNEE_R].0, world[KNEE_L].0],
        feet: [end(KNEE_R, SHIN), end(KNEE_L, SHIN)],
    }
}

/// `place` moved by `(up, forward)` turned by the pitch `angle`.
fn along(place: [f32; 2], angle: f32, (up, forward): (f32, f32)) -> [f32; 2] {
    [
        place[0] + up * angle.sin() + forward * angle.cos(),
        place[1] + up * angle.cos() - forward * angle.sin(),
    ]
}

// ---------------------------------------------------------------------------------------------
// The mesh: balls and tubes
// ---------------------------------------------------------------------------------------------

/// The cells of the texture atlas, in reading order.
const HAIR: usize = 0;
const WOOD: usize = 1;
const CLOTH: usize = 2;
const LEATHER: usize = 3;
const EYES: usize = 4;
/// The atlas: three cells across, two down, [`CELL`] pixels each.
const COLUMNS: usize = 3;
const ROWS: usize = 2;
const CELL: usize = 96;
/// The share of a cell left unused around its mapped area, so filtering never bleeds a
/// neighbouring paint onto a part.
const MARGIN: f32 = 0.03;

/// How finely the balls and the tubes are cut.
const SEGMENTS: usize = 32;
const RINGS: usize = 20;

/// The geometry of the body, one entry per vertex.
#[derive(Debug, Clone, Default)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// The joint every vertex follows, with all of its weight.
    pub joints: Vec<u8>,
    /// Triangles, counter-clockwise seen from outside.
    pub indices: Vec<u16>,
}

/// Rounds to ten-thousandths: the numbers of the model stay the same whatever the last bit of
/// a `sin` says on the machine that builds it.
fn round4(value: f32) -> f32 {
    (value * 1e4).round() / 1e4
}

fn normalized(vector: [f32; 3]) -> [f32; 3] {
    let length = vector
        .iter()
        .map(|v| v * v)
        .sum::<f32>()
        .sqrt()
        .max(f32::EPSILON);
    vector.map(|v| v / length)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The part being built: the joint it follows and the cell of the atlas it is painted with.
#[derive(Clone, Copy)]
struct Part {
    joint: usize,
    cell: usize,
}

impl Geometry {
    /// One vertex of `part` at `position` (relative to its joint at rest).
    fn vertex(&mut self, part: Part, position: [f32; 3], normal: [f32; 3], (s, t): (f32, f32)) {
        let at = joint_position(part.joint);
        self.positions
            .push([0, 1, 2].map(|axis| round4(at[axis] + position[axis])));
        self.normals.push(normalized(normal).map(round4));
        let (column, row) = ((part.cell % COLUMNS) as f32, (part.cell / COLUMNS) as f32);
        let inside = |fraction: f32| MARGIN + (1.0 - 2.0 * MARGIN) * fraction;
        self.uvs.push([
            round4((column + inside(s)) / COLUMNS as f32),
            round4((row + inside(t)) / ROWS as f32),
        ]);
        self.joints.push(part.joint as u8);
    }

    /// Joins the vertices from `first` on, laid out as a grid of `rows + 1` by `columns + 1`
    /// (the rows going down, the columns going right as seen from outside), into triangles.
    fn grid(&mut self, first: usize, rows: usize, columns: usize) {
        for row in 0..rows {
            for column in 0..columns {
                let top_left = first + row * (columns + 1) + column;
                let (top_right, bottom_left) = (top_left + 1, top_left + columns + 1);
                let bottom_right = bottom_left + 1;
                for corner in [
                    top_left,
                    bottom_left,
                    bottom_right,
                    top_left,
                    bottom_right,
                    top_right,
                ] {
                    self.indices
                        .push(u16::try_from(corner).expect("the puppet fits 16-bit indices"));
                }
            }
        }
    }

    /// A ball, or a slice of one: an ellipsoid of `radii` around `centre`, from the latitude
    /// `lat.0` to `lat.1` (degrees from the top pole) and the longitude `lon.0` to `lon.1`
    /// (degrees from the front, towards the right).
    fn ellipsoid(
        &mut self,
        part: Part,
        centre: [f32; 3],
        radii: [f32; 3],
        lat: (f32, f32),
        lon: (f32, f32),
    ) {
        let first = self.positions.len();
        for ring in 0..=RINGS {
            let t = ring as f32 / RINGS as f32;
            let theta = (lat.0 + (lat.1 - lat.0) * t).to_radians();
            for segment in 0..=SEGMENTS {
                let s = segment as f32 / SEGMENTS as f32;
                let phi = (lon.0 + (lon.1 - lon.0) * s).to_radians();
                let unit = [
                    theta.sin() * phi.sin(),
                    theta.cos(),
                    theta.sin() * phi.cos(),
                ];
                let position = [0, 1, 2].map(|axis| centre[axis] + radii[axis] * unit[axis]);
                let normal = [0, 1, 2].map(|axis| unit[axis] / radii[axis]);
                self.vertex(part, position, normal, (s, t));
            }
        }
        self.grid(first, RINGS, SEGMENTS);
    }

    /// A whole ball.
    fn ball(&mut self, part: Part, centre: [f32; 3], radii: [f32; 3]) {
        self.ellipsoid(part, centre, radii, (0.0, 180.0), (0.0, 360.0));
    }

    /// A tube from `from` to `to`, `radii.0` thick at the start and `radii.1` at the end,
    /// open at both ends (a ball or another part always covers them).
    fn tube(&mut self, part: Part, from: [f32; 3], to: [f32; 3], radii: (f32, f32)) {
        let axis = normalized([to[0] - from[0], to[1] - from[1], to[2] - from[2]]);
        // Any vector across the axis starts the frame; the axis never points along `x` here.
        let across = normalized(cross(axis, [1.0, 0.0, 0.0]));
        let around = cross(axis, across);
        let first = self.positions.len();
        // The first row is the end, the last row the start, so the columns run to the right.
        for (row, (end, radius)) in [(to, radii.1), (from, radii.0)].into_iter().enumerate() {
            for segment in 0..=SEGMENTS {
                let s = segment as f32 / SEGMENTS as f32;
                let angle = s * std::f32::consts::TAU;
                let radial =
                    [0, 1, 2].map(|axis| angle.cos() * across[axis] + angle.sin() * around[axis]);
                let position = [0, 1, 2].map(|axis| end[axis] + radius * radial[axis]);
                self.vertex(part, position, radial, (s, 1.0 - row as f32));
            }
        }
        self.grid(first, 1, SEGMENTS);
    }
}

/// The geometry of the whole puppet, in the rest pose: every part in the coordinates of its
/// joint.
///
/// ```
/// use srr::front::puppet::geometry;
///
/// let body = geometry();
/// assert!(body.positions.len() > 10_000, "a smooth body, not nine boxes");
/// assert_eq!(body.indices.len() % 3, 0);
/// ```
pub fn geometry() -> Geometry {
    let mut body = Geometry::default();
    let part = |joint: usize, cell: usize| Part { joint, cell };
    let head = 14.0 * K;
    let radius = 12.0 * K;

    // The head: a wooden ball, a long nose, two painted eyes and a cap of hair, with the
    // hair falling at the back and the sides.
    body.ball(part(HEAD, WOOD), [0.0, head, 0.0], [radius, radius, radius]);
    body.tube(
        part(HEAD, WOOD),
        [0.0, head - 0.01, radius - 0.01],
        [0.0, head + 0.02, radius + 0.15],
        (0.03, 0.004),
    );
    for side in [-1.0, 1.0] {
        body.ball(
            part(HEAD, EYES),
            [side * 0.065, head + 0.035, radius - 0.016],
            [0.022, 0.03, 0.02],
        );
    }
    let hair = radius * 1.035;
    body.ellipsoid(
        part(HEAD, HAIR),
        [0.0, head, 0.0],
        [hair, hair, hair],
        (0.0, 66.0),
        (0.0, 360.0),
    );
    body.ellipsoid(
        part(HEAD, HAIR),
        [0.0, head, 0.0],
        [hair, hair, hair],
        (66.0, 132.0),
        (90.0, 270.0),
    );

    // The torso: a barrel of cloth between a pelvis and a chest, a belt, a thin neck.
    let chest = TORSO * SHOULDER_SHARE;
    body.ball(part(HIPS, CLOTH), [0.0, 0.0, 0.0], [0.115, 0.09, 0.105]);
    body.tube(
        part(SPINE, CLOTH),
        [0.0, 0.0, 0.0],
        [0.0, chest, 0.0],
        (0.105, 0.125),
    );
    body.tube(
        part(SPINE, LEATHER),
        [0.0, 0.05, 0.0],
        [0.0, 0.11, 0.0],
        (0.112, 0.114),
    );
    body.ball(part(SPINE, CLOTH), [0.0, chest, 0.0], [0.14, 0.09, 0.1]);
    body.tube(
        part(SPINE, WOOD),
        [0.0, chest, 0.0],
        [0.0, TORSO + 0.04, 0.0],
        (0.04, 0.04),
    );

    // The arms: wooden balls and tubes, mitten hands.
    for (arm, elbow) in ARMS {
        body.ball(part(arm, WOOD), [0.0, 0.0, 0.0], [0.062; 3]);
        body.tube(
            part(arm, WOOD),
            [0.0, 0.0, 0.0],
            [0.0, -UPPER_ARM, 0.0],
            (0.04, 0.035),
        );
        body.ball(part(elbow, WOOD), [0.0, 0.0, 0.0], [0.045; 3]);
        body.tube(
            part(elbow, WOOD),
            [0.0, 0.0, 0.0],
            [0.0, -FOREARM, 0.0],
            (0.035, 0.03),
        );
        body.ball(
            part(elbow, WOOD),
            [0.0, -FOREARM - 0.03, 0.0],
            [0.05, 0.06, 0.04],
        );
    }

    // The legs: trousers to the knee, wooden shins, big shoes whose soles touch the floor.
    for (leg, knee) in LEGS {
        body.ball(part(leg, CLOTH), [0.0, 0.0, 0.0], [0.07; 3]);
        body.tube(
            part(leg, CLOTH),
            [0.0, 0.0, 0.0],
            [0.0, -THIGH, 0.0],
            (0.058, 0.048),
        );
        body.ball(part(knee, WOOD), [0.0, 0.0, 0.0], [0.052; 3]);
        body.tube(
            part(knee, WOOD),
            [0.0, 0.0, 0.0],
            [0.0, -SHIN, 0.0],
            (0.042, 0.034),
        );
        body.ball(part(knee, WOOD), [0.0, -SHIN, 0.0], [0.04; 3]);
        body.ball(
            part(knee, LEATHER),
            [0.0, -SHIN - SOLE + 0.045, 0.07],
            [0.065, 0.045, 0.14],
        );
    }
    body
}

// ---------------------------------------------------------------------------------------------
// The paint
// ---------------------------------------------------------------------------------------------

/// The colours a puppet is painted in: each paint with the darker colour of its grain.
#[derive(Debug, Clone, Copy)]
pub struct Paints {
    pub hair: [Rgba; 2],
    pub wood: [Rgba; 2],
    pub cloth: [Rgba; 2],
    pub leather: [Rgba; 2],
    pub eyes: Rgba,
}

impl Paints {
    /// The chalk's own: dark lacquered hair, pale wood, a red outfit, brown leather.
    pub const fn chalk() -> Paints {
        Paints {
            hair: [Rgba::hex(0x3b2a20), Rgba::hex(0x20150f)],
            wood: [Rgba::hex(0xdca25f), Rgba::hex(0xa4692e)],
            cloth: [Rgba::hex(0xc23b2e), Rgba::hex(0x8c2a22)],
            leather: [Rgba::hex(0x6b4426), Rgba::hex(0x432913)],
            eyes: Rgba::hex(0x15161a),
        }
    }

    /// The manga palette of `variant`: its hair, its skin as the wood, its outfit, its trim.
    pub fn of(variant: Variant) -> Paints {
        let palette = palette_of(&Skin::styled(Style::Manga, variant));
        Paints {
            hair: [palette.hair, palette.hair_shadow],
            wood: [palette.skin, palette.skin_shadow],
            cloth: [palette.outfit, palette.outfit_shadow],
            leather: [palette.trim, palette.trim.scaled(0.6)],
            eyes: palette.eye,
        }
    }
}

/// A pseudo-random number in `0..1` from a lattice point: a hash, the same everywhere.
fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9e37_79b1) ^ (y as u32).wrapping_mul(0x85eb_ca6b);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h & 0xffff) as f32 / 65535.0
}

/// Smooth value noise at `(x, y)`, repeating every `period` along `x` (so the grain closes
/// around a tube without a seam).
fn noise(x: f32, y: f32, period: i32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let at = |dx: i32, dy: i32| hash((x0 as i32 + dx).rem_euclid(period), y0 as i32 + dy);
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * smooth(fx);
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * smooth(fx);
    top + (bottom - top) * smooth(fy)
}

/// `a` to `b` by `amount`.
fn mix(a: Rgba, b: Rgba, amount: f32) -> [f32; 3] {
    [
        a.r + (b.r - a.r) * amount,
        a.g + (b.g - a.g) * amount,
        a.b + (b.b - a.b) * amount,
    ]
}

/// The paint at texel `(x, y)` of `cell`: its colour with its grain, as sRGB.
fn texel(paints: &Paints, cell: usize, x: usize, y: usize) -> Rgb8 {
    let (s, t) = (
        (x as f32 + 0.5) / CELL as f32,
        (y as f32 + 0.5) / CELL as f32,
    );
    let color = match cell {
        // Lacquered strands, with a band of light across the crown.
        HAIR => {
            let strand = (s * 14.0 + 0.8 * noise(s * 4.0, t * 3.0, 4)).fract();
            let dark = (1.0 - (2.0 * strand - 1.0).abs()).powi(2) * 0.5;
            let shine = (-((t - 0.28) / 0.08).powi(2)).exp() * 0.22;
            let base = mix(paints.hair[0], paints.hair[1], dark);
            base.map(|channel| channel + (1.0 - channel) * shine)
        }
        // Rings of wood along the length, a knot, a little mottling.
        WOOD => {
            let ring = (s * 8.0 + 1.5 * noise(s * 4.0, t * 3.0, 4)).fract();
            let line = (1.0 - (2.0 * ring - 1.0).abs()).powi(6);
            let knot = (-((s - 0.3).powi(2) + (t - 0.6).powi(2)) / 0.003).exp();
            let mottle = 0.04 * (noise(s * 16.0, t * 16.0, 16) - 0.5);
            mix(
                paints.wood[0],
                paints.wood[1],
                (0.55 * line + 0.6 * knot).min(1.0),
            )
            .map(|channel| channel + mottle)
        }
        // The weave of cloth.
        CLOTH => {
            let over = (x / 2 + y / 2).is_multiple_of(2);
            mix(
                paints.cloth[0],
                paints.cloth[1],
                if over { 0.0 } else { 0.14 },
            )
        }
        // Leather: a fine mottling.
        LEATHER => mix(
            paints.leather[0],
            paints.leather[1],
            0.3 * noise(x as f32 / 4.0, y as f32 / 4.0, 24),
        ),
        // The eyes: dark, with a spark.
        EYES => {
            let spark = ((s - 0.42) / 0.1).powi(2) + ((t - 0.33) / 0.1).powi(2) < 1.0;
            if spark {
                [1.0, 1.0, 1.0]
            } else {
                [paints.eyes.r, paints.eyes.g, paints.eyes.b]
            }
        }
        // The spare cell is plain wood.
        _ => [paints.wood[0].r, paints.wood[0].g, paints.wood[0].b],
    };
    color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The atlas painted with `paints`, as a PNG.
///
/// ```
/// use srr::front::puppet::{Paints, texture};
///
/// let png = texture(&Paints::chalk());
/// assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
/// ```
pub fn texture(paints: &Paints) -> Vec<u8> {
    let (width, height) = (COLUMNS * CELL, ROWS * CELL);
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let cell = y / CELL * COLUMNS + x / CELL;
            pixels.push(texel(paints, cell, x % CELL, y % CELL));
        }
    }
    Pixmap::from_pixels(width, height, pixels).to_png()
}

/// The skin texture of `variant` for the puppet: the atlas in its manga palette.
pub fn skin(variant: Variant) -> Vec<u8> {
    texture(&Paints::of(variant))
}

// ---------------------------------------------------------------------------------------------
// The glTF binary
// ---------------------------------------------------------------------------------------------

/// A quaternion `(x, y, z, w)` of `angle` radians about the `x` axis, rounded like the rest.
fn pitch_quaternion(angle: f32) -> [f32; 4] {
    let half = angle / 2.0;
    [half.sin(), 0.0, 0.0, half.cos()].map(|value| (value * 1e5).round() / 1e5)
}

/// The binary chunk under construction: the buffer views it holds, 4-byte aligned.
#[derive(Default)]
struct Bin {
    bytes: Vec<u8>,
    views: Vec<String>,
}

impl Bin {
    /// Appends `data` as a buffer view and returns its index.
    fn view(&mut self, data: &[u8], target: Option<u32>) -> usize {
        self.bytes.resize(self.bytes.len().next_multiple_of(4), 0);
        let offset = self.bytes.len();
        self.bytes.extend_from_slice(data);
        let target = target.map_or(String::new(), |t| format!(",\"target\":{t}"));
        self.views.push(format!(
            "{{\"buffer\":0,\"byteOffset\":{offset},\"byteLength\":{}{target}}}",
            data.len()
        ));
        self.views.len() - 1
    }
}

fn f32s(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}

/// A JSON array of numbers.
fn list(values: &[f32]) -> String {
    let items: Vec<String> = values.iter().map(|v| format!("{v:?}")).collect();
    format!("[{}]", items.join(","))
}

/// The accessor JSON of a buffer view of `count` elements of `kind` (`VEC3`...) with
/// `component` type, and `extra` fields (`,"min":...`).
fn accessor(view: usize, component: u32, count: usize, kind: &str, extra: &str) -> String {
    format!(
        "{{\"bufferView\":{view},\"componentType\":{component},\"count\":{count},\"type\":\"{kind}\"{extra}}}"
    )
}

/// Packs a JSON chunk and a binary chunk into a glTF binary.
pub fn pack(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json = json.as_bytes().to_vec();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let length = 12 + 8 + json.len() + 8 + bin.len();
    let mut glb = Vec::with_capacity(length);
    glb.extend_from_slice(&0x4654_6C67u32.to_le_bytes());
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(length as u32).to_le_bytes());
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    glb.extend_from_slice(&json);
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x004E_4942u32.to_le_bytes());
    glb.extend_from_slice(&bin);
    glb
}

/// The puppet as a glTF binary: one scene, a root node holding the skeleton and the skinned
/// mesh, one material with the embedded atlas, and every [`Clip`] keyed from the poses of the
/// stick figure.
pub fn glb() -> Vec<u8> {
    let mut bin = Bin::default();
    let mut accessors = Vec::new();
    let body = geometry();
    let count = body.positions.len();
    let mut bounds = ([f32::MAX; 3], [f32::MIN; 3]);
    for position in &body.positions {
        for (axis, &value) in position.iter().enumerate() {
            bounds.0[axis] = bounds.0[axis].min(value);
            bounds.1[axis] = bounds.1[axis].max(value);
        }
    }
    let view = bin.view(
        &f32s(body.positions.iter().flatten().copied()),
        Some(ARRAY_BUFFER),
    );
    accessors.push(accessor(
        view,
        FLOAT,
        count,
        "VEC3",
        &format!(",\"min\":{},\"max\":{}", list(&bounds.0), list(&bounds.1)),
    ));
    let view = bin.view(
        &f32s(body.normals.iter().flatten().copied()),
        Some(ARRAY_BUFFER),
    );
    accessors.push(accessor(view, FLOAT, count, "VEC3", ""));
    let view = bin.view(
        &f32s(body.uvs.iter().flatten().copied()),
        Some(ARRAY_BUFFER),
    );
    accessors.push(accessor(view, FLOAT, count, "VEC2", ""));
    // Every vertex follows one joint with all of its weight.
    let joints: Vec<u8> = body.joints.iter().flat_map(|&j| [j, 0, 0, 0]).collect();
    let view = bin.view(&joints, Some(ARRAY_BUFFER));
    accessors.push(accessor(view, UNSIGNED_BYTE, count, "VEC4", ""));
    let weights: Vec<u8> = body.joints.iter().flat_map(|_| [255, 0, 0, 0]).collect();
    let view = bin.view(&weights, Some(ARRAY_BUFFER));
    accessors.push(accessor(
        view,
        UNSIGNED_BYTE,
        count,
        "VEC4",
        ",\"normalized\":true",
    ));
    let index_bytes: Vec<u8> = body.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
    let view = bin.view(&index_bytes, Some(ELEMENT_ARRAY_BUFFER));
    accessors.push(accessor(
        view,
        UNSIGNED_SHORT,
        body.indices.len(),
        "SCALAR",
        "",
    ));
    // The inverse bind matrices: the joints carry no rotation at rest, so each is the
    // translation back from the joint (column-major).
    let mut inverse_bind = Vec::new();
    for joint in 0..JOINT_COUNT {
        let [x, y, z] = joint_position(joint);
        inverse_bind.extend_from_slice(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -x, -y, -z, 1.0,
        ]);
    }
    let view = bin.view(&f32s(inverse_bind), None);
    accessors.push(accessor(view, FLOAT, JOINT_COUNT, "MAT4", ""));
    let inverse_bind_accessor = accessors.len() - 1;

    let mut animations = Vec::new();
    for clip in Clip::ALL {
        let (seconds, keys) = clip_poses(clip);
        let rigs: Vec<Rig> = keys.iter().map(|(_, pose)| solve(pose)).collect();
        let times: Vec<f32> = keys.iter().map(|(at, _)| at * seconds).collect();
        let view = bin.view(&f32s(times.iter().copied()), None);
        accessors.push(accessor(
            view,
            FLOAT,
            times.len(),
            "SCALAR",
            &format!(",\"min\":[0.0],\"max\":[{seconds:?}]"),
        ));
        let input = accessors.len() - 1;
        let mut samplers = Vec::new();
        let mut targets = Vec::new();
        // Every joint is keyed in every clip, so a clip never inherits the pose of the one
        // before it; the hips carry the translation too.
        let mut channels: Vec<(usize, &str, &str, Vec<f32>)> = (0..JOINT_COUNT)
            .map(|joint| {
                let values = rigs
                    .iter()
                    .flat_map(|rig| pitch_quaternion(rig.pitch[joint]))
                    .collect();
                (joint, "VEC4", "rotation", values)
            })
            .collect();
        let hips = rigs.iter().flat_map(|rig| rig.hips).collect();
        channels.push((HIPS, "VEC3", "translation", hips));
        for (joint, kind, path, values) in channels {
            let view = bin.view(&f32s(values), None);
            accessors.push(accessor(view, FLOAT, rigs.len(), kind, ""));
            let sampler = samplers.len();
            samplers.push(format!(
                "{{\"input\":{input},\"interpolation\":\"LINEAR\",\"output\":{}}}",
                accessors.len() - 1
            ));
            targets.push(format!(
                "{{\"sampler\":{sampler},\"target\":{{\"node\":{},\"path\":\"{path}\"}}}}",
                joint + 1
            ));
        }
        animations.push(format!(
            "{{\"name\":\"{}\",\"channels\":[{}],\"samplers\":[{}]}}",
            clip.name(),
            targets.join(","),
            samplers.join(",")
        ));
    }
    let image_view = bin.view(&texture(&Paints::chalk()), None);

    let mesh_node = JOINT_COUNT + 1;
    let mut nodes = vec![format!(
        "{{\"name\":\"root\",\"children\":[1,{mesh_node}]}}"
    )];
    for (joint, &(name, _, offset)) in JOINTS.iter().enumerate() {
        let children: Vec<String> = JOINTS
            .iter()
            .enumerate()
            .filter(|(_, (_, parent, _))| *parent == Some(joint))
            .map(|(child, _)| (child + 1).to_string())
            .collect();
        let children = if children.is_empty() {
            String::new()
        } else {
            format!(",\"children\":[{}]", children.join(","))
        };
        nodes.push(format!(
            "{{\"name\":\"{name}\",\"translation\":{}{children}}}",
            list(&offset)
        ));
    }
    nodes.push("{\"name\":\"body\",\"mesh\":0,\"skin\":0}".to_string());
    let skin_joints: Vec<String> = (1..=JOINT_COUNT).map(|n| n.to_string()).collect();

    let json = format!(
        concat!(
            "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"{generator}\"}},",
            "\"scene\":0,\"scenes\":[{{\"name\":\"puppet\",\"nodes\":[0]}}],",
            "\"nodes\":[{nodes}],",
            "\"meshes\":[{{\"name\":\"body\",\"primitives\":[{{\"attributes\":{{",
            "\"POSITION\":0,\"NORMAL\":1,\"TEXCOORD_0\":2,\"JOINTS_0\":3,\"WEIGHTS_0\":4}},",
            "\"indices\":5,\"material\":0}}]}}],",
            "\"skins\":[{{\"name\":\"skeleton\",\"inverseBindMatrices\":{inverse_bind},",
            "\"skeleton\":1,\"joints\":[{joints}]}}],",
            "\"materials\":[{{\"name\":\"body\",\"pbrMetallicRoughness\":{{",
            "\"baseColorTexture\":{{\"index\":0}},\"baseColorFactor\":[1.0,1.0,1.0,1.0],",
            "\"metallicFactor\":0.0,\"roughnessFactor\":1.0}}}}],",
            "\"textures\":[{{\"sampler\":0,\"source\":0}}],",
            "\"images\":[{{\"name\":\"atlas\",\"mimeType\":\"image/png\",\"bufferView\":{image}}}],",
            "\"samplers\":[{{\"magFilter\":9729,\"minFilter\":9729,\"wrapS\":33071,\"wrapT\":33071}}],",
            "\"animations\":[{animations}],",
            "\"accessors\":[{accessors}],",
            "\"bufferViews\":[{views}],",
            "\"buffers\":[{{\"byteLength\":{length}}}]}}"
        ),
        generator = GENERATOR,
        nodes = nodes.join(","),
        inverse_bind = inverse_bind_accessor,
        joints = skin_joints.join(","),
        image = image_view,
        animations = animations.join(","),
        accessors = accessors.join(","),
        views = bin.views.join(","),
        length = bin.bytes.len(),
    );
    pack(&json, &bin.bytes)
}
