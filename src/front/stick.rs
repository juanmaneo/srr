//! The fighters as XKCD-style stick figures: a hollow round head, a single line for the body
//! and two-segment arms and legs, in chalk on the alley wall. Each player wears an XKCD-ish
//! accessory in their colour (P1 the hat, P2 the beret, P3 a ponytail, P4 a headband, then
//! again from P5, in other colours) so the figures stay apart without changing the ink.
//!
//! A [`Figure`] is a skeleton in screen coordinates, posed from the fighter's state: guard,
//! walk cycle, crouch, tucked jump, strikes (the striking limb stretches to the attack's range, in the
//! strength's colour; a flying kick shoots the leg out downwards), the special moves (a
//! shoulder charge, a two-palm fireball throw, an armored palm, a low slide: see
//! [`special`]), grabs, the judo techniques and every grapple position. The geometry is
//! pure and tested; [`Figure::draw`] then inks it with the pen functions [`stroke`] and
//! [`ring`], which only emit axis-aligned rectangles, so every engine (and the headless
//! renderer) draws the lines without knowing about them.

use std::f32::consts::TAU;

use super::arena::{body_color, feet, player_color, scale, strength_color};
use super::scene::{Frame, Rgba};
use crate::game::{
    Attack, AttackPhase, Fighter, FighterState, GrapplePosition, GrappleRole, Limb, MatchConfig,
    PlayerId, SpecialData, Strength, Technique,
};

/// The chalk the figures are drawn with.
pub const INK: Rgba = Rgba::hex(0xd9d4ca);
/// Thickness of the lines.
pub const LINE_WIDTH: f32 = 3.5;
/// Radius of the head (to the middle of its outline).
pub const HEAD_RADIUS: f32 = 12.0;
/// Arms reaching out (grabs, clinches).
pub const GRAB: Rgba = Rgba::hex(0x6cff8a);
/// The limbs of a judo technique.
pub const JUDO: Rgba = Rgba::hex(0x3fc8ff);
/// A limb locked in a submission.
pub const SUBMISSION: Rgba = Rgba::hex(0xff3a4a);
/// The stars around a stunned head.
pub const SPARK: Rgba = Rgba::hex(0xfff35c);
/// The limb of a special move (a shoulder charge, a palm, a slide).
pub const POWER: Rgba = Rgba::hex(0xff5fd2);
/// The hands throwing a fireball, and the fireball itself.
pub const FIRE: Rgba = Rgba::hex(0xff8a1e);
/// The arms of an armored special move while it winds up.
pub const ARMOR: Rgba = Rgba::hex(0x9fc4ff);

/// How high the top flies during the sweep's projection.
pub const SWEEP_LIFT: f32 = 40.0;
/// How high the bottom flies over the top's shoulder during the throw's projection.
pub const THROW_LIFT: f32 = 96.0;
/// The longest a limb stretches, whatever the configured range.
pub const MAX_REACH: f32 = 170.0;

/// Length of an arm and of a leg at rest (they stretch, cartoon style, to reach further).
const ARM: f32 = 42.0;
const LEG: f32 = 54.0;
/// Pen resolution: lines and circles are cut in runs this thick across their direction.
const STEP: f32 = 1.5;
/// Screen pixels per walk cycle.
const STRIDE: f32 = 44.0;
/// Room left above the head for the accessory.
const HEADROOM: f32 = 12.0;

/// A point on the virtual screen (or, inside a pose, `x` forward and `y` up from the feet).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

const fn p(x: f32, y: f32) -> Point {
    Point { x, y }
}

impl Point {
    fn plus(self, other: Point, factor: f32) -> Point {
        p(self.x + other.x * factor, self.y + other.y * factor)
    }

    fn to(self, other: Point) -> Point {
        p(other.x - self.x, other.y - self.y)
    }

    fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    /// The unit vector of the same direction (`self` when it has no length).
    fn unit(self) -> Point {
        let length = self.length().max(f32::EPSILON);
        p(self.x / length, self.y / length)
    }
}

/// The hat (or hair) that tells the players apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Accessory {
    /// A top hat, like XKCD's Black Hat Guy.
    Hat,
    /// A beret, like Beret Guy.
    Beret,
    /// A ponytail.
    Ponytail,
    /// A martial artist's headband with its tails.
    Headband,
}

/// The accessory of `player` (they cycle every four players; the colours tell them apart).
///
/// ```
/// use srr::front::stick::{Accessory, accessory};
/// use srr::game::PlayerId;
///
/// assert_eq!(accessory(PlayerId(0)), Accessory::Hat);
/// assert_eq!(accessory(PlayerId(5)), Accessory::Beret);
/// ```
pub fn accessory(player: PlayerId) -> Accessory {
    match player.0 % 4 {
        0 => Accessory::Hat,
        1 => Accessory::Beret,
        2 => Accessory::Ponytail,
        _ => Accessory::Headband,
    }
}

