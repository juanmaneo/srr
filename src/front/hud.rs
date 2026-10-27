//! The versus HUD: one health bar per fighter (two big ones facing each other in a 1v1, like in
//! every versus game; thinner stacked ones in bigger matches, up to five a side in a brawl of
//! ten), the red trail of recent damage, the combo counter, round wins, the timer, the grapple
//! meter, the network state of each player (`LAG`, `LEFT`), the big centred banners (READY /
//! FIGHT! / K.O. / results / lobby) and the practice notice.

use super::ascii::{Canvas, block_pixels};
use super::controls::{controls_lines, technique_keys};
use super::scene::{Align, Frame, Rgba, SCREEN_HEIGHT, SCREEN_WIDTH};
use super::seconds;
use crate::game::{
    Fighter, Grapple, GrapplePosition, GrappleRole, KeyBindings, Match, Outcome, Phase, PlayerId,
    TICKS_PER_SECOND,
};
use crate::net::{Client, MAX_PLAYERS, PlayerSet, Session, Status};

/// Ticks the red trail waits after a hit before draining.
pub const TRAIL_HOLD: u32 = 30;
/// Health the red trail loses per tick while draining.
pub const TRAIL_SPEED: i32 = 1;
/// Ticks the combo counter stays under the bar after the fighter recovered.
pub const COMBO_LINGER: u32 = 45;
/// Hits from which a combo is counted under the bar.
pub const COMBO_MIN_HITS: u32 = 2;
/// Below this share of the maximum health (in percent) the bar blinks.
pub const DANGER_PERCENT: i32 = 25;
/// Stalled ticks (a quarter of a second) before telling that the server is not heard from.
pub const STALL_WARNING_TICKS: u32 = TICKS_PER_SECOND / 4;
/// The banner of a match waiting for the server (see [`Client::stalled_ticks`]).
pub const STALL_TEXT: &str = "Waiting for the server...";

/// Top of the first line of controls at the bottom of the fight screen (see
/// [`controls_lines`]); the second one is [`CONTROLS_LINE_HEIGHT`] lower.
pub const CONTROLS_Y: f32 = SCREEN_HEIGHT - 50.0;
/// Distance between the two lines of controls.
pub const CONTROLS_LINE_HEIGHT: f32 = 20.0;

const MARGIN: f32 = 40.0;
const TIMER_WIDTH: f32 = 96.0;
const BAR_TOP: f32 = 28.0;
const BAR_HEIGHT: f32 = 26.0;
const SMALL_BAR_HEIGHT: f32 = 14.0;
const TINY_BAR_HEIGHT: f32 = 8.0;
const BORDER: f32 = 3.0;

const FRAME_COLOR: Rgba = Rgba::hex(0xe8e0f0);
const EMPTY: Rgba = Rgba::hex(0x22080e);
const TRAIL: Rgba = Rgba::hex(0xe8202a);
const HEALTH: Rgba = Rgba::hex(0xffd400);
const HEALTH_LOW: Rgba = Rgba::hex(0xff7a00);
const NAME: Rgba = Rgba::hex(0xf4f0ff);
const LOCAL_NAME: Rgba = Rgba::hex(0xfff08a);
const WIN_ON: Rgba = Rgba::hex(0xffd400);
const WIN_OFF: Rgba = Rgba::hex(0x3a3448);
const TIMER: Rgba = Rgba::hex(0xffffff);
const TIMER_LOW: Rgba = Rgba::hex(0xff4a3a);
const GRAPPLE_ON: Rgba = Rgba::hex(0x6cff8a);
const GRAPPLE_OFF: Rgba = Rgba::hex(0x4a5a50);
const ESCAPE: Rgba = Rgba::hex(0x3fc8ff);
const COOLING: Rgba = Rgba::hex(0x5a3a2a);
const BANNER: Rgba = Rgba::hex(0xffd84a);
const BANNER_SMALL: Rgba = Rgba::hex(0xf4f0ff);
const CONTROLS_COLOR: Rgba = Rgba::hex(0x8a8098);
const NOTICE: Rgba = Rgba::hex(0x3fc8ff);
/// Top of the line about practice: just above the controls, out of the fighters' way.
pub const NOTICE_Y: f32 = CONTROLS_Y - 26.0;
const BIG_HOT: [Rgba; 5] = [
    Rgba::hex(0xfff35c),
    Rgba::hex(0xffc93a),
    Rgba::hex(0xff8a25),
    Rgba::hex(0xff4a1a),
    Rgba::hex(0xd81e1e),
];

