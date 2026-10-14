//! The third-person 3D view (`srr::front::tps`): the integer rasterisers, the camera placement,
//! the scene with its mannequins, and `Tab` ([`VIEW_KEY`]) switching the view in the game.

use std::collections::HashSet;

use srr::front::arena::{self, ARENA_LEFT, FLOOR_Y, feet};
use srr::front::space::{Camera, Fx, NEAR, Vec3};
use srr::front::stick::Point;
use srr::front::tps::{
    self, AHEAD, BACK, BACKGROUND, CAMERA_LAG, GRAIN, HEIGHT, INK, SIDE, TARGET_HEIGHT, ball_shape,
    clip_half_plane, clip_near, clip_segment, draw_ball, draw_line, draw_polygon, draw_tube,
    fill_polygon, fog, ink_line, lift, overview, tone, tube_shape,
};
use srr::front::{
    Frame, Game, Headless, PAUSE_KEY, Rect, Rgba, SCREEN_HEIGHT, SCREEN_WIDTH, Screen, VIEW_KEY,
    View,
};
use srr::game::{FighterState, Key, MatchConfig, PlayerId};
use srr::net::{Client, PlayerSet, ServerMessage};

const WHITE: Rgba = Rgba::hex(0xffffff);

fn frame() -> Frame {
    Frame::new(BACKGROUND)
}

/// Every rectangle is within the screen.
fn within_screen(rects: &[Rect]) {
    for rect in rects {
        assert!(
            rect.x >= 0.0 && rect.x + rect.width <= SCREEN_WIDTH,
            "{rect:?}"
        );
        assert!(
            rect.y >= 0.0 && rect.y + rect.height <= SCREEN_HEIGHT,
            "{rect:?}"
        );
    }
}

/// Every rectangle is on the grain, within the screen, and not empty.
fn check_rects(rects: &[Rect]) {
    within_screen(rects);
    for rect in rects {
        assert!(rect.width > 0.0 && rect.height > 0.0, "{rect:?}");
        assert_eq!(rect.x as i32 % GRAIN, 0, "{rect:?}");
        assert_eq!(rect.y as i32 % GRAIN, 0, "{rect:?}");
    }
}

fn count(frame: &Frame, color: Rgba) -> usize {
    frame.rects.iter().filter(|r| r.color == color).count()
}

/// A camera at the origin looking along +z: the camera's frame is the world's.
fn straight() -> Camera {
    Camera::look_at(Vec3::ZERO, Vec3::Z)
}

// ---------------------------------------------------------------------------------------------
// Screen-space rasterisers
// ---------------------------------------------------------------------------------------------

#[test]
fn a_polygon_is_filled_with_rows_of_grain() {
    assert_eq!(GRAIN, 2);
    let mut f = frame();
    fill_polygon(&mut f, &[(10, 10), (30, 10), (30, 20), (10, 20)], WHITE);
    assert_eq!(f.rects.len(), 5, "ten pixels high, two per row");
    for (i, rect) in f.rects.iter().enumerate() {
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (10.0, 10.0 + 2.0 * i as f32, 20.0, 2.0)
        );
        assert_eq!(rect.color, WHITE);
    }
    check_rects(&f.rects);

    let mut f = frame();
    fill_polygon(&mut f, &[(0, 0), (40, 0), (0, 40)], WHITE);
    assert_eq!(f.rects.len(), 20);
    assert!(
        f.rects.windows(2).all(|w| w[0].width > w[1].width),
        "a triangle narrows row after row"
    );
    assert_eq!(f.rects[0].width, 40.0);
    check_rects(&f.rects);

    let mut f = frame();
    fill_polygon(&mut f, &[(11, 11), (30, 11), (30, 21)], WHITE);
    assert!(!f.rects.is_empty());
    check_rects(&f.rects);
}

