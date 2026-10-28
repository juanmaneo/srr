//! The arena: a back alley wall tagged with graffiti, the street, and the fighters drawn as
//! XKCD-style stick figures ([`super::stick`]) whose pose follows their state, from strikes,
//! grabs and judo techniques to every position of a grapple (clinch, mount, submission, the
//! projections, the pin and the armbar).
//!
//! Arena coordinates (x right, y up from the floor, in the rules' units) are mapped to the
//! virtual screen by [`feet`]. The depth (`z`, how far a fighter sidestepped towards the wall
//! or the street) is shown like in the belt scrollers: further back stands higher on the
//! screen ([`rise`]), and is drawn first.

use std::cmp::Reverse;

use super::ascii::{block_pixels, hash};
use super::hud::tag;
use super::scene::{Align, Frame, Rgba, SCREEN_WIDTH};
use super::stick::{FIRE, Figure, Point, ring};
use crate::game::{
    DetachedFist, Facing, Fighter, FighterState, GrapplePosition, GrappleRole, Match, MatchConfig,
    PlayerId, Projectile, Strength,
};
use crate::net::Client;

/// The screen's colour.
pub const BACKGROUND: Rgba = Rgba::hex(0x07060b);
/// Screen y of the floor.
pub const FLOOR_Y: f32 = 600.0;
/// Screen x of the arena's left end (arena x = 0).
pub const ARENA_LEFT: f32 = 90.0;
/// On-screen width of the whole arena.
pub const ARENA_PIXELS: f32 = 1100.0;
/// Where the name tag of a fighter lying on the floor goes: on the street, under it.
pub const LYING_TAG_Y: f32 = FLOOR_Y + 12.0;
/// How far above the top of a standing figure its name tag floats: clear of the local
/// player's marker.
pub const TAG_ABOVE: f32 = 36.0;
/// Screen pixels above the floor a fireball flies at: chest height, where it leaves the hands.
pub const FIREBALL_HEIGHT: f32 = 72.0;
/// Radius of a fireball on screen.
pub const FIREBALL_RADIUS: f32 = 13.0;
/// Screen pixels a fighter rises per pixel it stands towards the wall: the depth of the alley,
/// squashed.
pub const DEPTH_RISE: f32 = 0.25;

const MARKER_WIDTH: f32 = 18.0;
const MARKER_HEIGHT: f32 = 10.0;

const WALL_TOP: f32 = 96.0;
const BRICK_WIDTH: f32 = 64.0;
const BRICK_HEIGHT: f32 = 24.0;
const MORTAR: f32 = 3.0;
const STREET_HEIGHT: f32 = 56.0;

/// One colour per seat of a full room of ten: neon hues far enough apart to tell a brawl apart.
const PALETTE: [Rgba; 10] = [
    Rgba::hex(0xe84a4a),
    Rgba::hex(0x4a8cf2),
    Rgba::hex(0x4ad88a),
    Rgba::hex(0xe8b84a),
    Rgba::hex(0xc25af2),
    Rgba::hex(0x3fd8e0),
    Rgba::hex(0xf2783a),
    Rgba::hex(0xb8f24a),
    Rgba::hex(0xf24ad8),
    Rgba::hex(0xffb0c8),
];
const KNOCKED_OUT: Rgba = Rgba::hex(0x595959);
const STUNNED: Rgba = Rgba::hex(0xffffff);
const MARKER: Rgba = Rgba::hex(0xfff299);
const SHADOW: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.45,
};

const WALL_BRICK: Rgba = Rgba::hex(0x1a1422);
const WALL_MORTAR: Rgba = Rgba::hex(0x0e0b14);
const STREET: Rgba = Rgba::hex(0x15121c);
const CURB: Rgba = Rgba::hex(0x3a3448);
const LINE: Rgba = Rgba::hex(0x4a4258);
const TAG: [Rgba; 3] = [
    Rgba::hex(0x2a1840),
    Rgba::hex(0x3a1430),
    Rgba::hex(0x481420),
];
const NEON: Rgba = Rgba::hex(0xff3ab4);

/// The colour of `player`: name tag, accessory, HUD. Every seat of the room has its own; they
/// would only cycle beyond [`MAX_PLAYERS`](crate::net::MAX_PLAYERS).
pub fn player_color(player: PlayerId) -> Rgba {
    PALETTE[usize::from(player.0) % PALETTE.len()]
}