/// The state of one health bar: the health shown, the trail of recent damage that drains
/// after a short pause, so a combo is readable at a glance, and the combo counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthBar {
    player: PlayerId,
    health: i32,
    trail: i32,
    hold: u32,
    hits: u32,
    linger: u32,
}

impl HealthBar {
    /// A full bar (no trail, no combo) for `player` at `health`.
    pub fn new(player: PlayerId, health: i32) -> Self {
        HealthBar {
            player,
            health,
            trail: health,
            hold: 0,
            hits: 0,
            linger: 0,
        }
    }

    /// The fighter the bar belongs to.
    pub fn player(&self) -> PlayerId {
        self.player
    }

    /// Health shown.
    pub fn health(&self) -> i32 {
        self.health
    }

    /// Health the red trail reaches (at least [`HealthBar::health`]).
    pub fn trail(&self) -> i32 {
        self.trail
    }

    /// One tick with the fighter's current health. A new hit restarts the pause; once
    /// [`TRAIL_HOLD`] ticks passed without one, the trail drains by [`TRAIL_SPEED`] per tick.
    /// Health going up (a new round) resets the trail.
    ///
    /// ```
    /// use srr::front::HealthBar;
    /// use srr::front::hud::TRAIL_HOLD;
    /// use srr::game::PlayerId;
    ///
    /// let mut bar = HealthBar::new(PlayerId(0), 100);
    /// bar.update(80);
    /// assert_eq!((bar.health(), bar.trail()), (80, 100));
    /// for _ in 0..TRAIL_HOLD { bar.update(80); }
    /// assert_eq!(bar.trail(), 100, "still waiting");
    /// bar.update(80);
    /// assert_eq!(bar.trail(), 99, "draining");
    /// ```
    pub fn update(&mut self, health: i32) {
        if health >= self.trail {
            self.trail = health;
            self.hold = 0;
        } else if health < self.health {
            self.hold = 0;
        } else if self.hold < TRAIL_HOLD {
            self.hold += 1;
        } else {
            self.trail = (self.trail - TRAIL_SPEED).max(health);
        }
        self.health = health;
    }

    /// One tick with the hits of the combo the fighter is caught in
    /// ([`Fighter::combo_hits`]): the count shows at once and, once the fighter recovers (zero
    /// hits), lingers for [`COMBO_LINGER`] ticks.
    ///
    /// ```
    /// use srr::front::HealthBar;
    /// use srr::front::hud::COMBO_LINGER;
    /// use srr::game::PlayerId;
    ///
    /// let mut bar = HealthBar::new(PlayerId(0), 100);
    /// bar.count(3);
    /// assert_eq!(bar.hits(), 3);
    /// for _ in 0..COMBO_LINGER { bar.count(0); }
    /// assert_eq!(bar.hits(), 3, "still lingering");
    /// bar.count(0);
    /// assert_eq!(bar.hits(), 0);
    /// ```
    pub fn count(&mut self, hits: u32) {
        if hits > 0 {
            self.hits = hits;
            self.linger = COMBO_LINGER;
        } else if self.linger > 0 {
            self.linger -= 1;
        } else {
            self.hits = 0;
        }
    }

    /// The hits of the combo shown under the bar (zero: none).
    pub fn hits(&self) -> u32 {
        self.hits
    }
}