#[test]
fn filling_skips_what_cannot_be_seen() {
    let mut f = frame();
    fill_polygon(&mut f, &[(0, 0), (40, 0)], WHITE);
    assert!(f.rects.is_empty(), "not a polygon");
    fill_polygon(&mut f, &[(2000, 0), (2100, 0), (2100, 50)], WHITE);
    assert!(f.rects.is_empty(), "off the right edge");
    fill_polygon(&mut f, &[(-100, 0), (-50, 0), (-50, 50)], WHITE);
    assert!(f.rects.is_empty(), "off the left edge");
    fill_polygon(&mut f, &[(0, 800), (50, 800), (50, 900)], WHITE);
    assert!(f.rects.is_empty(), "below");
    fill_polygon(&mut f, &[(10, 10), (30, 10), (20, 10)], WHITE);
    assert!(f.rects.is_empty(), "flat");

    // Huge coordinates (the perspective of points near the eye) are clamped to the screen.
    fill_polygon(
        &mut f,
        &[(-100_000, -100_000), (100_000, -100_000), (0, 100_000)],
        WHITE,
    );
    assert_eq!(f.rects.len(), 360, "the whole screen");
    assert!(
        f.rects
            .iter()
            .all(|r| r.x == 0.0 && r.width == SCREEN_WIDTH)
    );
    check_rects(&f.rects);
}

#[test]
fn clipping_a_polygon_by_a_half_plane() {
    let square = [(0, 0), (10, 0), (10, 10), (0, 10)];
    // The vertical line x = 5 going down keeps its left side.
    let left = clip_half_plane(&square, (5, 0), (5, 10));
    assert_eq!(left, vec![(0, 0), (5, 0), (5, 10), (0, 10)]);
    let right = clip_half_plane(&square, (5, 10), (5, 0));
    assert_eq!(right, vec![(5, 0), (10, 0), (10, 10), (5, 10)]);
    let all = clip_half_plane(&square, (-5, 10), (-5, 0));
    assert_eq!(all.len(), 4, "the whole square is kept");
    let none = clip_half_plane(&square, (20, 10), (20, 0));
    assert!(none.is_empty());
    let corner = clip_half_plane(&square, (0, 12), (12, 0));
    assert_eq!(corner, vec![(10, 2), (10, 10), (2, 10)]);
}

#[test]
fn clipping_a_segment_to_the_screen() {
    assert_eq!(
        clip_segment((10, 10), (100, 50)),
        Some(((10, 10), (100, 50))),
        "inside: unchanged"
    );
    assert_eq!(
        clip_segment((-50, 10), (-20, 50)),
        None,
        "left of the screen"
    );
    assert_eq!(clip_segment((10, 900), (100, 950)), None, "below");
    assert_eq!(
        clip_segment((-100, 100), (100, 100)),
        Some(((-4, 100), (100, 100))),
        "the start is cut at the margin"
    );
    assert_eq!(
        clip_segment((100, 100), (100, 1000)),
        Some(((100, 100), (100, 724))),
        "the end is cut"
    );
    let (a, b) = clip_segment((-1000, -1000), (3000, 3000)).unwrap();
    assert_eq!(a, (-4, -4));
    assert_eq!(b, (724, 724), "both ends cut, the slope kept");
    let (a, b) = clip_segment((2000, 100), (-100, 100)).unwrap();
    assert_eq!((a, b), ((1284, 100), (-4, 100)));
}

#[test]
fn ink_lines_are_runs_of_grain_along_their_major_axis() {
    let mut f = frame();
    ink_line(&mut f, (10, 10), (50, 10), WHITE);
    assert_eq!(f.rects.len(), 1, "horizontal: a single run");
    assert_eq!(
        (
            f.rects[0].x,
            f.rects[0].y,
            f.rects[0].width,
            f.rects[0].height
        ),
        (10.0, 10.0, 42.0, 2.0)
    );

    let mut f = frame();
    ink_line(&mut f, (10, 50), (10, 10), WHITE);
    assert_eq!(f.rects.len(), 1, "vertical, drawn upwards: a single run");
    assert_eq!(
        (
            f.rects[0].x,
            f.rects[0].y,
            f.rects[0].width,
            f.rects[0].height
        ),
        (10.0, 10.0, 2.0, 42.0)
    );

    let mut f = frame();
    ink_line(&mut f, (50, 20), (10, 10), WHITE);
    assert_eq!(f.rects.len(), 5, "ten pixels down over forty: five steps");
    assert!(f.rects.iter().all(|r| r.height == 2.0));
    assert!(
        f.rects
            .windows(2)
            .all(|w| w[0].x < w[1].x && w[0].y < w[1].y)
    );

    let mut f = frame();
    ink_line(&mut f, (10, 50), (20, 10), WHITE);
    assert_eq!(f.rects.len(), 5, "steep: steps along y");
    assert!(f.rects.iter().all(|r| r.width == 2.0));

    let mut f = frame();
    ink_line(&mut f, (11, 11), (11, 11), WHITE);
    assert_eq!(f.rects.len(), 1, "a point is one grain");
    assert_eq!((f.rects[0].x, f.rects[0].y), (10.0, 10.0));

    let mut f = frame();
    ink_line(&mut f, (-50, 10), (-20, 10), WHITE);
    assert!(f.rects.is_empty(), "off screen");

    let mut f = frame();
    ink_line(&mut f, (-100_000, 360), (100_000, 360), WHITE);
    assert_eq!(f.rects.len(), 1, "clipped before being cut in runs");
    check_rects(&f.rects);
}

