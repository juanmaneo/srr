//! The cube man: a tiny rigged fighter generated procedurally as a glTF binary. It is the
//! test fixture of the 3D view (CI loads it instead of a model from the image-to-3D pipeline,
//! which it never has to download) and the placeholder a character wears until its own model
//! is made (`docs/3d-asset-pipeline.md`); `cargo xtask models` audits the models and
//! `cargo run --no-default-features --example models -- --write chalk` writes him.
//!
//! Nine boxes (head, torso, pelvis, two arms, two legs, two feet) skinned to a seven-joint
//! humanoid skeleton in the T-pose, 1.8 m tall, facing `+z` like a glTF character should, and
//! every [`Clip`] as a three-keyframe animation in place — the gameplay moves the model, the
//! clips never do. Each face of each box samples one of the four cells of a 4x1 texture (hair,
//! skin, outfit, trim), so a [`skin`] is a four-pixel PNG: the manga palette of the variant.
//!
//! [`glb`] is deterministic: the committed placeholder is checked against it.

use super::model::Clip;
use super::render::Pixmap;
use super::skin::palette_of;
use crate::game::{Skin, Style, Variant};

/// The `asset.generator` of the cube man, telling him apart from a real model.
pub const GENERATOR: &str = "srr cube man";

/// The height of the cube man in his own units (metres), head top to soles.
pub const HEIGHT: f32 = 1.8;

/// The cells of the texture, in order: hair, skin, outfit, trim.
const CELLS: usize = 4;

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
const ARM_R: usize = 4;
const LEG_L: usize = 5;
const LEG_R: usize = 6;

/// `(name, parent joint, offset from the parent in the T-pose)`: the hips stand at
/// `(0, 0.95, 0)`, the spine 0.25 above, the head 0.3 above that, the shoulders 0.22 above
/// the spine and aside, the legs aside the hips.
const JOINTS: [(&str, Option<usize>, [f32; 3]); 7] = [
    ("hips", None, [0.0, 0.95, 0.0]),
    ("spine", Some(HIPS), [0.0, 0.25, 0.0]),
    ("head", Some(SPINE), [0.0, 0.3, 0.0]),
    ("arm_l", Some(SPINE), [-0.22, 0.22, 0.0]),
    ("arm_r", Some(SPINE), [0.22, 0.22, 0.0]),
    ("leg_l", Some(HIPS), [-0.11, 0.0, 0.0]),
    ("leg_r", Some(HIPS), [0.11, 0.0, 0.0]),
];

/// Where `joint` stands in the T-pose: its offsets summed up to the root.
fn joint_position(joint: usize) -> [f32; 3] {
    let (_, parent, offset) = JOINTS[joint];
    let base = parent.map_or([0.0; 3], joint_position);
    [
        base[0] + offset[0],
        base[1] + offset[1],
        base[2] + offset[2],
    ]
}

/// `(joint, centre, size, cell of the side faces, cell of the top face)`.
type Box3 = (usize, [f32; 3], [f32; 3], usize, usize);

const BOXES: [Box3; 9] = [
    (HEAD, [0.0, 1.66, 0.0], [0.26, 0.28, 0.26], 1, 0),
    (SPINE, [0.0, 1.25, 0.0], [0.36, 0.44, 0.2], 2, 2),
    (HIPS, [0.0, 0.97, 0.0], [0.34, 0.14, 0.2], 2, 2),
    (ARM_L, [-0.52, 1.42, 0.0], [0.6, 0.12, 0.12], 1, 1),
    (ARM_R, [0.52, 1.42, 0.0], [0.6, 0.12, 0.12], 1, 1),
    (LEG_L, [-0.11, 0.5, 0.0], [0.16, 0.9, 0.18], 2, 2),
    (LEG_R, [0.11, 0.5, 0.0], [0.16, 0.9, 0.18], 2, 2),
    (LEG_L, [-0.11, 0.05, 0.03], [0.17, 0.1, 0.26], 3, 3),
    (LEG_R, [0.11, 0.05, 0.03], [0.17, 0.1, 0.26], 3, 3),
];

/// What a channel animates.
#[derive(Clone, Copy)]
enum Path {
    Rotation,
    Translation,
}

/// One animated joint of a clip: its value at the start, the middle and the end.
struct Channel {
    joint: usize,
    path: Path,
    keys: [[f32; 4]; 3],
}

