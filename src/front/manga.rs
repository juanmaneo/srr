//! The manga engine: **a rendering of its own**, sharing nothing with the stick figures —
//! not their skeleton, not their proportions, not their drawing. The game's logic is
//! unchanged (the same [`Fighter`] states, the same rules); this module reads the state and
//! poses **its own** manga skeleton: a big head (a third of the sprite), a small body,
//! thin limbs, cel-shaded surfaces (a base tone, one hard shadow band, one highlight) with
//! clean 1-pixel ink outlines, huge glossy eyes, and per-variant signature looks (twintails,
//! a braid, a flowing mane; crop tops and flaring skirts with thigh-high stockings for the
//! women, jackets for the men) — the look of a manga-themed brawler, drawn sprite by sprite.
//!
//! The stick figures ([`stick`]) stay the game's debug view of the joints, untouched: a
//! chalk fighter is drawn by [`Figure::draw`], a manga fighter by [`draw`] here, and never
//! the twain. The 3D view builds its own volumes from the same pose ([`parts`]).
//!
//! Everything works inside the pen the frame gives ([`stroke`] and [`ring`], only
//! axis-aligned rectangles), so every engine and the headless tests draw it unchanged.
//!
//! No character copies anyone's protected likeness: every face, haircut and outfit is the
//! game's own; only the broad manga rendering techniques are borrowed.

use super::arena::{feet, player_color, scale, strength_color};
use super::scene::{Frame, Rgba};
use super::skin::{HairShape, Palette, disc, quad};
use super::stick::{self, Point, ring, stroke};
use crate::game::{
    Attack, AttackPhase, Fighter, FighterState, GrapplePosition, GrappleRole, Limb, MatchConfig,
    SpecialData, Strength, Technique,
};

/// The clean outline ink of the manga sprites: near-black, thin.
const INK: Rgba = Rgba::hex(0x100c14);
/// The outline thickness.
const OUTLINE: f32 = 1.3;
/// How tall a standing sprite is, in virtual pixels — bigger than the stick figures, so
/// the faces read.
const SPRITE: f32 = 168.0;
/// The radius of the manga head: a third of the sprite (the manga proportion).
const HEAD: f32 = SPRITE * 0.19;
/// The walk cycle length, in screen pixels.
const STRIDE: f32 = 52.0;
/// How much the body coordinates shrink relative to the stick-scale layout they were
/// drawn from: the manga body is small under its big head.
const BODY: f32 = 0.68;

// -----------------------------------------------------------------------------------------
// The pose
// -----------------------------------------------------------------------------------------

/// A limb of the pose: the shoulder/hip it springs from is the pose's own.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LimbPose {
    joint: Point,
    end: Point,
}

/// The manga skeleton of a fighter, in screen coordinates: its own joints, its own
/// proportions, posed from the state the same way the game's logic dictates (the walk
/// cycle from the screen position, the strikes to the attack's reach, the grapples).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    head: Point,
    neck: Point,
    hip: Point,
    /// The front arm (on the facing side), then the back one.
    arms: [LimbPose; 2],
    /// The front leg, then the back one.
    legs: [LimbPose; 2],
    facing: f32,
    /// Flat on the floor (knocked out, pinned).
    lying: bool,
}

/// A point in the fighter's own frame: `x` forward, `y` up, the feet at the origin.
const fn p(x: f32, y: f32) -> Point {
    Point { x, y }
}

/// The idle guard: the fists up by the chin, the feet apart, breathing with `bob`.
fn idle(bob: f32) -> Skeleton {
    Skeleton {
        hip: p(-2.0, 88.0 - bob),
        neck: p(2.0, 136.0 - bob),
        hands: [p(26.0, 128.0 - bob), p(16.0, 122.0 - bob)],
        feet: [p(22.0, 0.0), p(-22.0, 0.0)],
        ..Skeleton::DEFAULT
    }
}

/// The inner skeleton the poses are built from, in the fighter's own frame.
#[derive(Clone, Copy)]
struct Skeleton {
    hip: Point,
    neck: Point,
    hands: [Point; 2],
    feet: [Point; 2],
    lying: bool,
}

impl Skeleton {
    const DEFAULT: Skeleton = Skeleton {
        hip: p(0.0, 88.0),
        neck: p(0.0, 136.0),
        hands: [p(26.0, 128.0), p(16.0, 122.0)],
        feet: [p(22.0, 0.0), p(-22.0, 0.0)],
        lying: false,
    };
}