#[test]
fn a_ball_is_a_shaded_disc_with_a_lit_spot_and_an_outline() {
    let mut f = frame();
    ball_shape(&mut f, (100, 100), 20, WHITE);
    let shade = count(&f, WHITE.scaled(tps::SHADE));
    let lit = count(&f, WHITE);
    let ink = count(&f, INK);
    assert!(shade > 0 && lit > 0 && ink > 0, "{shade} {lit} {ink}");
    assert!(shade >= lit, "the lit spot is smaller");
    let lit_rects: Vec<&Rect> = f.rects.iter().filter(|r| r.color == WHITE).collect();
    let lit_top = lit_rects.iter().map(|r| r.y as i32).min().unwrap();
    let lit_left = lit_rects.iter().map(|r| r.x as i32).min().unwrap();
    assert!(
        (100 - 20..100 - 14).contains(&lit_top),
        "towards the top-left light"
    );
    assert!((100 - 20..100 - 14).contains(&lit_left));
    check_rects(&f.rects);

    let mut f = frame();
    ball_shape(&mut f, (100, 100), 3, WHITE);
    assert_eq!(count(&f, INK), 0, "too small for an outline");
    assert!(count(&f, WHITE.scaled(tps::SHADE)) > 0);
}

#[test]
fn a_tube_is_a_lit_capsule_with_a_shade_band_and_an_outline() {
    for (a, b) in [((50, 100), (200, 100)), ((200, 100), (50, 100))] {
        let mut f = frame();
        tube_shape(&mut f, a, 10, b, 10, WHITE);
        let lit: Vec<&Rect> = f.rects.iter().filter(|r| r.color == WHITE).collect();
        let shade: Vec<&Rect> = f
            .rects
            .iter()
            .filter(|r| r.color == WHITE.scaled(tps::SHADE))
            .collect();
        assert!(!lit.is_empty() && !shade.is_empty() && count(&f, INK) > 0);
        // Lit from the top-left: the shade band is the lower part, whichever way it is drawn.
        let shade_top = shade.iter().map(|r| r.y).fold(f32::MAX, f32::min);
        let lit_top = lit.iter().map(|r| r.y).fold(f32::MAX, f32::min);
        assert!(shade_top > lit_top, "{shade_top} > {lit_top}");
        assert!(shade.iter().all(|r| r.y >= 100.0), "below the axis");
        check_rects(&f.rects);
    }

    let mut f = frame();
    tube_shape(&mut f, (100, 200), 10, (100, 50), 10, WHITE);
    let shade: Vec<&Rect> = f
        .rects
        .iter()
        .filter(|r| r.color == WHITE.scaled(tps::SHADE))
        .collect();
    assert!(!shade.is_empty());
    assert!(
        shade.iter().all(|r| r.x + r.width > 100.0),
        "vertical: the shade is on the right"
    );

    let mut f = frame();
    tube_shape(&mut f, (100, 100), 10, (100, 100), 6, WHITE);
    let mut ball = frame();
    ball_shape(&mut ball, (100, 100), 10, WHITE);
    assert_eq!(f, ball, "no length: a ball of the larger radius");

    let mut f = frame();
    tube_shape(&mut f, (50, 100), 2, (200, 100), 2, WHITE);
    assert_eq!(count(&f, INK), 0, "too thin for an outline");
    assert!(count(&f, WHITE) > 0);
}

// ---------------------------------------------------------------------------------------------
// World-space drawing
// ---------------------------------------------------------------------------------------------

