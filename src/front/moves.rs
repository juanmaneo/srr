//! The movements of a fighter: everything the game can show a fighter doing, named once.
//!
//! The rules know the states of a fighter ([`FighterState`]); the art shows [`Move`]s. A move
//! is a cell of the [chalk sprite sheet](super::chalk), a pose of the stick figure
//! ([`move_pose`](super::stick::move_pose)) and a clip of the 3D [puppet](super::puppet)
//! ([`Move::clip`]): the same thirty movements in every style, so a fighter never does in one
//! view what it does not do in another.
//!
//! The manga sheets (`assets/skins/manga/`) are painted by hand, one cell per [`Pose`], and
//! hold twenty-two of them; the ground positions of a grapple (the mount, the finishing holds,
//! and what the bottom suffers under each) are the eight moves they have no cell for.
//! [`Move::pose`] says which manga cell stands in for each: the top of a ground position
//! crouches over it, the bottom lies like a knockout.

use super::model::Clip;
use super::skin::Pose;
use crate::game::{
    Character, Fighter, FighterState, GrapplePosition, GrappleRole, Limb, Strength, Technique,
};

/// A thing a fighter can be seen doing. The first twenty-two are the manga [`Pose`]s, in the
/// same order; the last eight are the ground positions the manga sheets lack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Move {
    /// Standing in guard.
    Stand,
    /// The two walk frames, alternating with the steps.
    WalkA,
    WalkB,
    /// Crouching low.
    Crouch,
    /// Stepping back with the guard up.
    Block,
    /// A jab, a straight punch and an uppercut.
    PunchLow,
    PunchMid,
    PunchHigh,
    /// A low, a middle and a high kick.
    KickLow,
    KickMid,
    KickHigh,
    /// In the air.
    Jump,
    /// A kick thrown in the air.
    JumpKick,
    /// Recoiling from a hit.
    Hit,
    /// Reaching out to grab, and the clinch that follows.
    Grab,
    /// The shoulder throw (the top of its projection).
    Throw,
    /// The flying scissor sweep (the top of its projection).
    Sweep,
    /// The special moves of the four characters: the shoulder rush, the fireball, the armored
    /// palm and the slide.
    SpecialRush,
    SpecialFireball,
    SpecialPalm,
    SpecialSlide,
    /// Lying knocked out.
    Ko,
    /// Kneeling astride the opponent, punching down.
    Mount,
    /// Low over the opponent, an arm locked around the neck.
    Submission,
    /// Sitting at the opponent's side in the scarf hold.
    ScarfHold,
    /// On the back across the opponent, pulling the arm.
    Armbar,
    /// Swept off the feet, falling backwards (the bottom of the sweep).
    Swept,
    /// Flying head first over the thrower's shoulder (the bottom of the throw).
    Thrown,
    /// Pinned under the mount, the hands up to defend.
    Pinned,
    /// Caught in a finishing hold, the trapped arm stretched up.
    Caught,
}

impl Move {
    /// Every move, in the order of the chalk sheet's cells (row-major) and of
    /// [`index`](Move::index).
    pub const ALL: [Move; 30] = [
        Move::Stand,
        Move::WalkA,
        Move::WalkB,
        Move::Crouch,
        Move::Block,
        Move::PunchLow,
        Move::PunchMid,
        Move::PunchHigh,
        Move::KickLow,
        Move::KickMid,
        Move::KickHigh,
        Move::Jump,
        Move::JumpKick,
        Move::Hit,
        Move::Grab,
        Move::Throw,
        Move::Sweep,
        Move::SpecialRush,
        Move::SpecialFireball,
        Move::SpecialPalm,
        Move::SpecialSlide,
        Move::Ko,
        Move::Mount,
        Move::Submission,
        Move::ScarfHold,
        Move::Armbar,
        Move::Swept,
        Move::Thrown,
        Move::Pinned,
        Move::Caught,
    ];

