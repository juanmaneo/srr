//! Grappling: the clinch, the ground game and submissions.
//!
//! A grab ([`FighterState::Grabbing`](super::FighterState::Grabbing)) that connects puts two
//! fighters in a grapple: the grabber is on [`GrappleRole::Top`], the target on
//! [`GrappleRole::Bottom`], starting in the [`GrapplePosition::Clinch`]. Both fighters share one
//! hold state; one tick of the grapple is a pure function of that state and of the buttons both
//! fighters *freshly* pressed on the tick (see [`Input::pressed_since`]).
//!
//! Every tick after the connect (the connect tick itself does not count), in this order:
//!
//! 1. with [`Glitch::Reversal`] enabled, a bottom that freshly presses grab on its very first
//!    tick in the grapple swaps the roles, into a fresh clinch; nothing else happens that tick;
//! 2. the position has been held one tick longer and the top's strike cooldown runs down;
//! 3. the bottom struggles: a tick with at least one fresh press adds one escape point, and
//!    reaching [`Grapple::escape_needed`] breaks the grapple (the top is stunned);
//! 4. the referee stands both fighters up once the position has been held `max_hold_ticks`;
//! 5. in the submission the bottom loses `submission_damage` every `submission_interval` ticks;
//!    in the other positions a fresh grab press of the top advances one position once the
//!    current one has been held `advance_ticks` (the takedown into the mount hurts), and
//!    otherwise a fresh punch or kick press strikes, at most once every `strike_cooldown` ticks.
//!
//! A [`Technique`] that connects starts in its projection position instead
//! ([`GrapplePosition::Sweep`] or [`GrapplePosition::Throw`]). Nobody can escape a projection
//! and the referee does not stop it; every tick it has been held one tick longer, and:
//!
//! 1. during the first `counter_window` ticks, a bottom that enters the same technique's
//!    combination swaps the roles, into a fresh projection that cannot be countered again;
//! 2. after `projection_ticks` ticks the bottom hits the floor (losing the technique's
//!    `damage`) and the grapple moves to the technique's finish, a hold on the ground that drains
//!    the bottom like the submission and can be escaped with the technique's `escape_presses`.
//!
//! [`Input::pressed_since`]: super::Input::pressed_since

use std::fmt;

use super::config::{Glitch, MatchConfig};
use super::fighter::Facing;
use super::input::Input;
use super::roster::PlayerId;
use super::technique::Technique;

/// The positions of a grapple: the grab's ladder (clinch, mount, submission) and, for each
/// [`Technique`], its projection and its finish on the ground.
///
/// ```
/// use srr::game::{GrapplePosition, Technique};
///
/// assert_eq!(GrapplePosition::Clinch.next(), Some(GrapplePosition::Mount));
/// assert_eq!(GrapplePosition::Submission.next(), None);
/// assert_eq!(GrapplePosition::Mount.index(), 1);
/// assert_eq!(GrapplePosition::Mount.to_string(), "mount");
/// assert_eq!(GrapplePosition::Sweep.next(), Some(GrapplePosition::ScarfHold));
/// assert_eq!(GrapplePosition::Armbar.technique(), Some(Technique::Throw));
/// assert_eq!(GrapplePosition::Mount.technique(), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrapplePosition {
    /// Standing, chest to chest; the top lands knees and dirty boxing. Every grab starts here.
    Clinch,
    /// Taken down to the floor and mounted; the top lands ground-and-pound.
    Mount,
    /// A choke or an armbar: the bottom drains health until it escapes or taps out.
    Submission,
    /// The projection of [`Technique::Sweep`]: the top flies in, legs scissored around the
    /// bottom (a flying juji-gatame kick), and sweeps it off its feet.
    Sweep,
    /// The finish of [`Technique::Sweep`]: hon-gesa-gatame, the scarf hold, pinning the bottom
    /// down by the neck and the arm.
    ScarfHold,
    /// The projection of [`Technique::Throw`]: ippon-seoi-nage, the bottom flies over the top's
    /// shoulder.
    Throw,
    /// The finish of [`Technique::Throw`]: ude-hishigi-juji-gatame, the cross armbar.
    Armbar,
}

