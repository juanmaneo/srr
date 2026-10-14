//! Old-school 3D maths, the way a console of the nineties did it: no floats, no GPU.
//!
//! Everything is a [`Fx`], a fixed-point number with 12 fractional bits, so the pipeline is
//! bit-for-bit the same on every machine and the rounding shows as the "grain" the third-person
//! view ([`super::tps`]) is after. Vectors ([`Vec3`]) and quaternions ([`Quat`]) are built on
//! it, and a [`Camera`] is a position plus a quaternion: a point of the world is rotated into
//! the camera's frame and projected on the virtual screen with [`screen_point`].
//!
//! Precision: a product needs one factor of the order of one (a unit vector, a quaternion, a
//! ratio) when the other is a world coordinate; lengths and dot products are summed in 64 bits.

use std::ops::{Add, Div, Mul, Neg, Shr, Sub};

/// Fractional bits of [`Fx`].
pub const FRACTION_BITS: u32 = 12;

/// A fixed-point number: `value / 4096`. Integers up to about half a million are exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Fx(pub i32);

impl Fx {
    /// Zero.
    pub const ZERO: Fx = Fx(0);
    /// One.
    pub const ONE: Fx = Fx(1 << FRACTION_BITS);
    /// One half.
    pub const HALF: Fx = Fx(1 << (FRACTION_BITS - 1));

    /// The integer `value`.
    pub const fn int(value: i32) -> Fx {
        Fx(value << FRACTION_BITS)
    }

    /// The fraction `numerator / denominator` (rounded towards zero).
    ///
    /// # Panics
    ///
    /// When `denominator` is zero.
    pub const fn ratio(numerator: i32, denominator: i32) -> Fx {
        Fx((((numerator as i64) << FRACTION_BITS) / denominator as i64) as i32)
    }

    /// The nearest fixed-point number to `value`: for the few floats coming from the 2D
    /// figures, at the boundary only.
    ///
    /// ```
    /// use srr::front::space::Fx;
    ///
    /// assert_eq!(Fx::from_f32(1.5), Fx::ONE + Fx::HALF);
    /// assert_eq!(Fx::from_f32(-2.0).to_f32(), -2.0);
    /// ```
    pub fn from_f32(value: f32) -> Fx {
        Fx((value * Fx::ONE.0 as f32).round() as i32)
    }

    /// The value as a float, for the boundary with the 2D screen.
    pub fn to_f32(self) -> f32 {
        self.0 as f32 / Fx::ONE.0 as f32
    }

    /// The nearest integer (halves round up).
    pub const fn round(self) -> i32 {
        (self.0 + Fx::HALF.0) >> FRACTION_BITS
    }

    /// The absolute value.
    pub const fn abs(self) -> Fx {
        Fx(self.0.abs())
    }

    /// The square root (zero for negative values).
    ///
    /// ```
    /// use srr::front::space::Fx;
    ///
    /// assert_eq!(Fx::int(9).sqrt(), Fx::int(3));
    /// assert_eq!(Fx::int(-4).sqrt(), Fx::ZERO);
    /// ```
    pub fn sqrt(self) -> Fx {
        Fx(((self.0.max(0) as i64) << FRACTION_BITS).isqrt() as i32)
    }
}

impl Add for Fx {
    type Output = Fx;

    fn add(self, rhs: Fx) -> Fx {
        Fx(self.0 + rhs.0)
    }
}

impl Sub for Fx {
    type Output = Fx;

    fn sub(self, rhs: Fx) -> Fx {
        Fx(self.0 - rhs.0)
    }
}

impl Neg for Fx {
    type Output = Fx;

    fn neg(self) -> Fx {
        Fx(-self.0)
    }
}

impl Mul for Fx {
    type Output = Fx;

    /// The product, computed in 64 bits; the result must fit.
    fn mul(self, rhs: Fx) -> Fx {
        Fx(((self.0 as i64 * rhs.0 as i64) >> FRACTION_BITS) as i32)
    }
}

impl Div for Fx {
    type Output = Fx;

    /// The quotient (rounded towards zero).
    ///
    /// # Panics
    ///
    /// When `rhs` is zero.
    fn div(self, rhs: Fx) -> Fx {
        Fx((((self.0 as i64) << FRACTION_BITS) / rhs.0 as i64) as i32)
    }
}

impl Mul<i32> for Fx {
    type Output = Fx;

    fn mul(self, rhs: i32) -> Fx {
        Fx(self.0 * rhs)
    }
}

impl Div<i32> for Fx {
    type Output = Fx;

    fn div(self, rhs: i32) -> Fx {
        Fx(self.0 / rhs)
    }
}

impl Shr<u32> for Fx {
    type Output = Fx;

    /// Division by a power of two (rounded down).
    fn shr(self, rhs: u32) -> Fx {
        Fx(self.0 >> rhs)
    }
}

/// A point or a direction: `x` to the right, `y` up, `z` forward (away from the camera).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Vec3 {
    pub x: Fx,
    pub y: Fx,
    pub z: Fx,
}