/// Where a bar goes: its box, and which side of the screen it is on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarSlot {
    /// Left edge of the bar (border excluded).
    pub x: f32,
    /// Top edge of the bar (border excluded).
    pub y: f32,
    /// Width of a full bar.
    pub width: f32,
    /// Height of the bar.
    pub height: f32,
    /// Left bars empty towards the centre and are anchored on the left edge of the screen;
    /// right bars mirror them.
    pub left_side: bool,
}

/// The slots of `count` bars: the first half (rounded up) on the left, the rest on the right,
/// stacked from the top. A duel gets two big bars; bigger matches get thinner ones, and more
/// than four players tinier ones still, so that five rows stay well above the fighters.
///
/// ```
/// use srr::front::hud::bar_slots;
///
/// let duel = bar_slots(2);
/// assert!(duel[0].left_side && !duel[1].left_side);
/// assert_eq!(duel[0].y, duel[1].y);
/// assert_eq!(duel[0].width, duel[1].width);
///
/// let brawl = bar_slots(10);
/// assert_eq!(brawl.iter().filter(|slot| slot.left_side).count(), 5);
/// assert!(brawl[4].y < 300.0, "the HUD stays in the sky");
/// ```
pub fn bar_slots(count: usize) -> Vec<BarSlot> {
    let left_count = count.div_ceil(2);
    let width = (SCREEN_WIDTH - TIMER_WIDTH) / 2.0 - MARGIN - 16.0;
    // Thin bars leave room under each one for its name and the grapple meter.
    let (height, spacing) = match count {
        0..=2 => (BAR_HEIGHT, 0.0),
        3..=4 => (SMALL_BAR_HEIGHT, SMALL_BAR_HEIGHT + 50.0),
        _ => (TINY_BAR_HEIGHT, TINY_BAR_HEIGHT + 40.0),
    };
    (0..count)
        .map(|index| {
            let left_side = index < left_count;
            let row = if left_side { index } else { index - left_count };
            BarSlot {
                x: if left_side {
                    MARGIN
                } else {
                    SCREEN_WIDTH - MARGIN - width
                },
                y: BAR_TOP + row as f32 * spacing,
                width,
                height,
                left_side,
            }
        })
        .collect()
}

/// "P1", "P3"... or "CPU" for the computer: the name tag over a fighter.
pub fn tag(client: &Client, player: PlayerId) -> String {
    if client.bot_player() == Some(player) {
        "CPU".to_string()
    } else {
        format!("P{}", u32::from(player.0) + 1)
    }
}

/// "P1", "P2 (you)", "CPU"...: the name under a health bar.
pub fn name(client: &Client, player: PlayerId) -> String {
    let local = client
        .session()
        .is_some_and(|session| session.local_player() == Some(player));
    let you = if local { " (you)" } else { "" };
    format!("{}{you}", tag(client, player))
}

/// "P2, P3": the tags of `players`, by increasing id.
fn tags(client: &Client, players: PlayerSet) -> String {
    let tags: Vec<String> = players.iter().map(|player| tag(client, player)).collect();
    tags.join(", ")
}

/// Draws the HUD of the fight screen; the controls and the counter prompts name the keys of
/// `bindings`.
pub fn draw(
    frame: &mut Frame,
    client: &Client,
    bars: &[HealthBar],
    fight_ticks: u32,
    bindings: &KeyBindings,
) {
    if let Some(session) = client.session() {
        let current = session.current();
        for ((slot, bar), fighter) in bar_slots(bars.len())
            .into_iter()
            .zip(bars)
            .zip(current.fighters())
        {
            let hud = BarHud {
                client,
                session,
                fight_ticks,
                bindings,
            };
            draw_bar(frame, &slot, bar, fighter, &hud);
        }
        draw_timer(frame, current);
    }
    draw_banner(frame, client, fight_ticks);
    for (index, line) in controls_lines(bindings).into_iter().enumerate() {
        frame.label(
            line,
            SCREEN_WIDTH / 2.0,
            CONTROLS_Y + index as f32 * CONTROLS_LINE_HEIGHT,
            14.0,
            CONTROLS_COLOR,
            Align::Center,
        );
    }
}