impl GrapplePosition {
    /// Every position: the ladder, then each technique's projection and finish.
    pub const ALL: [GrapplePosition; 7] = [
        GrapplePosition::Clinch,
        GrapplePosition::Mount,
        GrapplePosition::Submission,
        GrapplePosition::Sweep,
        GrapplePosition::ScarfHold,
        GrapplePosition::Throw,
        GrapplePosition::Armbar,
    ];

    /// The grab's ladder, in order; [`GrappleData::escape_presses`] is indexed by
    /// [`GrapplePosition::index`] of these.
    ///
    /// [`GrappleData::escape_presses`]: super::GrappleData::escape_presses
    pub const LADDER: [GrapplePosition; 3] = [
        GrapplePosition::Clinch,
        GrapplePosition::Mount,
        GrapplePosition::Submission,
    ];

    /// Position in [`GrapplePosition::ALL`]: 0 for the clinch, 1 for the mount, 2 for the
    /// submission, and so on.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The position the grapple moves to next: the top advances the ladder by pressing grab, a
    /// projection lands in its finish. `None` for the submission and the finishes.
    pub const fn next(self) -> Option<GrapplePosition> {
        match self {
            GrapplePosition::Clinch => Some(GrapplePosition::Mount),
            GrapplePosition::Mount => Some(GrapplePosition::Submission),
            GrapplePosition::Sweep => Some(GrapplePosition::ScarfHold),
            GrapplePosition::Throw => Some(GrapplePosition::Armbar),
            GrapplePosition::Submission | GrapplePosition::ScarfHold | GrapplePosition::Armbar => {
                None
            }
        }
    }

    /// The technique this position belongs to, or `None` for the grab's ladder.
    pub const fn technique(self) -> Option<Technique> {
        match self {
            GrapplePosition::Sweep | GrapplePosition::ScarfHold => Some(Technique::Sweep),
            GrapplePosition::Throw | GrapplePosition::Armbar => Some(Technique::Throw),
            GrapplePosition::Clinch | GrapplePosition::Mount | GrapplePosition::Submission => None,
        }
    }

    /// Whether this is a technique's projection (the throw in the air).
    ///
    /// ```
    /// use srr::game::GrapplePosition;
    ///
    /// assert!(GrapplePosition::Throw.is_projection());
    /// assert!(!GrapplePosition::Armbar.is_projection());
    /// ```
    pub const fn is_projection(self) -> bool {
        matches!(self, GrapplePosition::Sweep | GrapplePosition::Throw)
    }
}

impl fmt::Display for GrapplePosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            GrapplePosition::Clinch => "clinch",
            GrapplePosition::Mount => "mount",
            GrapplePosition::Submission => "submission",
            GrapplePosition::Sweep => "juji-gatame sweep",
            GrapplePosition::ScarfHold => "hon-gesa-gatame",
            GrapplePosition::Throw => "ippon-seoi-nage",
            GrapplePosition::Armbar => "ude-hishigi-juji-gatame",
        })
    }
}

/// Which side of a grapple a fighter is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrappleRole {
    /// In control: advances the position and strikes.
    Top,
    /// Controlled: can only struggle to escape.
    Bottom,
}

impl GrappleRole {
    /// The partner's role.
    ///
    /// ```
    /// use srr::game::GrappleRole;
    ///
    /// assert_eq!(GrappleRole::Top.opposite(), GrappleRole::Bottom);
    /// assert_eq!(GrappleRole::Bottom.opposite(), GrappleRole::Top);
    /// ```
    pub const fn opposite(self) -> GrappleRole {
        match self {
            GrappleRole::Top => GrappleRole::Bottom,
            GrappleRole::Bottom => GrappleRole::Top,
        }
    }
}