/// What is drawn around the head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mark {
    /// Nothing.
    None,
    /// Stars of a hit.
    Stunned,
    /// Crossed-out eyes.
    KnockedOut,
}

/// An arm (shoulder to hand) or a leg (hip to foot): the elbow or knee, then the end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bone {
    pub joint: Point,
    pub end: Point,
    /// Its own colour (a strike, a grab, a technique, a caught limb), or `None` for the ink.
    pub ink: Option<Rgba>,
}

/// A posed stick figure, in screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Figure {
    /// The centre of the head.
    pub head: Point,
    pub neck: Point,
    pub shoulder: Point,
    pub hip: Point,
    /// The front arm (on the side the fighter faces), then the back one.
    pub arms: [Bone; 2],
    /// The front leg, then the back one.
    pub legs: [Bone; 2],
    /// `1.0` facing right, `-1.0` facing left.
    pub facing: f32,
    /// The colour of the lines ([`body_color`] of the [`INK`]).
    pub ink: Rgba,
    pub accessory: Accessory,
    /// The accessory's colour: the player's.
    pub accent: Rgba,
    pub mark: Mark,
    /// Flat on the floor (knocked out, pinned, or locking an armbar).
    pub lying: bool,
}

/// A pose in the fighter's own frame: `x` forward, `y` up, the feet at the origin.
#[derive(Debug, Clone, Copy)]
struct Pose {
    hip: Point,
    neck: Point,
    hands: [Point; 2],
    feet: [Point; 2],
    /// Colours of the front arm, back arm, front leg and back leg.
    inks: [Option<Rgba>; 4],
    /// Which way each elbow / knee bends (see [`bend`]), same order.
    bends: [f32; 4],
    lying: bool,
}

const FRONT_ARM: usize = 0;
const BACK_ARM: usize = 1;
const FRONT_LEG: usize = 2;
const BACK_LEG: usize = 3;

impl Pose {
    fn new(hip: Point, neck: Point, hands: [Point; 2], feet: [Point; 2]) -> Pose {
        Pose {
            hip,
            neck,
            hands,
            feet,
            inks: [None; 4],
            // Elbows bend down / back, knees forward.
            bends: [-1.0, -1.0, 1.0, 1.0],
            lying: false,
        }
    }

    fn ink(mut self, limb: usize, color: Rgba) -> Pose {
        self.inks[limb] = Some(color);
        self
    }

    fn bend(mut self, limb: usize, side: f32) -> Pose {
        self.bends[limb] = side;
        self
    }

    fn lying(mut self) -> Pose {
        self.lying = true;
        self
    }

    fn lifted(mut self, height: f32) -> Pose {
        for point in [&mut self.hip, &mut self.neck]
            .into_iter()
            .chain(&mut self.hands)
            .chain(&mut self.feet)
        {
            point.y += height;
        }
        self
    }
}

/// The joint of a two-segment limb of `length` from `a` to `b`: in the middle when the limb is
/// stretched, otherwise pushed sideways (`side > 0`: to the left of `a → b`, y up) so both
/// segments keep `length / 2`.
fn bend(a: Point, b: Point, length: f32, side: f32) -> Point {
    let span = a.to(b);
    let distance = span.length();
    let middle = a.plus(span, 0.5);
    if distance >= length || distance <= f32::EPSILON {
        return middle;
    }
    let offset = ((length / 2.0).powi(2) - (distance / 2.0).powi(2)).sqrt();
    let across = p(-span.y / distance, span.x / distance);
    middle.plus(across, side * offset)
}