/// The manga pose of `fighter`, standing at its screen position. The scale of the fight
/// (the arena) places it; `SPRITE` sizes it.
///
/// The states follow the game's logic exactly (see the stick poses for the reference
/// semantics): each arm of the match covers one fighter state.
pub fn pose(fighter: &Fighter, config: &MatchConfig, fight_ticks: u32) -> Pose {
    let (x, y) = feet(fighter, config);
    let facing = fighter.facing().sign() as f32;
    let stride = x + fighter.z() as f32 * scale(config);
    let world = scale(config);
    let reach = |range: i32| (range as f32 * world).clamp(SPRITE * 0.22, SPRITE * 0.62);
    let bob = if (fight_ticks / 24).is_multiple_of(2) {
        0.0
    } else {
        2.0
    };
    let lit = (fight_ticks / 6).is_multiple_of(2);
    let _ = lit;

    let skeleton = match (fighter.state(), fighter.attack()) {
        (FighterState::Walking, _) => {
            let angle = stride / STRIDE * std::f32::consts::TAU;
            let (swing, lift) = (angle.sin(), angle.cos());
            Skeleton {
                hip: p(0.0, 88.0),
                neck: p(6.0, 136.0),
                hands: [p(26.0 + 4.0 * swing, 126.0), p(14.0 - 4.0 * swing, 122.0)],
                feet: [
                    p(24.0 * swing, 8.0 * lift.max(0.0)),
                    p(-24.0 * swing, 8.0 * (-lift).max(0.0)),
                ],
                ..Skeleton::DEFAULT
            }
        }
        (FighterState::Airborne, _) => Skeleton {
            hip: p(-2.0, 84.0),
            neck: p(4.0, 132.0),
            hands: [p(30.0, 148.0), p(-24.0, 142.0)],
            feet: [p(22.0, 42.0), p(-16.0, 48.0)],
            ..Skeleton::DEFAULT
        },
        (FighterState::Blocking, _) => Skeleton {
            hip: p(-8.0, 86.0),
            neck: p(-10.0, 134.0),
            hands: [p(12.0, 152.0), p(8.0, 146.0)],
            feet: [p(18.0, 0.0), p(-28.0, 0.0)],
            ..Skeleton::DEFAULT
        },
        (FighterState::Crouching, _) => Skeleton {
            hip: p(-8.0, 48.0),
            neck: p(6.0, 96.0),
            hands: [p(30.0, 96.0), p(20.0, 88.0)],
            feet: [p(30.0, 0.0), p(-30.0, 0.0)],
            ..Skeleton::DEFAULT
        },
        (FighterState::HitStun, _) => Skeleton {
            hip: p(-6.0, 86.0),
            neck: p(-20.0, 132.0),
            hands: [p(24.0, 108.0), p(-48.0, 118.0)],
            feet: [p(14.0, 0.0), p(-30.0, 0.0)],
            ..Skeleton::DEFAULT
        },
        (FighterState::Knockout, _) => lying(),
        (FighterState::Attacking(phase), Some(attack)) if fighter.is_airborne() => {
            flying(attack, phase, reach(config.attack(attack).range))
        }
        (FighterState::Attacking(phase), Some(attack)) => {
            strike(attack, phase, reach(config.attack(attack).range))
        }
        (FighterState::Special(phase), _) => {
            special(&config.special, phase, reach(config.special.attack.range))
        }
        (FighterState::Grabbing(phase), _) => reaching(phase, reach(config.grapple.range)),
        (FighterState::Technique(Technique::Throw, phase), _) => {
            reaching(phase, reach(config.technique(Technique::Throw).range))
        }
        (FighterState::Technique(Technique::Sweep, phase), _) => {
            sweep(phase, reach(config.technique(Technique::Sweep).range))
        }
        (FighterState::Grappling(role, position), _) => {
            let distance = (config.grapple.grapple_distance as f32 * world).min(stick::MAX_REACH);
            grapple(role, position, distance)
        }
        _ => idle(bob),
    };

    // Place the skeleton on the screen: the body shrinks to the manga proportion (a small
    // body under the big head).
    let place = |q: Point| p(x + facing * q.x * BODY, y - q.y * BODY);
    let head = p(skeleton.neck.x, skeleton.neck.y + HEAD * 1.05 + 6.0);
    Pose {
        head: place(head),
        neck: place(skeleton.neck),
        hip: place(skeleton.hip),
        arms: skeleton
            .hands
            .map(|hand| LimbPose {
                joint: p(
                    skeleton.shoulder_x(),
                    (skeleton.neck.y + skeleton.hip.y) / 2.0 + 4.0,
                ),
                end: hand,
            })
            .map(|limb| LimbPose {
                joint: place(limb.joint),
                end: place(limb.end),
            }),
        legs: skeleton
            .feet
            .map(|foot| LimbPose {
                joint: p(
                    (skeleton.hip.x + foot.x) / 2.0,
                    (skeleton.hip.y + foot.y) / 2.0,
                ),
                end: foot,
            })
            .map(|limb| LimbPose {
                joint: place(limb.joint),
                end: place(limb.end),
            }),
        facing,
        lying: skeleton.lying,
    }
}

impl Skeleton {
    /// The x of the shoulders: a little ahead of the neck.
    fn shoulder_x(&self) -> f32 {
        self.neck.x + 4.0
    }
}

/// Flat on the back: the head away from the side it faces, the knees up.
fn lying() -> Skeleton {
    Skeleton {
        hip: p(20.0, 14.0),
        neck: p(-26.0, 16.0),
        hands: [p(-56.0, 30.0), p(-12.0, 42.0)],
        feet: [p(96.0, 8.0), p(84.0, 26.0)],
        lying: true,
    }
}

