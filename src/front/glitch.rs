//! The glitches you can see. The rules of the intentional glitches live in `game`; this is
//! what the fight looks like while they happen, in the arena of the stick figures and in the
//! 3D view alike (both draw it with the geometry below, so one glitch looks the same in 2D and
//! in 3D):
//!
//! * **Echo barrage**: while a victim has echo hits left to rain ([`Fighter::echoes`]), lag
//!   copies of it ring around it, turning, flickering between [`COLD`] and [`HOT`];
//! * **Phantom warp**: the model of a fighter that snapped along a hit in the air
//!   ([`Fighter::warp`]) leaves afterimages between where it stood and where it landed,
//!   fading with the trail;
//! * **Stretch strike**: a punch's arm reaches as far as the rules say ([`stretching`]; the
//!   stick figure's own arm grows with it), a sprite or a model gets a rubber arm over it,
//!   and the broken-off fist is drawn by the arena;
//! * **Crater slam**: the landing of a judo throw leaves a crater in the street
//!   ([`Match::craters`](crate::game::Match::craters)): a pit, cracks, a shock ring.
//!
//! Only looks: everything here is read from the state of the match, never fed back.

use std::f32::consts::TAU;

use super::scene::{Frame, Rgba};
use super::stick::{Figure, Mark, Point, stroke};
use crate::game::{
    AttackPhase, CRATER_TICKS, Fighter, FighterState, Glitch, Limb, MatchConfig, WARP_TRAIL,
};

/// The cold colour of a glitch: the ghosts flicker between it and [`HOT`].
pub const COLD: Rgba = Rgba::hex(0x00f5d4);
/// The hot colour of a glitch.
pub const HOT: Rgba = Rgba::hex(0xff2bd6);
/// How opaque a ghost is, at its most.
pub const GHOST_ALPHA: f32 = 0.5;
/// The radius of the ring of lag copies around a victim, in pixels of the virtual screen.
pub const RING_RADIUS: f32 = 52.0;
/// The most lag copies ringed around a victim at once.
pub const MOST_COPIES: usize = 6;
/// The cracks of a crater: the pale glow of a street split open.
pub const CRACK: Rgba = Rgba::hex(0xd6b8ff);
/// The pit of a crater.
pub const PIT: Rgba = Rgba::hex(0x0b0912);
/// Ticks the shock ring of a crater takes to reach the edge of the quake.
pub const SHOCK_TICKS: u32 = 10;
/// The cracks of a crater, as `(angle, share of the radius it reaches)`.
pub const CRACKS: [(f32, f32); 7] = [
    (0.20, 0.95),
    (0.95, 0.60),
    (1.75, 0.85),
    (2.60, 0.55),
    (3.35, 1.00),
    (4.25, 0.70),
    (5.30, 0.80),
];

/// How many lag copies ring a victim that has `echoes` hits left: none without any, then one
/// copy for every two hits, between two and [`MOST_COPIES`], so the ring thins as the barrage
/// runs out.
///
/// ```
/// use srr::front::glitch::{MOST_COPIES, copies};
///
/// assert_eq!(copies(0), 0);
/// assert_eq!(copies(1), 2);
/// assert_eq!(copies(5), 3);
/// assert_eq!(copies(99), MOST_COPIES);
/// ```
pub fn copies(echoes: u32) -> usize {
    if echoes == 0 {
        0
    } else {
        (echoes.div_ceil(2) as usize).clamp(2, MOST_COPIES)
    }
}

/// Where copy `copy` of `copies` stands on its ring `ticks` into the fight, as `(across,
/// depth)` in `-1.0..=1.0` of the ring's radius: the ring turns.
///
/// ```
/// use srr::front::glitch::ring;
///
/// let (across, depth) = ring(0, 4, 0);
/// assert_eq!((across, depth), (1.0, 0.0));
/// let (across, depth) = ring(1, 4, 0);
/// assert!(across.abs() < 1e-6 && (depth - 1.0).abs() < 1e-6);
/// assert_ne!(ring(0, 4, 5), ring(0, 4, 0), "it turns");
/// ```
pub fn ring(copy: usize, copies: usize, ticks: u32) -> (f32, f32) {
    let angle = TAU * copy as f32 / copies.max(1) as f32 + ticks as f32 * 0.3;
    (angle.cos(), angle.sin())
}

/// The colour of copy `copy` `ticks` into the fight: the copies take turns being cold and hot,
/// and swap now and then.
///
/// ```
/// use srr::front::glitch::{COLD, HOT, tint};
///
/// assert_eq!(tint(0, 0), COLD);
/// assert_eq!(tint(1, 0), HOT);
/// assert_eq!(tint(0, 4), HOT);
/// ```
pub fn tint(copy: usize, ticks: u32) -> Rgba {
    if (copy + ticks as usize / 4).is_multiple_of(2) {
        COLD
    } else {
        HOT
    }
}

/// The afterimages of a warp whose trail has `remaining` ticks left: how far along the way from
/// where the fighter stood (`0.0`) to where it is (`1.0`) each one is, and how opaque. They
/// fade with the trail.
///
/// ```
/// use srr::front::glitch::trail;
/// use srr::game::WARP_TRAIL;
///
/// let fresh = trail(WARP_TRAIL);
/// let old = trail(1);
/// assert!(fresh[0].1 > old[0].1);
/// assert!(fresh.iter().all(|&(along, _)| (0.0..1.0).contains(&along)));
/// ```
pub fn trail(remaining: u32) -> [(f32, f32); 3] {
    let fade = remaining.min(WARP_TRAIL) as f32 / WARP_TRAIL as f32;
    [
        (0.25, GHOST_ALPHA * fade),
        (0.5, GHOST_ALPHA * fade * 0.75),
        (0.75, GHOST_ALPHA * fade * 0.5),
    ]
}