impl Figure {
    /// `fighter` posed for its state, `fight_ticks` into the fight (idle breathing, flashing
    /// holds).
    pub fn of(fighter: &Fighter, config: &MatchConfig, fight_ticks: u32) -> Figure {
        let (x, y) = feet(fighter, config);
        let facing = fighter.facing().sign() as f32;
        // A sidestep moves the legs too.
        let stride = x + fighter.z() as f32 * scale(config);
        let pose = pose(fighter, config, fight_ticks, stride);
        let place = |q: Point| p(x + facing * q.x, y - q.y);

        let head = pose
            .neck
            .plus(pose.hip.to(pose.neck).unit(), HEAD_RADIUS + 2.0);
        let shoulder = pose.neck.plus(pose.neck.to(pose.hip), 0.15);
        let bone = |from: Point, index: usize, end: Point, length: f32| Bone {
            joint: place(bend(from, end, length, pose.bends[index])),
            end: place(end),
            ink: pose.inks[index],
        };
        let state = fighter.state();
        let accent = player_color(fighter.player());
        Figure {
            head: place(head),
            neck: place(pose.neck),
            shoulder: place(shoulder),
            hip: place(pose.hip),
            arms: [
                bone(shoulder, FRONT_ARM, pose.hands[0], ARM),
                bone(shoulder, BACK_ARM, pose.hands[1], ARM),
            ],
            legs: [
                bone(pose.hip, FRONT_LEG, pose.feet[0], LEG),
                bone(pose.hip, BACK_LEG, pose.feet[1], LEG),
            ],
            facing,
            ink: body_color(INK, state),
            accessory: accessory(fighter.player()),
            accent: if state == FighterState::Knockout {
                accent.scaled(0.5)
            } else {
                accent
            },
            mark: match state {
                FighterState::HitStun => Mark::Stunned,
                FighterState::Knockout => Mark::KnockedOut,
                _ => Mark::None,
            },
            lying: pose.lying,
        }
    }

    /// The figure moved `dx` pixels to the right (left when negative), e.g. onto a card of
    /// the character select screen.
    pub fn shifted(mut self, dx: f32) -> Figure {
        for point in [
            &mut self.head,
            &mut self.neck,
            &mut self.shoulder,
            &mut self.hip,
        ] {
            point.x += dx;
        }
        for bone in self.arms.iter_mut().chain(&mut self.legs) {
            bone.joint.x += dx;
            bone.end.x += dx;
        }
        self
    }

    /// The figure moved `rise` pixels up the screen (down when negative): how the side view
    /// shows a fighter standing deeper in the alley.
    pub fn raised(mut self, rise: f32) -> Figure {
        for point in [
            &mut self.head,
            &mut self.neck,
            &mut self.shoulder,
            &mut self.hip,
        ] {
            point.y -= rise;
        }
        for bone in self.arms.iter_mut().chain(&mut self.legs) {
            bone.joint.y -= rise;
            bone.end.y -= rise;
        }
        self
    }

    /// The highest screen y the figure reaches, accessory included.
    pub fn top(&self) -> f32 {
        self.arms
            .iter()
            .chain(&self.legs)
            .flat_map(|bone| [bone.joint.y, bone.end.y])
            .fold(self.head.y - HEAD_RADIUS - HEADROOM, f32::min)
    }

    /// The colour `bone` is drawn in; the back limbs are a bit dimmer, for depth.
    pub fn bone_color(&self, bone: &Bone, front: bool) -> Rgba {
        bone.ink.unwrap_or(if front {
            self.ink
        } else {
            self.ink.scaled(0.75)
        })
    }

    /// Inks the figure: back limbs, body, front leg, head, front arm, accessory, marks.
    pub fn draw(&self, frame: &mut Frame) {
        let limb = |frame: &mut Frame, from: Point, bone: &Bone, front: bool| {
            let color = self.bone_color(bone, front);
            stroke(frame, from, bone.joint, LINE_WIDTH, color);
            stroke(frame, bone.joint, bone.end, LINE_WIDTH, color);
        };
        limb(frame, self.hip, &self.legs[1], false);
        limb(frame, self.shoulder, &self.arms[1], false);
        stroke(frame, self.neck, self.hip, LINE_WIDTH, self.ink);
        limb(frame, self.hip, &self.legs[0], true);
        ring(frame, self.head, HEAD_RADIUS, LINE_WIDTH, self.ink);
        limb(frame, self.shoulder, &self.arms[0], true);
        self.draw_accessory(frame);
        self.draw_mark(frame);
    }

    /// Only the marks around the head (the stars of a stun, the cross of a knockout): what a
    /// [skin](super::skin) draws over its own figure, so the state of the fighter stays
    /// readable whatever it wears.
    pub fn draw_marks_only(&self, frame: &mut Frame) {
        self.draw_mark(frame);
    }

    /// The direction from the neck to the head (screen coordinates), and the one the face looks
    /// to (perpendicular, towards the facing side when standing).
    fn head_axes(&self) -> (Point, Point) {
        let up = self.neck.to(self.head).unit();
        (up, p(-up.y * self.facing, up.x * self.facing))
    }

