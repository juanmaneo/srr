//! The third-person 3D view of the fight: the camera over the shoulder of the local fighter,
//! like the story-mode brawls of Tekken, in an old-school software renderer.
//!
//! The scene is the same alley as the side view ([`super::arena`]), extruded: the tagged brick
//! wall at the back, the pavement the fighters stand on, the kerb and the street the camera
//! floats over. The stick figures ([`super::stick`]) become wooden mannequins: every bone is a
//! tube, every joint a ball, the head a sphere, Pinocchio style, with the same poses, accessories
//! and marks as the 2D figures they are built from.
//!
//! The look is a comic book on a console of the nineties: flat two-tone shading (a lit side and
//! a shade band on every tube and ball), dark ink outlines, and everything drawn in integer
//! fixed-point maths ([`super::space`]) with the rows and vertices snapped to a [`GRAIN`] of
//! pixels, so the picture has the grain of the time. Like every screen of the game, the result
//! is nothing but axis-aligned rectangles and labels in a [`Frame`], so every engine and the
//! headless renderer draw it unchanged.
//!
//! World units are the pixels of the side view: `x` along the arena (`0` at the left bound),
//! `y` up from the pavement, `z` towards the wall. Fighters stand on the line they sidestepped to
//! ([`depth`]), `z = 0` being the middle line where everybody starts; the camera follows the
//! local fighter's line.

use std::cmp::Reverse;

use super::arena::{ARENA_LEFT, ARENA_PIXELS, FLOOR_Y, feet, player_color, scale};
use super::ascii::{block_pixels, hash};
use super::hud::tag;
use super::scene::{Align, Frame, Rgba, SCREEN_HEIGHT, SCREEN_WIDTH};
use super::space::{Camera, Fx, NEAR, Vec3, div_round, perspective, screen_point};
use super::stick::{self, Accessory, Figure, HEAD_RADIUS, Mark, Point};
use crate::game::{Fighter, MatchConfig};
use crate::net::Client;

/// Pixels per grain: the rows of every fill and the runs of every ink line are this thick and
/// aligned on multiples of it.
pub const GRAIN: i32 = 2;
/// The colour of the sky (the frame's background) and of the fog in the distance.
pub const BACKGROUND: Rgba = Rgba::hex(0x0b0910);
/// The ink of the outlines.
pub const INK: Rgba = Rgba::hex(0x0b0812);
/// The wood of the mannequins.
pub const WOOD: Rgba = Rgba::hex(0xe0b070);
/// How much darker the shade side of a tube or a ball is.
pub const SHADE: f32 = 0.55;
/// The camera moves `1 / 2^CAMERA_LAG` of the way to where it should be on every tick.
pub const CAMERA_LAG: u32 = 3;

/// The camera: this far behind the local fighter...
pub const BACK: i32 = 220;
/// ...this high...
pub const HEIGHT: i32 = 160;
/// ...this far on the street side (negative `z`)...
pub const SIDE: i32 = 270;
/// ...looking this far ahead of the fighter...
pub const AHEAD: i32 = 180;
/// ...at this height.
pub const TARGET_HEIGHT: i32 = 80;

/// `z` of the back wall: past the deepest sidestep towards it.
pub const WALL_Z: i32 = 180;
/// Height of the back wall (the side view's wall reaches the top of its screen).
pub const WALL_HEIGHT: i32 = 504;
/// `z` of the kerb: the pavement ends there, past the deepest sidestep towards the street.
pub const KERB_Z: i32 = -180;
/// How far below the pavement the street is.
pub const STREET_DROP: i32 = 10;
/// `z` of the far edge of the street.
pub const STREET_FAR_Z: i32 = -1000;
/// `x` of the left cross wall closing the alley.
pub const ALLEY_LEFT: i32 = -1600;
/// `x` of the right cross wall closing the alley.
pub const ALLEY_RIGHT: i32 = 2700;

const LANE_Z: i32 = -240;
const COURSE: i32 = 42;
const BLOCK: i32 = 96;
const TAG_CELL: i32 = 30;
const ARENA_WIDTH: i32 = ARENA_PIXELS as i32;

const STREET: Rgba = Rgba::hex(0x15121c);
const LANE: Rgba = Rgba::hex(0x4a4258);
const KERB: Rgba = Rgba::hex(0x3a3448);
const PAVEMENT: Rgba = Rgba::hex(0x221c2e);
const BRICK: Rgba = Rgba::hex(0x2a2036);
const MORTAR: Rgba = Rgba::hex(0x1c1626);
const BOUND: Rgba = Rgba::hex(0x6a4dff);
const NEON: Rgba = Rgba::hex(0x3fd0ff);
const SIGN: Rgba = Rgba::hex(0xff2bd6);
const MARKER: Rgba = Rgba::hex(0xfff299);
const SHADOW: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.45,
};
/// The rows of the tag on the wall, hot like the title's.
const HOT: [Rgba; 5] = [
    Rgba::hex(0xfff35c),
    Rgba::hex(0xffc93a),
    Rgba::hex(0xff8a25),
    Rgba::hex(0xff4a1a),
    Rgba::hex(0xd81e1e),
];