impl Vec3 {
    /// The origin.
    pub const ZERO: Vec3 = Vec3::int(0, 0, 0);
    /// The unit vector to the right.
    pub const X: Vec3 = Vec3::int(1, 0, 0);
    /// The unit vector up.
    pub const Y: Vec3 = Vec3::int(0, 1, 0);
    /// The unit vector forward.
    pub const Z: Vec3 = Vec3::int(0, 0, 1);

    /// The vector `(x, y, z)`.
    pub const fn new(x: Fx, y: Fx, z: Fx) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// The vector of integer coordinates `(x, y, z)`.
    pub const fn int(x: i32, y: i32, z: i32) -> Vec3 {
        Vec3::new(Fx::int(x), Fx::int(y), Fx::int(z))
    }

    /// The dot product.
    pub fn dot(self, other: Vec3) -> Fx {
        let sum = self.x.0 as i64 * other.x.0 as i64
            + self.y.0 as i64 * other.y.0 as i64
            + self.z.0 as i64 * other.z.0 as i64;
        Fx((sum >> FRACTION_BITS) as i32)
    }

    /// The cross product.
    pub fn cross(self, other: Vec3) -> Vec3 {
        Vec3::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// The length.
    ///
    /// ```
    /// use srr::front::space::{Fx, Vec3};
    ///
    /// assert_eq!(Vec3::int(3, 4, 0).length(), Fx::int(5));
    /// ```
    pub fn length(self) -> Fx {
        let squares = self.x.0 as i64 * self.x.0 as i64
            + self.y.0 as i64 * self.y.0 as i64
            + self.z.0 as i64 * self.z.0 as i64;
        Fx(squares.isqrt() as i32)
    }

    /// The unit vector of the same direction (the zero vector stays zero).
    pub fn normalized(self) -> Vec3 {
        let length = self.length();
        if length == Fx::ZERO {
            return self;
        }
        Vec3::new(self.x / length, self.y / length, self.z / length)
    }
}

impl Add for Vec3 {
    type Output = Vec3;