/// A strike: wound up, stretched to `reach` at its height, then pulled back.
fn strike(attack: Attack, phase: AttackPhase, reach: f32) -> Skeleton {
    let height = match (attack.limb, attack.strength) {
        (Limb::Punch, Strength::Low) => 128.0,
        (Limb::Punch, Strength::Medium) => 134.0,
        (Limb::Punch, Strength::High) => 152.0,
        (Limb::Kick, Strength::Low) => 30.0,
        (Limb::Kick, Strength::Medium) => 80.0,
        (Limb::Kick, Strength::High) => 138.0,
    };
    match attack.limb {
        Limb::Punch => {
            let (neck, fist) = match phase {
                AttackPhase::Startup => (p(2.0, 136.0), p(12.0, 120.0)),
                AttackPhase::Active => (p(14.0, 136.0), p(reach, height)),
                AttackPhase::Recovery => (p(6.0, 136.0), p(reach * 0.5, 126.0)),
            };
            Skeleton {
                hip: p(-2.0, 88.0),
                neck,
                hands: [fist, p(18.0, 124.0)],
                feet: [p(26.0, 0.0), p(-24.0, 0.0)],
                ..Skeleton::DEFAULT
            }
        }
        Limb::Kick => {
            let (neck, foot) = match phase {
                AttackPhase::Startup => (p(-2.0, 136.0), p(14.0, 48.0)),
                AttackPhase::Active => (p(-18.0, 134.0), p(reach, height)),
                AttackPhase::Recovery => (p(-6.0, 136.0), p(reach * 0.4, 24.0)),
            };
            Skeleton {
                hip: p(0.0, 88.0),
                neck,
                hands: [p(18.0, 132.0), p(-20.0, 120.0)],
                feet: [foot, p(-22.0, 0.0)],
                ..Skeleton::DEFAULT
            }
        }
    }
}

/// A flying kick: the front leg shooting out and down.
fn flying(attack: Attack, phase: AttackPhase, reach: f32) -> Skeleton {
    let foot = match phase {
        AttackPhase::Startup => p(24.0, 56.0),
        AttackPhase::Active => p(reach, 34.0),
        AttackPhase::Recovery => p(reach * 0.4, 48.0),
    };
    Skeleton {
        hip: p(-6.0, 86.0),
        neck: p(-22.0, 138.0),
        hands: [p(-8.0, 148.0), p(-40.0, 130.0)],
        feet: [foot, p(-18.0, 52.0)],
        ..Skeleton::DEFAULT
    }
    .with_attack_color(strength_color(attack.strength))
}

/// The special move: the same shapes as the game's logic dictates.
fn special(data: &SpecialData, phase: AttackPhase, reach: f32) -> Skeleton {
    if data.projectile.is_some() {
        let (neck, hands) = match phase {
            AttackPhase::Startup => (p(-8.0, 132.0), [p(-6.0, 100.0), p(-14.0, 96.0)]),
            _ => (p(12.0, 134.0), [p(64.0, 124.0), p(56.0, 114.0)]),
        };
        Skeleton {
            hip: p(0.0, 82.0),
            neck,
            hands,
            feet: [p(32.0, 0.0), p(-34.0, 0.0)],
            ..Skeleton::DEFAULT
        }
    } else if data.low_profile {
        match phase {
            AttackPhase::Recovery => Skeleton {
                hip: p(-8.0, 50.0),
                neck: p(6.0, 96.0),
                hands: [p(30.0, 96.0), p(20.0, 88.0)],
                feet: [p(30.0, 0.0), p(-30.0, 0.0)],
                ..Skeleton::DEFAULT
            },
            _ => Skeleton {
                hip: p(-8.0, 28.0),
                neck: p(-52.0, 64.0),
                hands: [p(-70.0, 14.0), p(-30.0, 50.0)],
                feet: [p(reach.min(120.0), 10.0), p(20.0, 6.0)],
                lying: true,
            },
        }
    } else if data.armored {
        let palm = match phase {
            AttackPhase::Startup => p(-26.0, 118.0),
            AttackPhase::Active => p(reach, 132.0),
            AttackPhase::Recovery => p(reach * 0.5, 128.0),
        };
        Skeleton {
            hip: p(-2.0, 78.0),
            neck: p(2.0, 132.0),
            hands: [palm, p(-14.0, 100.0)],
            feet: [p(40.0, 0.0), p(-40.0, 0.0)],
            ..Skeleton::DEFAULT
        }
    } else {
        match phase {
            AttackPhase::Recovery => idle(0.0),
            _ => Skeleton {
                hip: p(2.0, 84.0),
                neck: p(38.0, 126.0),
                hands: [p(52.0, 118.0), p(22.0, 100.0)],
                feet: [p(34.0, 0.0), p(-38.0, 8.0)],
                ..Skeleton::DEFAULT
            },
        }
    }
}

/// Both arms reaching out (a grab, or the throw going for the collar).
fn reaching(phase: AttackPhase, reach: f32) -> Skeleton {
    let hands = match phase {
        AttackPhase::Startup => [p(24.0, 134.0), p(16.0, 126.0)],
        AttackPhase::Active => [p(reach, 134.0), p(reach - 12.0, 118.0)],
        AttackPhase::Recovery => [p(22.0, 126.0), p(12.0, 118.0)],
    };
    Skeleton {
        hip: p(0.0, 88.0),
        neck: p(14.0, 134.0),
        hands,
        feet: [p(28.0, 0.0), p(-24.0, 0.0)],
        ..Skeleton::DEFAULT
    }
}

/// The sweep: the front leg reaping low.
fn sweep(phase: AttackPhase, reach: f32) -> Skeleton {
    let (neck, foot) = match phase {
        AttackPhase::Startup => (p(-6.0, 136.0), p(20.0, 20.0)),
        AttackPhase::Active => (p(-16.0, 134.0), p(reach, 10.0)),
        AttackPhase::Recovery => (p(-6.0, 136.0), p(reach * 0.4, 8.0)),
    };
    Skeleton {
        hip: p(0.0, 84.0),
        neck,
        hands: [p(26.0, 134.0), p(38.0, 138.0)],
        feet: [foot, p(-24.0, 0.0)],
        ..Skeleton::DEFAULT
    }
}