#[test]
fn polygons_are_cut_at_the_near_plane() {
    let square = [
        Vec3::int(-10, 0, 100),
        Vec3::int(10, 0, 100),
        Vec3::int(10, 0, 200),
        Vec3::int(-10, 0, 200),
    ];
    let kept = clip_near(&square);
    assert_eq!(kept.len(), 4);
    assert!(kept.iter().all(|&(_, created)| !created));

    let behind = [
        Vec3::int(-10, 0, -100),
        Vec3::int(10, 0, -100),
        Vec3::int(10, 0, 0),
    ];
    assert!(clip_near(&behind).is_empty());

    let crossing = [
        Vec3::int(-10, 0, -100),
        Vec3::int(10, 0, -100),
        Vec3::int(10, 0, 132),
        Vec3::int(-10, 0, 132),
    ];
    let kept = clip_near(&crossing);
    assert_eq!(kept.len(), 4);
    let created: Vec<Vec3> = kept
        .iter()
        .filter(|&&(_, created)| created)
        .map(|&(p, _)| p)
        .collect();
    assert_eq!(created, vec![Vec3::int(10, 0, 16), Vec3::int(-10, 0, 16)]);
    assert!(created.iter().all(|p| p.z == NEAR), "exactly on the plane");
    assert_eq!(kept[0], (Vec3::int(10, 0, 16), true));
    assert_eq!(kept[1], (Vec3::int(10, 0, 132), false));
}

#[test]
fn world_polygons_are_projected_filled_and_inked() {
    let camera = straight();
    let mut f = frame();
    let floor = [
        Vec3::int(-50, -20, 100),
        Vec3::int(50, -20, 100),
        Vec3::int(50, -20, 300),
        Vec3::int(-50, -20, 300),
    ];
    draw_polygon(&mut f, &camera, &floor, WHITE, Some(INK));
    assert!(count(&f, WHITE) > 0);
    assert!(count(&f, INK) > 0);
    // Below the centre (y = -20), the near edge lower (bigger) than the far one.
    assert!(f.rects.iter().all(|r| r.y >= 360.0));
    check_rects(&f.rects);

    let mut f = frame();
    draw_polygon(&mut f, &camera, &floor, WHITE, None);
    assert_eq!(count(&f, INK), 0);

    let mut f = frame();
    let behind = [
        Vec3::int(-50, 0, -100),
        Vec3::int(50, 0, -100),
        Vec3::int(0, 0, -50),
    ];
    draw_polygon(&mut f, &camera, &behind, WHITE, Some(INK));
    assert!(f.rects.is_empty(), "behind the eye");

    // A floor running under the camera: cut at the near plane, the cut edge not inked.
    let mut f = frame();
    let under = [
        Vec3::int(-50, -20, -100),
        Vec3::int(50, -20, -100),
        Vec3::int(50, -20, 300),
        Vec3::int(-50, -20, 300),
    ];
    draw_polygon(&mut f, &camera, &under, WHITE, Some(INK));
    assert!(count(&f, WHITE) > 0);
    let ink: Vec<&Rect> = f.rects.iter().filter(|r| r.color == INK).collect();
    assert!(!ink.is_empty());
    assert!(
        ink.iter().all(|r| r.y + r.height < SCREEN_HEIGHT),
        "the near plane's edge, at the bottom of the screen, is not an edge of the floor"
    );
    check_rects(&f.rects);
}

#[test]
fn world_lines_are_cut_at_the_near_plane() {
    let camera = straight();
    let mut f = frame();
    draw_line(
        &mut f,
        &camera,
        Vec3::int(-50, 0, 100),
        Vec3::int(50, 0, 100),
        WHITE,
    );
    assert_eq!(f.rects.len(), 1);
    assert_eq!((f.rects[0].x, f.rects[0].y), (190.0, 360.0));
    assert_eq!(f.rects[0].width, 902.0);

    let mut f = frame();
    draw_line(
        &mut f,
        &camera,
        Vec3::int(0, 0, -100),
        Vec3::int(0, 0, 100),
        WHITE,
    );
    assert!(!f.rects.is_empty(), "the part in front is drawn");
    check_rects(&f.rects);
    let mut g = frame();
    draw_line(
        &mut g,
        &camera,
        Vec3::int(0, 0, 100),
        Vec3::int(0, 0, -100),
        WHITE,
    );
    assert_eq!(f, g, "whichever end is behind");

    let mut f = frame();
    draw_line(
        &mut f,
        &camera,
        Vec3::int(0, 0, -100),
        Vec3::int(0, 0, -50),
        WHITE,
    );
    assert!(f.rects.is_empty(), "behind");
}