    /// The position of the move in [`Move::ALL`]: the declaration order.
    ///
    /// ```
    /// use srr::front::moves::Move;
    ///
    /// assert_eq!(Move::Stand.index(), 0);
    /// assert_eq!(Move::Caught.index(), Move::ALL.len() - 1);
    /// ```
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The move of a strike: the limb decides the family, the strength the height.
    ///
    /// ```
    /// use srr::front::moves::Move;
    /// use srr::game::{Limb, Strength};
    ///
    /// assert_eq!(Move::strike(Limb::Punch, Strength::High), Move::PunchHigh);
    /// assert_eq!(Move::strike(Limb::Kick, Strength::Low), Move::KickLow);
    /// ```
    pub const fn strike(limb: Limb, strength: Strength) -> Move {
        match (limb, strength) {
            (Limb::Punch, Strength::Low) => Move::PunchLow,
            (Limb::Punch, Strength::Medium) => Move::PunchMid,
            (Limb::Punch, Strength::High) => Move::PunchHigh,
            (Limb::Kick, Strength::Low) => Move::KickLow,
            (Limb::Kick, Strength::Medium) => Move::KickMid,
            (Limb::Kick, Strength::High) => Move::KickHigh,
        }
    }

    /// The manga cell that stands for the move: its own for the first twenty-two, the
    /// crouch for the top of a ground position and the knockout for the bottom of one.
    ///
    /// ```
    /// use srr::front::moves::Move;
    /// use srr::front::skin::Pose;
    ///
    /// assert_eq!(Move::PunchHigh.pose(), Pose::PunchHigh);
    /// assert_eq!(Move::Armbar.pose(), Pose::Crouch);
    /// assert_eq!(Move::Pinned.pose(), Pose::Ko);
    /// ```
    pub const fn pose(self) -> Pose {
        match self {
            Move::Stand => Pose::Stand,
            Move::WalkA => Pose::WalkA,
            Move::WalkB => Pose::WalkB,
            Move::Crouch | Move::Mount | Move::Submission | Move::ScarfHold | Move::Armbar => {
                Pose::Crouch
            }
            Move::Block => Pose::Block,
            Move::PunchLow => Pose::PunchLow,
            Move::PunchMid => Pose::PunchMid,
            Move::PunchHigh => Pose::PunchHigh,
            Move::KickLow => Pose::KickLow,
            Move::KickMid => Pose::KickMid,
            Move::KickHigh => Pose::KickHigh,
            Move::Jump => Pose::Jump,
            Move::JumpKick => Pose::JumpKick,
            Move::Hit => Pose::Hit,
            Move::Grab => Pose::Grab,
            Move::Throw => Pose::Throw,
            Move::Sweep => Pose::Sweep,
            Move::SpecialRush => Pose::SpecialRush,
            Move::SpecialFireball => Pose::SpecialFireball,
            Move::SpecialPalm => Pose::SpecialPalm,
            Move::SpecialSlide => Pose::SpecialSlide,
            Move::Ko | Move::Swept | Move::Thrown | Move::Pinned | Move::Caught => Pose::Ko,
        }
    }

    /// The 3D clip that plays the move: one clip for the three heights of a strike, one for the
    /// four specials (every character's model carries its own), the grapple's positions on
    /// their own.
    ///
    /// ```
    /// use srr::front::model::Clip;
    /// use srr::front::moves::Move;
    ///
    /// assert_eq!(Move::KickHigh.clip(), Clip::Kick);
    /// assert_eq!(Move::SpecialSlide.clip(), Clip::Special);
    /// assert_eq!(Move::Armbar.clip(), Clip::Armbar);
    /// ```
    pub const fn clip(self) -> Clip {
        match self {
            Move::Stand => Clip::Idle,
            Move::WalkA | Move::WalkB => Clip::WalkF,
            Move::Crouch => Clip::Crouch,
            Move::Block => Clip::Block,
            Move::PunchLow | Move::PunchMid | Move::PunchHigh => Clip::Punch,
            Move::KickLow | Move::KickMid | Move::KickHigh => Clip::Kick,
            Move::Jump => Clip::Jump,
            Move::JumpKick => Clip::JumpKick,
            Move::Hit => Clip::HitLight,
            Move::Grab => Clip::Grab,
            Move::Throw => Clip::Throw,
            Move::Sweep => Clip::Scissor,
            Move::SpecialRush | Move::SpecialFireball | Move::SpecialPalm | Move::SpecialSlide => {
                Clip::Special
            }
            Move::Ko => Clip::Ko,
            Move::Mount => Clip::Mount,
            Move::Submission => Clip::Submission,
            Move::ScarfHold => Clip::ScarfHold,
            Move::Armbar => Clip::Armbar,
            Move::Swept => Clip::Swept,
            Move::Thrown => Clip::Thrown,
            Move::Pinned => Clip::Pinned,
            Move::Caught => Clip::Caught,
        }
    }
}