/// Where the light comes from on screen (up and left), for the shade of tubes and balls.
const LIGHT: (i64, i64) = (-3, -4);
/// Where the light comes from in the world (left, above, from the street), for the planes.
const LIGHT_3D: Vec3 = Vec3::int(-2, 3, -1);
/// The shade band starts this fraction of the radius past the axis, towards the dark side.
const TERMINATOR: i64 = 4;

/// Radii of the mannequin's parts, in world units.
const TORSO: i32 = 7;
const ARM: i32 = 5;
const LEG: i32 = 6;
const JOINT: i32 = 7;
const HAND: i32 = 5;
const FOOT: i32 = 6;
const SHOULDER: i32 = 8;
const HEAD: i32 = 12;
/// The front limbs are this much towards the camera, the back ones this much away.
const LIMB_Z: i32 = 5;
/// Parts thinner than this on screen are drawn without their outline (it would be all ink).
const INKED_RADIUS: i32 = 4;

/// Cosines of the twelve directions of a disc, in twelfths of a turn, as [`Fx`] values.
const COS: [i32; 12] = [
    4096, 3547, 2048, 0, -2048, -3547, -4096, -3547, -2048, 0, 2048, 3547,
];

fn cos(twelfth: usize) -> i64 {
    COS[twelfth % 12] as i64
}

fn sin(twelfth: usize) -> i64 {
    COS[(twelfth + 9) % 12] as i64
}

/// A pixel of the virtual screen.
pub type Px = (i32, i32);

const SCREEN_W: i32 = SCREEN_WIDTH as i32;
const SCREEN_H: i32 = SCREEN_HEIGHT as i32;
/// Ink lines are cut this far outside the screen, so their runs stay small.
const MARGIN: i64 = 4;

fn snap(value: i64) -> i64 {
    value.div_euclid(GRAIN as i64) * GRAIN as i64
}

/// Adds the rectangle `x0..x1` x `y0..y1`, cut to the screen, unless nothing is left.
fn grain_rect(frame: &mut Frame, x0: i64, y0: i64, x1: i64, y1: i64, color: Rgba) {
    let (x0, y0) = (x0.max(0), y0.max(0));
    let (x1, y1) = (x1.min(SCREEN_W as i64), y1.min(SCREEN_H as i64));
    if x0 < x1 && y0 < y1 {
        frame.rect(
            x0 as f32,
            y0 as f32,
            (x1 - x0) as f32,
            (y1 - y0) as f32,
            color,
        );
    }
}

/// The world point of the side view's screen point `point` at depth `z`.
pub fn lift(point: Point, z: i32) -> Vec3 {
    Vec3::new(
        Fx::from_f32(point.x - ARENA_LEFT),
        Fx::from_f32(FLOOR_Y - point.y),
        Fx::int(z),
    )
}

/// The camera showing the whole arena from the street, when no match runs.
pub fn overview() -> Camera {
    Camera::look_at(Vec3::int(550, 260, -620), Vec3::int(550, 70, 0))
}

/// Where the camera should be: behind the local fighter, over the shoulder, on the street
/// side, looking past them towards the other fighters (towards where they face when they are
/// right in the middle of the others). [`overview`] when no match runs.
pub fn aim(client: &Client) -> Camera {
    let Some(session) = client.session() else {
        return overview();
    };
    let current = session.current();
    let config = current.config();
    let fighters = current.fighters();
    let local = current
        .fighter(session.local_player())
        .unwrap_or(&fighters[0]);
    let (x, _) = feet(local, config);
    let z = depth(local, config);
    let others: Vec<f32> = fighters
        .iter()
        .filter(|f| f.player() != local.player())
        .map(|f| feet(f, config).0)
        .collect();
    let mean = others.iter().sum::<f32>() / others.len().max(1) as f32;
    let ahead = if mean > x {
        1
    } else if mean < x {
        -1
    } else {
        local.facing().sign()
    };
    let x = (x - ARENA_LEFT) as i32;
    Camera::look_at(
        Vec3::int(x - ahead * BACK, HEIGHT, z - SIDE),
        Vec3::int(x + ahead * AHEAD, TARGET_HEIGHT, z),
    )
}

/// The world `z` of `fighter`: how far it sidestepped, in world units (towards the wall when
/// positive).
///
/// ```
/// use srr::front::tps::depth;
/// use srr::game::{Match, MatchConfig, PlayerId, Roster};
///
/// let m = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
/// assert_eq!(depth(m.fighter(PlayerId(0)).unwrap(), m.config()), 0, "on the middle line");
/// ```
pub fn depth(fighter: &Fighter, config: &MatchConfig) -> i32 {
    (fighter.z() as f32 * scale(config)).round() as i32
}

