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
use super::stick::Figure;
use crate::game::{
    Fighter, FighterState, GrapplePosition, GrappleRole, MatchConfig, PlayerId, Strength,
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
/// How far above the top of a standing figure its name tag floats.
pub const TAG_ABOVE: f32 = 30.0;
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

const PALETTE: [Rgba; 4] = [
    Rgba::hex(0xe84a4a),
    Rgba::hex(0x4a8cf2),
    Rgba::hex(0x4ad88a),
    Rgba::hex(0xe8b84a),
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

/// The colour of `player`: name tag, accessory, HUD.
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
    let figure =
        |fighter: &Fighter| Figure::of(fighter, config, fight_ticks).raised(rise(fighter, config));
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
            | FighterState::Grabbing(_)
            | FighterState::Technique(..) => 3,
            _ => 2,
        };
        (Reverse(f.z()), layer)
    });
    for fighter in order {
        figure(fighter).draw(frame);
    }
    if let Some(local) = current.fighter(session.local_player()) {
        let (x, _) = feet(local, config);
        let top = figure(local).top();
        frame.rect(
            x - MARKER_WIDTH / 2.0,
            top - 8.0 - MARKER_HEIGHT,
            MARKER_WIDTH,
            MARKER_HEIGHT,
            MARKER,
        );
    }
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
            tag(client, fighter.player()),
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