    fn draw_accessory(&self, frame: &mut Frame) {
        let (up, front) = self.head_axes();
        let at = |along: f32, ahead: f32| self.head.plus(up, along).plus(front, ahead);
        let r = HEAD_RADIUS;
        let color = self.accent;
        match self.accessory {
            Accessory::Hat => {
                stroke(
                    frame,
                    at(r - 1.0, -r - 5.0),
                    at(r - 1.0, r + 5.0),
                    3.5,
                    color,
                );
                stroke(frame, at(r, 0.0), at(r + 12.0, 0.0), 2.0 * r - 4.0, color);
            }
            Accessory::Beret => {
                stroke(
                    frame,
                    at(r - 2.0, -r - 3.0),
                    at(r + 2.0, r - 3.0),
                    7.0,
                    color,
                );
                stroke(frame, at(r + 4.0, -2.0), at(r + 8.0, -1.0), 2.5, color);
            }
            Accessory::Ponytail => {
                stroke(frame, at(r - 1.0, -4.0), at(4.0, -r - 6.0), 4.0, color);
                stroke(frame, at(4.0, -r - 6.0), at(-10.0, -r - 10.0), 4.0, color);
            }
            Accessory::Headband => {
                stroke(frame, at(4.0, -r - 1.0), at(4.0, r + 1.0), 4.0, color);
                stroke(frame, at(4.0, -r), at(-2.0, -r - 12.0), 3.0, color);
                stroke(frame, at(5.0, -r), at(7.0, -r - 13.0), 3.0, color);
            }
        }
    }

    fn draw_mark(&self, frame: &mut Frame) {
        let (up, front) = self.head_axes();
        match self.mark {
            Mark::None => {}
            Mark::Stunned => {
                for (along, ahead) in [(0.9_f32, 0.45_f32), (0.6, 0.8), (0.15, 1.0)] {
                    let direction = p(0.0, 0.0).plus(up, along).plus(front, ahead).unit();
                    let inner = self.head.plus(direction, HEAD_RADIUS + 5.0);
                    let outer = self.head.plus(direction, HEAD_RADIUS + 13.0);
                    stroke(frame, inner, outer, 2.5, SPARK);
                }
            }
            Mark::KnockedOut => {
                for ahead in [-1.0, 6.0] {
                    let eye = self.head.plus(up, 2.0).plus(front, ahead);
                    stroke(
                        frame,
                        p(eye.x - 2.5, eye.y - 2.5),
                        p(eye.x + 2.5, eye.y + 2.5),
                        1.8,
                        self.ink,
                    );
                    stroke(
                        frame,
                        p(eye.x - 2.5, eye.y + 2.5),
                        p(eye.x + 2.5, eye.y - 2.5),
                        1.8,
                        self.ink,
                    );
                }
            }
        }
    }
}

/// The pose of `fighter` in its own frame; `screen_x` drives the walk cycle so the feet do not
/// slide.
fn pose(fighter: &Fighter, config: &MatchConfig, fight_ticks: u32, screen_x: f32) -> Pose {
    let scale = scale(config);
    let reach = |range: i32| (range as f32 * scale).clamp(ARM, MAX_REACH);
    let bob = if (fight_ticks / 24).is_multiple_of(2) {
        0.0
    } else {
        1.5
    };
    let lit = (fight_ticks / 6).is_multiple_of(2);
    match (fighter.state(), fighter.attack()) {
        (FighterState::Walking, _) => walk(screen_x),
        (FighterState::Airborne, _) => Pose::new(
            p(0.0, 50.0),
            p(2.0, 84.0),
            [p(24.0, 94.0), p(-18.0, 90.0)],
            [p(14.0, 24.0), p(-10.0, 28.0)],
        ),
        (FighterState::Blocking, _) => Pose::new(
            p(-4.0, 50.0),
            p(-6.0, 82.0),
            [p(10.0, 98.0), p(6.0, 92.0)],
            [p(12.0, 0.0), p(-18.0, 0.0)],
        ),
        // Squatting low, knees forward, fists at the chin.
        (FighterState::Crouching, _) => Pose::new(
            p(-6.0, 26.0),
            p(4.0, 56.0),
            [p(20.0, 54.0), p(12.0, 50.0)],
            [p(18.0, 0.0), p(-20.0, 0.0)],
        ),
        (FighterState::HitStun, _) => Pose::new(
            p(-4.0, 50.0),
            p(-14.0, 80.0),
            [p(16.0, 60.0), p(-34.0, 72.0)],
            [p(10.0, 0.0), p(-20.0, 0.0)],
        ),
        (FighterState::Knockout, _) => lying([p(-40.0, 2.0), p(-6.0, 26.0)]),
        (FighterState::Attacking(phase), Some(attack)) if fighter.is_airborne() => {
            flying_kick(attack, phase, reach(config.attack(attack).range))
        }
        (FighterState::Attacking(phase), Some(attack)) => {
            strike(attack, phase, reach(config.attack(attack).range))
        }
        (FighterState::Special(phase), _) => {
            special(&config.special, phase, reach(config.special.attack.range))
        }
        (FighterState::Grabbing(phase), _) => reaching(phase, reach(config.grapple.range), GRAB),
        (FighterState::Technique(Technique::Throw, phase), _) => {
            reaching(phase, reach(config.technique(Technique::Throw).range), JUDO)
        }
        (FighterState::Technique(Technique::Sweep, phase), _) => {
            sweep(phase, reach(config.technique(Technique::Sweep).range))
        }
        (FighterState::Grappling(role, position), _) => {
            let distance = config.grapple.grapple_distance as f32 * scale;
            grapple(role, position, distance, lit)
        }
        // Idle, breathing (an attack always has its `attack()`, so nothing else lands here).
        _ => Pose::new(
            p(-2.0, 50.0 - bob * 0.5),
            p(2.0, 82.0 - bob),
            [p(22.0, 76.0 - bob), p(12.0, 72.0 - bob)],
            [p(16.0, 0.0), p(-16.0, 0.0)],
        ),
    }
}