/// Virtual pixels per arena unit.
pub fn scale(config: &MatchConfig) -> f32 {
    ARENA_PIXELS / config.arena_width.max(1) as f32
}

/// Screen position of the middle of `fighter`'s feet.
///
/// ```
/// use srr::front::arena::{ARENA_LEFT, FLOOR_Y, feet};
/// use srr::game::{Match, MatchConfig, PlayerId, Roster};
///
/// let m = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
/// let (x, y) = feet(m.fighter(PlayerId(0)).unwrap(), m.config());
/// assert!(x > ARENA_LEFT);
/// assert_eq!(y, FLOOR_Y, "standing on the floor");
/// ```
pub fn feet(fighter: &Fighter, config: &MatchConfig) -> (f32, f32) {
    let scale = scale(config);
    (
        ARENA_LEFT + fighter.x() as f32 * scale,
        FLOOR_Y - fighter.y() as f32 * scale,
    )
}

/// How much higher than [`feet`] the side view draws `fighter`: its depth, squashed by
/// [`DEPTH_RISE`] (negative towards the street).
///
/// ```
/// use srr::front::arena::rise;
/// use srr::game::{Match, MatchConfig, PlayerId, Roster};
///
/// let m = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
/// assert_eq!(rise(m.fighter(PlayerId(0)).unwrap(), m.config()), 0.0, "on the middle line");
/// ```
pub fn rise(fighter: &Fighter, config: &MatchConfig) -> f32 {
    fighter.z() as f32 * scale(config) * DEPTH_RISE
}

/// The ink of a figure in `state`: grey when knocked out, a white flash when stunned, `base`
/// otherwise (the guard is shown by the pose).
pub fn body_color(base: Rgba, state: FighterState) -> Rgba {
    match state {
        FighterState::Knockout => KNOCKED_OUT,
        FighterState::HitStun => STUNNED,
        _ => base,
    }
}

/// The rules `fighter` of `current` follows (its character's, see
/// [`Match::rules`](crate::game::Match::rules)).
pub fn rules_of<'a>(current: &'a Match, fighter: &Fighter) -> &'a MatchConfig {
    current.rules(fighter.player()).unwrap_or(current.config())
}

/// The name tag over a fighter: its player's [`tag`] and its character, e.g. `"P1 Torch"`.
pub fn figure_tag(client: &Client, current: &Match, player: PlayerId) -> String {
    format!(
        "{} {}",
        tag(client, player),
        current.config().character(player)
    )
}

/// Screen centre of `shot` in the side view (depth included, like the fighters).
///
/// ```
/// use srr::front::arena::{ARENA_LEFT, FIREBALL_HEIGHT, FLOOR_Y, shot_center};
/// use srr::game::{Character, Input, Match, MatchConfig, PlayerId, Roster};
/// use std::collections::HashMap;
///
/// let config = MatchConfig { countdown_ticks: 0, ..MatchConfig::default() }
///     .with_character(PlayerId(0), Character::Torch);
/// let mut m = Match::new(Roster::duel(), config).unwrap();
/// let fireball = HashMap::from([(PlayerId(0), Input { special: true, ..Input::NONE })]);
/// for _ in 0..13 {
///     m.step(&fireball);
/// }
/// let center = shot_center(&m.projectiles()[0], m.config());
/// assert!(center.x > ARENA_LEFT);
/// assert_eq!(center.y, FLOOR_Y - FIREBALL_HEIGHT);
/// ```
pub fn shot_center(shot: &Projectile, config: &MatchConfig) -> Point {
    let scale = scale(config);
    Point {
        x: ARENA_LEFT + shot.x() as f32 * scale,
        y: FLOOR_Y
            - shot.y() as f32 * scale
            - FIREBALL_HEIGHT
            - shot.z() as f32 * scale * DEPTH_RISE,
    }
}

/// Where a broken-off fist ([`DetachedFist`]) floats on screen, at the height it snapped off.
pub fn fist_center(fist: &DetachedFist, config: &MatchConfig) -> Point {
    let scale = scale(config);
    Point {
        x: ARENA_LEFT + fist.x() as f32 * scale,
        y: FLOOR_Y
            - fist.y() as f32 * scale
            - FIREBALL_HEIGHT
            - fist.z() as f32 * scale * DEPTH_RISE,
    }
}