/// Whether `fighter`'s arm is stretched at this moment: a punch at its active ticks under the
/// rules of [`Glitch::StretchStrike`].
pub fn stretching(fighter: &Fighter, rules: &MatchConfig) -> bool {
    rules.glitches.contains(Glitch::StretchStrike)
        && fighter.state() == FighterState::Attacking(AttackPhase::Active)
        && fighter
            .attack()
            .is_some_and(|attack| attack.limb == Limb::Punch)
}

/// `figure` as a ghost: every stroke of it in `color` (the back limbs a little dimmer, as ever),
/// its marks gone. Drawn by the stick engine of the arena or the mannequin of the 3D view.
pub fn ghosted(figure: &Figure, color: Rgba) -> Figure {
    let mut ghost = *figure;
    ghost.ink = color;
    ghost.accent = color;
    ghost.mark = Mark::None;
    for bone in ghost.arms.iter_mut().chain(&mut ghost.legs) {
        bone.ink = None;
    }
    ghost
}

/// The lag copies of an echo barrage with `echoes` hits left, ringed around `figure` (the
/// victim, as drawn): `across` and `depth` give the ring's radius, in pixels, on the screen's
/// two axes (the depth of the arena is a quarter as tall as it is wide on the side view).
pub fn draw_echo(frame: &mut Frame, figure: &Figure, echoes: u32, ticks: u32, depth_rise: f32) {
    let count = copies(echoes);
    for copy in 0..count {
        let (across, depth) = ring(copy, count, ticks);
        let moved = figure
            .shifted(across * RING_RADIUS)
            .raised(depth * RING_RADIUS * depth_rise);
        ghosted(&moved, tint(copy, ticks).with_alpha(GHOST_ALPHA)).draw(frame);
    }
}

/// The afterimages of a warp whose trail has `remaining` ticks left, between `from` (the
/// screen `x` the fighter's feet stood at before it snapped) and `now` (where they are):
/// `figure` is the fighter as drawn now.
pub fn draw_warp(
    frame: &mut Frame,
    figure: &Figure,
    from: f32,
    now: f32,
    remaining: u32,
    ticks: u32,
) {
    let along = from - now;
    for (index, (fraction, alpha)) in trail(remaining).into_iter().enumerate() {
        let moved = figure.shifted(along * (1.0 - fraction));
        ghosted(&moved, tint(index, ticks).with_alpha(alpha)).draw(frame);
    }
}

/// The rubber arm of a stretched punch over a sprite or a model: a thick, hot arm from the
/// shoulder to the fist of `figure` (the stick figure of the same pose, whose arm reaches as
/// far as the rules do), and a knuckle of light at the end.
pub fn rubber_arm(frame: &mut Frame, figure: &Figure) {
    let fist = figure.arms[0].end;
    stroke(
        frame,
        figure.shoulder,
        fist,
        6.0,
        HOT.with_alpha(GHOST_ALPHA + 0.2),
    );
    frame.rect(fist.x - 5.0, fist.y - 5.0, 10.0, 10.0, COLD.with_alpha(0.8));
}

/// How far the shock ring of a crater with `remaining` ticks left has run, from `0.0` at the
/// landing to `1.0` at the edge of the quake (where it is gone).
///
/// ```
/// use srr::front::glitch::shock;
/// use srr::game::CRATER_TICKS;
///
/// assert_eq!(shock(CRATER_TICKS), 0.0);
/// assert_eq!(shock(1), 1.0);
/// ```
pub fn shock(remaining: u32) -> f32 {
    let age = CRATER_TICKS.saturating_sub(remaining);
    (age as f32 / SHOCK_TICKS as f32).min(1.0)
}

/// The crater of a slam landing, with `remaining` of its [`CRATER_TICKS`] left, centred on the
/// screen point `center` (on the street), `radius` pixels wide: the pit, the cracks, then the
/// shock ring that runs out to the edge of the quake and goes.
pub fn draw_crater(frame: &mut Frame, center: Point, radius: f32, remaining: u32) {
    let fade = remaining.min(CRATER_TICKS) as f32 / CRATER_TICKS as f32;
    // The pit: a dark lens, three rows narrowing.
    for (row, share) in [0.5, 0.38, 0.24].into_iter().enumerate() {
        let width = radius * share;
        frame.rect(
            center.x - width,
            center.y - 4.0 + row as f32 * 4.0,
            2.0 * width,
            4.0,
            PIT.with_alpha(0.9),
        );
    }
    // The cracks: zigzags out from the pit, flat on the street (a fifth as tall as wide).
    for (angle, reach) in CRACKS {
        let at = |share: f32, kink: f32| Point {
            x: center.x + (angle + kink).cos() * radius * reach * share,
            y: center.y + (angle + kink).sin() * radius * reach * share * 0.2,
        };
        let points = [at(0.2, 0.0), at(0.55, 0.12), at(0.8, -0.1), at(1.0, 0.05)];
        for pair in points.windows(2) {
            stroke(
                frame,
                pair[0],
                pair[1],
                2.5,
                CRACK.with_alpha(0.5 + 0.5 * fade),
            );
        }
    }
    // The shock ring: an ellipse growing to the edge of the quake, thinning as it goes.
    let spread = shock(remaining);
    if spread < 1.0 {
        let across = radius * spread;
        let corners: Vec<Point> = (0..=16)
            .map(|step| {
                let angle = TAU * step as f32 / 16.0;
                Point {
                    x: center.x + angle.cos() * across,
                    y: center.y + angle.sin() * across * 0.2,
                }
            })
            .collect();
        for pair in corners.windows(2) {
            stroke(frame, pair[0], pair[1], 3.0, COLD.with_alpha(1.0 - spread));
        }
    }
}
