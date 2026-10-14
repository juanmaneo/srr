//! The fixed-point 3D maths of the third-person view (`srr::front::space`): numbers, vectors,
//! quaternions, the camera and the perspective projection.

use srr::front::space::{
    CENTER_X, CENTER_Y, Camera, FOCAL, FRACTION_BITS, Fx, NEAR, Projected, Quat, Vec3, div_round,
    perspective, screen_point,
};

/// `value` is within `tolerance` of `expected` (in raw fixed-point units).
fn close(value: Fx, expected: Fx, tolerance: i32) -> bool {
    (value.0 - expected.0).abs() <= tolerance
}

fn close_vec(value: Vec3, expected: Vec3, tolerance: i32) -> bool {
    close(value.x, expected.x, tolerance)
        && close(value.y, expected.y, tolerance)
        && close(value.z, expected.z, tolerance)
}

// ---------------------------------------------------------------------------------------------
// Fx
// ---------------------------------------------------------------------------------------------

#[test]
fn fixed_point_numbers_have_twelve_fractional_bits() {
    assert_eq!(FRACTION_BITS, 12);
    assert_eq!(Fx::ONE, Fx(4096));
    assert_eq!(Fx::HALF, Fx(2048));
    assert_eq!(Fx::ZERO, Fx::default());
    assert_eq!(Fx::int(3), Fx(3 * 4096));
    assert_eq!(Fx::int(-2), Fx(-2 * 4096));
    assert_eq!(Fx::ratio(1, 4), Fx(1024));
    assert_eq!(Fx::ratio(-1, 4), Fx(-1024));
    assert_eq!(Fx::from_f32(0.25), Fx(1024));
    assert_eq!(Fx::from_f32(-1.5), Fx(-6144));
    assert_eq!(Fx(6144).to_f32(), 1.5);
}

#[test]
fn rounding_abs_and_square_root() {
    assert_eq!(Fx::int(3).round(), 3);
    assert_eq!(Fx::from_f32(2.5).round(), 3, "halves round up");
    assert_eq!(Fx::from_f32(2.49).round(), 2);
    assert_eq!(
        Fx::from_f32(-2.5).round(),
        -2,
        "halves round up, towards +infinity"
    );
    assert_eq!(Fx::from_f32(-2.51).round(), -3);
    assert_eq!(Fx::int(-7).abs(), Fx::int(7));
    assert_eq!(Fx::int(7).abs(), Fx::int(7));
    assert_eq!(Fx::int(16).sqrt(), Fx::int(4));
    assert_eq!(Fx::int(2).sqrt(), Fx(5792), "sqrt(2) = 1.4142");
    assert_eq!(Fx::int(-9).sqrt(), Fx::ZERO);
}

#[test]
fn arithmetic_on_fixed_point_numbers() {
    let (a, b) = (Fx::int(3), Fx::from_f32(1.5));
    assert_eq!(a + b, Fx::from_f32(4.5));
    assert_eq!(a - b, Fx::from_f32(1.5));
    assert_eq!(-a, Fx::int(-3));
    assert_eq!(a * b, Fx::from_f32(4.5));
    assert_eq!(a / b, Fx::int(2));
    assert_eq!(b / a, Fx(2048), "0.5");
    assert_eq!(Fx::int(-3) / Fx::int(2), Fx::from_f32(-1.5));
    assert_eq!(a * 4, Fx::int(12));
    assert_eq!(a / 2, Fx::from_f32(1.5));
    assert_eq!(a >> 1, Fx::from_f32(1.5));
    assert_eq!(Fx::int(-1) >> 1, Fx::from_f32(-0.5));
    // Big times small is fine: the product is computed in 64 bits.
    assert_eq!(Fx::int(100_000) * Fx::from_f32(0.5), Fx::int(50_000));
    assert!(Fx::int(1) < Fx::int(2), "ordered");
}

#[test]
#[should_panic]
fn dividing_by_zero_panics() {
    let _ = Fx::ONE / Fx::ZERO;
}

#[test]
#[should_panic]
fn a_ratio_over_zero_panics() {
    let _ = Fx::ratio(1, 0);
}