/// The walk cycle at `screen_x`: the legs scissor, the fists sway in the guard.
fn walk(screen_x: f32) -> Pose {
    let angle = screen_x / STRIDE * TAU;
    let (swing, lift) = (angle.sin(), angle.cos());
    Pose::new(
        p(0.0, 50.0),
        p(4.0, 82.0),
        [p(20.0 + 3.0 * swing, 76.0), p(10.0 - 3.0 * swing, 72.0)],
        [
            p(16.0 * swing, 5.0 * lift.max(0.0)),
            p(-16.0 * swing, 5.0 * (-lift).max(0.0)),
        ],
    )
}

/// The height of a punch (`Limb::Punch`) or kick aimed with `strength`: jab, straight and
/// uppercut; low, middle and head kick.
fn aim(limb: Limb, strength: Strength) -> f32 {
    match (limb, strength) {
        (Limb::Punch, Strength::Low) => 76.0,
        (Limb::Punch, Strength::Medium) => 80.0,
        (Limb::Punch, Strength::High) => 92.0,
        (Limb::Kick, Strength::Low) => 18.0,
        (Limb::Kick, Strength::Medium) => 48.0,
        (Limb::Kick, Strength::High) => 84.0,
    }
}

/// A strike: wound up, stretched to `reach` at its height, then pulled back.
fn strike(attack: Attack, phase: AttackPhase, reach: f32) -> Pose {
    let color = strength_color(attack.strength);
    let height = aim(attack.limb, attack.strength);
    match attack.limb {
        Limb::Punch => {
            let (neck, fist) = match phase {
                AttackPhase::Startup => (p(0.0, 82.0), p(6.0, 74.0)),
                AttackPhase::Active => (p(8.0, 82.0), p(reach, height)),
                AttackPhase::Recovery => (p(4.0, 82.0), p(reach * 0.5, 76.0)),
            };
            Pose::new(
                p(-2.0, 50.0),
                neck,
                [fist, p(12.0, 74.0)],
                [p(18.0, 0.0), p(-16.0, 0.0)],
            )
            .ink(FRONT_ARM, color)
        }
        Limb::Kick => {
            // The body leans back to balance the leg.
            let (neck, foot) = match phase {
                AttackPhase::Startup => (p(-2.0, 82.0), p(10.0, 30.0)),
                AttackPhase::Active => (p(-12.0, 80.0), p(reach, height)),
                AttackPhase::Recovery => (p(-4.0, 82.0), p(reach * 0.4, 14.0)),
            };
            Pose::new(
                p(0.0, 50.0),
                neck,
                [p(14.0, 78.0), p(-14.0, 72.0)],
                [foot, p(-14.0, 0.0)],
            )
            .ink(FRONT_LEG, color)
        }
    }
}

/// A flying kick: the body leans back in the air, the arms trail behind, the back leg stays
/// tucked and the front leg shoots out to `reach`, downwards, in the strength's colour.
fn flying_kick(attack: Attack, phase: AttackPhase, reach: f32) -> Pose {
    let foot = match phase {
        AttackPhase::Startup => p(16.0, 36.0),
        AttackPhase::Active => p(reach, 20.0),
        AttackPhase::Recovery => p(reach * 0.4, 30.0),
    };
    Pose::new(
        p(-4.0, 52.0),
        p(-14.0, 84.0),
        [p(-6.0, 90.0), p(-26.0, 78.0)],
        [foot, p(-12.0, 30.0)],
    )
    .ink(FRONT_LEG, strength_color(attack.strength))
}