// ---------------------------------------------------------------------------------------------
// Screen-space rasterisers
// ---------------------------------------------------------------------------------------------

/// Fills the convex polygon `points` with rows of [`GRAIN`] pixels (each row covers what the
/// polygon covers at its middle, snapped to the grain). Rows and runs outside the screen are
/// dropped.
pub fn fill_polygon(frame: &mut Frame, points: &[Px], color: Rgba) {
    if points.len() < 3 {
        return;
    }
    let g = GRAIN as i64;
    let top = points.iter().map(|p| p.1 as i64).min().unwrap_or(0);
    let bottom = points.iter().map(|p| p.1 as i64).max().unwrap_or(0);
    let mut y = snap(top.max(0));
    let bottom = bottom.min(SCREEN_H as i64);
    while y < bottom {
        // Doubled coordinates: the middle of the row is a whole number.
        let middle = 2 * y + g;
        let (mut left, mut right) = (i64::MAX, i64::MIN);
        for (i, p) in points.iter().enumerate() {
            let q = points[(i + 1) % points.len()];
            let (px, py, qx, qy) = (p.0 as i64, p.1 as i64, q.0 as i64, q.1 as i64);
            if middle < 2 * py.min(qy) || middle >= 2 * py.max(qy) {
                continue;
            }
            let x = px + div_round((middle - 2 * py) * (qx - px), 2 * (qy - py));
            left = left.min(x);
            right = right.max(x);
        }
        if left <= right {
            grain_rect(frame, snap(left), y, snap(right + g - 1), y + g, color);
        }
        y += g;
    }
}

/// The part of the convex polygon `points` on the left of the line `a` → `b` (y down: the side
/// where the cross product is positive), the line included.
pub fn clip_half_plane(points: &[Px], a: Px, b: Px) -> Vec<Px> {
    let side =
        |p: Px| (b.0 - a.0) as i64 * (p.1 - a.1) as i64 - (b.1 - a.1) as i64 * (p.0 - a.0) as i64;
    let mut kept = Vec::with_capacity(points.len() + 1);
    for (i, &p) in points.iter().enumerate() {
        let q = points[(i + 1) % points.len()];
        let (sp, sq) = (side(p), side(q));
        if sp >= 0 {
            kept.push(p);
        }
        if (sp >= 0) != (sq >= 0) {
            kept.push((
                p.0 + div_round((q.0 - p.0) as i64 * sp, sp - sq) as i32,
                p.1 + div_round((q.1 - p.1) as i64 * sp, sp - sq) as i32,
            ));
        }
    }
    kept
}

/// The part of the segment `a` → `b` within the screen (plus a small margin), if any.
pub fn clip_segment(a: Px, b: Px) -> Option<(Px, Px)> {
    let mut a = (a.0 as i64, a.1 as i64);
    let mut b = (b.0 as i64, b.1 as i64);
    // Each bound as `nx * x + ny * y + c >= 0`.
    let bounds = [
        (1, 0, MARGIN),
        (-1, 0, SCREEN_W as i64 + MARGIN),
        (0, 1, MARGIN),
        (0, -1, SCREEN_H as i64 + MARGIN),
    ];
    for (nx, ny, c) in bounds {
        let da = nx * a.0 + ny * a.1 + c;
        let db = nx * b.0 + ny * b.1 + c;
        if da < 0 && db < 0 {
            return None;
        }
        if da < 0 {
            a = cut(a, b, da, db);
        } else if db < 0 {
            b = cut(b, a, db, da);
        }
    }
    Some(((a.0 as i32, a.1 as i32), (b.0 as i32, b.1 as i32)))
}

/// The point of `out` → `inside` on the bound, from their signed distances to it.
fn cut(out: (i64, i64), inside: (i64, i64), d_out: i64, d_in: i64) -> (i64, i64) {
    (
        out.0 + div_round((inside.0 - out.0) * -d_out, d_in - d_out),
        out.1 + div_round((inside.1 - out.1) * -d_out, d_in - d_out),
    )
}