#[test]
fn division_rounds_to_the_nearest_integer() {
    assert_eq!(div_round(7, 2), 4, "3.5 rounds up");
    assert_eq!(div_round(-7, 2), -3, "-3.5 rounds up");
    assert_eq!(div_round(7, -2), -3, "negative divisor");
    assert_eq!(div_round(-7, -2), 4);
    assert_eq!(div_round(10, 4), 3, "2.5");
    assert_eq!(div_round(9, 4), 2, "2.25");
    assert_eq!(div_round(0, 5), 0);
}

// ---------------------------------------------------------------------------------------------
// Vec3
// ---------------------------------------------------------------------------------------------

#[test]
fn vectors_add_subtract_scale_and_shift() {
    let v = Vec3::int(1, 2, 3);
    let w = Vec3::new(Fx::int(4), Fx::int(5), Fx::int(6));
    assert_eq!(v + w, Vec3::int(5, 7, 9));
    assert_eq!(w - v, Vec3::int(3, 3, 3));
    assert_eq!(-v, Vec3::int(-1, -2, -3));
    assert_eq!(v * Fx::int(2), Vec3::int(2, 4, 6));
    assert_eq!(w >> 1, Vec3::new(Fx::int(2), Fx::from_f32(2.5), Fx::int(3)));
    assert_eq!(Vec3::ZERO, Vec3::default());
    assert_eq!(Vec3::X + Vec3::Y + Vec3::Z, Vec3::int(1, 1, 1));
}

#[test]
fn dot_cross_and_length() {
    assert_eq!(Vec3::int(1, 2, 3).dot(Vec3::int(4, 5, 6)), Fx::int(32));
    assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z, "right-handed");
    assert_eq!(Vec3::Y.cross(Vec3::Z), Vec3::X);
    assert_eq!(Vec3::Z.cross(Vec3::X), Vec3::Y);
    assert_eq!(Vec3::Y.cross(Vec3::X), -Vec3::Z);
    assert_eq!(Vec3::int(3, 4, 0).length(), Fx::int(5));
    assert_eq!(Vec3::int(2, 3, 6).length(), Fx::int(7));
    assert_eq!(
        Vec3::int(3000, 0, 4000).length(),
        Fx::int(5000),
        "world sizes"
    );
    assert_eq!(Vec3::ZERO.length(), Fx::ZERO);
}

#[test]
fn normalizing_keeps_the_direction_and_zero() {
    let unit = Vec3::int(3, 0, 4).normalized();
    assert_eq!(unit, Vec3::new(Fx::ratio(3, 5), Fx::ZERO, Fx::ratio(4, 5)));
    assert!(close(unit.length(), Fx::ONE, 4));
    assert_eq!(Vec3::ZERO.normalized(), Vec3::ZERO);
    assert_eq!(Vec3::int(0, -9, 0).normalized(), -Vec3::Y);
}

// ---------------------------------------------------------------------------------------------
// Quat
// ---------------------------------------------------------------------------------------------

#[test]
fn the_identity_leaves_vectors_alone() {
    let v = Vec3::int(12, -34, 56);
    assert_eq!(Quat::IDENTITY.rotate(v), v);
    assert_eq!(Quat::IDENTITY.norm(), Fx::ONE);
    assert_eq!(Quat::IDENTITY.conjugate(), Quat::IDENTITY);
    assert_eq!(Quat::IDENTITY * Quat::IDENTITY, Quat::IDENTITY);
    assert_eq!(Quat::IDENTITY.dot(Quat::IDENTITY), Fx::ONE);
}

#[test]
fn the_rotation_between_two_directions_takes_one_to_the_other() {
    let quarter = Quat::between(Vec3::Z, Vec3::X);
    assert!(close(quarter.norm(), Fx::ONE, 4));
    assert!(close_vec(
        quarter.rotate(Vec3::int(0, 0, 100)),
        Vec3::int(100, 0, 0),
        512
    ));
    assert!(
        close_vec(quarter.rotate(Vec3::Y), Vec3::Y, 8),
        "the axis is vertical"
    );
    let eighth = Quat::between(Vec3::Z, Vec3::int(1, 0, 1));
    assert!(close_vec(
        eighth.rotate(Vec3::int(0, 0, 1000)),
        Vec3::int(707, 0, 707),
        4096
    ));
    assert!(
        close_vec(
            eighth.conjugate().rotate(Vec3::int(707, 0, 707)),
            Vec3::int(0, 0, 1000),
            4096
        ),
        "the conjugate turns back"
    );
    let none = Quat::between(Vec3::int(0, 0, 5), Vec3::Z);
    assert_eq!(none, Quat::IDENTITY, "same direction");
}