/// A fist broken off at full stretch, hanging in the air: the chalk-white outline of a fist,
/// trembling where it floats.
pub fn draw_fist(frame: &mut Frame, center: Point, fight_ticks: u32) {
    let jitter = ((fight_ticks % 4) as f32 - 1.5) * 1.5;
    for (dx, dy) in [(0.0, -5.0), (0.0, 5.0), (-5.0, 0.0), (5.0, 0.0)] {
        frame.rect(
            center.x + dx + jitter - 2.0,
            center.y + dy - 2.0,
            4.0,
            4.0,
            Rgba::hex(0xfff6d8),
        );
    }
    frame.rect(
        center.x + jitter - 3.0,
        center.y - 3.0,
        6.0,
        6.0,
        Rgba::hex(0xe8e8e8),
    );
}

/// A fireball centred on `center`, flying towards `facing`: a flickering trail of flames
/// behind it, then glowing rings from the red rim to the white-hot core.
pub fn draw_fireball(frame: &mut Frame, center: Point, facing: Facing, fight_ticks: u32) {
    let behind = -facing.sign() as f32;
    for (index, color) in [Rgba::hex(0xff3a1e), FIRE, Rgba::hex(0xffd24a)]
        .into_iter()
        .enumerate()
    {
        let flicker = ((fight_ticks + index as u32 * 3) % 6) as f32;
        let length = 18.0 + 8.0 * index as f32 + flicker;
        let height = 10.0 - 3.0 * index as f32;
        let start = if behind < 0.0 {
            center.x - FIREBALL_RADIUS * 0.5 - length
        } else {
            center.x + FIREBALL_RADIUS * 0.5
        };
        frame.rect(
            start,
            center.y - height / 2.0,
            length,
            height,
            color.scaled(0.8),
        );
    }
    for (radius, color) in [
        (FIREBALL_RADIUS, Rgba::hex(0xff3a1e)),
        (FIREBALL_RADIUS * 0.75, FIRE),
        (FIREBALL_RADIUS * 0.5, Rgba::hex(0xffd24a)),
        (FIREBALL_RADIUS * 0.25, Rgba::hex(0xfff6d8)),
    ] {
        ring(frame, center, radius * 0.6, radius * 0.8, color);
    }
}

/// The colour of an attack of `strength`.
pub fn strength_color(strength: Strength) -> Rgba {
    match strength {
        Strength::Low => Rgba::hex(0xf2f28c),
        Strength::Medium => Rgba::hex(0xffa633),
        Strength::High => Rgba::hex(0xff4026),
    }
}

/// Draws the arena and, when a match runs, its fighters (the computer's tagged "CPU").
pub fn draw(frame: &mut Frame, client: &Client, fight_ticks: u32) {
    draw_wall(frame, fight_ticks);
    let Some(session) = client.session() else {
        return;
    };
    let current = session.current();
    let config = current.config();
    let figure = |fighter: &Fighter| {
        Figure::of(fighter, rules_of(current, fighter), fight_ticks).raised(rise(fighter, config))
    };
    for fighter in current.fighters() {
        let (x, _) = feet(fighter, config);
        let width = if fighter.y() > 0 { 36.0 } else { 64.0 };
        let y = FLOOR_Y - rise(fighter, config) - 4.0;
        frame.rect(x - width / 2.0, y, width, 8.0, SHADOW);
    }
    // The furthest back first. On the same line, fighters underneath a grapple first, so the
    // one on top covers them, and the ones reaching out last, so their limbs are never hidden.
    let mut order: Vec<&Fighter> = current.fighters().iter().collect();
    order.sort_by_key(|f| {
        let layer = match f.state() {
            FighterState::Knockout => 0,
            // Thrown over the shoulder: in front of the thrower.
            FighterState::Grappling(GrappleRole::Bottom, GrapplePosition::Throw) => 3,
            FighterState::Grappling(GrappleRole::Bottom, _) => 1,
            FighterState::Attacking(_)
            | FighterState::Special(_)
            | FighterState::Grabbing(_)
            | FighterState::Technique(..) => 3,
            _ => 2,
        };
        (Reverse(f.z()), layer)
    });
    for fighter in order {
        // Every fighter wears the skin its player picked, whatever the others wear. The
        // chalk is drawn by the stick engine (the debug view); the manga fighter is its
        // AI-art sprite, drawn by the image primitive (the art carries the look).
        let skin = client.skin(fighter.player());
        if let Some(name) = super::skin::sprite_name_of(&skin) {
            let (x, y) = feet(fighter, config);
            let risen = rise(fighter, config);
            let pose = super::skin::pose_of(fighter, fight_ticks);
            frame.image(name, pose, x, y - risen, 150.0, fighter.facing().sign() > 0);
        } else {
            super::skin::draw_inked(frame, &figure(fighter), &skin);
        }
    }
    for shot in current.projectiles() {
        draw_fireball(frame, shot_center(shot, config), shot.facing(), fight_ticks);
    }
    for fighter in current.fighters() {
        if let Some(fist) = fighter.detached_fist() {
            draw_fist(frame, fist_center(fist, config), fight_ticks);
        }
    }
    // A session only exists for a local player who is in the match, so there is one fighter;
    // a spectator watches nobody in particular, so no fighter is marked.
    session
        .local_player()
        .and_then(|me| current.fighter(me))
        .iter()
        .for_each(|&local| {
            let (x, _) = feet(local, config);
            let top = figure(local).top();
            frame.rect(
                x - MARKER_WIDTH / 2.0,
                top - 8.0 - MARKER_HEIGHT,
                MARKER_WIDTH,
                MARKER_HEIGHT,
                MARKER,
            );
        });
    for fighter in current.fighters() {
        let (x, _) = feet(fighter, config);
        let figure = figure(fighter);
        // Name tags float over the head, or lie on the street under a grounded fighter.
        let tag_y = if figure.lying {
            LYING_TAG_Y - rise(fighter, config)
        } else {
            figure.top() - TAG_ABOVE
        };
        frame.label(
            figure_tag(client, current, fighter.player()),
            x,
            tag_y,
            14.0,
            player_color(fighter.player()),
            Align::Center,
        );
    }
}