/// A grapple as seen by one of its two fighters; see
/// [`Fighter::grapple`](super::Fighter::grapple).
///
/// Both fighters of a grapple report the same `position`, `escape`, `escape_needed` and
/// `held_ticks`; `partner` and `role` are mirrored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Grapple {
    /// The other fighter of the grapple.
    pub partner: PlayerId,
    /// This fighter's side.
    pub role: GrappleRole,
    /// The current rung of the ladder.
    pub position: GrapplePosition,
    /// Escape points the bottom has gathered in the current position.
    pub escape: u32,
    /// Escape points that break the grapple in the current position (from
    /// [`GrappleData::escape_presses`] or [`TechniqueData::escape_presses`]); zero during a
    /// projection, which cannot be escaped.
    ///
    /// [`GrappleData::escape_presses`]: super::GrappleData::escape_presses
    /// [`TechniqueData::escape_presses`]: super::TechniqueData::escape_presses
    pub escape_needed: u32,
    /// Ticks the current position has been held (0 on the tick it was entered).
    pub held_ticks: u32,
}

/// The state shared by the two fighters of a grapple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hold {
    position: GrapplePosition,
    escape: u32,
    escape_needed: u32,
    held_ticks: u32,
    /// Ticks before the top may strike again.
    cooldown: u32,
    /// True until the first tick after the connect has been played ([`Glitch::Reversal`]).
    opening: bool,
    /// A projection that is itself a counter: it cannot be countered again.
    countered: bool,
}

/// What one tick of a grapple did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Turn {
    /// The grapple goes on; the bottom loses `damage` health (possibly zero).
    Continue { damage: i32 },
    /// [`Glitch::Reversal`] or a technique counter: the roles swap, in the fresh hold.
    Reversal,
    /// The bottom struggled free; the top is stunned.
    Escape,
    /// The referee separates both fighters; nobody is stunned.
    StandUp,
}

impl Hold {
    /// A position just entered: no escape progress, no tick held.
    fn entered(position: GrapplePosition, cooldown: u32, config: &MatchConfig) -> Self {
        let escape_needed = match position.technique() {
            None => config.grapple.escape_presses[position.index()],
            Some(_) if position.is_projection() => 0,
            Some(technique) => config.technique(technique).escape_presses,
        };
        Hold {
            position,
            escape: 0,
            escape_needed,
            held_ticks: 0,
            cooldown,
            opening: false,
            countered: false,
        }
    }

    /// The hold of a grab that just connected: a clinch whose first tick is still to come.
    pub(crate) fn connected(config: &MatchConfig) -> Self {
        Hold {
            opening: true,
            ..Hold::entered(GrapplePosition::Clinch, 0, config)
        }
    }

    /// The hold of a technique that just connected: its projection; `countered` when the
    /// projection is a counter, which cannot be countered again.
    pub(crate) fn projecting(technique: Technique, countered: bool, config: &MatchConfig) -> Self {
        Hold {
            countered,
            ..Hold::entered(technique.projection(), 0, config)
        }
    }

    /// The current position.
    pub(crate) fn position(&self) -> GrapplePosition {
        self.position
    }

    /// Plays one tick of the grapple; `top` and `bottom` are the buttons each fighter freshly
    /// pressed on the tick, `combo` the technique the bottom's buttons completed on the tick, if
    /// any. See the module documentation for the order of the rules.
    pub(crate) fn tick(
        &mut self,
        top: Input,
        bottom: Input,
        combo: Option<Technique>,
        config: &MatchConfig,
    ) -> Turn {
        let data = &config.grapple;
        if let Some(technique) = self
            .position
            .technique()
            .filter(|_| self.position.is_projection())
        {
            return self.project(technique, combo, config);
        }
        let opening = std::mem::replace(&mut self.opening, false);
        if opening && bottom.grab && config.glitches.contains(Glitch::Reversal) {
            *self = Hold::entered(GrapplePosition::Clinch, 0, config);
            return Turn::Reversal;
        }
        self.held_ticks = self.held_ticks.saturating_add(1);
        self.cooldown = self.cooldown.saturating_sub(1);
        if bottom != Input::NONE {
            self.escape = self.escape.saturating_add(1);
            if self.escape >= self.escape_needed {
                return Turn::Escape;
            }
        }
        if self.held_ticks >= data.max_hold_ticks {
            return Turn::StandUp;
        }
        let damage = match self.position.next() {
            None if self.held_ticks.is_multiple_of(data.submission_interval) => {
                data.submission_damage
            }
            None => 0,
            Some(next) if top.grab && self.held_ticks >= data.advance_ticks => {
                let takedown = self.position == GrapplePosition::Clinch;
                *self = Hold::entered(next, self.cooldown, config);
                if takedown { data.takedown_damage } else { 0 }
            }
            Some(_) if top.attack().is_some() && self.cooldown == 0 => {
                self.cooldown = data.strike_cooldown;
                if self.position == GrapplePosition::Clinch {
                    data.clinch_strike_damage
                } else {
                    data.ground_strike_damage
                }
            }
            Some(_) => 0,
        };
        Turn::Continue { damage }
    }

