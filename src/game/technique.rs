//! Judo techniques: two-button throws that end in a pin or an armlock on the ground.
//!
//! A technique is triggered by pressing its two buttons *simultaneously*: both held, one of them
//! pressed on this very tick and the other at most [`MatchConfig::combo_window`] ticks earlier.
//! The first button usually starts its own attack on the tick it is pressed; the second one,
//! arriving while that attack is still winding up, converts it into the technique.
//!
//! | Technique | Buttons (standard keys) | Projection | Finish on the ground |
//! | --- | --- | --- | --- |
//! | [`Technique::Sweep`] | medium punch + high kick (`5` + `9`) | flying juji-gatame foot sweep | hon-gesa-gatame (scarf hold) |
//! | [`Technique::Throw`] | medium kick + high kick (`8` + `9`) | ippon-seoi-nage (te-waza) | ude-hishigi-juji-gatame (cross armbar) |
//!
//! Like a grab, a technique runs a start-up / active / recovery timeline, cannot be blocked and
//! only catches fighters on the floor that are neither stunned nor grappling. Once it connects,
//! both fighters enter the projection position of a grapple (see
//! [`GrapplePosition::Sweep`] and [`GrapplePosition::Throw`]), then the finish.
//!
//! **Counters** need the exact same combination: `5` + `9` counters `5` + `9`, `8` + `9`
//! counters `8` + `9`. A fighter caught while winding up the same technique, or entering the
//! same combination during the first [`TechniqueData::counter_window`] ticks of the projection,
//! reverses it. A counter cannot be countered again.
//!
//! [`MatchConfig::combo_window`]: super::MatchConfig::combo_window
//! [`TechniqueData::counter_window`]: super::TechniqueData::counter_window

use std::fmt;

use super::grapple::GrapplePosition;
use super::input::{Attack, Strength};

/// One of the two judo techniques.
///
/// ```
/// use srr::game::{Attack, GrapplePosition, Strength, Technique};
///
/// assert_eq!(
///     Technique::Throw.buttons(),
///     [Attack::kick(Strength::Medium), Attack::kick(Strength::High)]
/// );
/// assert_eq!(Technique::Throw.projection(), GrapplePosition::Throw);
/// assert_eq!(Technique::Throw.finish(), GrapplePosition::Armbar);
/// assert_eq!(Technique::Sweep.to_string(), "juji-gatame sweep");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Technique {
    /// Medium punch + high kick: a flying juji-gatame foot kick that sweeps the opponent off its
    /// feet and rides it down into hon-gesa-gatame.
    Sweep,
    /// Medium kick + high kick: ippon-seoi-nage, the one-arm shoulder throw (te-waza), followed
    /// by ude-hishigi-juji-gatame on the ground.
    Throw,
}

impl Technique {
    /// Every technique; when one input completes both combinations, the first one wins.
    pub const ALL: [Technique; 2] = [Technique::Sweep, Technique::Throw];

    /// Position in [`Technique::ALL`]; indexes [`MatchConfig::techniques`].
    ///
    /// [`MatchConfig::techniques`]: super::MatchConfig::techniques
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The two attack buttons to press simultaneously.
    pub const fn buttons(self) -> [Attack; 2] {
        match self {
            Technique::Sweep => [
                Attack::punch(Strength::Medium),
                Attack::kick(Strength::High),
            ],
            Technique::Throw => [Attack::kick(Strength::Medium), Attack::kick(Strength::High)],
        }
    }

    /// The grapple position of the projection (both fighters off balance, nobody can escape).
    pub const fn projection(self) -> GrapplePosition {
        match self {
            Technique::Sweep => GrapplePosition::Sweep,
            Technique::Throw => GrapplePosition::Throw,
        }
    }

    /// The grapple position the projection lands in.
    pub const fn finish(self) -> GrapplePosition {
        match self {
            Technique::Sweep => GrapplePosition::ScarfHold,
            Technique::Throw => GrapplePosition::Armbar,
        }
    }
}

impl fmt::Display for Technique {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Technique::Sweep => "juji-gatame sweep",
            Technique::Throw => "ippon-seoi-nage",
        })
    }
}