/// A quaternion `(x, y, z, w)` of `degrees` around `axis` (a unit vector).
fn turn(axis: [f32; 3], degrees: f32) -> [f32; 4] {
    let half = degrees.to_radians() / 2.0;
    let s = half.sin();
    [axis[0] * s, axis[1] * s, axis[2] * s, half.cos()]
}

/// `a` after `b`: the rotation applying `b` first, then `a`.
fn then(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

const X: [f32; 3] = [1.0, 0.0, 0.0];
const Y: [f32; 3] = [0.0, 1.0, 0.0];
const Z: [f32; 3] = [0.0, 0.0, 1.0];

/// The arms hanging down from the T-pose (an A-pose), the base of every clip.
fn arm_down(joint: usize) -> [f32; 4] {
    turn(Z, if joint == ARM_L { 75.0 } else { -75.0 })
}

/// An arm pointing forward (`+z`), in front of the fighter.
fn arm_forward(joint: usize) -> [f32; 4] {
    turn(Y, if joint == ARM_L { 90.0 } else { -90.0 })
}

/// The arm `joint` swinging `degrees` forward and back while hanging down.
fn arm_swing(joint: usize, degrees: f32) -> [f32; 4] {
    then(turn(X, degrees), arm_down(joint))
}

fn rotation(joint: usize, keys: [[f32; 4]; 3]) -> Channel {
    Channel {
        joint,
        path: Path::Rotation,
        keys,
    }
}

fn translation(joint: usize, keys: [[f32; 3]; 3]) -> Channel {
    Channel {
        joint,
        path: Path::Translation,
        keys: keys.map(|[x, y, z]| [x, y, z, 0.0]),
    }
}

/// Both arms hanging still.
fn arms_down() -> [Channel; 2] {
    [
        rotation(ARM_L, [arm_down(ARM_L); 3]),
        rotation(ARM_R, [arm_down(ARM_R); 3]),
    ]
}

/// The hips at rest.
const HIPS_AT_REST: [f32; 3] = [0.0, 0.95, 0.0];

/// The duration of `clip` in seconds, and its channels. Loops end where they start; the
/// strikes reach their contact pose in the middle, where the active ticks sample them.
fn channels_of(clip: Clip) -> (f32, Vec<Channel>) {
    let still = |joint: usize, q: [f32; 4]| rotation(joint, [q; 3]);
    match clip {
        Clip::Idle => (
            2.0,
            vec![
                rotation(SPINE, [turn(X, 0.0), turn(X, 3.0), turn(X, 0.0)]),
                rotation(ARM_L, [arm_down(ARM_L), turn(Z, 72.0), arm_down(ARM_L)]),
                rotation(ARM_R, [arm_down(ARM_R), turn(Z, -72.0), arm_down(ARM_R)]),
            ],
        ),
        Clip::WalkF | Clip::WalkB => {
            let stride = if clip == Clip::WalkF { 25.0 } else { -25.0 };
            (
                0.8,
                vec![
                    rotation(LEG_L, [turn(X, -stride), turn(X, stride), turn(X, -stride)]),
                    rotation(LEG_R, [turn(X, stride), turn(X, -stride), turn(X, stride)]),
                    rotation(
                        ARM_L,
                        [
                            arm_swing(ARM_L, stride),
                            arm_swing(ARM_L, -stride),
                            arm_swing(ARM_L, stride),
                        ],
                    ),
                    rotation(
                        ARM_R,
                        [
                            arm_swing(ARM_R, -stride),
                            arm_swing(ARM_R, stride),
                            arm_swing(ARM_R, -stride),
                        ],
                    ),
                ],
            )
        }
        Clip::Crouch => {
            let [arm_l, arm_r] = arms_down();
            (
                1.0,
                vec![
                    translation(HIPS, [[0.0, 0.6, 0.0]; 3]),
                    still(LEG_L, turn(X, -60.0)),
                    still(LEG_R, turn(X, -60.0)),
                    rotation(SPINE, [turn(X, 25.0), turn(X, 28.0), turn(X, 25.0)]),
                    arm_l,
                    arm_r,
                ],
            )
        }
        Clip::Jump => (
            0.6,
            vec![
                translation(HIPS, [HIPS_AT_REST, [0.0, 1.35, 0.0], HIPS_AT_REST]),
                rotation(LEG_L, [turn(X, 0.0), turn(X, -40.0), turn(X, 0.0)]),
                rotation(LEG_R, [turn(X, 0.0), turn(X, -40.0), turn(X, 0.0)]),
                rotation(ARM_L, [arm_down(ARM_L), turn(Z, -60.0), arm_down(ARM_L)]),
                rotation(ARM_R, [arm_down(ARM_R), turn(Z, 60.0), arm_down(ARM_R)]),
            ],
        ),
        Clip::Block => (
            1.0,
            vec![
                still(ARM_L, then(turn(X, -40.0), arm_forward(ARM_L))),
                still(ARM_R, then(turn(X, -40.0), arm_forward(ARM_R))),
                rotation(SPINE, [turn(X, 8.0), turn(X, 10.0), turn(X, 8.0)]),
            ],
        ),
        Clip::HitLight => {
            let [arm_l, arm_r] = arms_down();
            (
                0.3,
                vec![
                    rotation(SPINE, [turn(X, 0.0), turn(X, -15.0), turn(X, -5.0)]),
                    rotation(HEAD, [turn(X, 0.0), turn(X, -20.0), turn(X, 0.0)]),
                    arm_l,
                    arm_r,
                ],
            )
        }
        Clip::HitHeavy => {
            let [arm_l, arm_r] = arms_down();
            (
                0.5,
                vec![
                    rotation(SPINE, [turn(X, 0.0), turn(X, -35.0), turn(X, -20.0)]),
                    translation(HIPS, [HIPS_AT_REST, [0.0, 0.9, -0.15], [0.0, 0.92, -0.1]]),
                    rotation(LEG_L, [turn(X, 0.0), turn(X, 15.0), turn(X, 10.0)]),
                    rotation(LEG_R, [turn(X, 0.0), turn(X, 15.0), turn(X, 10.0)]),
                    arm_l,
                    arm_r,
                ],
            )
        }
        Clip::Ko => {
            let [arm_l, arm_r] = arms_down();
            (
                0.8,
                vec![
                    rotation(HIPS, [turn(X, 0.0), turn(X, -45.0), turn(X, -90.0)]),
                    translation(HIPS, [HIPS_AT_REST, [0.0, 0.5, 0.0], [0.0, 0.12, 0.0]]),
                    arm_l,
                    arm_r,
                ],
            )
        }
        Clip::Punch => (
            0.5,
            vec![
                rotation(
                    ARM_R,
                    [arm_down(ARM_R), arm_forward(ARM_R), arm_down(ARM_R)],
                ),
                rotation(
                    ARM_L,
                    [arm_down(ARM_L), arm_swing(ARM_L, 30.0), arm_down(ARM_L)],
                ),
                rotation(SPINE, [turn(Y, 0.0), turn(Y, -20.0), turn(Y, 0.0)]),
            ],
        ),
        Clip::Kick => {
            let [arm_l, arm_r] = arms_down();
            (
                0.6,
                vec![
                    rotation(LEG_R, [turn(X, 0.0), turn(X, -85.0), turn(X, 0.0)]),
                    rotation(SPINE, [turn(X, 0.0), turn(X, -10.0), turn(X, 0.0)]),
                    arm_l,
                    arm_r,
                ],
            )
        }
        Clip::Sweep => {
            let [arm_l, arm_r] = arms_down();
            (
                0.8,
                vec![
                    translation(HIPS, [HIPS_AT_REST, [0.0, 0.55, 0.0], HIPS_AT_REST]),
                    rotation(LEG_L, [turn(X, 0.0), turn(X, -60.0), turn(X, 0.0)]),
                    rotation(
                        LEG_R,
                        [
                            turn(X, 0.0),
                            then(turn(Y, 150.0), turn(X, -80.0)),
                            turn(X, 0.0),
                        ],
                    ),
                    arm_l,
                    arm_r,
                ],
            )
        }
        Clip::Special => (
            0.8,
            vec![
                rotation(SPINE, [turn(X, 0.0), turn(X, 30.0), turn(X, 0.0)]),
                rotation(
                    ARM_L,
                    [arm_down(ARM_L), arm_forward(ARM_L), arm_down(ARM_L)],
                ),
                rotation(
                    ARM_R,
                    [arm_down(ARM_R), arm_forward(ARM_R), arm_down(ARM_R)],
                ),
                translation(HIPS, [HIPS_AT_REST, [0.0, 0.95, 0.25], HIPS_AT_REST]),
            ],
        ),
    }
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

fn f32s(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
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

/// The vertices of the boxes: positions, normals, texture coordinates, joints and weights,
/// and the triangle indices.
#[allow(clippy::type_complexity)]
fn mesh() -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<u8>, Vec<f32>, Vec<u16>) {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut joints = Vec::new();
    let mut weights = Vec::new();
    let mut indices = Vec::new();
    // Each face: its normal and its four corners (counter-clockwise seen from outside), as
    // signs of the half-sizes.
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            X,
            [[1., -1., 1.], [1., -1., -1.], [1., 1., -1.], [1., 1., 1.]],
        ),
        (
            [-1., 0., 0.],
            [
                [-1., -1., -1.],
                [-1., -1., 1.],
                [-1., 1., 1.],
                [-1., 1., -1.],
            ],
        ),
        (
            Y,
            [[-1., 1., 1.], [1., 1., 1.], [1., 1., -1.], [-1., 1., -1.]],
        ),
        (
            [0., -1., 0.],
            [
                [-1., -1., -1.],
                [1., -1., -1.],
                [1., -1., 1.],
                [-1., -1., 1.],
            ],
        ),
        (
            Z,
            [[-1., -1., 1.], [1., -1., 1.], [1., 1., 1.], [-1., 1., 1.]],
        ),
        (
            [0., 0., -1.],
            [
                [1., -1., -1.],
                [-1., -1., -1.],
                [-1., 1., -1.],
                [1., 1., -1.],
            ],
        ),
    ];
    for (joint, centre, size, side_cell, top_cell) in BOXES {
        for (normal, corners) in &faces {
            let cell = if *normal == Y { top_cell } else { side_cell };
            let u = (cell as f32 + 0.5) / CELLS as f32;
            let first = (positions.len() / 3) as u16;
            for corner in corners {
                for axis in 0..3 {
                    positions.push(centre[axis] + corner[axis] * size[axis] / 2.0);
                }
                normals.extend_from_slice(normal);
                uvs.extend_from_slice(&[u, 0.5]);
                joints.extend_from_slice(&[joint as u8, 0, 0, 0]);
                weights.extend_from_slice(&[1.0, 0.0, 0.0, 0.0]);
            }
            indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
        }
    }
    (positions, normals, uvs, joints, weights, indices)
}