/// Every grapple position, on both sides (the same shapes the game's logic dictates).
fn grapple(role: GrappleRole, position: GrapplePosition, distance: f32) -> Skeleton {
    use GrapplePosition::*;
    use GrappleRole::*;
    let reach = distance * 0.5 + 8.0;
    match (role, position) {
        (_, Clinch) => Skeleton {
            hip: p(-14.0, 88.0),
            neck: p(2.0, 136.0),
            hands: [p(reach + 16.0, 146.0), p(reach, 104.0)],
            feet: [p(24.0, 0.0), p(-28.0, 0.0)],
            ..Skeleton::DEFAULT
        },
        (Top, Mount) => Skeleton {
            hip: p(6.0, 58.0),
            neck: p(14.0, 118.0),
            hands: [p(distance + 16.0, 36.0), p(-6.0, 74.0)],
            feet: [p(-4.0, 4.0), p(-34.0, 4.0)],
            ..Skeleton::DEFAULT
        },
        (Top, Submission) => Skeleton {
            hip: p(6.0, 52.0),
            neck: p(42.0, 84.0),
            hands: [p(distance + 26.0, 26.0), p(distance + 6.0, 16.0)],
            feet: [p(-6.0, 4.0), p(-38.0, 4.0)],
            ..Skeleton::DEFAULT
        },
        (Top, Sweep) => Skeleton {
            hip: p(0.0, 68.0),
            neck: p(-38.0, 120.0),
            hands: [p(-64.0, 96.0), p(-34.0, 156.0)],
            feet: [p(distance + 16.0, 120.0), p(distance + 6.0, 50.0)],
            ..Skeleton::DEFAULT
        },
        (Top, Throw) => Skeleton {
            hip: p(-10.0, 80.0),
            neck: p(32.0, 112.0),
            hands: [p(10.0, 128.0), p(32.0, 96.0)],
            feet: [p(22.0, 0.0), p(-28.0, 0.0)],
            ..Skeleton::DEFAULT
        },
        (Top, ScarfHold) => Skeleton {
            hip: p(16.0, 26.0),
            neck: p(46.0, 72.0),
            hands: [p(distance + 22.0, 24.0), p(distance + 4.0, 44.0)],
            feet: [p(48.0, 4.0), p(-52.0, 4.0)],
            ..Skeleton::DEFAULT
        },
        (Top, Armbar) => Skeleton {
            hip: p(-6.0, 18.0),
            neck: p(-58.0, 22.0),
            hands: [p(24.0, 58.0), p(14.0, 48.0)],
            feet: [p(distance + 28.0, 50.0), p(distance + 16.0, 24.0)],
            lying: true,
        },
        (Bottom, Sweep) => Skeleton {
            hip: p(-6.0, 74.0),
            neck: p(-54.0, 114.0),
            hands: [p(-38.0, 162.0), p(-88.0, 128.0)],
            feet: [p(28.0, 14.0), p(16.0, 38.0)],
            ..Skeleton::DEFAULT
        },
        (Bottom, Throw) => Skeleton {
            hip: p(-18.0, 32.0),
            neck: p(32.0, 12.0),
            hands: [p(48.0, -22.0), p(24.0, -20.0)],
            feet: [p(-64.0, 80.0), p(-86.0, 56.0)],
            lying: false,
        },
        (Bottom, Mount) => lying(),
        (Bottom, Submission | ScarfHold | Armbar) => Skeleton {
            hip: p(20.0, 14.0),
            neck: p(-26.0, 16.0),
            hands: [p(10.0, 74.0), p(-42.0, 50.0)],
            feet: [p(96.0, 8.0), p(84.0, 26.0)],
            lying: true,
        },
    }
}

impl Skeleton {
    /// The colour the striking limbs of a special move wear (unused placeholder kept for
    /// symmetry with the game's ink colours).
    fn with_attack_color(self, _color: Rgba) -> Skeleton {
        self
    }
}

// -----------------------------------------------------------------------------------------
// The sprite
// -----------------------------------------------------------------------------------------