#[test]
fn opposite_directions_turn_half_a_turn_around_a_perpendicular_axis() {
    let flip = Quat::between(Vec3::Z, -Vec3::Z);
    assert_eq!(flip.w, Fx::ZERO);
    assert!(close_vec(
        flip.rotate(Vec3::int(0, 0, 10)),
        Vec3::int(0, 0, -10),
        32
    ));
    // Along x, the first axis tried (x) is parallel: the cross product with y, z, is used.
    let flip_x = Quat::between(Vec3::X, Vec3::int(-7, 0, 0));
    assert_eq!(flip_x.w, Fx::ZERO);
    assert!(close_vec(
        flip_x.rotate(Vec3::int(10, 0, 0)),
        Vec3::int(-10, 0, 0),
        32
    ));
    assert!(
        close_vec(flip_x.rotate(Vec3::Z), Vec3::Z, 8),
        "turned around z"
    );
    let flip_y = Quat::between(Vec3::Y, -Vec3::Y);
    assert!(close_vec(
        flip_y.rotate(Vec3::int(0, 10, 0)),
        Vec3::int(0, -10, 0),
        32
    ));
}

#[test]
fn normalizing_quaternions() {
    let doubled = Quat {
        w: Fx::int(2),
        x: Fx::ZERO,
        y: Fx::ZERO,
        z: Fx::ZERO,
    };
    assert_eq!(doubled.norm(), Fx::int(2));
    assert_eq!(doubled.normalized(), Quat::IDENTITY);
    let zero = Quat {
        w: Fx::ZERO,
        x: Fx::ZERO,
        y: Fx::ZERO,
        z: Fx::ZERO,
    };
    assert_eq!(zero.normalized(), Quat::IDENTITY, "no rotation at all");
    let negated = -Quat::IDENTITY;
    assert_eq!(negated.w, Fx::int(-1));
    assert_eq!(negated.rotate(Vec3::X), Vec3::X, "the same rotation");
}

#[test]
fn composing_rotations_applies_the_right_one_first() {
    let yaw = Quat::between(Vec3::Z, Vec3::X);
    let pitch = Quat::between(Vec3::Z, Vec3::Y);
    let both = pitch * yaw;
    let v = Vec3::int(0, 0, 100);
    assert!(close_vec(both.rotate(v), pitch.rotate(yaw.rotate(v)), 512));
    assert!(close(both.norm(), Fx::ONE, 8));
    assert!(close(yaw.dot(yaw), Fx::ONE, 8));
}

#[test]
fn interpolating_rotations_takes_the_short_way() {
    let quarter = Quat::between(Vec3::Z, Vec3::X);
    let start = Quat::IDENTITY.nlerp(quarter, Fx::ZERO);
    assert_eq!(start, Quat::IDENTITY);
    let end = Quat::IDENTITY.nlerp(quarter, Fx::ONE);
    assert!(close(end.dot(quarter), Fx::ONE, 8));
    let half = Quat::IDENTITY.nlerp(quarter, Fx::HALF);
    assert!(close(half.norm(), Fx::ONE, 8));
    assert!(close_vec(
        half.rotate(Vec3::int(0, 0, 1000)),
        Vec3::int(707, 0, 707),
        4096
    ));
    // The negated target is the same rotation; the interpolation goes through it the same way.
    let other_way = Quat::IDENTITY.nlerp(-quarter, Fx::HALF);
    assert_eq!(other_way, half);
}

// ---------------------------------------------------------------------------------------------
// Camera and projection
// ---------------------------------------------------------------------------------------------

#[test]
fn projection_is_a_pinhole_of_focal_length_at_the_screen_centre() {
    assert_eq!((CENTER_X, CENTER_Y), (640, 360));
    assert_eq!(FOCAL, 900);
    assert_eq!(NEAR, Fx::int(16));
    assert_eq!(screen_point(Vec3::int(0, 0, 100)), (640, 360));
    assert_eq!(
        screen_point(Vec3::int(100, 50, 900)),
        (740, 310),
        "x right, y up"
    );
    assert_eq!(screen_point(Vec3::int(-9, 0, 9)), (640 - 900, 360));
    assert_eq!(perspective(Fx::int(10), Fx::int(0)), 0, "at the eye");
    assert_eq!(perspective(Fx::int(10), Fx::int(-5)), 0, "behind the eye");
    assert_eq!(perspective(Fx::int(100_000), Fx(1)), 1 << 17, "clamped");
    assert_eq!(perspective(Fx::int(-100_000), Fx(1)), -(1 << 17));
    assert_eq!(perspective(Fx::int(1), Fx::int(1800)), 1, "0.5 rounds up");
}