#[test]
fn world_tubes_and_balls_shrink_with_the_distance_and_vanish_near_the_eye() {
    let camera = straight();
    let mut near = frame();
    draw_ball(&mut near, &camera, Vec3::int(0, 0, 100), 10, WHITE);
    let mut far = frame();
    draw_ball(&mut far, &camera, Vec3::int(0, 0, 400), 10, WHITE);
    let width = |f: &Frame| {
        f.rects.iter().map(|r| r.x + r.width).fold(0.0, f32::max)
            - f.rects.iter().map(|r| r.x).fold(f32::MAX, f32::min)
    };
    assert!(
        width(&near) > 3.0 * width(&far),
        "{} {}",
        width(&near),
        width(&far)
    );
    check_rects(&near.rects);

    let mut f = frame();
    draw_ball(&mut f, &camera, Vec3::int(0, 0, 10), 10, WHITE);
    assert!(f.rects.is_empty(), "too close");

    let mut f = frame();
    draw_tube(
        &mut f,
        &camera,
        Vec3::int(-30, 0, 100),
        Vec3::int(30, 0, 300),
        5,
        WHITE,
    );
    assert!(count(&f, WHITE) > 0);
    check_rects(&f.rects);
    let mut f = frame();
    draw_tube(
        &mut f,
        &camera,
        Vec3::int(0, 0, 10),
        Vec3::int(0, 0, 100),
        5,
        WHITE,
    );
    assert!(f.rects.is_empty(), "one end too close");
}

#[test]
fn planes_get_three_flat_tones_and_the_distance_three_steps_of_fog() {
    assert_eq!(tone(Vec3::Y), 1.0, "lit from above");
    assert_eq!(tone(-Vec3::Z), 0.8, "facing the street: half lit");
    assert_eq!(tone(Vec3::X), 0.6, "facing right: in the shade");
    assert_eq!(fog(WHITE, Fx::int(100)), WHITE);
    assert_eq!(fog(WHITE, Fx::int(1000)), WHITE.mix(BACKGROUND, 0.35));
    assert_eq!(fog(WHITE, Fx::int(2000)), WHITE.mix(BACKGROUND, 0.6));
}

#[test]
fn side_view_points_are_lifted_into_the_world() {
    let p = lift(
        Point {
            x: ARENA_LEFT,
            y: FLOOR_Y,
        },
        0,
    );
    assert_eq!(p, Vec3::ZERO, "the left bound, on the pavement");
    let p = lift(
        Point {
            x: ARENA_LEFT + 100.0,
            y: FLOOR_Y - 50.5,
        },
        -7,
    );
    assert_eq!(p, Vec3::new(Fx::int(100), Fx::from_f32(50.5), Fx::int(-7)));
}

// ---------------------------------------------------------------------------------------------
// The camera
// ---------------------------------------------------------------------------------------------

/// An online client controlling `local` in a match between `players`, just started.
fn brawl(local: u8, players: PlayerSet) -> Client {
    let mut client = Client::online(MatchConfig::default(), 0);
    client.handle(ServerMessage::Welcome {
        player: PlayerId(local),
    });
    client.handle(ServerMessage::Lobby {
        seated: players,
        ready: PlayerSet::EMPTY,
    });
    client.handle(ServerMessage::Start {
        session: 1,
        players,
    });
    client
}

/// The camera over the shoulder of the fighter at side-view `x`, looking `ahead` (+1 / -1).
fn shoulder(x: f32, ahead: i32) -> Camera {
    let x = (x - ARENA_LEFT) as i32;
    Camera::look_at(
        Vec3::int(x - ahead * BACK, HEIGHT, -SIDE),
        Vec3::int(x + ahead * AHEAD, TARGET_HEIGHT, 0),
    )
}

#[test]
fn without_a_match_the_camera_overlooks_the_arena() {
    let client = Client::online(MatchConfig::default(), 0);
    assert_eq!(tps::aim(&client), overview());
    assert_eq!(overview().eye, Vec3::int(550, 260, -620));
    let centre = overview().project(Vec3::int(550, 70, 0)).unwrap();
    assert!((centre.x - 640).abs() <= 1 && (centre.y - 360).abs() <= 1);
}