/// What a special move looks like: `config` holds the fighter's own rules (see
/// [`MatchConfig::rules_of`]).
///
/// * a projectile (the fireball): both palms drawn back to the hip, then thrust forward
///   together at chest height, in [`FIRE`];
/// * a low move (the slide): leaning back, almost lying, the front leg shooting out along the
///   floor to `reach`, in [`POWER`];
/// * an armored move (the palm): a wide stance, the arms in [`ARMOR`] while it winds up, then
///   the palm stretched to `reach`, in [`POWER`];
/// * otherwise (the shoulder charge): bent forward, the front shoulder and arm leading.
///
/// ```
/// use srr::front::stick::{FIRE, special_ink};
/// use srr::game::{AttackPhase, Character, MatchConfig};
///
/// let torch = Character::Torch.rules(&MatchConfig::default());
/// assert_eq!(special_ink(&torch.special, AttackPhase::Active), [Some(FIRE), Some(FIRE)]);
/// ```
fn special(data: &SpecialData, phase: AttackPhase, reach: f32) -> Pose {
    let [front, back] = special_ink(data, phase);
    let pose = if data.projectile.is_some() {
        let (neck, hands) = match phase {
            AttackPhase::Startup => (p(-6.0, 78.0), [p(-4.0, 58.0), p(-10.0, 56.0)]),
            _ => (p(8.0, 80.0), [p(40.0, 74.0), p(34.0, 68.0)]),
        };
        Pose::new(p(0.0, 46.0), neck, hands, [p(20.0, 0.0), p(-22.0, 0.0)])
    } else if data.low_profile {
        match phase {
            AttackPhase::Recovery => Pose::new(
                p(-6.0, 28.0),
                p(4.0, 58.0),
                [p(20.0, 54.0), p(12.0, 50.0)],
                [p(18.0, 0.0), p(-20.0, 0.0)],
            ),
            _ => Pose::new(
                p(-6.0, 16.0),
                p(-36.0, 36.0),
                [p(-46.0, 8.0), p(-20.0, 30.0)],
                [p(reach.min(LEG * 1.8), 6.0), p(12.0, 2.0)],
            ),
        }
    } else if data.armored {
        let palm = match phase {
            AttackPhase::Startup => p(-18.0, 70.0),
            AttackPhase::Active => p(reach, 78.0),
            AttackPhase::Recovery => p(reach * 0.5, 76.0),
        };
        Pose::new(
            p(-2.0, 44.0),
            p(2.0, 78.0),
            [palm, p(-10.0, 60.0)],
            [p(26.0, 0.0), p(-26.0, 0.0)],
        )
    } else {
        match phase {
            AttackPhase::Recovery => Pose::new(
                p(0.0, 50.0),
                p(6.0, 82.0),
                [p(20.0, 70.0), p(12.0, 68.0)],
                [p(16.0, 0.0), p(-16.0, 0.0)],
            ),
            _ => Pose::new(
                p(2.0, 48.0),
                p(24.0, 76.0),
                [p(34.0, 70.0), p(14.0, 60.0)],
                [p(22.0, 0.0), p(-26.0, 4.0)],
            ),
        }
    };
    let limbs = if data.low_profile {
        [FRONT_LEG, BACK_LEG]
    } else {
        [FRONT_ARM, BACK_ARM]
    };
    let pose = match front {
        Some(color) => pose.ink(limbs[0], color),
        None => pose,
    };
    match back {
        Some(color) => pose.ink(limbs[1], color),
        None => pose,
    }
}

/// The colours of the front and back limbs of a special move in `phase` (the arms, or the legs
/// of a slide); `None` keeps the chalk.
pub fn special_ink(data: &SpecialData, phase: AttackPhase) -> [Option<Rgba>; 2] {
    match phase {
        AttackPhase::Recovery => [None, None],
        _ if data.projectile.is_some() => [Some(FIRE), Some(FIRE)],
        AttackPhase::Startup if data.armored => [Some(ARMOR), Some(ARMOR)],
        AttackPhase::Startup => [None, None],
        AttackPhase::Active => [Some(POWER), None],
    }
}

/// Both arms reaching out to `reach` (a grab, or the throw going for the collar and the
/// sleeve), the back arm a shade darker.
fn reaching(phase: AttackPhase, reach: f32, color: Rgba) -> Pose {
    let (neck, hands) = match phase {
        AttackPhase::Startup => (p(4.0, 82.0), [p(16.0, 80.0), p(10.0, 76.0)]),
        AttackPhase::Active => (p(10.0, 80.0), [p(reach, 80.0), p(reach - 8.0, 70.0)]),
        AttackPhase::Recovery => (p(4.0, 82.0), [p(14.0, 74.0), p(8.0, 70.0)]),
    };
    Pose::new(p(0.0, 50.0), neck, hands, [p(18.0, 0.0), p(-16.0, 0.0)])
        .ink(FRONT_ARM, color)
        .ink(BACK_ARM, color.scaled(0.7))
}