/// Draws `fighter` as its manga sprite: its own pose, its own renderer. The chalk stick
/// figures are never drawn under it — this shares nothing with them.
pub fn draw(
    frame: &mut Frame,
    fighter: &Fighter,
    config: &MatchConfig,
    fight_ticks: u32,
    palette: &Palette,
) {
    let pose = pose(fighter, config, fight_ticks);
    let look = &palette.look;
    let facing = pose.facing;
    let accent = player_color(fighter.player());

    // The cel surface of a limb: two segments, each a cel with its outline; a fist or a
    // shoe at the end.
    let limb = |frame: &mut Frame, from: Point, limb: LimbPose, arm: bool, front: bool| {
        let shade = if front { 1.0 } else { 0.85 };
        let w = if arm { look.arm } else { look.leg } * 2.0;
        let (upper, upper_shadow, lower, lower_shadow) = if arm {
            if look.feminine {
                (
                    palette.skin,
                    palette.skin_shadow,
                    palette.skin,
                    palette.skin_shadow,
                )
            } else {
                (
                    palette.outfit,
                    palette.outfit_shadow,
                    palette.outfit,
                    palette.outfit_shadow,
                )
            }
        } else {
            (
                palette.trim,
                palette.trim.scaled(0.62),
                palette.trim,
                palette.trim.scaled(0.62),
            )
        };
        cel(
            frame,
            from,
            limb.joint,
            w,
            w * 0.92,
            upper.scaled(shade),
            upper_shadow.scaled(shade),
        );
        outline(frame, from, limb.joint, w);
        cel(
            frame,
            limb.joint,
            limb.end,
            w * 0.92,
            w * 0.82,
            lower.scaled(shade),
            lower_shadow.scaled(shade),
        );
        outline(frame, limb.joint, limb.end, w * 0.92);
        if arm {
            // The fist: a glove disc with its outline.
            disc(frame, limb.end, w * 0.6, palette.skin.scaled(shade));
            ring(frame, limb.end, w * 0.6, OUTLINE, INK);
        } else {
            // The shoe: a flat disc ahead of the ankle with a sole.
            let toe = p(limb.end.x + facing * w * 0.5, limb.end.y + w * 0.06);
            disc(frame, toe, w * 0.58, palette.trim.scaled(shade));
            ring(frame, toe, w * 0.58, OUTLINE, INK);
            stroke(
                frame,
                p(limb.end.x - facing * w * 0.2, limb.end.y + w * 0.4),
                p(toe.x + facing * w * 0.3, toe.y + w * 0.4),
                1.6,
                INK,
            );
        }
    };

    // The painter's order: back limbs, the torso, the front leg, the head, the front arm.
    limb(frame, pose.hip, pose.legs[1], false, false);
    limb(frame, pose.neck, pose.arms[1], true, false);

    // The torso: the look's silhouette — an hourglass dress for the women, a jacket for
    // the men.
    let neck = pose.neck;
    let hip = pose.hip;
    let up = p(hip.x - neck.x, hip.y - neck.y);
    let length = up.x.hypot(up.y).max(f32::EPSILON);
    let across = p(-up.y / length, up.x / length);
    let at = |along: f32, side: f32| {
        p(
            neck.x + up.x * along + across.x * side,
            neck.y + up.y * along + across.y * side,
        )
    };
    if look.feminine {
        // The crop top.
        cel(
            frame,
            neck,
            at(0.42, 0.0),
            look.shoulders * 1.7,
            look.shoulders * 1.5,
            palette.trim,
            palette.trim.scaled(0.62),
        );
        outline(frame, neck, at(0.42, 0.0), look.shoulders * 1.45);
        // The bare midriff.
        cel(
            frame,
            at(0.42, 0.0),
            at(0.74, 0.0),
            look.waist * 1.3,
            look.waist,
            palette.skin,
            palette.skin_shadow,
        );
        outline(frame, at(0.42, 0.0), at(0.74, 0.0), look.waist * 1.3);
        // The skirt: two flaring quads, then the shadow side.
        for side in [1.0, -1.0] {
            quad(
                frame,
                at(0.74, side * look.waist),
                at(0.74, side * look.hips * 1.7),
                at(1.12, side * look.hips * 1.45),
                at(1.12, side * look.hips * 0.3),
                if side > 0.0 {
                    palette.outfit
                } else {
                    palette.outfit_shadow
                },
            );
        }
        stroke(
            frame,
            at(0.74, look.waist),
            at(1.12, look.hips * 1.2),
            OUTLINE,
            INK,
        );
        stroke(
            frame,
            at(0.74, -look.waist),
            at(1.12, -look.hips * 1.2),
            OUTLINE,
            INK,
        );
        // The belt.
        stroke(
            frame,
            at(0.74, -look.waist),
            at(0.74, look.waist),
            2.4,
            palette.trim,
        );
    } else {
        // The jacket.
        cel(
            frame,
            neck,
            at(0.55, 0.0),
            look.shoulders * 1.9,
            look.waist * 1.6,
            palette.outfit,
            palette.outfit_shadow,
        );
        outline(frame, neck, at(0.55, 0.0), look.shoulders * 1.7);
        cel(
            frame,
            at(0.55, 0.0),
            hip,
            look.waist * 1.6,
            look.hips * 1.5,
            palette.outfit.scaled(0.88),
            palette.outfit_shadow.scaled(0.88),
        );
        outline(frame, at(0.55, 0.0), hip, look.hips * 1.5);
        // The collar.
        stroke(
            frame,
            neck,
            at(0.12, look.shoulders * 0.7),
            2.6,
            palette.trim,
        );
        stroke(
            frame,
            neck,
            at(0.12, -look.shoulders * 0.7),
            2.6,
            palette.trim,
        );
    }

    limb(frame, pose.hip, pose.legs[0], false, true);
    limb(frame, pose.neck, pose.arms[0], true, true);

    // The head, big and manga, drawn last: the sprite's focus, its hair flowing over the
    // shoulders and the raised fists.
    head(frame, &pose, palette, accent);

    // The state marks: the stars of a stun, the cross of a knockout, around the head.
    marks(frame, fighter, &pose);
}