/// The base texture: the chalk look (dark hair, pale skin, a blue-grey outfit, dark trim).
fn base_texture() -> Vec<u8> {
    cells_png([0x3a2a22, 0xf1c9a5, 0x5a6b8a, 0x2b2b33].map(super::Rgba::hex))
}

/// A 4x1 PNG of the given cells.
fn cells_png(colors: [super::Rgba; CELLS]) -> Vec<u8> {
    let mut pixmap = Pixmap::new(CELLS, 1, colors[0]);
    for (x, color) in colors.iter().enumerate() {
        pixmap.fill_rect(x as f32, 0.0, 1.0, 1.0, *color);
    }
    pixmap.to_png()
}

/// The skin texture of `variant` for the cube man: the four cells of its manga palette.
pub fn skin(variant: Variant) -> Vec<u8> {
    let palette = palette_of(&Skin::styled(Style::Manga, variant));
    cells_png([palette.hair, palette.skin, palette.outfit, palette.trim])
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

/// The cube man as a glTF binary: one scene, a root node holding the skeleton and the
/// skinned mesh, one material with the embedded base texture, and every [`Clip`].
pub fn glb() -> Vec<u8> {
    let mut bin = Bin::default();
    let mut accessors = Vec::new();
    let (positions, normals, uvs, joints, weights, indices) = mesh();
    let count = positions.len() / 3;
    let mut bounds = ([f32::MAX; 3], [f32::MIN; 3]);
    for (axis, value) in positions.iter().enumerate().map(|(i, v)| (i % 3, *v)) {
        bounds.0[axis] = bounds.0[axis].min(value);
        bounds.1[axis] = bounds.1[axis].max(value);
    }
    let view = bin.view(&f32s(&positions), Some(ARRAY_BUFFER));
    accessors.push(accessor(
        view,
        FLOAT,
        count,
        "VEC3",
        &format!(",\"min\":{},\"max\":{}", list(&bounds.0), list(&bounds.1)),
    ));
    let view = bin.view(&f32s(&normals), Some(ARRAY_BUFFER));
    accessors.push(accessor(view, FLOAT, count, "VEC3", ""));
    let view = bin.view(&f32s(&uvs), Some(ARRAY_BUFFER));
    accessors.push(accessor(view, FLOAT, count, "VEC2", ""));
    let view = bin.view(&joints, Some(ARRAY_BUFFER));
    accessors.push(accessor(view, UNSIGNED_BYTE, count, "VEC4", ""));
    let view = bin.view(&f32s(&weights), Some(ARRAY_BUFFER));
    accessors.push(accessor(view, FLOAT, count, "VEC4", ""));
    let index_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
    let view = bin.view(&index_bytes, Some(ELEMENT_ARRAY_BUFFER));
    accessors.push(accessor(view, UNSIGNED_SHORT, indices.len(), "SCALAR", ""));
    // The inverse bind matrices: the joints carry no rotation in the T-pose, so each is the
    // translation back from the joint (column-major).
    let mut inverse_bind = Vec::new();
    for joint in 0..JOINTS.len() {
        let [x, y, z] = joint_position(joint);
        inverse_bind.extend_from_slice(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -x, -y, -z, 1.0,
        ]);
    }
    let view = bin.view(&f32s(&inverse_bind), None);
    accessors.push(accessor(view, FLOAT, JOINTS.len(), "MAT4", ""));
    let inverse_bind_accessor = accessors.len() - 1;

    let mut animations = Vec::new();
    for clip in Clip::ALL {
        let (duration, channels) = channels_of(clip);
        let times = [0.0, duration / 2.0, duration];
        let view = bin.view(&f32s(&times), None);
        accessors.push(accessor(
            view,
            FLOAT,
            3,
            "SCALAR",
            &format!(",\"min\":[0.0],\"max\":[{duration:?}]"),
        ));
        let input = accessors.len() - 1;
        let mut samplers = Vec::new();
        let mut targets = Vec::new();
        for channel in channels {
            let (kind, width, path) = match channel.path {
                Path::Rotation => ("VEC4", 4, "rotation"),
                Path::Translation => ("VEC3", 3, "translation"),
            };
            let values: Vec<f32> = channel
                .keys
                .iter()
                .flat_map(|key| key[..width].to_vec())
                .collect();
            let view = bin.view(&f32s(&values), None);
            accessors.push(accessor(view, FLOAT, 3, kind, ""));
            let sampler = samplers.len();
            samplers.push(format!(
                "{{\"input\":{input},\"interpolation\":\"LINEAR\",\"output\":{}}}",
                accessors.len() - 1
            ));
            targets.push(format!(
                "{{\"sampler\":{sampler},\"target\":{{\"node\":{},\"path\":\"{path}\"}}}}",
                channel.joint + 1
            ));
        }
        animations.push(format!(
            "{{\"name\":\"{}\",\"channels\":[{}],\"samplers\":[{}]}}",
            clip.name(),
            targets.join(","),
            samplers.join(",")
        ));
    }
    let image_view = bin.view(&base_texture(), None);

    let mesh_node = JOINTS.len() + 1;
    let mut nodes = vec![format!(
        "{{\"name\":\"root\",\"children\":[1,{mesh_node}]}}"
    )];
    for (joint, &(name, _, offset)) in JOINTS.iter().enumerate() {
        let children: Vec<String> = JOINTS
            .iter()
            .enumerate()
            .filter(|(_, (_, p, _))| *p == Some(joint))
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
    let skin_joints: Vec<String> = (1..=JOINTS.len()).map(|n| n.to_string()).collect();

    let json = format!(
        concat!(
            "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"{generator}\"}},",
            "\"scene\":0,\"scenes\":[{{\"name\":\"cube man\",\"nodes\":[0]}}],",
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
            "\"images\":[{{\"name\":\"cells\",\"mimeType\":\"image/png\",\"bufferView\":{image}}}],",
            "\"samplers\":[{{\"magFilter\":9728,\"minFilter\":9728,\"wrapS\":33071,\"wrapT\":33071}}],",
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