/// The move `fighter` is seen doing: the sprite engines draw its cell, the stick engine its
/// pose and the stage plays its clip. A fighter without a move of its own stands in guard.
/// `fight_ticks` alternates the walk frames with the steps; `character` picks which special
/// move a special is.
pub fn move_of(fighter: &Fighter, character: Character, fight_ticks: u32) -> Move {
    match fighter.state() {
        FighterState::Walking => {
            // The two walk frames alternate with the screen's stride.
            if (fight_ticks / 8).is_multiple_of(2) {
                Move::WalkA
            } else {
                Move::WalkB
            }
        }
        FighterState::Airborne => Move::Jump,
        FighterState::Blocking => Move::Block,
        FighterState::Crouching => Move::Crouch,
        FighterState::HitStun => Move::Hit,
        FighterState::Knockout => Move::Ko,
        // An attacking fighter always has its attack (the state is set with the strike);
        // every phase of it poses the strike — the wind-up reads as the punch coming.
        FighterState::Attacking(_) => {
            let attack = fighter
                .attack()
                .expect("an attacking fighter has its attack");
            if fighter.is_airborne() {
                Move::JumpKick
            } else {
                Move::strike(attack.limb, attack.strength)
            }
        }
        FighterState::Grabbing(_) => Move::Grab,
        FighterState::Technique(Technique::Throw, _) => Move::Throw,
        FighterState::Technique(Technique::Sweep, _) => Move::Sweep,
        FighterState::Special(_) => match character {
            Character::Chalk => Move::SpecialRush,
            Character::Torch => Move::SpecialFireball,
            Character::Brick => Move::SpecialPalm,
            Character::Zip => Move::SpecialSlide,
        },
        // The grab's ladder (clinch, mount, submission) and the two techniques'
        // projections (sweep, throw) and finishes (scarf hold, armbar), on both sides.
        // A counter (a reversal) just swaps the roles, so it lands in the same moves.
        FighterState::Grappling(role, position) => match (role, position) {
            (_, GrapplePosition::Clinch) => Move::Grab,
            (GrappleRole::Top, GrapplePosition::Throw) => Move::Throw,
            (GrappleRole::Top, GrapplePosition::Sweep) => Move::Sweep,
            (GrappleRole::Top, GrapplePosition::Mount) => Move::Mount,
            (GrappleRole::Top, GrapplePosition::Submission) => Move::Submission,
            (GrappleRole::Top, GrapplePosition::ScarfHold) => Move::ScarfHold,
            (GrappleRole::Top, GrapplePosition::Armbar) => Move::Armbar,
            (GrappleRole::Bottom, GrapplePosition::Sweep) => Move::Swept,
            (GrappleRole::Bottom, GrapplePosition::Throw) => Move::Thrown,
            (GrappleRole::Bottom, GrapplePosition::Mount) => Move::Pinned,
            (
                GrappleRole::Bottom,
                GrapplePosition::Submission | GrapplePosition::ScarfHold | GrapplePosition::Armbar,
            ) => Move::Caught,
        },
        FighterState::Idle => Move::Stand,
    }
}
