//! The manga pose engine: the joint skeleton behind the manga fighters, sharing nothing
//! with the stick figures — not their skeleton, not their proportions. The game's logic is
//! unchanged (the same [`Fighter`] states, the same rules); this module reads the state and
//! poses **its own** manga skeleton: a big head (a third of the sprite), a small body, thin
//! limbs — the proportions of the KonoSuba school of bright, big-eyed girls, in the broad
//! tradition of sprite brawlers like Phantom Breaker: Battle Grounds (nothing is copied
//! from either: every face, haircut and outfit is the game's own).
//!
//! The 2D fight view does not draw this skeleton: a manga fighter is its sprite sheet (see
//! [`skin`](super::skin)) — one PNG per character, every movement in a fixed grid. This
//! module is what the rest of the game reads: the 3D view (`Tab`) builds its cel-shaded
//! volumes from the same pose ([`volumes`]), and the name tags float over the head
//! ([`top`], [`is_lying`]).
//!
//! The stick figures ([`stick`]) stay the game's debug view of the joints, untouched: a
//! chalk fighter is drawn by [`Figure::draw`], a manga fighter by its sheet, and never the
//! twain.

use super::arena::{feet, scale};
use super::scene::Rgba;
use super::skin::Palette;
use super::stick::{self, Point};
use crate::game::{
    Attack, AttackPhase, Fighter, FighterState, GrapplePosition, GrappleRole, Limb, MatchConfig,
    SpecialData, Strength, Technique,
};

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
            flying(phase, reach(config.attack(attack).range))
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
fn flying(phase: AttackPhase, reach: f32) -> Skeleton {
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

/// One 3D volume of the manga figure: a tube (or a ball when `b` is `None`).
#[derive(Debug, Clone, Copy, PartialEq)]
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
            let color = if arm { palette.skin } else { palette.trim };
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