#[test]
fn the_camera_stands_behind_the_local_fighter_facing_the_others() {
    let client = brawl(0, PlayerSet::first(2));
    let current = client.session().unwrap().current();
    let (x0, _) = feet(current.fighter(PlayerId(0)).unwrap(), current.config());
    assert_eq!(
        tps::aim(&client),
        shoulder(x0, 1),
        "the opponent is on the right"
    );

    let client = brawl(1, PlayerSet::first(2));
    let current = client.session().unwrap().current();
    let (x1, _) = feet(current.fighter(PlayerId(1)).unwrap(), current.config());
    assert_eq!(
        tps::aim(&client),
        shoulder(x1, -1),
        "the opponent is on the left"
    );

    // In the middle of a 1v1v1 the others are on both sides: look where the fighter faces.
    let client = brawl(1, PlayerSet::first(3));
    let current = client.session().unwrap().current();
    let middle = current.fighter(PlayerId(1)).unwrap();
    let (x, _) = feet(middle, current.config());
    assert_eq!(tps::aim(&client), shoulder(x, middle.facing().sign()));
}

// ---------------------------------------------------------------------------------------------
// The scene
// ---------------------------------------------------------------------------------------------

#[test]
fn without_a_match_only_the_alley_is_drawn() {
    let client = Client::online(MatchConfig::default(), 0);
    let mut f = frame();
    tps::draw(&mut f, &client, &overview(), 0);
    assert!(f.rects.len() > 100, "{}", f.rects.len());
    assert!(f.labels.is_empty(), "nobody to tag");
    check_rects(&f.rects);
    let mut flicker = frame();
    let tick = (0..600)
        .find(|&t| {
            let mut g = frame();
            tps::draw(&mut g, &client, &overview(), t);
            g != f
        })
        .expect("the neon flickers within ten seconds");
    tps::draw(&mut flicker, &client, &overview(), tick);
    assert_ne!(flicker.rects.len(), 0);
}

#[test]
fn a_brawl_draws_every_mannequin_with_its_accessory_and_tag() {
    let client = brawl(0, PlayerSet::first(4));
    let mut f = frame();
    tps::draw(&mut f, &client, &tps::aim(&client), 10);
    let tags: Vec<&str> = f.labels.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(tags, ["P1", "P2", "P3", "P4"]);
    for (label, player) in f.labels.iter().zip(0..) {
        assert_eq!(label.color, arena::player_color(PlayerId(player)));
    }
    assert!(count(&f, tps::WOOD) > 0, "wooden bodies");
    assert!(count(&f, Rgba::hex(0xfff299)) > 0, "the marker over P1");
    for player in 0..4 {
        let accent = arena::player_color(PlayerId(player));
        let worn = f.rects.iter().any(|r| {
            [Fx::int(0), Fx::int(1000), Fx::int(2000)]
                .iter()
                .any(|&depth| r.color == fog(accent, depth))
        });
        assert!(worn, "P{} wears its colour, fogged or not", player + 1);
    }
    assert!(count(&f, INK) > 0, "outlines");
    check_rects(&f.rects);
    assert!(f.rects.len() < 12_000, "{} rects", f.rects.len());

    let away = Camera::look_at(Vec3::int(550, 100, -600), Vec3::int(550, 100, -1200));
    let mut f = frame();
    tps::draw(&mut f, &client, &away, 10);
    assert!(f.labels.is_empty(), "nobody in sight, no tag");
    assert_eq!(count(&f, tps::WOOD), 0);
    check_rects(&f.rects);
}

/// A practice game on the fight screen against the dummy, with `max_health`, no countdown.
fn practice(max_health: i32) -> Headless {
    let config = MatchConfig {
        max_health,
        countdown_ticks: 0,
        ..MatchConfig::default()
    };
    let mut game = Headless::new(Game::new(Client::practice(config)));
    game.skip_intro();
    game.tap(&[Key::N]);
    game
}

fn dummy_state(game: &Headless) -> FighterState {
    game.game()
        .shown_match()
        .unwrap()
        .fighter(PlayerId(1))
        .unwrap()
        .state()
}