/// What every bar of the HUD shares.
struct BarHud<'a> {
    client: &'a Client,
    session: &'a Session,
    fight_ticks: u32,
    bindings: &'a KeyBindings,
}

fn draw_bar(frame: &mut Frame, slot: &BarSlot, bar: &HealthBar, fighter: &Fighter, hud: &BarHud) {
    let BarHud {
        client,
        session,
        fight_ticks,
        bindings,
    } = *hud;
    let current = session.current();
    // Every character has its own maximum: a full bar is a full bar.
    let rules = super::arena::rules_of(current, fighter);
    let max = rules.max_health.max(1);
    let share = |health: i32| slot.width * health.clamp(0, max) as f32 / max as f32;
    // Anchored on the outer edge: the bar empties towards the timer.
    let span = |length: f32| {
        if slot.left_side {
            slot.x
        } else {
            slot.x + slot.width - length
        }
    };

    frame.rect(
        slot.x - BORDER,
        slot.y - BORDER,
        slot.width + 2.0 * BORDER,
        slot.height + 2.0 * BORDER,
        FRAME_COLOR,
    );
    frame.rect(slot.x, slot.y, slot.width, slot.height, EMPTY);
    let trail = share(bar.trail());
    frame.rect(span(trail), slot.y, trail, slot.height, TRAIL);
    let health = share(bar.health());
    let low = bar.health() * 100 <= max * DANGER_PERCENT;
    let color = if low && (fight_ticks / 8).is_multiple_of(2) {
        HEALTH_LOW
    } else {
        HEALTH
    };
    frame.rect(span(health), slot.y, health, slot.height, color);
    // A thin highlight on the top third gives the bar some volume.
    frame.rect(
        span(health),
        slot.y,
        health,
        (slot.height / 4.0).floor(),
        color.mix(Rgba::rgb(1.0, 1.0, 1.0), 0.45),
    );

    let player = fighter.player();
    let label_y = slot.y + slot.height + BORDER + 4.0;
    let size = if slot.height >= BAR_HEIGHT {
        18.0
    } else {
        13.0
    };
    let (x, align) = if slot.left_side {
        (slot.x, Align::Left)
    } else {
        (slot.x + slot.width, Align::Right)
    };
    let local = session.local_player() == Some(player);
    frame.label(
        format!(
            "{} {}  {:>3} HP{}",
            name(client, player),
            current.config().character(player),
            fighter.health(),
            network_mark(client, session, player)
        ),
        x,
        label_y,
        size,
        if local { LOCAL_NAME } else { NAME },
        align,
    );

    // Round wins as pips next to the timer side of the bar.
    let wins = session.wins(player);
    let pip = size * 0.6;
    for index in 0..3u32 {
        let offset = (index as f32 + 1.0) * (pip + 6.0);
        let px = if slot.left_side {
            slot.x + slot.width - offset
        } else {
            slot.x + offset - pip
        };
        let lit = index < wins;
        frame.rect(
            px,
            label_y + 3.0,
            pip,
            pip,
            if lit { WIN_ON } else { WIN_OFF },
        );
    }
    if wins > 3 {
        let text_x = if slot.left_side {
            slot.x + slot.width - 4.0 * (pip + 6.0)
        } else {
            slot.x + 4.0 * (pip + 6.0)
        };
        frame.label(
            format!("x{wins}"),
            text_x,
            label_y,
            size * 0.8,
            WIN_ON,
            if slot.left_side {
                Align::Right
            } else {
                Align::Left
            },
        );
    }

    // A special move that has to cool down (the fireball) fills a thin gauge along the bottom
    // of the bar while it does, and the gauge disappears once it is ready again.
    if fighter.special_cooldown() > 0 {
        // Only the rules start a cooldown, so they have one.
        let cooldown = rules.special.cooldown.max(1);
        let ready = 1.0 - fighter.special_cooldown() as f32 / cooldown as f32;
        let length = slot.width * ready;
        let gauge_y = slot.y + slot.height - 3.0;
        frame.rect(slot.x, gauge_y, slot.width, 3.0, COOLING);
        frame.rect(span(length), gauge_y, length, 3.0, super::stick::FIRE);
    }

    if let Some(grapple) = fighter.grapple() {
        draw_grapple(frame, slot, &grapple, label_y + size + 4.0, size, bindings);
    } else if bar.hits() >= COMBO_MIN_HITS {
        // The combo counter, hotter with every hit, where the grapple meter would be.
        let heat = (bar.hits() - COMBO_MIN_HITS) as usize;
        frame.label(
            format!("{} HITS", bar.hits()),
            x,
            label_y + size + 4.0,
            size * 0.85,
            BIG_HOT[heat.min(BIG_HOT.len() - 1)],
            align,
        );
    }
}