/// The sweep's entry: leaning back, the front leg reaps low while the back arm grips.
fn sweep(phase: AttackPhase, reach: f32) -> Pose {
    let (neck, foot) = match phase {
        AttackPhase::Startup => (p(-4.0, 82.0), p(14.0, 12.0)),
        AttackPhase::Active => (p(-10.0, 80.0), p(reach, 6.0)),
        AttackPhase::Recovery => (p(-4.0, 82.0), p(reach * 0.4, 4.0)),
    };
    Pose::new(
        p(0.0, 48.0),
        neck,
        [p(18.0, 80.0), p(26.0, 82.0)],
        [foot, p(-16.0, 0.0)],
    )
    .ink(FRONT_LEG, JUDO)
    .ink(BACK_ARM, JUDO.scaled(0.7))
}

/// Flat on the back, fallen backwards: the head away from the side it faces, knees up towards
/// the opponent, hands at `hands`.
fn lying(hands: [Point; 2]) -> Pose {
    Pose::new(
        p(14.0, 10.0),
        p(-18.0, 12.0),
        hands,
        [p(62.0, 4.0), p(54.0, 14.0)],
    )
    .lying()
}

/// Every grapple position, on both sides. `distance` is the gap between the two fighters on
/// screen; `lit` flashes the limb caught in a finishing hold.
fn grapple(role: GrappleRole, position: GrapplePosition, distance: f32, lit: bool) -> Pose {
    use GrapplePosition::*;
    use GrappleRole::*;
    let distance = distance.min(MAX_REACH);
    let reach = distance * 0.5 + 6.0;
    match (role, position) {
        // Chest to chest: one hand behind the opponent's neck, the other under the arm.
        (_, Clinch) => Pose::new(
            p(-10.0, 50.0),
            p(2.0, 82.0),
            [p(reach + 10.0, 88.0), p(reach, 62.0)],
            [p(16.0, 0.0), p(-18.0, 0.0)],
        )
        .ink(FRONT_ARM, GRAB)
        .ink(BACK_ARM, GRAB.scaled(0.7)),
        // Kneeling astride the hips, punching down at the face.
        (Top, Mount) => Pose::new(
            p(4.0, 34.0),
            p(10.0, 70.0),
            [p(distance + 10.0, 22.0), p(-4.0, 44.0)],
            [p(-2.0, 2.0), p(-22.0, 2.0)],
        )
        .ink(FRONT_ARM, GRAB),
        // Low over the opponent, an arm locked around the neck.
        (Top, Submission) => Pose::new(
            p(4.0, 30.0),
            p(26.0, 50.0),
            [p(distance + 16.0, 16.0), p(distance + 4.0, 10.0)],
            [p(-4.0, 2.0), p(-24.0, 2.0)],
        )
        .ink(FRONT_ARM, SUBMISSION),
        // The flying juji-gatame kick: airborne, leaning back, legs scissored around.
        (Top, Sweep) => {
            let reach = distance + 10.0;
            Pose::new(
                p(0.0, 40.0),
                p(-24.0, 72.0),
                [p(-40.0, 58.0), p(-22.0, 96.0)],
                [p(reach, 72.0), p(reach - 6.0, 30.0)],
            )
            .ink(FRONT_LEG, JUDO)
            .ink(BACK_LEG, JUDO.scaled(0.7))
            .lifted(SWEEP_LIFT)
        }
        // Ippon-seoi-nage: bent forward under the opponent, pulling the arm over the shoulder.
        (Top, Throw) => Pose::new(
            p(-6.0, 46.0),
            p(20.0, 66.0),
            [p(6.0, 76.0), p(20.0, 56.0)],
            [p(14.0, 0.0), p(-18.0, 0.0)],
        )
        .ink(FRONT_ARM, JUDO),
        // Hon-gesa-gatame: sitting at the side, legs sprawled, an arm around the neck.
        (Top, ScarfHold) => Pose::new(
            p(10.0, 14.0),
            p(28.0, 42.0),
            [p(distance + 14.0, 14.0), p(distance + 2.0, 26.0)],
            [p(30.0, 2.0), p(-34.0, 2.0)],
        )
        .bend(BACK_LEG, -1.0)
        .ink(FRONT_ARM, JUDO),
        // Ude-hishigi-juji-gatame: on the back across the opponent, legs over the chest,
        // pulling the arm.
        (Top, Armbar) => Pose::new(
            p(-4.0, 10.0),
            p(-36.0, 12.0),
            [p(14.0, 34.0), p(8.0, 28.0)],
            [p(distance + 18.0, 30.0), p(distance + 10.0, 14.0)],
        )
        .ink(FRONT_ARM, SUBMISSION)
        .lying(),
        // Swept off the feet, falling backwards.
        (Bottom, Sweep) => Pose::new(
            p(-4.0, 44.0),
            p(-34.0, 68.0),
            [p(-24.0, 98.0), p(-56.0, 76.0)],
            [p(18.0, 8.0), p(10.0, 22.0)],
        )
        .lifted(SWEEP_LIFT * 0.5),
        // Flying head first over the thrower's shoulder, legs in the air.
        (Bottom, Throw) => Pose::new(
            p(-12.0, 18.0),
            p(20.0, 8.0),
            [p(30.0, -14.0), p(14.0, -12.0)],
            [p(-40.0, 48.0), p(-54.0, 34.0)],
        )
        .bend(FRONT_LEG, -1.0)
        .bend(BACK_LEG, -1.0)
        .lifted(THROW_LIFT),
        // Pinned under the mount, hands up to defend.
        (Bottom, Mount) => lying([p(-6.0, 42.0), p(-14.0, 38.0)]),
        // Caught in a finishing hold: the trapped arm, stretched up, flashes.
        (Bottom, Submission | ScarfHold | Armbar) => {
            let caught = lying([p(6.0, 44.0), p(-26.0, 30.0)]);
            if lit {
                caught.ink(FRONT_ARM, SUBMISSION)
            } else {
                caught
            }
        }
    }
}