    /// One tick of the projection of `technique`: the counter, or the landing into the finish.
    fn project(
        &mut self,
        technique: Technique,
        combo: Option<Technique>,
        config: &MatchConfig,
    ) -> Turn {
        let data = config.technique(technique);
        self.held_ticks = self.held_ticks.saturating_add(1);
        if !self.countered && combo == Some(technique) && self.held_ticks <= data.counter_window {
            *self = Hold::projecting(technique, true, config);
            return Turn::Reversal;
        }
        if self.held_ticks < data.projection_ticks {
            return Turn::Continue { damage: 0 };
        }
        *self = Hold::entered(technique.finish(), 0, config);
        Turn::Continue {
            damage: data.damage,
        }
    }
}

/// One fighter's side of a grapple: the partner (position in the match and player) and the
/// shared hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Link {
    pub(crate) partner: usize,
    pub(crate) partner_id: PlayerId,
    pub(crate) role: GrappleRole,
    pub(crate) hold: Hold,
}

impl Link {
    /// The public view of this side of the grapple.
    pub(crate) fn view(&self) -> Grapple {
        Grapple {
            partner: self.partner_id,
            role: self.role,
            position: self.hold.position,
            escape: self.hold.escape,
            escape_needed: self.hold.escape_needed,
            held_ticks: self.hold.held_ticks,
        }
    }
}

/// Where a grab that connected puts the two fighters: the bottom `distance` in front of the
/// top, inside the arena; when a wall is in the way the top steps back to keep the distance, as
/// far as the arena allows. Returns `(top_x, bottom_x)`.
pub(crate) fn snap(top_x: i32, facing: Facing, distance: i32, width: i32) -> (i32, i32) {
    let offset = i64::from(facing.sign()) * i64::from(distance);
    let bottom = clamp_x(i64::from(top_x) + offset, width);
    let top = clamp_x(i64::from(bottom) - offset, width);
    (top, bottom)
}

/// Where a broken grapple leaves the two fighters: `distance` apart around their midpoint, the
/// top keeping the side it faces from, the pair shifted back inside the arena when a wall is in
/// the way (and only then closer, when the arena is narrower than `distance`). Returns
/// `(top_x, bottom_x)`.
pub(crate) fn separate(
    top_x: i32,
    bottom_x: i32,
    facing: Facing,
    distance: i32,
    width: i32,
) -> (i32, i32) {
    let offset = i64::from(facing.sign()) * i64::from(distance);
    let middle = (i64::from(top_x) + i64::from(bottom_x)).div_euclid(2);
    let top = middle - offset / 2;
    let bottom = top + offset;
    let shift = (-top.min(bottom)).max(0) + (i64::from(width) - top.max(bottom)).min(0);
    (clamp_x(top + shift, width), clamp_x(bottom + shift, width))
}

/// `x` clamped into the arena `0..=width`; the result always fits in an `i32`.
fn clamp_x(x: i64, width: i32) -> i32 {
    x.clamp(0, i64::from(width)) as i32
}