/// The grapple meter under a bar: the positions of the grab's ladder or of the technique for
/// the fighter on top; the escape gauge (or, in the air, the counter to enter) for the one
/// underneath.
fn draw_grapple(
    frame: &mut Frame,
    slot: &BarSlot,
    grapple: &Grapple,
    y: f32,
    size: f32,
    bindings: &KeyBindings,
) {
    let (x, align) = if slot.left_side {
        (slot.x, Align::Left)
    } else {
        (slot.x + slot.width, Align::Right)
    };
    let technique = grapple.position.technique();
    match grapple.role {
        GrappleRole::Bottom if grapple.position.is_projection() => {
            let keys = technique.map_or(String::new(), |t| technique_keys(t, bindings));
            let text = format!("{}!  COUNTER {keys}", position_name(grapple.position));
            frame.label(text, x, y, size * 0.85, ESCAPE, align);
        }
        GrappleRole::Top => {
            let positions = match technique {
                Some(technique) => vec![technique.projection(), technique.finish()],
                None => GrapplePosition::LADDER.to_vec(),
            };
            let ladder: Vec<String> = positions
                .iter()
                .map(|&position| {
                    let text = position_name(position);
                    if position == grapple.position {
                        format!("[{text}]")
                    } else {
                        text.to_ascii_lowercase()
                    }
                })
                .collect();
            frame.label(ladder.join(" > "), x, y, size * 0.85, GRAPPLE_ON, align);
        }
        GrappleRole::Bottom => {
            let text = format!("ESCAPE {}", position_name(grapple.position));
            frame.label(text, x, y, size * 0.85, ESCAPE, align);
            let gauge_width = slot.width * 0.4;
            let gauge_x = if slot.left_side {
                slot.x
            } else {
                slot.x + slot.width - gauge_width
            };
            let gauge_y = y + size + 2.0;
            let needed = grapple.escape_needed.max(1);
            let filled = gauge_width * grapple.escape.min(needed) as f32 / needed as f32;
            frame.rect(gauge_x, gauge_y, gauge_width, 6.0, GRAPPLE_OFF);
            let filled_x = if slot.left_side {
                gauge_x
            } else {
                gauge_x + gauge_width - filled
            };
            frame.rect(filled_x, gauge_y, filled, 6.0, ESCAPE);
        }
    }
}

/// What the bar of `player` says about their connection: `  LEFT` once they left the match
/// (their fighter stands still), `  LAG` while the server has to replace their late inputs
/// (their fighter repeats its moves), nothing otherwise.
pub fn network_mark(client: &Client, session: &Session, player: PlayerId) -> &'static str {
    if client.departed().contains(player) {
        "  LEFT"
    } else if session.lagging().contains(player) {
        "  LAG"
    } else {
        ""
    }
}

/// Upper-case name of a grapple position, as shown on the HUD.
pub fn position_name(position: GrapplePosition) -> &'static str {
    match position {
        GrapplePosition::Clinch => "CLINCH",
        GrapplePosition::Mount => "MOUNT",
        GrapplePosition::Submission => "SUBMISSION",
        GrapplePosition::Sweep => "JUJI SWEEP",
        GrapplePosition::ScarfHold => "HON-GESA-GATAME",
        GrapplePosition::Throw => "IPPON-SEOI-NAGE",
        GrapplePosition::Armbar => "JUJI-GATAME",
    }
}