    fn add(self, rhs: Vec3) -> Vec3 {
        Vec3::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;

    fn sub(self, rhs: Vec3) -> Vec3 {
        Vec3::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;

    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

impl Mul<Fx> for Vec3 {
    type Output = Vec3;

    fn mul(self, rhs: Fx) -> Vec3 {
        Vec3::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl Shr<u32> for Vec3 {
    type Output = Vec3;

    fn shr(self, rhs: u32) -> Vec3 {
        Vec3::new(self.x >> rhs, self.y >> rhs, self.z >> rhs)
    }
}

/// A rotation, as a unit quaternion `w + xi + yj + zk`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Quat {
    pub w: Fx,
    pub x: Fx,
    pub y: Fx,
    pub z: Fx,
}

/// Below this, the sum of two unit vectors is taken as zero: they point opposite ways.
const TINY: Fx = Fx(16);

impl Quat {
    /// No rotation.
    pub const IDENTITY: Quat = Quat {
        w: Fx::ONE,
        x: Fx::ZERO,
        y: Fx::ZERO,
        z: Fx::ZERO,
    };

    /// The shortest rotation taking the direction of `from` to the direction of `to`. Opposite
    /// directions turn half a turn around an axis perpendicular to `from`.
    ///
    /// ```
    /// use srr::front::space::{Quat, Vec3};
    ///
    /// let turn = Quat::between(Vec3::Z, Vec3::X);
    /// let turned = turn.rotate(Vec3::int(0, 0, 100));
    /// assert!((turned.x.to_f32() - 100.0).abs() < 0.1);
    /// assert!(turned.z.to_f32().abs() < 0.1);
    /// ```
    pub fn between(from: Vec3, to: Vec3) -> Quat {
        let a = from.normalized();
        let b = to.normalized();
        let w = Fx::ONE + a.dot(b);
        if w < TINY {
            let mut axis = a.cross(Vec3::X);
            if axis.length() < TINY {
                axis = a.cross(Vec3::Y);
            }
            let axis = axis.normalized();
            return Quat {
                w: Fx::ZERO,
                x: axis.x,
                y: axis.y,
                z: axis.z,
            };
        }
        let u = a.cross(b);
        Quat {
            w,
            x: u.x,
            y: u.y,
            z: u.z,
        }
        .normalized()
    }

    /// The length of the quaternion (one for a rotation).
    pub fn norm(self) -> Fx {
        let squares = self.w.0 as i64 * self.w.0 as i64
            + self.x.0 as i64 * self.x.0 as i64
            + self.y.0 as i64 * self.y.0 as i64
            + self.z.0 as i64 * self.z.0 as i64;
        Fx(squares.isqrt() as i32)
    }

    /// The unit quaternion of the same rotation (zero becomes [`Quat::IDENTITY`]).
    pub fn normalized(self) -> Quat {
        let norm = self.norm();
        if norm == Fx::ZERO {
            return Quat::IDENTITY;
        }
        Quat {
            w: self.w / norm,
            x: self.x / norm,
            y: self.y / norm,
            z: self.z / norm,
        }
    }

    /// The inverse rotation.
    pub fn conjugate(self) -> Quat {
        Quat {
            w: self.w,
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }

    /// The dot product (the cosine of half the angle between two rotations).
    pub fn dot(self, other: Quat) -> Fx {
        self.w * other.w + self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// `v` rotated.
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let u = Vec3::new(self.x, self.y, self.z);
        let t = u.cross(v);
        let t = t + t;
        v + t * self.w + u.cross(t)
    }

    /// The rotation `t` of the way from `self` to `to` (`t` in `0..=1`), by normalised linear
    /// interpolation along the shorter path.
    pub fn nlerp(self, to: Quat, t: Fx) -> Quat {
        let to = if self.dot(to) < Fx::ZERO { -to } else { to };
        Quat {
            w: self.w + (to.w - self.w) * t,
            x: self.x + (to.x - self.x) * t,
            y: self.y + (to.y - self.y) * t,
            z: self.z + (to.z - self.z) * t,
        }
        .normalized()
    }
}

impl Mul for Quat {
    type Output = Quat;

    /// The composition: `(a * b).rotate(v)` is `a.rotate(b.rotate(v))`.
    fn mul(self, rhs: Quat) -> Quat {
        Quat {
            w: self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
            x: self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            y: self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            z: self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
        }
    }
}

impl Neg for Quat {
    type Output = Quat;

    /// The same rotation, written the other way round.
    fn neg(self) -> Quat {
        Quat {
            w: -self.w,
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }
}

/// Distance from the eye to the screen, in virtual pixels: the field of view.
pub const FOCAL: i32 = 900;
/// Nothing closer than this to the eye is drawn.
pub const NEAR: Fx = Fx::int(16);
/// Screen x of the camera's axis.
pub const CENTER_X: i32 = 640;
/// Screen y of the camera's axis.
pub const CENTER_Y: i32 = 360;

/// A point of the camera's frame on the virtual screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Projected {
    pub x: i32,
    pub y: i32,
    /// Distance along the camera's axis.
    pub depth: Fx,
}

/// A viewpoint: the eye, and the rotation taking the camera's axes (`x` right, `y` up, `z`
/// forward) to the world's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Camera {
    pub eye: Vec3,
    pub orientation: Quat,
}

impl Camera {
    /// The camera at `eye` looking at `target`, upright (no roll): a yaw around the vertical,
    /// then a pitch.
    pub fn look_at(eye: Vec3, target: Vec3) -> Camera {
        let forward = (target - eye).normalized();
        let mut level = Vec3::new(forward.x, Fx::ZERO, forward.z);
        if level.length() == Fx::ZERO {
            level = Vec3::Z;
        }
        let yaw = Quat::between(Vec3::Z, level);
        let pitch = Quat::between(level, forward);
        Camera {
            eye,
            orientation: pitch * yaw,
        }
    }

    /// `point` in the camera's frame.
    pub fn to_camera(&self, point: Vec3) -> Vec3 {
        self.orientation.conjugate().rotate(point - self.eye)
    }

    /// The direction the camera looks to.
    pub fn forward(&self) -> Vec3 {
        self.orientation.rotate(Vec3::Z)
    }

    /// `point` on the screen, unless it is closer than [`NEAR`] (or behind the eye).
    pub fn project(&self, point: Vec3) -> Option<Projected> {
        let c = self.to_camera(point);
        if c.z < NEAR {
            return None;
        }
        let (x, y) = screen_point(c);
        Some(Projected { x, y, depth: c.z })
    }

    /// Moves `1 / 2^lag` of the way towards `target`: a smooth follow when called every tick.
    pub fn approach(&mut self, target: &Camera, lag: u32) {
        self.eye = self.eye + ((target.eye - self.eye) >> lag);
        self.orientation = self.orientation.nlerp(target.orientation, Fx::ONE >> lag);
    }
}

/// The screen pixel of a point `c` of the camera's frame, which must be in front of the eye
/// (`c.z > 0`).
pub fn screen_point(c: Vec3) -> (i32, i32) {
    (
        CENTER_X + perspective(c.x, c.z),
        CENTER_Y - perspective(c.y, c.z),
    )
}

/// `value` seen at `depth`, in pixels: the perspective division. Zero at or behind the eye;
/// huge results are clamped so the rasterisers can take them.
pub fn perspective(value: Fx, depth: Fx) -> i32 {
    if depth <= Fx::ZERO {
        return 0;
    }
    div_round(value.0 as i64 * FOCAL as i64, depth.0 as i64).clamp(-(1 << 17), 1 << 17) as i32
}

/// `a / b` rounded to the nearest integer.
///
/// # Panics
///
/// When `b` is zero.
pub fn div_round(a: i64, b: i64) -> i64 {
    let (a, b) = if b < 0 { (-a, -b) } else { (a, b) };
    (a + b / 2).div_euclid(b)
}