#[test]
fn the_mannequins_follow_the_fight_into_the_air_the_stun_and_the_floor() {
    let mut game = practice(20);
    game.game_mut().set_view(View::ThirdPerson);
    game.hold(&[Key::ArrowUp], 1);
    game.idle(3);
    let me = game
        .game()
        .shown_match()
        .unwrap()
        .fighter(PlayerId(0))
        .unwrap();
    assert_eq!(me.state(), FighterState::Airborne);
    let airborne = game.frame();
    within_screen(&airborne.rects);
    assert!(airborne.text().contains("P1 (you)"), "the HUD is on top");

    game.hold(&[Key::ArrowRight], 70);
    game.tap(&[Key::Numpad1]);
    game.idle(4);
    assert_eq!(dummy_state(&game), FighterState::HitStun);
    let stunned = game.frame();
    assert!(count(&stunned, srr::front::stick::SPARK) > 0, "stars");
    within_screen(&stunned.rects);

    // Every hit pushes the dummy back: step in before the next one.
    for _ in 0..8 {
        game.idle(20);
        game.hold(&[Key::ArrowRight], 8);
        game.tap(&[Key::Numpad1]);
    }
    game.idle(30);
    assert_eq!(dummy_state(&game), FighterState::Knockout);
    let down = game.frame();
    within_screen(&down.rects);
    let dummy = down.labels.iter().find(|l| l.text == "P2").expect("tagged");
    let me = down.labels.iter().find(|l| l.text == "P1").expect("tagged");
    assert!(dummy.y > me.y, "the tag of a fighter on the floor lies low");
    assert_ne!(airborne.rects, down.rects);
}

// ---------------------------------------------------------------------------------------------
// The game: Tab
// ---------------------------------------------------------------------------------------------

fn keys(list: &[Key]) -> HashSet<Key> {
    list.iter().copied().collect()
}

#[test]
fn tab_switches_the_view_in_the_middle_of_the_fight() {
    assert_eq!(VIEW_KEY, Key::Tab);
    assert_eq!(View::default(), View::Side);
    assert_eq!(View::Side.next(), View::ThirdPerson);
    assert_eq!(View::ThirdPerson.next(), View::Side);

    let mut game = practice(100);
    assert_eq!(game.game().view(), View::Side);
    assert_eq!(game.game().camera(), &overview());
    let side = game.screenshot();

    let before = game.game().shown_match().unwrap().remaining_ticks();
    game.hold(&[Key::Tab], 5);
    assert_eq!(
        game.game().view(),
        View::ThirdPerson,
        "a held Tab switches once"
    );
    assert_eq!(
        game.game().shown_match().unwrap().remaining_ticks(),
        before - 5,
        "the fight goes on"
    );
    assert_ne!(
        game.game().camera(),
        &overview(),
        "the camera is gliding in"
    );
    assert_eq!(game.frame().background, BACKGROUND);
    assert_ne!(game.screenshot().pixels(), side.pixels());
    let text = game.text_snapshot(160, 45);
    assert!(text.contains("P1") && text.contains("HP"), "{text}");

    game.idle(1);
    game.tap(&[Key::Tab]);
    assert_eq!(game.game().view(), View::Side);
    game.tap(&[Key::Tab]);
    assert_eq!(game.game().view(), View::ThirdPerson);

    // The view survives the pause menu, where Tab does nothing.
    game.tap(&[PAUSE_KEY]);
    assert_eq!(game.game().screen(), Screen::Controls);
    game.tap(&[Key::Tab]);
    game.tap(&[Key::Escape]);
    assert_eq!(game.game().screen(), Screen::Fight);
    assert_eq!(game.game().view(), View::ThirdPerson);
}

#[test]
fn the_camera_settles_over_the_shoulder() {
    let mut game = Game::new(Client::practice(MatchConfig::default()));
    while !game.intro().can_skip() {
        game.tick(&keys(&[]));
    }
    game.tick(&keys(&[Key::Enter]));
    game.tick(&keys(&[Key::N]));
    game.set_view(View::ThirdPerson);
    assert_eq!(game.view(), View::ThirdPerson);
    for _ in 0..200 {
        game.tick(&keys(&[]));
    }
    let aim = tps::aim(game.active_client());
    let camera = game.camera();
    let gap = (camera.eye - aim.eye).length();
    assert!(gap < Fx::int(1), "{camera:?} vs {aim:?}");
    assert!(camera.orientation.dot(aim.orientation).abs() > Fx::from_f32(0.995));
    assert_eq!(CAMERA_LAG, 3);
}