fn draw_timer(frame: &mut Frame, current: &Match) {
    let x = SCREEN_WIDTH / 2.0 - TIMER_WIDTH / 2.0;
    frame.rect(x, BAR_TOP - 12.0, TIMER_WIDTH, 64.0, Rgba::hex(0x120c1c));
    frame.rect(x, BAR_TOP - 12.0, TIMER_WIDTH, 3.0, FRAME_COLOR);
    frame.rect(x, BAR_TOP + 49.0, TIMER_WIDTH, 3.0, FRAME_COLOR);
    let left = seconds(current.remaining_ticks());
    let color = if left <= 10 && current.phase() == Phase::Fighting {
        TIMER_LOW
    } else {
        TIMER
    };
    frame.label(
        format!("{left:02}"),
        SCREEN_WIDTH / 2.0,
        BAR_TOP - 6.0,
        48.0,
        color,
        Align::Center,
    );
}

/// The big words of the banner (READY, FIGHT!, K.O.) in the graffiti block font, in the hot
/// colours of the title.
fn draw_big(frame: &mut Frame, text: &str, top: f32) {
    let (pixels, width) = block_pixels(text);
    let columns = (width * 2 + 2) as usize;
    let mut canvas = Canvas::new(columns, 6);
    for (x, y) in pixels {
        let color = BIG_HOT[y as usize];
        for dx in 0..2 {
            canvas.put(x * 2 + dx + 1, y + 1, ':', BIG_HOT[4].scaled(0.35));
            canvas.put(x * 2 + dx, y, '@', color);
        }
    }
    let size = 22.0;
    let left = (SCREEN_WIDTH - columns as f32 * size * super::MONO_ADVANCE) / 2.0;
    canvas.draw(frame, left, top, size, 20.0, None);
}

/// The big word and the smaller lines of the banner for the current state.
pub fn banner(client: &Client) -> (Option<&'static str>, String) {
    match client.status() {
        Status::Connecting => (None, "Connecting to the server...".to_string()),
        Status::Lobby => (None, lobby_text(client)),
        Status::Watching => client
            .session()
            .map_or((None, watching_text(client)), |session| {
                round_banner(client, session)
            }),
        Status::Playing => client.session().map_or((None, String::new()), |session| {
            round_banner(client, session)
        }),
        Status::RoomFull => (
            None,
            format!("The room is full ({MAX_PLAYERS} players)\nTry again later"),
        ),
        Status::Disconnected => (
            None,
            "Disconnected from the server\nReload the page".to_string(),
        ),
        Status::Failed(error) => (None, format!("Error: {error}\nReload the page")),
    }
}

fn round_banner(client: &Client, session: &Session) -> (Option<&'static str>, String) {
    let current = session.current();
    match current.phase() {
        Phase::Countdown { remaining } => (Some("READY"), format!("{}", seconds(remaining))),
        Phase::Fighting if client.stalled_ticks() > STALL_WARNING_TICKS => {
            (None, STALL_TEXT.to_string())
        }
        Phase::Fighting
            if current.config().round_time_ticks - current.remaining_ticks() < TICKS_PER_SECOND =>
        {
            (Some("FIGHT!"), String::new())
        }
        Phase::Fighting => (None, String::new()),
        Phase::Finished(outcome) => {
            let knockout = current.fighters().iter().any(|f| f.is_knocked_out());
            let big = if knockout { "KO" } else { "TIME" };
            let result = match outcome {
                Outcome::Winner(team) => {
                    let members = current.roster().members(team);
                    if session
                        .local_player()
                        .is_some_and(|me| members.contains(&me))
                    {
                        "You win!".to_string()
                    } else {
                        let names: Vec<String> =
                            members.iter().map(|&player| name(client, player)).collect();
                        format!("{} wins!", names.join(" & "))
                    }
                }
                Outcome::Draw => "Draw!".to_string(),
            };
            (
                Some(big),
                format!("{result}\n{}", ready_text(client, session)),
            )
        }
    }
}