/// A cel surface from `a` to `b`: the base fill, one hard shadow band on the dark side, one
/// highlight on the lit side.
fn cel(frame: &mut Frame, a: Point, b: Point, wa: f32, wb: f32, color: Rgba, shadow: Rgba) {
    let span = p(b.x - a.x, b.y - a.y);
    let length = span.x.hypot(span.y).max(f32::EPSILON);
    let across = p(-span.y / length, span.x / length);
    // The base fill.
    for i in 0..8 {
        let t = i as f32 / 8.0;
        let w = (wa + (wb - wa) * t) / 2.0;
        let q = p(a.x + span.x * t, a.y + span.y * t);
        stroke(
            frame,
            p(q.x + across.x * w, q.y + across.y * w),
            p(q.x - across.x * w, q.y - across.y * w),
            2.6,
            color,
        );
    }
    // The hard shadow: the third of the width on the dark side (away from up-left).
    let dark = if across.x + across.y > 0.0 { -1.0 } else { 1.0 };
    for i in 0..8 {
        let t = i as f32 / 8.0;
        let w = (wa + (wb - wa) * t) / 2.0;
        let q = p(a.x + span.x * t, a.y + span.y * t);
        let edge = p(q.x + across.x * w * dark, q.y + across.y * w * dark);
        let inner = p(
            q.x + across.x * w * dark / 3.0,
            q.y + across.y * w * dark / 3.0,
        );
        stroke(frame, edge, inner, 2.6, shadow);
    }
    // The highlight on the lit side.
    let lit = -dark;
    stroke(
        frame,
        p(
            a.x + across.x * wa * 0.3 * lit,
            a.y + across.y * wa * 0.3 * lit,
        ),
        p(
            b.x + across.x * wb * 0.3 * lit,
            b.y + across.y * wb * 0.3 * lit,
        ),
        1.8,
        color.mix(Rgba::hex(0xffffff), 0.25),
    );
}

/// The 1-pixel outlines of a limb segment, both sides.
fn outline(frame: &mut Frame, a: Point, b: Point, w: f32) {
    let span = p(b.x - a.x, b.y - a.y);
    let length = span.x.hypot(span.y).max(f32::EPSILON);
    let across = p(-span.y / length, span.x / length);
    for side in [1.0, -1.0] {
        stroke(
            frame,
            p(
                a.x + across.x * w * side / 2.0,
                a.y + across.y * w * side / 2.0,
            ),
            p(
                b.x + across.x * w * side / 2.0,
                b.y + across.y * w * side / 2.0,
            ),
            OUTLINE,
            INK,
        );
    }
}

/// The manga head: a round face, a huge glossy eye, a small mouth, a blush, the variant's
/// hair silhouette — and the player's accent in the hair ribbon.
fn head(frame: &mut Frame, pose: &Pose, palette: &Palette, accent: Rgba) {
    let head = pose.head;
    let facing = pose.facing;
    let r = HEAD;
    let at = |along: f32, ahead: f32| p(head.x + facing * ahead, head.y - along);
    // The face: a round skull and a small chin, with the cel shadow on the lower part.
    disc(frame, head, r, palette.skin);
    disc(frame, at(-r * 0.5, r * 0.05), r * 0.68, palette.skin);
    disc(frame, at(-r * 0.8, facing * r * 0.1), r * 0.4, palette.skin);
    disc(
        frame,
        at(-r * 0.45, -facing * r * 0.2),
        r * 0.6,
        palette.skin_shadow,
    );
    disc(
        frame,
        at(-r * 0.45, facing * r * 0.1),
        r * 0.58,
        palette.skin,
    );
    // The huge eye: half the head's height.
    let eye = at(-r * 0.42, r * 0.2);
    let eye_r = r * 0.44;
    disc(frame, eye, eye_r, Rgba::hex(0xffffff));
    disc(frame, eye, eye_r * 0.62, palette.eye);
    // The dark band: the top of the iris only, inside it.
    disc(
        frame,
        p(eye.x, eye.y - eye_r * 0.4),
        eye_r * 0.4,
        palette.eye.scaled(0.4),
    );
    disc(
        frame,
        p(eye.x + facing * eye_r * 0.08, eye.y + eye_r * 0.1),
        eye_r * 0.3,
        INK,
    );
    disc(
        frame,
        p(eye.x + facing * eye_r * 0.2, eye.y + eye_r * 0.45),
        eye_r * 0.24,
        Rgba::hex(0xffffff),
    );
    disc(
        frame,
        p(eye.x - facing * eye_r * 0.3, eye.y - eye_r * 0.3),
        eye_r * 0.13,
        Rgba::hex(0xffffff),
    );
    ring(frame, eye, eye_r, OUTLINE, INK);
    stroke(
        frame,
        p(eye.x - facing * eye_r * 0.9, eye.y - eye_r * 0.85),
        p(eye.x + facing * eye_r * 0.9, eye.y - eye_r * 0.95),
        OUTLINE * 1.5,
        INK,
    );
    // The brow.
    stroke(
        frame,
        at(r * 0.35, r * 0.08),
        at(r * 0.4, r * 0.6),
        1.4,
        palette.hair_shadow,
    );
    // The mouth.
    stroke(
        frame,
        at(-r * 0.55, r * 0.3),
        at(-r * 0.55, r * 0.55),
        1.4,
        Rgba::hex(0xc05a5a),
    );
    // The blush.
    disc(
        frame,
        at(-r * 0.25, r * 0.02),
        r * 0.14,
        Rgba {
            r: 1.0,
            g: 0.5,
            b: 0.5,
            a: 0.45,
        },
    );
    // The clean outline of the face.
    ring(frame, head, r, OUTLINE, INK);
    // The hair.
    hair(frame, head, facing, palette, accent);
}