/// The back alley: a brick wall with a big dim tag and a flickering neon strip, then the street.
fn draw_wall(frame: &mut Frame, fight_ticks: u32) {
    let street_top = FLOOR_Y;
    frame.rect(
        0.0,
        WALL_TOP,
        SCREEN_WIDTH,
        street_top - WALL_TOP,
        WALL_MORTAR,
    );
    let rows = ((street_top - WALL_TOP) / BRICK_HEIGHT).ceil() as i32;
    for row in 0..rows {
        let y = WALL_TOP + row as f32 * BRICK_HEIGHT;
        let height = (BRICK_HEIGHT - MORTAR).min(street_top - y - MORTAR);
        let shift = if row % 2 == 0 { 0.0 } else { BRICK_WIDTH / 2.0 };
        let mut x = -shift;
        let mut column = 0;
        while x < SCREEN_WIDTH {
            // Some bricks are a bit lighter or darker, like a real wall.
            let tone = 0.8 + (hash(row, column, 7) % 5) as f32 * 0.1;
            frame.rect(
                x.max(0.0),
                y,
                (x + BRICK_WIDTH - MORTAR).min(SCREEN_WIDTH) - x.max(0.0),
                height,
                WALL_BRICK.scaled(tone),
            );
            x += BRICK_WIDTH;
            column += 1;
        }
    }

    // The tag, sprayed across the wall in the block font of the title.
    let (pixels, width) = block_pixels("SRR");
    let cell = 30.0;
    let left = (SCREEN_WIDTH - width as f32 * cell) / 2.0;
    let top = WALL_TOP + 70.0;
    for (x, y) in pixels {
        let color = TAG[(y as usize * TAG.len()) / 5];
        frame.rect(
            left + x as f32 * cell + 6.0,
            top + y as f32 * cell + 6.0,
            cell,
            cell,
            BACKGROUND,
        );
        frame.rect(
            left + x as f32 * cell,
            top + y as f32 * cell,
            cell,
            cell,
            color,
        );
    }

    // A neon strip that flickers now and then.
    let flicker = hash(fight_ticks as i32 / 6, 3, 11).is_multiple_of(9);
    let neon = if flicker { NEON.scaled(0.3) } else { NEON };
    frame.rect(120.0, WALL_TOP + 24.0, 220.0, 5.0, neon);
    frame.rect(120.0, WALL_TOP + 29.0, 220.0, 14.0, neon.with_alpha(0.12));

    frame.rect(0.0, street_top, SCREEN_WIDTH, STREET_HEIGHT, STREET);
    frame.rect(0.0, street_top, SCREEN_WIDTH, 4.0, CURB);
    let mut x = 20.0;
    while x < SCREEN_WIDTH {
        frame.rect(x, street_top + STREET_HEIGHT / 2.0, 70.0, 4.0, LINE);
        x += 140.0;
    }
}
