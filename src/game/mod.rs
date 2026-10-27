//! The engine-agnostic game rules.
//!
//! Nothing in here knows about rendering, wall-clock time, threads or randomness: a [`Match`]
//! is a pure function of its previous state and the inputs of one tick. That keeps the rules
//! unit-testable, identical in the browser, and ready for rollback online play.
//!
//! One [`Match::step`] is one tick of a fixed timestep ([`TICKS_PER_SECOND`]):
//!
//! 1. every fighter is updated, in roster order, from its own input and its previous state
//!    (nobody turns by itself: free fighters face the direction pressed with down, then act
//!    with the priority technique > attack > grab > jump > crouch > walk and sidestep on the
//!    floor, or steer and kick in the air; a strike that connected chains into a stronger
//!    one; grappling fighters stay put);
//! 2. overlapping push boxes are separated, so fighters cannot walk through each other (but
//!    can jump over each other, or sidestep past each other);
//! 3. every attack (strike) that is active after that resolves against the updated state, so
//!    simultaneous attacks trade hits, high attacks from the floor pass over crouching
//!    fighters, a sidestep out of the attacker's line dodges and stepping back on the hit
//!    tick blocks; hits taken inside the stun of the previous one count as a combo; a
//!    grappler that is hit frees its partner;
//! 4. every grapple plays one tick from the fresh presses of its two fighters (a projection
//!    counters or lands; otherwise reversal glitch, escape, referee stand-up, then submission
//!    drain, advance or strike);
//! 5. every active judo technique, then every active grab, connects with the first fighter it
//!    can catch: they come after strikes (a strike interrupts them) and cannot be blocked;
//! 6. the round timer runs and the outcome is evaluated.
//!
//! Every player fights as a [`Character`]: its own health, speed, jump, power and reach, and
//! one special move (a lunge, a [`Projectile`] like the fireball, an armored strike or a low
//! slide), on [`Input::special`].
//!
//! A [`Bot`] plays a fighter for a player who is alone: it reads the [`Match`] and answers with
//! an [`Input`], deterministically (its randomness is a seeded generator). [`Rival`] lists the
//! bots: the training [`Dummy`] and the [`Brawler`], the computer that fights back.

mod bot;
mod character;
mod config;
mod fighter;
mod grapple;
mod input;
mod projectile;
mod replay;
mod roster;
mod simulation;
pub mod storage;
mod style;
mod technique;

pub use bot::{Bot, Brawler, BrawlerConfig, CloneBot, Dummy, Rival};
pub use character::{Character, CharacterData, ProjectileData, SHOULDER_RUSH, SpecialData};
pub use config::{
    AttackData, ConfigError, Glitch, GlitchSet, GrappleData, MatchConfig, TICKS_PER_SECOND,
    TechniqueData,
};
pub use fighter::{AttackPhase, Facing, Fighter, FighterState};
pub use grapple::{Grapple, GrapplePosition, GrappleRole};
pub use input::{Attack, Input, Key, KeyBindings, Limb, Strength};
pub use projectile::Projectile;
pub use replay::{Replay, ReplayError};
pub use roster::{PlayerId, PlayerSlot, Roster, RosterError, TeamId};
pub use simulation::{Match, Outcome, Phase, SetupError};
pub use style::{Skin, Style, StyleSet, Variant};
pub use technique::Technique;