/// Who the next round waits for, once a round is over: "Enter for the next round", "Waiting
/// for P2, P3 to get ready...", plus the players who join the next round. Alone: "Enter for a
/// rematch".
pub fn ready_text(client: &Client, session: &Session) -> String {
    if client.is_local() {
        return "Enter for a rematch".to_string();
    }
    let seated = client.seated();
    let ready = client.ready_players();
    let me = session.local_player();
    if me.is_none() {
        return "Watching the room".to_string();
    }
    let mut lines = vec![if ready.contains(me.unwrap()) {
        waiting_text(client, seated, ready)
    } else {
        "Enter for the next round".to_string()
    }];
    let joining: PlayerSet = seated
        .iter()
        .filter(|&player| !session.players().contains(player))
        .collect();
    if !joining.is_empty() {
        let verb = if joining.len() == 1 { "joins" } else { "join" };
        lines.push(format!("{} {verb} the next round", tags(client, joining)));
    }
    lines.join("\n")
}

/// "Waiting for P2, P3 to get ready..." (or for a second player).
fn waiting_text(client: &Client, seated: PlayerSet, ready: PlayerSet) -> String {
    let waiting: PlayerSet = seated.iter().filter(|&p| !ready.contains(p)).collect();
    if seated.len() < 2 {
        "Waiting for another player...".to_string()
    } else {
        // Never empty here: the server starts the match as soon as everybody is ready.
        format!("Waiting for {} to get ready...", tags(client, waiting))
    }
}

/// The banner of a spectator between matches: who is here, who is ready, and that the next
/// match shows up on its own.
pub fn watching_text(client: &Client) -> String {
    let seated = client.seated();
    let ready = client.ready_players();
    if seated.is_empty() {
        return "Watching the room - nobody is here yet".to_string();
    }
    format!(
        "Watching {} players, {} ready\nThe next match shows up on its own",
        seated.len(),
        ready.len()
    )
}

/// The banner between matches: who left, who is here and who is ready.
pub fn lobby_text(client: &Client) -> String {
    let seated = client.seated();
    let ready = client.ready_players();
    let mut lines = Vec::new();
    if let Some(player) = client.left_player() {
        lines.push(format!("{} left", tag(client, player)));
    }
    if seated.len() <= 1 {
        lines.push("Nobody else is here yet".to_string());
        lines.push(format!(
            "Open this page on other machines (up to {MAX_PLAYERS} players)"
        ));
    } else if client.player().is_some_and(|player| ready.contains(player)) {
        lines.push(waiting_text(client, seated, ready));
    } else {
        lines.push(format!(
            "{} players here, {} ready - Enter to fight",
            seated.len(),
            ready.len()
        ));
    }
    lines.join("\n")
}

/// A small line above the controls (about practice, see
/// [`Game::practice_notice`](super::Game::practice_notice)).
pub fn draw_notice(frame: &mut Frame, text: &str) {
    frame.label(
        text,
        SCREEN_WIDTH / 2.0,
        NOTICE_Y,
        20.0,
        NOTICE,
        Align::Center,
    );
}

fn draw_banner(frame: &mut Frame, client: &Client, fight_ticks: u32) {
    let (big, lines) = banner(client);
    let mut y = SCREEN_HEIGHT * 0.26;
    if let Some(word) = big {
        // "FIGHT!" shakes for a few ticks, the way the title glitches.
        let shake = if word == "FIGHT!" && fight_ticks % 4 < 2 {
            3.0
        } else {
            0.0
        };
        draw_big(frame, word, y + shake);
        y += 140.0;
    }
    for line in lines.lines().filter(|l| !l.is_empty()) {
        let (size, color) = if big.is_some() {
            (30.0, BANNER_SMALL)
        } else {
            (36.0, BANNER)
        };
        frame.label(line, SCREEN_WIDTH / 2.0, y, size, color, Align::Center);
        y += size * 1.4;
    }
}