/// Draws a straight line from `a` to `b`, `width` thick, as a staircase of thin rectangles
/// (one per [`STEP`] across the line's main direction). The ends of thin lines are extended by
/// half the width so joined segments overlap; thick ones (filled shapes) keep their length.
pub fn stroke(frame: &mut Frame, a: Point, b: Point, width: f32, color: Rgba) {
    let direction = a.to(b).unit();
    let cap = width.min(LINE_WIDTH) / 2.0;
    let a = a.plus(direction, -cap);
    let b = b.plus(direction, cap);
    let span = a.to(b);
    let horizontal = span.x.abs() >= span.y.abs();
    // Thickness of each run, measured along the minor axis, so the line keeps `width`
    // perpendicular to itself.
    let major = if horizontal { direction.x } else { direction.y };
    let thickness = width / major.abs().max(0.5);
    let minor_span = if horizontal { span.y } else { span.x };
    let runs = (minor_span.abs() / STEP).ceil().max(1.0) as usize;
    for run in 0..runs {
        let t0 = run as f32 / runs as f32;
        let t1 = (run + 1) as f32 / runs as f32;
        let start = a.plus(span, t0);
        let end = a.plus(span, t1);
        let middle = a.plus(span, (t0 + t1) / 2.0);
        if horizontal {
            let left = start.x.min(end.x);
            let length = (end.x - start.x).abs();
            frame.rect(left, middle.y - thickness / 2.0, length, thickness, color);
        } else {
            let top = start.y.min(end.y);
            let length = (end.y - start.y).abs();
            frame.rect(middle.x - thickness / 2.0, top, thickness, length, color);
        }
    }
}

/// Draws a circle outline of `radius` (to the middle of the line) around `center`, `width`
/// thick, row by row: one rectangle across the top and bottom caps, two (left and right) in
/// between.
pub fn ring(frame: &mut Frame, center: Point, radius: f32, width: f32, color: Rgba) {
    let outer = radius + width / 2.0;
    let inner = radius - width / 2.0;
    let rows = (2.0 * outer / STEP).ceil() as usize;
    let height = 2.0 * outer / rows as f32;
    for row in 0..rows {
        let top = center.y - outer + row as f32 * height;
        // The row's widest point is its edge nearest the centre.
        let dy = (top + height / 2.0 - center.y).abs();
        let near = (dy - height / 2.0).max(0.0);
        let out = (outer * outer - near * near).max(0.0).sqrt();
        if near < inner {
            // Use the far edge for the hole, so neighbouring rows overlap instead of leaving
            // gaps in the outline.
            let far = (dy + height / 2.0).min(inner);
            let hole = (inner * inner - far * far).max(0.0).sqrt();
            frame.rect(center.x - out, top, out - hole, height, color);
            frame.rect(center.x + hole, top, out - hole, height, color);
        } else {
            frame.rect(center.x - out, top, 2.0 * out, height, color);
        }
    }
}