#[test]
fn a_camera_looks_from_the_eye_to_the_target() {
    let camera = Camera::look_at(Vec3::int(0, 0, -100), Vec3::ZERO);
    assert_eq!(camera.eye, Vec3::int(0, 0, -100));
    assert_eq!(
        camera.orientation,
        Quat::IDENTITY,
        "already looking along z"
    );
    assert_eq!(camera.forward(), Vec3::Z);
    assert_eq!(camera.to_camera(Vec3::ZERO), Vec3::int(0, 0, 100));
    assert_eq!(
        camera.project(Vec3::int(10, 20, 0)),
        Some(Projected {
            x: 640 + 90,
            y: 360 - 180,
            depth: Fx::int(100),
        })
    );
    assert_eq!(camera.project(Vec3::int(0, 0, -200)), None, "behind");
    assert_eq!(
        camera.project(Vec3::int(0, 0, -90)),
        None,
        "closer than NEAR"
    );
    assert!(camera.project(Vec3::int(0, 0, -84)).is_some(), "at NEAR");
}

#[test]
fn a_camera_turns_towards_its_target_without_rolling() {
    let camera = Camera::look_at(Vec3::int(100, 50, -100), Vec3::int(300, 0, 0));
    let forward = camera.forward();
    assert!(close_vec(
        forward,
        Vec3::int(200, -50, 100).normalized(),
        40
    ));
    let right = camera.orientation.rotate(Vec3::X);
    assert!(close(right.y, Fx::ZERO, 40), "no roll: right stays level");
    let target = camera.to_camera(Vec3::int(300, 0, 0));
    assert!(close(target.x, Fx::ZERO, 2048), "{target:?}");
    assert!(close(target.y, Fx::ZERO, 2048));
    assert!(close(target.z, Vec3::int(200, -50, 100).length(), 2048));
    let (x, y) = screen_point(target);
    assert!(
        (x - 640).abs() <= 1 && (y - 360).abs() <= 1,
        "the target is centred"
    );
    // Something above the target is higher on screen, something on its right further right.
    let above = camera.project(Vec3::int(300, 40, 0)).unwrap();
    assert!(above.y < 360);
    let beyond = camera.project(Vec3::int(300, 0, 100)).unwrap();
    assert!(beyond.x < 640, "looking along +x, +z is on the left");
    let ahead = camera.project(Vec3::int(400, 0, 0)).unwrap();
    assert!(ahead.x > 640, "and +x goes to the right, away");
    assert!(ahead.depth > target.z);
}

#[test]
fn looking_straight_down_keeps_the_yaw_of_the_z_axis() {
    let camera = Camera::look_at(Vec3::int(0, 100, 0), Vec3::ZERO);
    assert!(close_vec(camera.forward(), -Vec3::Y, 40));
    let right = camera.orientation.rotate(Vec3::X);
    assert!(close_vec(right, Vec3::X, 40), "x stays to the right");
    let same_place = Camera::look_at(Vec3::ZERO, Vec3::ZERO);
    assert_eq!(same_place.orientation, Quat::IDENTITY);
}

#[test]
fn approaching_moves_a_fraction_of_the_way_each_time() {
    let mut camera = Camera::look_at(Vec3::int(0, 0, -100), Vec3::ZERO);
    let target = Camera::look_at(Vec3::int(80, 0, -100), Vec3::int(80, 0, 100));
    camera.approach(&target, 2);
    assert_eq!(camera.eye, Vec3::int(20, 0, -100), "a quarter of the way");
    assert_eq!(camera.orientation, Quat::IDENTITY, "nothing to turn");

    let mut camera = Camera::look_at(Vec3::ZERO, Vec3::Z);
    let turned = Camera::look_at(Vec3::ZERO, Vec3::X);
    for _ in 0..60 {
        camera.approach(&turned, 3);
    }
    assert!(
        close_vec(camera.forward(), Vec3::X, 64),
        "settled on the target"
    );
    assert!(close(camera.orientation.norm(), Fx::ONE, 8));
}