/// Draws the segment `a` → `b` as runs of [`GRAIN`] pixels along its major axis, snapped to
/// the grain: a line one grain thick, stepped like a pixel line. Clipped to the screen first.
pub fn ink_line(frame: &mut Frame, a: Px, b: Px, color: Rgba) {
    let Some((a, b)) = clip_segment(a, b) else {
        return;
    };
    let g = GRAIN as i64;
    let (dx, dy) = ((b.0 - a.0) as i64, (b.1 - a.1) as i64);
    if dx.abs() >= dy.abs() {
        let (a, b) = if a.0 <= b.0 { (a, b) } else { (b, a) };
        let (dx, dy) = ((b.0 - a.0) as i64, (b.1 - a.1) as i64);
        let runs = ((dy.abs() + g - 1) / g).max(1);
        for i in 0..runs {
            let x0 = snap(a.0 as i64 + div_round(dx * i, runs));
            let x1 = snap(a.0 as i64 + div_round(dx * (i + 1), runs));
            let y = snap(a.1 as i64 + div_round(dy * (2 * i + 1), 2 * runs));
            grain_rect(frame, x0, y, x1 + g, y + g, color);
        }
    } else {
        let (a, b) = if a.1 <= b.1 { (a, b) } else { (b, a) };
        let (dx, dy) = ((b.0 - a.0) as i64, (b.1 - a.1) as i64);
        let runs = ((dx.abs() + g - 1) / g).max(1);
        for i in 0..runs {
            let y0 = snap(a.1 as i64 + div_round(dy * i, runs));
            let y1 = snap(a.1 as i64 + div_round(dy * (i + 1), runs));
            let x = snap(a.0 as i64 + div_round(dx * (2 * i + 1), 2 * runs));
            grain_rect(frame, x, y0, x + g, y1 + g, color);
        }
    }
}

/// A polygon with the lit side of a disc of `radius` around `centre`.
fn disc(centre: Px, radius: i64) -> Vec<Px> {
    (0..12)
        .map(|k| {
            (
                centre.0 + div_round(cos(k) * radius, Fx::ONE.0 as i64) as i32,
                centre.1 + div_round(sin(k) * radius, Fx::ONE.0 as i64) as i32,
            )
        })
        .collect()
}

/// A ball on screen: a disc in the shade, a smaller lit disc towards the light, the outline.
pub fn ball_shape(frame: &mut Frame, centre: Px, radius: i32, color: Rgba) {
    let r = radius as i64;
    let outline = disc(centre, r);
    fill_polygon(frame, &outline, color.scaled(SHADE));
    let lit = (
        centre.0 + div_round(LIGHT.0 * r, 20) as i32,
        centre.1 + div_round(LIGHT.1 * r, 20) as i32,
    );
    fill_polygon(frame, &disc(lit, r * 7 / 10), color);
    if radius >= INKED_RADIUS {
        for (i, &p) in outline.iter().enumerate() {
            ink_line(frame, p, outline[(i + 1) % outline.len()], INK);
        }
    }
}