/// The hair silhouette of the variant, with its shine band and the player's ribbon.
fn hair(frame: &mut Frame, head: Point, facing: f32, palette: &Palette, accent: Rgba) {
    let r = HEAD;
    let at = |along: f32, ahead: f32| p(head.x + facing * ahead, head.y - along);
    let hair = palette.hair;
    let dark = palette.hair_shadow;
    let shine = hair.mix(Rgba::hex(0xffffff), 0.35);
    let look = &palette.look;
    // The cap: one bold mass sitting on the top of the skull, leaving the face clear.
    disc(frame, at(r * 0.72, -facing * r * 0.15), r * 0.68, hair);
    // The fringe: three sharp clumps over the forehead, tips stopping above the eyes.
    for (y0, x0, y1, x1, w) in [
        (r * 0.95, r * 0.45, r * 0.55, r * 1.0, r * 0.32),
        (r * 0.9, r * 0.0, r * 0.45, r * 1.05, r * 0.28),
        (r * 0.85, -r * 0.45, r * 0.4, r * 0.9, r * 0.24),
    ] {
        leaf(frame, at(y0, x0), at(y1, x1), w, w * 0.5, hair);
    }
    // The shadow under the fringe.
    stroke(
        frame,
        at(r * 0.3, r * 0.55),
        at(r * 0.25, r * 0.95),
        r * 0.16,
        dark,
    );
    // The shine band across the crown.
    stroke(
        frame,
        at(r * 0.85, -r * 0.3),
        at(r * 0.9, r * 0.35),
        r * 0.18,
        shine,
    );
    // The silhouette per variant.
    match look.hair_shape {
        HairShape::Twintails => {
            for side in [1.0, -1.0] {
                let anchor = at(r * 0.4, side * r * 0.9);
                leaf(
                    frame,
                    anchor,
                    p(anchor.x - facing * r * 0.4, anchor.y - r * 1.1),
                    r * 0.55,
                    r * 0.3,
                    hair,
                );
                leaf(
                    frame,
                    anchor,
                    p(anchor.x - facing * r * 1.2, anchor.y + r * 2.2),
                    r * 0.6,
                    r * 0.35,
                    hair,
                );
                // The ribbons, in the player's colour.
                disc(frame, anchor, r * 0.16, accent);
            }
            disc(frame, at(r * 0.1, -facing * r * 0.8), r * 0.6, dark);
        }
        HairShape::Braid => {
            let mut q = at(r * 0.3, -facing * r * 0.9);
            for i in 0..4 {
                let next = p(q.x - facing * r * 0.25, q.y + r * 0.6);
                leaf(
                    frame,
                    q,
                    next,
                    r * 0.3 - i as f32 * 0.04,
                    r * 0.26,
                    if i % 2 == 0 { hair } else { dark },
                );
                q = next;
            }
            disc(frame, at(r * 0.3, -facing * r * 0.9), r * 0.18, accent);
        }
        HairShape::Mane => {
            let back = at(r * 0.2, -facing * r * 0.85);
            leaf(
                frame,
                back,
                p(back.x - facing * r * 0.5, back.y + r * look.hair_length),
                r * 0.6,
                r * 0.3,
                hair,
            );
            leaf(
                frame,
                at(-r * 0.1, -facing * r * 0.7),
                p(
                    back.x - facing * r * 0.9,
                    back.y + r * look.hair_length * 0.8,
                ),
                r * 0.45,
                r * 0.22,
                dark,
            );
        }
        HairShape::Spikes => {
            for ahead in [r * 0.5, 0.0, -r * 0.5] {
                stroke(
                    frame,
                    at(r * 0.8, ahead),
                    at(r * 1.7, ahead * 1.3),
                    r * 0.3,
                    hair,
                );
            }
        }
        HairShape::Bald => {
            stroke(
                frame,
                at(r * 0.9, -r * 0.25),
                at(r * 0.95, r * 0.25),
                r * 0.14,
                palette.skin.mix(Rgba::hex(0xffffff), 0.3),
            );
        }
        HairShape::Mohawk => {
            for ahead in [-r * 0.3, 0.0, r * 0.3] {
                let base = at(r * 0.9, ahead);
                leaf(
                    frame,
                    base,
                    p(base.x, base.y - r * 0.9),
                    r * 0.22,
                    r * 0.12,
                    hair,
                );
            }
        }
    }
    if matches!(
        look.hair_shape,
        HairShape::Twintails | HairShape::Braid | HairShape::Mane
    ) {
        for ahead in [-r * 0.4, 0.0, r * 0.4] {
            stroke(
                frame,
                at(r * 0.85, ahead),
                at(r * 1.3, ahead * 1.3),
                r * 0.26,
                hair,
            );
        }
    }
}

/// A filled leaf (the hair clumps): two arcs meeting at the tip.
fn leaf(frame: &mut Frame, a: Point, b: Point, wa: f32, wb: f32, color: Rgba) {
    let span = p(b.x - a.x, b.y - a.y);
    let length = span.x.hypot(span.y).max(f32::EPSILON);
    let across = p(-span.y / length, span.x / length);
    for side in [1.0, -1.0] {
        for i in 0..8 {
            let t0 = i as f32 / 8.0;
            let t1 = (i + 1) as f32 / 8.0;
            let bulge = |t: f32| {
                let w = (wa + (wb - wa) * t) / 2.0;
                let curve = (1.0 - (2.0 * t - 1.0).powi(2)).sqrt();
                w * (1.0 + side * curve * 0.6)
            };
            let p0 = p(
                a.x + span.x * t0 + across.x * bulge(t0),
                a.y + span.y * t0 + across.y * bulge(t0),
            );
            let p1 = p(
                a.x + span.x * t1 + across.x * bulge(t1),
                a.y + span.y * t1 + across.y * bulge(t1),
            );
            stroke(frame, p0, p1, 2.4, color);
        }
    }
    for i in 0..6 {
        let t = i as f32 / 6.0;
        let w = (wa + (wb - wa) * t) / 2.0;
        disc(frame, p(a.x + span.x * t, a.y + span.y * t), w * 0.9, color);
    }
}

/// The state marks around the head: the stars of a stun, the crosses of a knockout.
fn marks(frame: &mut Frame, fighter: &Fighter, pose: &Pose) {
    let head = pose.head;
    match fighter.state() {
        FighterState::HitStun => {
            for (along, ahead) in [(0.9_f32, 0.45_f32), (0.6, 0.8), (0.15, 1.0)] {
                let length = along.hypot(ahead);
                let (along, ahead) = (along / length, ahead / length);
                let inner = p(
                    head.x + along * (HEAD + 8.0),
                    head.y + ahead * (HEAD + 8.0) * pose.facing,
                );
                let outer = p(inner.x + along * 12.0, inner.y + ahead * 12.0 * pose.facing);
                stroke(frame, inner, outer, 2.6, stick::SPARK);
            }
        }
        FighterState::Knockout => {
            for ahead in [-1.0, 8.0] {
                let eye = p(head.x + ahead * pose.facing, head.y + 3.0);
                stroke(
                    frame,
                    p(eye.x - 3.0, eye.y - 3.0),
                    p(eye.x + 3.0, eye.y + 3.0),
                    2.0,
                    INK,
                );
                stroke(
                    frame,
                    p(eye.x - 3.0, eye.y + 3.0),
                    p(eye.x + 3.0, eye.y - 3.0),
                    2.0,
                    INK,
                );
            }
        }
        _ => {}
    }
}

/// One volume of the 3D manga figure, for the third-person view.
pub struct Volume {
    /// The world `x` and screen `y` of the volume's ends, with the radius of each.
    pub a: (Point, i32),
    /// The second end (`None` for a ball).
    pub b: Option<(Point, i32)>,
    /// The colour.
    pub color: Rgba,
}

/// The 3D volumes of the manga figure: its own pose, its own proportions — the stick
/// mannequin shares nothing with it.
pub fn volumes(
    fighter: &Fighter,
    config: &MatchConfig,
    fight_ticks: u32,
    palette: &Palette,
) -> Vec<Volume> {
    let pose = pose(fighter, config, fight_ticks);
    let look = &palette.look;
    let mut volumes = Vec::new();
    // The torso in three sections.
    let chest = p(
        pose.neck.x + (pose.hip.x - pose.neck.x) / 3.0,
        pose.neck.y + (pose.hip.y - pose.neck.y) / 3.0,
    );
    let lower = p(
        pose.neck.x + (pose.hip.x - pose.neck.x) * 2.0 / 3.0,
        pose.neck.y + (pose.hip.y - pose.neck.y) * 2.0 / 3.0,
    );
    volumes.push(Volume {
        a: (pose.neck, (look.shoulders * 1.1) as i32),
        b: Some((chest, (look.waist * 1.2) as i32)),
        color: palette.trim,
    });
    volumes.push(Volume {
        a: (chest, (look.waist * 1.2) as i32),
        b: Some((lower, (look.waist) as i32)),
        color: palette.outfit,
    });
    volumes.push(Volume {
        a: (lower, (look.waist) as i32),
        b: Some((pose.hip, (look.hips * 1.3) as i32)),
        color: palette.outfit,
    });
    // The head: a ball of the manga radius, with the hair ball on top.
    volumes.push(Volume {
        a: (pose.head, HEAD as i32),
        b: None,
        color: palette.skin,
    });
    let hair_top = p(pose.head.x, pose.head.y - HEAD * 0.4);
    volumes.push(Volume {
        a: (hair_top, (HEAD * 3.0 / 4.0) as i32),
        b: None,
        color: palette.hair,
    });
    // The limbs.
    for (from, limbs, arm) in [(pose.neck, &pose.arms, true), (pose.hip, &pose.legs, false)] {
        for (index, limb) in limbs.iter().enumerate() {
            let w = if arm { look.arm } else { look.leg } * 1.2;
            let color = if arm {
                if look.feminine {
                    palette.skin
                } else {
                    palette.outfit
                }
            } else {
                palette.trim
            };
            let shade = if index == 0 { 1.0 } else { 0.85 };
            volumes.push(Volume {
                a: (from, w as i32),
                b: Some((limb.joint, (w * 0.92) as i32)),
                color: color.scaled(shade),
            });
            volumes.push(Volume {
                a: (limb.joint, (w * 0.92) as i32),
                b: Some((limb.end, (w * 0.82) as i32)),
                color: color.scaled(shade),
            });
            // The fist or the shoe.
            volumes.push(Volume {
                a: (limb.end, (w * 0.6) as i32),
                b: None,
                color: if arm {
                    palette.skin.scaled(shade)
                } else {
                    palette.trim.scaled(shade)
                },
            });
        }
    }
    volumes
}

/// The [`PlayerId`] of the pose's fighter, for the tags and the markers that float over it.
pub fn top(pose: &Pose) -> f32 {
    pose.head.y - HEAD - 14.0
}

/// Whether the pose lies flat (for the tags).
pub fn is_lying(pose: &Pose) -> bool {
    pose.lying
}