/// A tube on screen, from `a` (radius `ra`) to `b` (radius `rb`): a capsule, lit, with a shade
/// band along its dark side, and the outline.
pub fn tube_shape(frame: &mut Frame, a: Px, ra: i32, b: Px, rb: i32, color: Rgba) {
    let (dx, dy) = ((b.0 - a.0) as i64, (b.1 - a.1) as i64);
    let length = (dx * dx + dy * dy).isqrt();
    if length == 0 {
        ball_shape(frame, a, ra.max(rb), color);
        return;
    }
    let one = Fx::ONE.0 as i64;
    // The axis and its normal, unit vectors in fixed point.
    let (ux, uy) = (dx * one / length, dy * one / length);
    let (nx, ny) = (-uy, ux);
    let around = |p: Px, r: i32, k: usize| -> Px {
        let (c, s) = (cos(k), sin(k));
        (
            p.0 + div_round((ux * c + nx * s) * r as i64, one * one) as i32,
            p.1 + div_round((uy * c + ny * s) * r as i64, one * one) as i32,
        )
    };
    // The cap around `a` bulges away from `b` (a quarter turn to three quarters), then the
    // cap around `b`.
    let mut capsule: Vec<Px> = (0..=6).map(|k| around(a, ra, 3 + k)).collect();
    capsule.extend((0..=6).map(|k| around(b, rb, 9 + k)));
    fill_polygon(frame, &capsule, color);

    // The shade: the capsule past a line parallel to the axis, on the side away from the light.
    let (dark_x, dark_y) = if nx * LIGHT.0 + ny * LIGHT.1 > 0 {
        (-nx, -ny)
    } else {
        (nx, ny)
    };
    let past = |p: Px, r: i32| {
        (
            p.0 + div_round(dark_x * r as i64, TERMINATOR * one) as i32,
            p.1 + div_round(dark_y * r as i64, TERMINATOR * one) as i32,
        )
    };
    let (qa, qb) = (past(a, ra), past(b, rb));
    let towards_dark = (qb.0 - qa.0) as i64 * dark_y - (qb.1 - qa.1) as i64 * dark_x;
    let (la, lb) = if towards_dark >= 0 {
        (qa, qb)
    } else {
        (qb, qa)
    };
    fill_polygon(
        frame,
        &clip_half_plane(&capsule, la, lb),
        color.scaled(SHADE),
    );

    if ra.min(rb) >= INKED_RADIUS {
        for (i, &p) in capsule.iter().enumerate() {
            ink_line(frame, p, capsule[(i + 1) % capsule.len()], INK);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// World-space drawing
// ---------------------------------------------------------------------------------------------

/// The polygon `points` (in the camera's frame) cut at the near plane. Every vertex comes with
/// whether the cut created it: an edge between two created vertices lies on the near plane and
/// is no edge of the original polygon.
pub fn clip_near(points: &[Vec3]) -> Vec<(Vec3, bool)> {
    let mut kept = Vec::with_capacity(points.len() + 1);
    for (i, &p) in points.iter().enumerate() {
        let q = points[(i + 1) % points.len()];
        let (p_in, q_in) = (p.z >= NEAR, q.z >= NEAR);
        if p_in {
            kept.push((p, false));
        }
        if p_in != q_in {
            let cut = |pc: Fx, qc: Fx| {
                Fx(pc.0
                    + div_round(
                        (qc.0 - pc.0) as i64 * (NEAR.0 - p.z.0) as i64,
                        (q.z.0 - p.z.0) as i64,
                    ) as i32)
            };
            kept.push((Vec3::new(cut(p.x, q.x), cut(p.y, q.y), NEAR), true));
        }
    }
    kept
}

/// Draws the convex polygon `corners` of the world, filled, with its edges in `ink` if given.
pub fn draw_polygon(
    frame: &mut Frame,
    camera: &Camera,
    corners: &[Vec3],
    fill: Rgba,
    ink: Option<Rgba>,
) {
    let in_camera: Vec<Vec3> = corners.iter().map(|&c| camera.to_camera(c)).collect();
    let clipped = clip_near(&in_camera);
    if clipped.len() < 3 {
        return;
    }
    let screen: Vec<Px> = clipped.iter().map(|&(c, _)| screen_point(c)).collect();
    fill_polygon(frame, &screen, fill);
    if let Some(ink) = ink {
        for (i, &p) in screen.iter().enumerate() {
            let j = (i + 1) % screen.len();
            if clipped[i].1 && clipped[j].1 {
                continue;
            }
            ink_line(frame, p, screen[j], ink);
        }
    }
}

/// Draws the segment `a` → `b` of the world as an ink line.
pub fn draw_line(frame: &mut Frame, camera: &Camera, a: Vec3, b: Vec3, color: Rgba) {
    let clipped = clip_near(&[camera.to_camera(a), camera.to_camera(b)]);
    if clipped.len() < 2 {
        return;
    }
    ink_line(
        frame,
        screen_point(clipped[0].0),
        screen_point(clipped[1].0),
        color,
    );
}

/// Draws a tube of the world from `a` to `b`, skipped when an end is too close to the eye.
pub fn draw_tube(frame: &mut Frame, camera: &Camera, a: Vec3, b: Vec3, radius: i32, color: Rgba) {
    let (ca, cb) = (camera.to_camera(a), camera.to_camera(b));
    if ca.z < NEAR || cb.z < NEAR {
        return;
    }
    let r = Fx::int(radius);
    tube_shape(
        frame,
        screen_point(ca),
        perspective(r, ca.z).max(1),
        screen_point(cb),
        perspective(r, cb.z).max(1),
        color,
    );
}

/// Draws a ball of the world, skipped when too close to the eye.
pub fn draw_ball(frame: &mut Frame, camera: &Camera, centre: Vec3, radius: i32, color: Rgba) {
    let c = camera.to_camera(centre);
    if c.z < NEAR {
        return;
    }
    ball_shape(
        frame,
        screen_point(c),
        perspective(Fx::int(radius), c.z).max(1),
        color,
    );
}

/// How lit a plane of `normal` is: full, a bit in the shade, or in the shade (three flat tones).
pub fn tone(normal: Vec3) -> f32 {
    let facing = normal.normalized().dot(LIGHT_3D.normalized());
    if facing > Fx::HALF {
        1.0
    } else if facing > Fx::ZERO {
        0.8
    } else {
        0.6
    }
}

/// `color` seen at `depth`: fading into the [`BACKGROUND`] in three steps.
pub fn fog(color: Rgba, depth: Fx) -> Rgba {
    if depth < Fx::int(800) {
        color
    } else if depth < Fx::int(1600) {
        color.mix(BACKGROUND, 0.35)
    } else {
        color.mix(BACKGROUND, 0.6)
    }
}

// ---------------------------------------------------------------------------------------------
// The scene
// ---------------------------------------------------------------------------------------------

/// Draws the alley and, when a match runs, its fighters as mannequins, with their shadows and
/// name tags (the local fighter's marked with a triangle).
pub fn draw(frame: &mut Frame, client: &Client, camera: &Camera, fight_ticks: u32) {
    draw_alley(frame, camera, fight_ticks);
    let Some(session) = client.session() else {
        return;
    };
    let current = session.current();
    let config = current.config();

    for fighter in current.fighters() {
        let (x, _) = feet(fighter, config);
        let (rx, rz) = if fighter.y() > 0 { (18, 7) } else { (32, 12) };
        let z = Fx::int(depth(fighter, config));
        let centre = Vec3::new(Fx::from_f32(x - ARENA_LEFT), Fx::int(1), z);
        let ellipse: Vec<Vec3> = (0..12)
            .map(|k| centre + Vec3::new(Fx(cos(k) as i32 * rx), Fx::ZERO, Fx(sin(k) as i32 * rz)))
            .collect();
        draw_polygon(frame, camera, &ellipse, SHADOW, None);
    }

    // Painter's order: the farthest part of any fighter first.
    let mut parts: Vec<Part> = current
        .fighters()
        .iter()
        .flat_map(|fighter| {
            let figure = Figure::of(fighter, config, fight_ticks);
            mannequin(&figure, depth(fighter, config), camera)
        })
        .collect();
    parts.sort_by_key(|part| Reverse(part.depth));
    for part in parts {
        match part.shape {
            Shape::Tube {
                a,
                b,
                radius,
                color,
            } => draw_tube(frame, camera, a, b, radius, fog(color, part.depth)),
            Shape::Ball {
                centre,
                radius,
                color,
            } => draw_ball(frame, camera, centre, radius, fog(color, part.depth)),
        }
    }

    for fighter in current.fighters() {
        let (x, y) = feet(fighter, config);
        let figure = Figure::of(fighter, config, fight_ticks);
        let z = depth(fighter, config);
        // Name tags float over the head, or lie on the pavement in front of a grounded fighter.
        let anchor = if figure.lying {
            lift(Point { x, y }, z - 40)
        } else {
            lift(
                Point {
                    x,
                    y: figure.top() - 8.0,
                },
                z,
            )
        };
        if let Some(p) = camera.project(anchor) {
            if fighter.player() == session.local_player() {
                let marker = [(p.x - 12, p.y - 26), (p.x + 12, p.y - 26), (p.x, p.y - 12)];
                fill_polygon(frame, &marker, MARKER);
                for (i, &corner) in marker.iter().enumerate() {
                    ink_line(frame, corner, marker[(i + 1) % 3], INK);
                }
            }
            frame.label(
                tag(client, fighter.player()),
                p.x as f32,
                (p.y - 46) as f32,
                14.0,
                player_color(fighter.player()),
                Align::Center,
            );
        }
    }
}

/// The alley, from the street to the tagged wall: the big planes first, then what lies on
/// them, then the lights.
fn draw_alley(frame: &mut Frame, camera: &Camera, fight_ticks: u32) {
    let (l, r) = (ALLEY_LEFT, ALLEY_RIGHT);
    let street_y = -STREET_DROP;
    let street = [
        Vec3::int(l, street_y, KERB_Z),
        Vec3::int(r, street_y, KERB_Z),
        Vec3::int(r, street_y, STREET_FAR_Z),
        Vec3::int(l, street_y, STREET_FAR_Z),
    ];
    draw_polygon(frame, camera, &street, STREET.scaled(tone(Vec3::Y)), None);
    let mut x = l + 20;
    while x < r {
        let dash = [
            Vec3::int(x, street_y + 1, LANE_Z + 4),
            Vec3::int(x + 70, street_y + 1, LANE_Z + 4),
            Vec3::int(x + 70, street_y + 1, LANE_Z - 4),
            Vec3::int(x, street_y + 1, LANE_Z - 4),
        ];
        draw_polygon(frame, camera, &dash, LANE, None);
        x += 140;
    }
    let kerb = [
        Vec3::int(l, street_y, KERB_Z),
        Vec3::int(r, street_y, KERB_Z),
        Vec3::int(r, 0, KERB_Z),
        Vec3::int(l, 0, KERB_Z),
    ];
    draw_polygon(frame, camera, &kerb, KERB, None);
    let pavement = [
        Vec3::int(l, 0, KERB_Z),
        Vec3::int(r, 0, KERB_Z),
        Vec3::int(r, 0, WALL_Z),
        Vec3::int(l, 0, WALL_Z),
    ];
    draw_polygon(
        frame,
        camera,
        &pavement,
        PAVEMENT.scaled(tone(Vec3::Y)),
        None,
    );
    for x in [0, ARENA_WIDTH] {
        let stripe = [
            Vec3::int(x - 3, 1, KERB_Z),
            Vec3::int(x + 3, 1, KERB_Z),
            Vec3::int(x + 3, 1, WALL_Z),
            Vec3::int(x - 3, 1, WALL_Z),
        ];
        draw_polygon(frame, camera, &stripe, BOUND.scaled(0.7), None);
    }

    let wall = [
        Vec3::int(l, 0, WALL_Z),
        Vec3::int(r, 0, WALL_Z),
        Vec3::int(r, WALL_HEIGHT, WALL_Z),
        Vec3::int(l, WALL_HEIGHT, WALL_Z),
    ];
    draw_polygon(
        frame,
        camera,
        &wall,
        BRICK.scaled(tone(-Vec3::Z)),
        Some(INK),
    );
    for (x, normal) in [(l, Vec3::X), (r, -Vec3::X)] {
        let cross_wall = [
            Vec3::int(x, 0, STREET_FAR_Z),
            Vec3::int(x, 0, WALL_Z),
            Vec3::int(x, WALL_HEIGHT, WALL_Z),
            Vec3::int(x, WALL_HEIGHT, STREET_FAR_Z),
        ];
        draw_polygon(
            frame,
            camera,
            &cross_wall,
            BRICK.scaled(tone(normal)),
            Some(INK),
        );
    }
    // The courses of blocks, their vertical joints staggered.
    let z = WALL_Z - 1;
    for course in 0..WALL_HEIGHT / COURSE {
        let y = course * COURSE;
        if course > 0 {
            draw_line(
                frame,
                camera,
                Vec3::int(l, y, z),
                Vec3::int(r, y, z),
                MORTAR,
            );
        }
        let mut x = l + if course % 2 == 0 { 0 } else { BLOCK / 2 };
        while x < r {
            draw_line(
                frame,
                camera,
                Vec3::int(x, y, z),
                Vec3::int(x, y + COURSE, z),
                MORTAR,
            );
            x += BLOCK;
        }
    }

    // The tag, sprayed across the wall in the block font of the title, with its drop shadow.
    let (pixels, width) = block_pixels("SRR");
    let left = ARENA_WIDTH / 2 - width * TAG_CELL / 2;
    let top = WALL_HEIGHT - 70;
    let z = WALL_Z - 2;
    for (x, y) in pixels {
        let color = HOT[(y as usize).min(HOT.len() - 1)];
        let cell = |dx: i32, dy: i32| {
            let (x0, y1) = (left + x * TAG_CELL + dx, top - y * TAG_CELL - dy);
            [
                Vec3::int(x0, y1 - TAG_CELL, z),
                Vec3::int(x0 + TAG_CELL, y1 - TAG_CELL, z),
                Vec3::int(x0 + TAG_CELL, y1, z),
                Vec3::int(x0, y1, z),
            ]
        };
        draw_polygon(frame, camera, &cell(6, 6), INK, None);
        draw_polygon(frame, camera, &cell(0, 0), color, None);
    }

    // The neon strip along the wall flickers now and then; the sign glows steadily.
    let flicker = hash(fight_ticks as i32 / 6, 3, 11).is_multiple_of(9);
    let neon = if flicker { NEON.scaled(0.3) } else { NEON };
    let z = WALL_Z - 4;
    let strip = |y0: i32, y1: i32| {
        [
            Vec3::int(30, y0, z),
            Vec3::int(250, y0, z),
            Vec3::int(250, y1, z),
            Vec3::int(30, y1, z),
        ]
    };
    draw_polygon(frame, camera, &strip(248, 272), neon.with_alpha(0.12), None);
    draw_polygon(frame, camera, &strip(257, 263), neon, None);
    let sign = |x0: i32, x1: i32| {
        [
            Vec3::int(x0, 120, z),
            Vec3::int(x1, 120, z),
            Vec3::int(x1, 300, z),
            Vec3::int(x0, 300, z),
        ]
    };
    draw_polygon(frame, camera, &sign(938, 962), SIGN.with_alpha(0.12), None);
    draw_polygon(frame, camera, &sign(947, 953), SIGN, None);
}

// ---------------------------------------------------------------------------------------------
// The mannequins
// ---------------------------------------------------------------------------------------------

/// One piece of a mannequin, with its distance from the camera for the painter's order.
struct Part {
    depth: Fx,
    shape: Shape,
}

enum Shape {
    Tube {
        a: Vec3,
        b: Vec3,
        radius: i32,
        color: Rgba,
    },
    Ball {
        centre: Vec3,
        radius: i32,
        color: Rgba,
    },
}

struct Sculpt<'a> {
    camera: &'a Camera,
    parts: Vec<Part>,
}

impl Sculpt<'_> {
    fn tube(&mut self, a: Vec3, b: Vec3, radius: i32, color: Rgba) {
        self.parts.push(Part {
            depth: self.camera.to_camera((a + b) >> 1).z,
            shape: Shape::Tube {
                a,
                b,
                radius,
                color,
            },
        });
    }

    fn ball(&mut self, centre: Vec3, radius: i32, color: Rgba) {
        self.parts.push(Part {
            depth: self.camera.to_camera(centre).z,
            shape: Shape::Ball {
                centre,
                radius,
                color,
            },
        });
    }
}

/// The direction from the neck to the head, and the one the face looks to (see
/// [`Figure`]'s accessories, drawn with the same geometry).
fn head_axes(figure: &Figure) -> (Point, Point) {
    let (dx, dy) = (figure.head.x - figure.neck.x, figure.head.y - figure.neck.y);
    let length = dx.hypot(dy).max(f32::EPSILON);
    let up = Point {
        x: dx / length,
        y: dy / length,
    };
    let front = Point {
        x: -up.y * figure.facing,
        y: up.x * figure.facing,
    };
    (up, front)
}

/// The wooden mannequin of `figure`: a tube per bone, a ball per joint, the head a sphere, the
/// accessory and the marks in relief, standing on the line `depth`.
fn mannequin(figure: &Figure, depth: i32, camera: &Camera) -> Vec<Part> {
    let lift = |point: Point, z: i32| self::lift(point, depth + z);
    let mut s = Sculpt {
        camera,
        parts: Vec::new(),
    };
    let base = if figure.ink == stick::INK {
        WOOD
    } else {
        figure.ink
    };
    s.tube(lift(figure.neck, 0), lift(figure.hip, 0), TORSO, base);
    s.ball(lift(figure.head, 0), HEAD, base);
    s.ball(lift(figure.shoulder, 0), SHOULDER, base);
    s.ball(lift(figure.hip, 0), SHOULDER, base);
    let limbs = [
        (figure.shoulder, &figure.arms, ARM, HAND),
        (figure.hip, &figure.legs, LEG, FOOT),
    ];
    for (from, bones, radius, end) in limbs {
        for (index, bone) in bones.iter().enumerate() {
            // The front limb is the one on the camera's side.
            let z = if index == 0 { -LIMB_Z } else { LIMB_Z };
            let color = bone.ink.unwrap_or(base);
            s.tube(lift(from, z), lift(bone.joint, z), radius, color);
            s.ball(lift(bone.joint, z), JOINT, color);
            s.tube(lift(bone.joint, z), lift(bone.end, z), radius, color);
            s.ball(lift(bone.end, z), end, color);
        }
    }

    let (up, front) = head_axes(figure);
    let at = |along: f32, ahead: f32| Point {
        x: figure.head.x + up.x * along + front.x * ahead,
        y: figure.head.y + up.y * along + front.y * ahead,
    };
    let r = HEAD_RADIUS;
    let accent = figure.accent;
    match figure.accessory {
        Accessory::Hat => {
            s.tube(
                lift(at(r - 1.0, -r - 5.0), 0),
                lift(at(r - 1.0, r + 5.0), 0),
                2,
                accent,
            );
            s.tube(
                lift(at(r, 0.0), 0),
                lift(at(r + 12.0, 0.0), 0),
                HEAD - 2,
                accent,
            );
        }
        Accessory::Beret => {
            s.tube(
                lift(at(r - 2.0, -r - 3.0), 0),
                lift(at(r + 2.0, r - 3.0), 0),
                4,
                accent,
            );
            s.tube(
                lift(at(r + 4.0, -2.0), 0),
                lift(at(r + 8.0, -1.0), 0),
                2,
                accent,
            );
        }
        Accessory::Ponytail => {
            s.tube(
                lift(at(r - 1.0, -4.0), 0),
                lift(at(4.0, -r - 6.0), 0),
                2,
                accent,
            );
            s.tube(
                lift(at(4.0, -r - 6.0), 0),
                lift(at(-10.0, -r - 10.0), 0),
                2,
                accent,
            );
        }
        Accessory::Headband => {
            let band_z = -(HEAD * 9 / 10);
            s.tube(
                lift(at(4.0, -r - 1.0), band_z),
                lift(at(4.0, r + 1.0), band_z),
                2,
                accent,
            );
            s.tube(
                lift(at(4.0, -r), 0),
                lift(at(-2.0, -r - 12.0), 0),
                2,
                accent,
            );
            s.tube(lift(at(5.0, -r), 0), lift(at(7.0, -r - 13.0), 0), 2, accent);
        }
    }

    match figure.mark {
        Mark::None => {}
        Mark::Stunned => {
            for (along, ahead) in [(0.9_f32, 0.45_f32), (0.6, 0.8), (0.15, 1.0)] {
                let length = along.hypot(ahead);
                let (along, ahead) = (along / length, ahead / length);
                s.tube(
                    lift(at(along * (r + 5.0), ahead * (r + 5.0)), -8),
                    lift(at(along * (r + 13.0), ahead * (r + 13.0)), -8),
                    1,
                    stick::SPARK,
                );
            }
        }
        Mark::KnockedOut => {
            for ahead in [-1.0, 6.0] {
                let eye = at(2.0, ahead);
                let corner = |dx: f32, dy: f32| {
                    lift(
                        Point {
                            x: eye.x + dx,
                            y: eye.y + dy,
                        },
                        -HEAD,
                    )
                };
                s.tube(corner(-2.5, -2.5), corner(2.5, 2.5), 1, INK);
                s.tube(corner(-2.5, 2.5), corner(2.5, -2.5), 1, INK);
            }
        }
    }
    s.parts
}
