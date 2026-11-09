//! The 3D fighters as the front end describes them (`srr::front::model`), and the puppet that
//! stands in for a real model (`srr::front::puppet`):
//!
//! * the toon band math the shader mirrors;
//! * the clip of every state the rules can hold, and how it runs: the strikes, the special move,
//!   the grab and the techniques are scrubbed through their frame data, so the swing reaches its
//!   contact pose on the active ticks exactly;
//! * the stage of the third-person view: one actor per fighter with a model, left out of the
//!   software mannequins, never fed back into the rules;
//! * the layout of `assets/models/` and its audit, the glTF reading behind it;
//! * the puppet, byte for byte the committed placeholder of `assets/models/chalk/`, and the
//!   movements it shares with the stick figure and the sprite sheets.

mod common;

use std::collections::HashSet;

use srr::front::model::{
    self, Actor, Audit, Body, CROSSFADE_SECONDS, Clip, FEATHER, GIRLS_DIR, HEAVY_HIT_COMBO,
    HOLD_POSE, MODEL_HEIGHT, Models, OUTLINE_SHADE, OUTLINE_WIDTH, Playback, RIM, SHADOW_EDGES,
    SHADOW_TINTS, SPECULAR, STRIKE_CONTACT, STRIKE_WINDUP, Severity, audit, body_of, clip_of,
    clips_of, generator_of, girl_path, glb_json, lit, model_dir, model_path, outline_offset,
    playback_of, rim, shade, skin_path, specular, step, strike_fraction,
};
use srr::front::moves::{Move, move_of};
use srr::front::puppet;
use srr::front::skin::{SPRITE_NAMES, pose_of};
use srr::front::space::{Camera, Vec3};
use srr::front::stick::clip_poses;
use srr::front::{Game, Headless, Screen, VIEW_KEY, View};
use srr::game::{
    Attack, AttackPhase, Character, Fighter, FighterState, GrapplePosition, GrappleRole, Input,
    Key, Limb, Match, MatchConfig, PlayerId, Skin, Strength, Style, Variant,
};
use srr::net::Client;

// ---------------------------------------------------------------------------------------------
// Tuning and the toon bands
// ---------------------------------------------------------------------------------------------

/// The cosine between the normal and the light at which the half-Lambert is `half`.
fn cosine_at(half: f32) -> f32 {
    half * 2.0 - 1.0
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn the_step_is_a_feathered_smoothstep() {
    // Flat outside the feather, exactly 0 and 1 (the bands are flat, not gradients).
    assert_eq!(step(0.5, 0.0), 0.0);
    assert_eq!(step(0.5, 0.5 - 2.0 * FEATHER), 0.0);
    assert_eq!(step(0.5, 0.5 + 2.0 * FEATHER), 1.0);
    assert_eq!(step(0.5, 1.0), 1.0);
    // Through the feather: the smooth S of `smoothstep`, half way on the edge, monotonic.
    assert!(close(step(0.5, 0.5), 0.5));
    let quarter = step(0.5, 0.5 - FEATHER / 2.0);
    assert!(close(quarter, 0.15625), "{quarter}");
    let mut previous = 0.0;
    for i in 0..=40 {
        let x = 0.5 - FEATHER + (i as f32 / 40.0) * 2.0 * FEATHER;
        let value = step(0.5, x);
        assert!(value >= previous, "{x}: {value} < {previous}");
        previous = value;
    }
}

#[test]
fn the_toon_shading_steps_at_its_edges() {
    let light = [1.0, 1.0, 1.0];
    // Facing away: the deep shadow; past the terminator: the shadow; facing the light: the
    // texture as painted. The edges are on the half-Lambert scale.
    assert_eq!(shade(-1.0), SHADOW_TINTS[0]);
    assert_eq!(
        shade(cosine_at(SHADOW_EDGES[0] - 2.0 * FEATHER)),
        SHADOW_TINTS[0]
    );
    assert_eq!(
        shade(cosine_at(SHADOW_EDGES[0] + 2.0 * FEATHER)),
        SHADOW_TINTS[1]
    );
    assert_eq!(
        shade(cosine_at(SHADOW_EDGES[1] - 2.0 * FEATHER)),
        SHADOW_TINTS[1]
    );
    assert_eq!(shade(cosine_at(SHADOW_EDGES[1] + 2.0 * FEATHER)), light);
    assert_eq!(shade(1.0), light);
    // On an edge: half way between the two tints, channel by channel.
    for (channel, value) in shade(cosine_at(SHADOW_EDGES[1])).into_iter().enumerate() {
        assert!(
            close(value, (SHADOW_TINTS[1][channel] + 1.0) / 2.0),
            "{value}"
        );
    }
    // `lit` is the last step alone.
    assert_eq!(lit(-1.0), 0.0);
    assert_eq!(lit(cosine_at(SHADOW_EDGES[1] + 2.0 * FEATHER)), 1.0);
    assert_eq!(lit(1.0), 1.0);
}

#[test]
fn the_highlight_and_the_rim_show_on_the_lit_side_only() {
    // The highlight: where the normal is close enough to the half vector, lit.
    assert_eq!(specular(1.0, 1.0), SPECULAR.2);
    assert_eq!(specular(1.0, -1.0), 0.0, "in the shadow");
    assert_eq!(specular(0.0, 1.0), 0.0, "far from the half vector");
    assert_eq!(specular(-0.5, 1.0), 0.0, "facing away from it");
    let threshold = SPECULAR.1.powf(1.0 / SPECULAR.0);
    assert_eq!(specular(threshold - 0.05, 1.0), 0.0);
    assert_eq!(specular(threshold + 0.05, 1.0), SPECULAR.2);
    // The rim: on the silhouette, lit.
    assert_eq!(rim(RIM.0 - 2.0 * FEATHER, 1.0), RIM.1);
    assert_eq!(rim(0.0, 1.0), RIM.1);
    assert_eq!(rim(RIM.0 + 2.0 * FEATHER, 1.0), 0.0);
    assert_eq!(rim(1.0, 1.0), 0.0);
    assert_eq!(rim(0.0, -1.0), 0.0, "in the shadow");
}

#[test]
fn the_outline_keeps_its_width_on_screen_at_any_depth() {
    // The world length of OUTLINE_WIDTH pixels at the depth `w`: on a 720 px tall viewport
    // through a projection of scale 2 (a 53° vertical field of view), one unit of depth
    // shows 1 world unit over 720 px, so one pixel is 1/720 world units there.
    assert!(close(
        outline_offset(1.0, 2.0, 720.0),
        OUTLINE_WIDTH / 720.0
    ));
    // Twice as far: twice as long, the same pixels.
    assert!(close(
        outline_offset(2.0, 2.0, 720.0),
        2.0 * outline_offset(1.0, 2.0, 720.0)
    ));
    // A narrower field of view (a larger scale) magnifies: fewer world units per pixel.
    assert!(outline_offset(1.0, 4.0, 720.0) < outline_offset(1.0, 2.0, 720.0));
    // A taller viewport has smaller pixels.
    assert!(outline_offset(1.0, 2.0, 1440.0) < outline_offset(1.0, 2.0, 720.0));
}

#[test]
fn the_tuning_knobs_are_in_range() {
    const {
        assert!(CROSSFADE_SECONDS > 0.0 && CROSSFADE_SECONDS <= 0.25);
        assert!(0.0 < STRIKE_WINDUP && STRIKE_WINDUP < STRIKE_CONTACT && STRIKE_CONTACT < 1.0);
        assert!(STRIKE_WINDUP < HOLD_POSE && HOLD_POSE < STRIKE_CONTACT);
        assert!(HEAVY_HIT_COMBO >= 2);
        assert!(MODEL_HEIGHT > 0.0 && OUTLINE_WIDTH > 0.0);
        // The shadows step in order on the half-Lambert scale, with room for the feather on
        // both sides of every edge (and the feather is never zero: the shader divides by it).
        assert!(FEATHER > 0.0 && FEATHER < 0.1);
        assert!(FEATHER < SHADOW_EDGES[0] && SHADOW_EDGES[0] + FEATHER < SHADOW_EDGES[1]);
        assert!(SHADOW_EDGES[1] + FEATHER < 1.0);
        // Darker to lighter, every tint under white: the light keeps the texture as painted.
        let mut channel = 0;
        while channel < 3 {
            assert!(
                0.0 < SHADOW_TINTS[0][channel]
                    && SHADOW_TINTS[0][channel] < SHADOW_TINTS[1][channel]
            );
            assert!(SHADOW_TINTS[1][channel] < 1.0);
            assert!(
                0.0 < OUTLINE_SHADE[channel] && OUTLINE_SHADE[channel] < SHADOW_TINTS[0][channel]
            );
            channel += 1;
        }
        assert!(SPECULAR.0 >= 1.0 && 0.0 < SPECULAR.1 && SPECULAR.1 < 1.0 && SPECULAR.2 >= 0.0);
        assert!(0.0 < RIM.0 && RIM.0 < 1.0 && RIM.1 >= 0.0);
    }
}

// ---------------------------------------------------------------------------------------------
// Clips
// ---------------------------------------------------------------------------------------------

#[test]
fn every_clip_has_its_own_name_and_index() {
    let names: Vec<&str> = Clip::ALL.iter().map(|clip| clip.name()).collect();
    assert_eq!(
        names,
        [
            "idle",
            "walk_f",
            "walk_b",
            "crouch",
            "jump",
            "block",
            "hit_light",
            "hit_heavy",
            "ko",
            "punch",
            "kick",
            "sweep",
            "special",
            "grab",
            "jump_kick",
            "throw",
            "scissor",
            "mount",
            "submission",
            "scarf_hold",
            "armbar",
            "swept",
            "thrown",
            "pinned",
            "caught"
        ]
    );
    assert_eq!(Clip::BASIC, 13);
    for clip in Clip::ALL {
        assert_eq!(clip.is_basic(), clip.index() < Clip::BASIC, "{clip:?}");
    }
    for (index, clip) in Clip::ALL.into_iter().enumerate() {
        assert_eq!(clip.index(), index);
        assert_eq!(Clip::by_name(clip.name()), Some(clip));
    }
    assert_eq!(Clip::by_name("Idle"), None, "names are case-sensitive");
}

#[test]
fn a_strike_is_scrubbed_through_its_timeline() {
    // Wind-up over the start-up, contact over the active ticks, the rest over the recovery.
    let (startup, active, recovery) = (3, 2, 4);
    let total = startup + active + recovery;
    let fractions: Vec<f32> = (0..=total + 2)
        .map(|frame| strike_fraction(frame, startup, active, recovery))
        .collect();
    assert_eq!(fractions[0], 0.0);
    assert!(
        fractions.windows(2).all(|pair| pair[0] <= pair[1]),
        "{fractions:?}"
    );
    assert_eq!(fractions[startup as usize], STRIKE_WINDUP);
    assert_eq!(fractions[(startup + active) as usize], STRIKE_CONTACT);
    assert_eq!(fractions[total as usize], 1.0);
    assert_eq!(*fractions.last().unwrap(), 1.0);
    // Empty phases skip straight through.
    assert_eq!(strike_fraction(0, 0, 0, 0), 0.0);
    assert_eq!(strike_fraction(1, 0, 0, 0), 1.0);
    assert_eq!(strike_fraction(1, 0, 1, 0), STRIKE_CONTACT);
}

/// How a clip runs, without the fraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Loop,
    Once,
    Scrub,
}

fn kind(playback: Playback) -> Kind {
    match playback {
        Playback::Loop => Kind::Loop,
        Playback::Once => Kind::Once,
        Playback::Scrub(_) => Kind::Scrub,
    }
}

/// The specification of the mapping: the clip and the playback of `fighter` (having `moved`
/// since the last tick, or not).
fn expected(fighter: &Fighter, moved: bool) -> (Clip, Kind) {
    use FighterState::*;
    match fighter.state() {
        Idle => (Clip::Idle, Kind::Loop),
        Walking => (if moved { Clip::WalkF } else { Clip::Idle }, Kind::Loop),
        Blocking => (if moved { Clip::WalkB } else { Clip::Block }, Kind::Loop),
        Crouching => (Clip::Crouch, Kind::Loop),
        Airborne => (Clip::Jump, Kind::Once),
        HitStun if fighter.is_airborne() || fighter.combo_hits() >= HEAVY_HIT_COMBO => {
            (Clip::HitHeavy, Kind::Once)
        }
        HitStun => (Clip::HitLight, Kind::Once),
        Knockout => (Clip::Ko, Kind::Once),
        Attacking(_) if fighter.is_airborne() => (Clip::JumpKick, Kind::Scrub),
        Attacking(_) if fighter.attack().unwrap().limb == Limb::Kick => (Clip::Kick, Kind::Scrub),
        Attacking(_) => (Clip::Punch, Kind::Scrub),
        Special(_) => (Clip::Special, Kind::Scrub),
        // The grab reaches out, and so does the throw going for the collar; the sweep reaps.
        Grabbing(_) | Technique(srr::game::Technique::Throw, _) => (Clip::Grab, Kind::Scrub),
        Technique(srr::game::Technique::Sweep, _) => (Clip::Sweep, Kind::Scrub),
        // Both sides of a clinch hold the same pose.
        Grappling(_, GrapplePosition::Clinch) => (Clip::Grab, Kind::Scrub),
        // The top: a position of its own, held; the bottom: what it suffers, once.
        Grappling(GrappleRole::Top, GrapplePosition::Throw) => (Clip::Throw, Kind::Loop),
        Grappling(GrappleRole::Top, GrapplePosition::Sweep) => (Clip::Scissor, Kind::Loop),
        Grappling(GrappleRole::Top, GrapplePosition::Mount) => (Clip::Mount, Kind::Loop),
        Grappling(GrappleRole::Top, GrapplePosition::Submission) => (Clip::Submission, Kind::Loop),
        Grappling(GrappleRole::Top, GrapplePosition::ScarfHold) => (Clip::ScarfHold, Kind::Loop),
        Grappling(GrappleRole::Top, GrapplePosition::Armbar) => (Clip::Armbar, Kind::Loop),
        Grappling(GrappleRole::Bottom, GrapplePosition::Sweep) => (Clip::Swept, Kind::Once),
        Grappling(GrappleRole::Bottom, GrapplePosition::Throw) => (Clip::Thrown, Kind::Once),
        Grappling(GrappleRole::Bottom, GrapplePosition::Mount) => (Clip::Pinned, Kind::Once),
        Grappling(GrappleRole::Bottom, _) => (Clip::Caught, Kind::Once),
    }
}

/// The phase of a move the rules are in, if the fighter is in one.
fn phase(state: FighterState) -> Option<AttackPhase> {
    match state {
        FighterState::Attacking(phase)
        | FighterState::Special(phase)
        | FighterState::Grabbing(phase)
        | FighterState::Technique(_, phase) => Some(phase),
        _ => None,
    }
}

/// What the mapping produced over a set of fights.
#[derive(Default)]
struct Seen {
    clips: HashSet<Clip>,
    moves: HashSet<Move>,
    kinds: HashSet<Kind>,
    phases: HashSet<AttackPhase>,
    holds: bool,
}

impl Seen {
    /// Checks both fighters of `m` against the specification, and the scrub of a move against
    /// its phase: the wind-up over the start-up, the contact over the active ticks.
    fn check(&mut self, m: &Match) {
        for fighter in m.fighters() {
            let rules = m.rules(fighter.player()).unwrap_or(m.config());
            let playback = playback_of(fighter, rules);
            for moved in [false, true] {
                let clip = clip_of(fighter, moved);
                assert_eq!(
                    (clip, kind(playback)),
                    expected(fighter, moved),
                    "{:?} (moved: {moved})",
                    fighter.state()
                );
                self.clips.insert(clip);
            }
            self.kinds.insert(kind(playback));
            // The same state is the same movement for the sprites, the stick figure and the
            // stage: the manga cell is the movement's, the clip is the movement's.
            let character = m.config().character(fighter.player());
            for ticks in [0, 8] {
                let movement = move_of(fighter, character, ticks);
                assert_eq!(movement.pose(), pose_of(fighter, character, ticks));
                self.moves.insert(movement);
            }
            let Playback::Scrub(fraction) = playback else {
                continue;
            };
            assert!((0.0..=1.0).contains(&fraction), "{fraction}");
            match phase(fighter.state()) {
                Some(phase) => {
                    let (low, high) = match phase {
                        AttackPhase::Startup => (0.0, STRIKE_WINDUP),
                        AttackPhase::Active => (STRIKE_WINDUP, STRIKE_CONTACT),
                        AttackPhase::Recovery => (STRIKE_CONTACT, 1.0),
                    };
                    assert!(
                        low <= fraction && fraction <= high,
                        "{:?} on step {:?} shows {fraction}, out of {low}..={high}",
                        fighter.state(),
                        fighter.move_frame()
                    );
                    self.phases.insert(phase);
                }
                None => {
                    assert_eq!(fraction, HOLD_POSE, "a clinch holds its pose");
                    self.holds = true;
                }
            }
        }
    }

    /// Steps `m` `steps` times with `inputs`, checking after every step.
    fn run(&mut self, m: &mut Match, steps: usize, inputs: &common::Inputs) {
        for _ in 0..steps {
            m.step(inputs);
            self.check(m);
        }
    }
}

#[test]
fn every_state_of_the_rules_plays_its_clip_in_step_with_the_frame_data() {
    use common::*;
    let mut seen = Seen::default();

    // Standing, walking, stepping back, crouching, jumping.
    let mut m = close_duel(cfg());
    seen.check(&m);
    seen.run(&mut m, 2, &only(0, press_right()));
    seen.run(&mut m, 1, &no_inputs());
    seen.run(&mut m, 2, &only(0, press_left()));
    seen.run(&mut m, 2, &only(0, press_down()));
    seen.run(&mut m, 1, &only(0, press_jump()));
    seen.run(&mut m, 12, &no_inputs());

    // Every strike, through its whole timeline; the target flinches.
    for attack in Attack::ALL {
        let mut m = close_duel(cfg());
        seen.run(&mut m, 12, &only(0, press(attack)));
    }

    // A combo: the second hit inside the stun throws the target back, the third knocks it out.
    let config = MatchConfig {
        max_health: 50,
        ..cfg_with(|data| data.knockback = 0)
    };
    let mut m = close_duel(config);
    for attack in [
        Attack::punch(Strength::Low),
        Attack::punch(Strength::Medium),
        Attack::punch(Strength::High),
    ] {
        seen.run(&mut m, 1, &only(0, press(attack)));
        seen.run(&mut m, 3, &no_inputs());
    }
    assert!(m.fighter(PlayerId(1)).unwrap().is_knocked_out());
    seen.run(&mut m, 10, &no_inputs());

    // A kick thrown in the air.
    let mut m = duel_at(cfg(), 100, 1100);
    seen.run(&mut m, 1, &only(0, press_jump()));
    seen.run(&mut m, 1, &only(0, press(Attack::kick(Strength::Medium))));
    seen.run(&mut m, 14, &no_inputs());

    // Every character's special move.
    for character in Character::ALL {
        let mut config = cfg();
        config.special = character.rules(&cfg()).special;
        config.characters = [character; 10];
        let mut m = close_duel(config);
        let special = Input {
            special: true,
            ..Input::NONE
        };
        seen.run(&mut m, 30, &only(0, special));
    }

    // The grab and its ladder: the clinch, then the ground.
    let mut m = duel_at(cfg(), 580, 620);
    seen.run(&mut m, 1, &only(0, press_grab()));
    seen.run(&mut m, 10, &no_inputs());
    for _ in 0..3 {
        seen.run(&mut m, 4, &no_inputs());
        seen.run(&mut m, 1, &only(0, press_grab()));
        seen.run(&mut m, 6, &no_inputs());
    }

    // Both techniques, their projections and their finishes.
    for buttons in [
        press(Attack::punch(Strength::Medium)).with(Attack::kick(Strength::High)),
        press(Attack::kick(Strength::Medium)).with(Attack::kick(Strength::High)),
    ] {
        let mut m = duel_at(cfg(), 560, 600);
        seen.run(&mut m, 1, &only(0, buttons));
        seen.run(&mut m, 70, &no_inputs());
    }

    let all: HashSet<Clip> = Clip::ALL.into_iter().collect();
    assert_eq!(seen.clips, all, "every clip is played by some state");
    let moves: HashSet<Move> = Move::ALL.into_iter().collect();
    assert_eq!(seen.moves, moves, "every movement is shown in some state");
    assert_eq!(
        seen.kinds,
        [Kind::Loop, Kind::Once, Kind::Scrub].into_iter().collect()
    );
    assert_eq!(
        seen.phases,
        [
            AttackPhase::Startup,
            AttackPhase::Active,
            AttackPhase::Recovery
        ]
        .into_iter()
        .collect(),
        "every phase of a move was scrubbed"
    );
    assert!(seen.holds, "a clinch was held");
}

// ---------------------------------------------------------------------------------------------
// The stage
// ---------------------------------------------------------------------------------------------

#[test]
fn the_set_of_models_holds_characters() {
    assert!(Models::NONE.is_empty());
    assert_eq!(Models::default(), Models::NONE);
    assert!(Models::NONE.characters().is_empty());
    let all = Character::ALL
        .into_iter()
        .fold(Models::NONE, |models, character| models.with(character));
    assert_eq!(all.characters(), Character::ALL.to_vec());
    let one = Models::NONE.with(Character::Brick).with(Character::Brick);
    assert_eq!(one.characters(), vec![Character::Brick]);
    assert!(one.contains(Character::Brick) && !one.contains(Character::Torch));
}

#[test]
fn the_set_of_models_holds_girls_too() {
    let raven = Models::NONE.with_girl(Variant::Raven);
    assert!(!raven.is_empty(), "a girl alone is a model");
    assert!(raven.has_girl(Variant::Raven) && !raven.has_girl(Variant::Pink));
    assert!(raven.characters().is_empty(), "a girl is no character");
    assert_eq!(raven.girls(), [Variant::Raven]);
    assert!(Models::NONE.girls().is_empty());
    let both = raven.with(Character::Zip).with_girl(Variant::Brunette);
    assert_eq!(both.characters(), [Character::Zip]);
    assert_eq!(
        both.girls(),
        [Variant::Brunette, Variant::Raven],
        "menu order"
    );
    let all = Variant::ALL
        .into_iter()
        .fold(Models::NONE, |models, variant| models.with_girl(variant));
    assert_eq!(all.girls(), Variant::ALL.to_vec());

    // The bodies: every character's model, then every girl's; each in the set by its own bit.
    let bodies = Body::all();
    assert_eq!(bodies.len(), Character::ALL.len() + Variant::ALL.len());
    assert_eq!(bodies[0], Body::Puppet(Character::ALL[0]));
    assert_eq!(bodies[Character::ALL.len()], Body::Girl(Variant::ALL[0]));
    for body in bodies {
        let alone = Models::NONE.with_body(body);
        assert!(alone.has(body), "{body:?}");
        for other in Body::all().into_iter().filter(|&other| other != body) {
            assert!(!alone.has(other), "{body:?} is not {other:?}");
        }
        assert!(body.path().starts_with("models/") && body.path().ends_with(".glb"));
    }
    assert_eq!(
        Body::Puppet(Character::Zip).path(),
        model_path(Character::Zip)
    );
    assert_eq!(Body::Puppet(Character::Zip).name(), "Zip");
    assert_eq!(Body::Girl(Variant::Pink).path(), girl_path(Variant::Pink));
    assert_eq!(Body::Girl(Variant::Pink).name(), "pink");
}

#[test]
fn a_skin_with_a_girl_is_drawn_as_her_else_as_its_character() {
    let chalk = Models::NONE.with(Character::Chalk);
    let girls = chalk.with_girl(Variant::Pink);
    // Her own model first, whatever the character that wears her.
    for character in Character::ALL {
        assert_eq!(
            body_of(girls, character, Some(Variant::Pink)),
            Some(Body::Girl(Variant::Pink))
        );
    }
    // A girl without a model is the character's model in her colours, if it has one.
    assert_eq!(
        body_of(girls, Character::Chalk, Some(Variant::Raven)),
        Some(Body::Puppet(Character::Chalk))
    );
    assert_eq!(body_of(girls, Character::Zip, Some(Variant::Raven)), None);
    // The chalk skin has no girl: the character's model, as it comes.
    assert_eq!(
        body_of(girls, Character::Chalk, None),
        Some(Body::Puppet(Character::Chalk))
    );
    assert_eq!(body_of(girls, Character::Zip, None), None);
    assert_eq!(body_of(Models::NONE, Character::Chalk, None), None);
}

/// A practice fight on the third-person view, the countdown over.
fn third_person(client: Client) -> Headless {
    let mut game = Headless::new(Game::new(client));
    game.skip_intro();
    game.tap(&[VIEW_KEY]);
    assert_eq!(game.game().screen(), Screen::Fight);
    assert_eq!(game.game().view(), View::ThirdPerson);
    game
}

#[test]
fn without_a_fight_the_stage_is_empty() {
    let camera = Camera::look_at(Vec3::int(0, 100, -500), Vec3::int(0, 100, 0));
    let client = Client::online(MatchConfig::default(), 0);
    let stage = model::stage(
        &client,
        &camera,
        Models::NONE.with(Character::Chalk),
        &[],
        7,
    );
    assert!(stage.actors.is_empty());
    assert_eq!(stage.layer, 7);
    assert_eq!(stage.eye, [0.0, 100.0, -500.0]);
    assert_eq!(
        stage.target,
        [0.0, 100.0, -400.0],
        "100 units ahead of the eye"
    );
    // The side view and the menus have no stage at all.
    let game = Headless::practice();
    assert!(game.frame().stage.is_none());
}

#[test]
fn the_fighters_with_a_model_are_actors_of_the_stage_instead_of_mannequins() {
    let mut client = Client::practice(MatchConfig::default());
    let _ = client.set_skin(Skin::styled(Style::Manga, Variant::Raven));
    let mut game = third_person(client);
    game.idle(1);

    // No model: everything is a mannequin, the stage has no actor.
    assert_eq!(game.game().models(), Models::NONE);
    let without = game.frame();
    let stage = without
        .stage
        .clone()
        .expect("the third-person view has a stage");
    assert!(stage.actors.is_empty());
    assert!(stage.layer > 0 && stage.layer <= without.rects.len());

    // Chalk's model: both fighters of the practice fight are actors, their mannequins gone.
    let chalk = Models::NONE.with(Character::Chalk);
    game.game_mut().set_models(chalk);
    assert_eq!(game.game().models(), chalk);
    let with = game.frame();
    let stage = with.stage.clone().expect("a stage");
    assert!(
        stage.layer < without.stage.as_ref().unwrap().layer,
        "fewer rectangles under the stage: the mannequins are left out"
    );
    let players: Vec<PlayerId> = stage.actors.iter().map(|actor| actor.player).collect();
    assert_eq!(players, [PlayerId(0), PlayerId(1)]);
    let current = game.game().client().session().unwrap().current();
    for actor in &stage.actors {
        let fighter = current.fighter(actor.player).unwrap();
        assert_eq!(actor.character, Character::Chalk);
        assert_eq!(actor.facing, fighter.facing());
        assert_eq!(actor.position[1], 0.0, "on the pavement");
        assert_eq!(actor.clip, Clip::Idle);
        assert_eq!(actor.playback, Playback::Loop);
        assert_eq!((actor.cue, actor.flash), (0, false));
    }
    assert_eq!(stage.actors[0].variant, Some(Variant::Raven));
    assert!(stage.actors[0].position[0] < stage.actors[1].position[0]);

    // Another character's model leaves Chalk's fighters to the mannequins.
    game.game_mut()
        .set_models(Models::NONE.with(Character::Zip));
    let stage = game.frame().stage.unwrap();
    assert!(stage.actors.is_empty());
    assert_eq!(stage.layer, without.stage.unwrap().layer);
}

#[test]
fn a_girl_with_a_model_is_drawn_as_her_in_her_own_texture() {
    let mut client = Client::practice(MatchConfig::default());
    let _ = client.set_skin(Skin::styled(Style::Manga, Variant::Raven));
    let mut game = third_person(client);
    game.idle(1);
    let without = game.frame().stage.unwrap().layer;

    // Raven's girl alone: P1 is her, the dummy (chalk) stays a mannequin.
    game.game_mut()
        .set_models(Models::NONE.with_girl(Variant::Raven));
    let stage = game.frame().stage.unwrap();
    assert_eq!(stage.actors.len(), 1);
    let raven = &stage.actors[0];
    assert_eq!(raven.player, PlayerId(0));
    assert_eq!(raven.body, Body::Girl(Variant::Raven));
    assert_eq!(
        raven.character,
        Character::Chalk,
        "the rules' character stays"
    );
    assert_eq!(
        raven.variant, None,
        "she wears her own texture, no skin on top"
    );
    assert!(stage.layer < without, "her mannequin is left out");

    // With the chalk model as well: the dummy is an actor too, as the puppet.
    game.game_mut().set_models(
        Models::NONE
            .with(Character::Chalk)
            .with_girl(Variant::Raven),
    );
    let stage = game.frame().stage.unwrap();
    let bodies: Vec<Body> = stage.actors.iter().map(|actor| actor.body).collect();
    assert_eq!(
        bodies,
        [Body::Girl(Variant::Raven), Body::Puppet(Character::Chalk)]
    );
    assert_eq!(stage.actors[1].variant, None);

    // Another girl's model: Raven is the puppet in her colours, with the skin on top.
    game.game_mut()
        .set_models(Models::NONE.with(Character::Chalk).with_girl(Variant::Pink));
    let stage = game.frame().stage.unwrap();
    assert_eq!(stage.actors[0].body, Body::Puppet(Character::Chalk));
    assert_eq!(stage.actors[0].variant, Some(Variant::Raven));
}

#[test]
fn a_walking_actor_plays_the_walk_while_it_moves() {
    let mut game = third_person(Client::practice(MatchConfig::default()));
    game.game_mut()
        .set_models(Models::NONE.with(Character::Chalk));
    let actor = |game: &Headless| -> Actor { game.frame().stage.unwrap().actors[0].clone() };
    let walked = (0..240).any(|_| {
        game.hold(&[Key::D], 1);
        actor(&game).clip == Clip::WalkF
    });
    assert!(walked, "the walk plays once the countdown is over");
    let stood = (0..10).any(|_| {
        game.idle(1);
        actor(&game).clip == Clip::Idle
    });
    assert!(stood);
}

#[test]
fn the_models_never_reach_the_rules() {
    // The same keys with and without models: the same fight, tick for tick.
    let play = |models: Models| {
        let mut game = third_person(Client::practice(MatchConfig::default()));
        game.game_mut().set_models(models);
        for keys in [[Key::D], [Key::B], [Key::A], [Key::Space]] {
            game.hold(&keys, 40);
        }
        let current = game.game().client().session().unwrap().current();
        current
            .fighters()
            .iter()
            .map(|f| (f.x(), f.y(), f.z(), f.health(), f.state()))
            .collect::<Vec<_>>()
    };
    let all = Character::ALL
        .into_iter()
        .fold(Models::NONE, |models, character| models.with(character));
    assert_eq!(play(Models::NONE), play(all));
}

// ---------------------------------------------------------------------------------------------
// The layout of assets/models/ and its audit
// ---------------------------------------------------------------------------------------------

#[test]
fn the_models_are_laid_out_by_character_and_skin() {
    assert_eq!(model_dir(Character::Chalk), "models/chalk");
    assert_eq!(model_path(Character::Zip), "models/zip/zip.glb");
    for variant in Variant::ALL {
        let path = skin_path(Character::Brick, variant);
        let name = SPRITE_NAMES[variant.index() as usize];
        assert_eq!(path, format!("models/brick/skins/{name}.png"));
        assert!(
            std::path::Path::new("assets/skins/manga")
                .join(format!("{name}.png"))
                .exists(),
            "{name} is a sprite sheet of assets/skins/manga"
        );
        // Her own model, next to the characters' directories: models/manga/<name>.glb.
        assert_eq!(girl_path(variant), format!("models/manga/{name}.glb"));
        assert!(girl_path(variant).starts_with(GIRLS_DIR));
    }
}

/// The files of a complete model of Chalk: the puppet and every skin.
fn complete_chalk() -> Vec<String> {
    let mut files = vec![model_path(Character::Chalk)];
    files.extend(Variant::ALL.map(|variant| skin_path(Character::Chalk, variant)));
    files
}

fn audit_of(files: &[String], glb: Option<Vec<u8>>) -> Audit {
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    audit(&files, &|path| {
        assert_eq!(path, model_path(Character::Chalk), "only the model is read");
        glb.clone()
    })
}

/// The audit of `files`, every model of which reads as `glb`.
fn audit_all_as(files: &[String], glb: Option<Vec<u8>>) -> Audit {
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    audit(&files, &|path| {
        assert!(path.ends_with(".glb"), "only the models are read: {path}");
        glb.clone()
    })
}

fn problems(audit: &Audit) -> Vec<&str> {
    audit
        .lines
        .iter()
        .filter(|(severity, _)| *severity == Severity::Problem)
        .map(|(_, line)| line.as_str())
        .collect()
}

#[test]
fn the_audit_notes_missing_models_and_skins_and_flags_broken_ones() {
    // Nothing at all: four mannequins and ten girls drawn as puppets, nothing wrong.
    let empty = audit_of(&[], None);
    assert!(empty.passed());
    assert_eq!(empty.lines.len(), Character::ALL.len() + Variant::ALL.len());
    assert!(empty.lines[0].1.contains("no model"));
    assert!(
        empty
            .lines
            .last()
            .unwrap()
            .1
            .contains("puppet in her colours"),
        "{:?}",
        empty.lines
    );

    // The committed layout: the puppet and every skin.
    let complete = audit_of(&complete_chalk(), Some(puppet::glb()));
    assert!(complete.passed(), "{:?}", complete.lines);
    assert!(
        complete
            .lines
            .iter()
            .any(|(_, line)| line.contains(&format!(
                "{} clips ({})",
                Clip::ALL.len(),
                puppet::GENERATOR
            )))
    );

    // A missing skin is only noted: the model's own texture stands in.
    let mut files = complete_chalk();
    files.pop();
    let partial = audit_of(&files, Some(puppet::glb()));
    assert!(partial.passed());
    assert!(
        partial
            .lines
            .iter()
            .any(|(_, line)| line.contains("missing"))
    );

    // A file in skins/ named after no manga skin is a problem.
    let mut files = complete_chalk();
    files.push("models/chalk/skins/purple.png".into());
    let stray = audit_of(&files, Some(puppet::glb()));
    assert_eq!(problems(&stray).len(), 1, "{:?}", stray.lines);
    assert!(problems(&stray)[0].contains("purple.png"));

    // A model that cannot be read, or that is not a glTF binary.
    let unreadable = audit_of(&complete_chalk(), None);
    assert!(!unreadable.passed());
    assert!(problems(&unreadable)[0].contains("truncated"));

    // A model missing clips: one problem per basic clip, a note for each of the others
    // that says what plays instead; the generator unknown.
    let json = r#"{"asset":{"version":"2.0"},"animations":[{"name":"idle"},{"name":"kick"}]}"#;
    let gaps = audit_of(&complete_chalk(), Some(puppet::pack(json, &[])));
    assert_eq!(problems(&gaps).len(), Clip::BASIC - 2, "{:?}", gaps.lines);
    assert!(problems(&gaps).iter().any(|line| line.contains("`walk_f`")));
    assert!(
        gaps.lines
            .iter()
            .any(|(_, line)| line.contains("2 clips (unknown generator)"))
    );
    let noted: Vec<&String> = gaps
        .lines
        .iter()
        .filter(|(severity, line)| *severity == Severity::Info && line.contains("plays `"))
        .map(|(_, line)| line)
        .collect();
    assert_eq!(noted.len(), Clip::ALL.len() - Clip::BASIC, "{noted:?}");
    assert!(
        noted
            .iter()
            .any(|line| line.contains("no `mount` clip: the fighter plays `crouch` instead"))
    );

    // A model of the first thirteen clips only is complete: the rest falls back.
    let names: Vec<String> = Clip::ALL[..Clip::BASIC]
        .iter()
        .map(|clip| format!("{{\"name\":\"{}\"}}", clip.name()))
        .collect();
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"animations":[{}]}}"#,
        names.join(",")
    );
    let old = audit_of(&complete_chalk(), Some(puppet::pack(&json, &[])));
    assert!(old.passed(), "{:?}", old.lines);
}

#[test]
fn the_audit_checks_the_girls_like_the_characters() {
    let girl = girl_path(Variant::Redhead);
    let mut files = vec![girl.clone()];

    // A girl with every clip: fine, and counted with her generator.
    let complete = audit_all_as(&files, Some(puppet::glb()));
    assert!(complete.passed(), "{:?}", complete.lines);
    let counted = format!("{girl}: {} clips ({})", Clip::ALL.len(), puppet::GENERATOR);
    assert!(complete.lines.iter().any(|(_, line)| *line == counted));
    // The nine others are only noted, the redhead is not.
    let missing = complete
        .lines
        .iter()
        .filter(|(_, line)| line.contains("puppet in her colours"))
        .count();
    assert_eq!(missing, Variant::ALL.len() - 1);

    // A girl who cannot be read, or who misses the clips every model needs, is a problem.
    let unreadable = audit_all_as(&files, None);
    assert!(!unreadable.passed());
    assert!(problems(&unreadable)[0].contains(&format!("{girl}: truncated")));
    let json = r#"{"asset":{"version":"2.0"},"animations":[{"name":"idle"}]}"#;
    let gaps = audit_all_as(&files, Some(puppet::pack(json, &[])));
    assert_eq!(problems(&gaps).len(), Clip::BASIC - 1, "{:?}", gaps.lines);
    assert!(problems(&gaps).iter().all(|line| line.contains(&girl)));
    assert!(problems(&gaps).iter().any(|line| line.contains("`punch`")));

    // A file in models/manga/ that is no girl's model is a problem.
    files.push("models/manga/purple.glb".into());
    files.push("models/manga/redhead.png".into());
    let stray = audit_all_as(&files, Some(puppet::glb()));
    let found = problems(&stray);
    assert_eq!(found.len(), 2, "{:?}", stray.lines);
    assert!(found[0].contains("purple.glb") && found[1].contains("redhead.png"));
}

#[test]
fn the_committed_models_pass_the_audit() {
    fn list(dir: &std::path::Path, prefix: &str, files: &mut Vec<String>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                list(&entry.path(), &format!("{path}/"), files);
            } else {
                files.push(path);
            }
        }
    }
    let root = std::path::Path::new("assets");
    let mut files = Vec::new();
    list(&root.join("models"), "models/", &mut files);
    let paths: Vec<&str> = files.iter().map(String::as_str).collect();
    let found = audit(&paths, &|path| std::fs::read(root.join(path)).ok());
    assert!(found.passed(), "cargo xtask models: {:?}", found.lines);
}

/// The puppet's header with `patch` applied.
fn patched(patch: impl Fn(&mut Vec<u8>)) -> Vec<u8> {
    let mut glb = puppet::pack("{}", &[]);
    patch(&mut glb);
    glb
}

#[test]
fn a_broken_gltf_binary_says_what_is_wrong() {
    let error = |bytes: &[u8]| glb_json(bytes).unwrap_err();
    assert_eq!(error(&[]), "truncated at byte 0");
    assert_eq!(error(b"glTF"), "truncated at byte 4");
    assert_eq!(error(b"glTF\x02\0\0\0"), "truncated at byte 8");
    let chunk_header_cut = patched(|glb| {
        glb.truncate(16);
        glb[8..12].copy_from_slice(&16u32.to_le_bytes());
    });
    assert_eq!(error(&chunk_header_cut), "truncated at byte 16");
    assert!(error(b"PK\x03\x04 not a model").contains("magic"));
    assert!(error(&patched(|glb| glb[4] = 1)).contains("version 1"));
    assert!(error(&patched(|glb| glb.push(0))).contains("declares"));
    let header_only = patched(|glb| {
        glb.truncate(12);
        glb[8..12].copy_from_slice(&12u32.to_le_bytes());
    });
    assert_eq!(error(&header_only), "truncated at byte 12");
    assert!(error(&patched(|glb| glb[16] = b'X')).contains("not JSON"));
    assert!(error(&patched(|glb| glb[12] = 200)).contains("truncated"));
    assert!(error(&patched(|glb| glb[20] = 0xff)).contains("UTF-8"));
    assert_eq!(glb_json(&puppet::pack("{}", &[1, 2, 3])).unwrap(), "{}  ");
    assert!(clips_of(&[]).is_err());
    assert_eq!(generator_of(&[]), None);
}

fn clips_in(json: &str) -> Vec<String> {
    clips_of(&puppet::pack(json, &[])).unwrap()
}

#[test]
fn the_clip_names_are_read_from_the_animations_only() {
    // The names of the animations, not of their channels, nodes or extras.
    assert_eq!(
        clips_in(
            r#"{"nodes":[{"name":"hips"}],"animations":[{"name":"a","channels":[{"name":"x"}]},
            {"extras":{"name":"y"},"name":"b"}],"meshes":[{"name":"m"}]}"#
        ),
        ["a", "b"]
    );
    // Strings are skipped whole: brackets, quotes and keys inside them do not count.
    assert_eq!(
        clips_in(r#"{"k":"[{\"animations\":[","animations":[{"name":"p\"q\\n\t\n\/"}]}"#),
        ["p\"q\\n\t\n/"]
    );
    // Nested or valueless lookalikes are not the animations.
    assert!(clips_in(r#"{"a":{"animations":[{"name":"z"}]}}"#).is_empty());
    assert!(clips_in(r#"{"k":"animations","animations":[{"name":5}]}"#).is_empty());
    assert!(clips_in(r#"{"asset":{}}"#).is_empty());
    // A string that never closes ends the scan.
    assert!(clips_in(r#"{"animations":[{"name":"unclosed"#).is_empty());
    assert_eq!(clips_in(r#"{"animations":[{"name":"a"}],"x":"#), ["a"]);
}

#[test]
fn the_generator_is_read_when_it_is_a_string() {
    let generator = |json: &str| generator_of(&puppet::pack(json, &[]));
    assert_eq!(
        generator(r#"{"asset":{"generator" : "Meshy"}}"#),
        Some("Meshy".into())
    );
    assert_eq!(generator(r#"{"asset":{"generator":3}}"#), None);
    assert_eq!(generator(r#"{"asset":{"version":"2.0"}}"#), None);
    assert_eq!(generator(r#"{"generator"}"#), None);
    assert_eq!(generator(r#"{"x":"#), None);
}

// ---------------------------------------------------------------------------------------------
// The puppet
// ---------------------------------------------------------------------------------------------

#[test]
fn the_puppet_has_every_clip_and_is_deterministic() {
    let glb = puppet::glb();
    assert_eq!(glb, puppet::glb());
    assert_eq!(glb.len() % 4, 0);
    let names: Vec<&str> = Clip::ALL.iter().map(|clip| clip.name()).collect();
    assert_eq!(clips_of(&glb).unwrap(), names);
    assert_eq!(generator_of(&glb).as_deref(), Some(puppet::GENERATOR));
    let json = glb_json(&glb).unwrap();
    for needle in [
        "\"skins\"",
        "\"inverseBindMatrices\"",
        "\"JOINTS_0\"",
        "\"images\"",
    ] {
        assert!(json.contains(needle), "{needle}");
    }
    assert!(puppet::made_here(puppet::GENERATOR));
    assert!(
        puppet::made_here("srr cube man"),
        "the old placeholder is ours"
    );
    assert!(!puppet::made_here("Meshy"));
}

#[test]
fn the_clips_export_holds_the_skeleton_and_every_key_of_every_clip() {
    let json = puppet::clips_json();
    assert!(json.starts_with(&format!("{{\"generator\":\"{}\"", puppet::GENERATOR)));
    // The eleven joints, the root first, each with its parent and rest offset.
    assert_eq!(json.matches("\"parent\":").count(), puppet::JOINT_COUNT);
    assert!(json.contains("\"name\":\"hips\",\"parent\":-1"));
    assert!(json.contains("\"name\":\"knee_r\",\"parent\":9"));
    // Every clip by its name, with every keyframe of the clip, each carrying one pitch per joint.
    let mut keys = 0;
    for clip in Clip::ALL {
        assert!(json.contains(&format!("\"name\":\"{}\",\"seconds\"", clip.name())));
        keys += clip_poses(clip).1.len();
    }
    assert_eq!(json.matches("\"pitch\":[").count(), keys);
    assert_eq!(json.matches("\"keys\":[").count(), Clip::ALL.len());
    // The first key of the idle clip is the pose the puppet is solved to.
    let rig = puppet::solve(&clip_poses(Clip::Idle).1[0].1);
    assert!(json.contains(&format!(
        "\"t\":0.0,\"hips\":[0.0,{:?},{:?}]",
        rig.hips[1], rig.hips[2]
    )));
}

#[test]
fn the_puppet_packs_padded_chunks() {
    let glb = puppet::pack("{\"a\":1}", &[1, 2, 3, 4, 5]);
    assert_eq!(glb.len() % 4, 0);
    assert_eq!(glb_json(&glb).unwrap().trim_end(), "{\"a\":1}");
}

#[test]
fn the_puppet_is_a_smooth_body_standing_on_the_floor() {
    let body = puppet::geometry();
    let count = body.positions.len();
    assert!(
        count > 10_000 && count < 65_536,
        "{count} vertices fit 16-bit indices"
    );
    assert_eq!(body.normals.len(), count);
    assert_eq!(body.uvs.len(), count);
    assert_eq!(body.joints.len(), count);
    assert_eq!(body.indices.len() % 3, 0);
    assert!(body.indices.iter().all(|&i| (i as usize) < count));
    // Every joint carries something, and no vertex follows a joint the skeleton lacks.
    let used: HashSet<u8> = body.joints.iter().copied().collect();
    assert_eq!(used.len(), puppet::JOINT_COUNT);
    assert!(
        used.iter()
            .all(|&joint| (joint as usize) < puppet::JOINT_COUNT)
    );
    // Unit normals, texture coordinates inside the atlas.
    for normal in &body.normals {
        let length = normal.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((length - 1.0).abs() < 1e-3, "{normal:?}");
    }
    for uv in &body.uvs {
        assert!(uv.iter().all(|&c| (0.0..=1.0).contains(&c)), "{uv:?}");
    }
    // 1.8 m from the soles to the hair, the soles on the floor, centred, facing +z.
    let bounds = |axis: usize| {
        let values = body.positions.iter().map(|p| p[axis]);
        (
            values.clone().fold(f32::MAX, f32::min),
            values.fold(f32::MIN, f32::max),
        )
    };
    let (low, high) = bounds(1);
    assert!(low.abs() < 0.01, "the soles are at {low}");
    assert!((high - puppet::HEIGHT).abs() < 0.03, "{high} tall");
    let (back, front) = bounds(2);
    assert!(front > -back, "the nose and the shoes point to +z");
    let (left, right) = bounds(0);
    assert!((left + right).abs() < 0.01, "{left} {right}");
    // A triangle faces out: the normal of its first corner is on the side of its winding.
    let corner = |i: u16| body.positions[i as usize];
    let mut outward = 0;
    let mut checked = 0;
    for triangle in body.indices.chunks(3).step_by(97) {
        let [a, b, c] = [
            corner(triangle[0]),
            corner(triangle[1]),
            corner(triangle[2]),
        ];
        let edge = |p: [f32; 3], q: [f32; 3]| [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
        let (u, v) = (edge(a, b), edge(a, c));
        let face = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let size = face.iter().map(|x| x * x).sum::<f32>().sqrt();
        if size < 1e-9 {
            continue; // a sliver at a pole
        }
        let normal = body.normals[triangle[0] as usize];
        checked += 1;
        if face.iter().zip(normal).map(|(f, n)| f * n).sum::<f32>() > 0.0 {
            outward += 1;
        }
    }
    assert!(
        checked > 100 && outward * 100 >= checked * 99,
        "{outward} of {checked}"
    );
}

/// The pose of `movement` at its active moment.
fn pose_at(movement: Move) -> srr::front::stick::Pose {
    srr::front::stick::move_pose(movement, AttackPhase::Active)
}

/// The unit vector from `a` to `b` (`None` when they are the same point).
fn direction(a: [f32; 2], b: [f32; 2]) -> Option<[f32; 2]> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx.hypot(dy);
    (length > 1e-4).then(|| [dx / length, dy / length])
}

#[test]
fn the_skeleton_points_where_the_stick_figure_points() {
    use srr::front::stick::{ARM, LEG, Point, bend};
    // Forward kinematics of the solved pose: every bone points the way the same bone of the
    // stick figure points (their lengths differ where the cartoon stretches), and the hips
    // stand where the hip does, at sixty-four pixels to the metre.
    let k = 1.0 / 64.0;
    let flat = |q: Point| [q.x, q.y];
    let mut bones = 0;
    for movement in Move::ALL {
        let pose = pose_at(movement);
        let points = puppet::points(&puppet::solve(&pose));
        assert!(
            (points.hips[0] - pose.hip.x * k).abs() < 1e-5,
            "{movement:?}"
        );
        assert!(
            (points.hips[1] - 0.05 - pose.hip.y * k).abs() < 1e-5,
            "{movement:?}"
        );
        let mut same = |got: Option<[f32; 2]>, want: Option<[f32; 2]>, what: &str| {
            if let (Some(got), Some(want)) = (got, want) {
                let error = (got[0] - want[0]).hypot(got[1] - want[1]);
                assert!(
                    error < 2e-3,
                    "{movement:?} {what}: {got:?} against {want:?}"
                );
                bones += 1;
            }
        };
        same(
            direction(points.hips, points.neck),
            direction(flat(pose.hip), flat(pose.neck)),
            "torso",
        );
        let torso = [pose.neck.x - pose.hip.x, pose.neck.y - pose.hip.y];
        let shoulder = Point {
            x: pose.hip.x + torso[0] * 0.85,
            y: pose.hip.y + torso[1] * 0.85,
        };
        for side in 0..2 {
            let hand = pose.hands[side];
            let elbow = bend(shoulder, hand, ARM, pose.bends[side]);
            same(
                direction(points.shoulder, points.elbows[side]),
                direction(flat(shoulder), flat(elbow)),
                "upper arm",
            );
            same(
                direction(points.elbows[side], points.hands[side]),
                direction(flat(elbow), flat(hand)),
                "forearm",
            );
            let foot = pose.feet[side];
            let knee = bend(pose.hip, foot, LEG, pose.bends[2 + side]);
            same(
                direction(points.hips, points.knees[side]),
                direction(flat(pose.hip), flat(knee)),
                "thigh",
            );
            same(
                direction(points.knees[side], points.feet[side]),
                direction(flat(knee), flat(foot)),
                "shin",
            );
        }
    }
    assert!(bones > 200, "{bones} bones checked");
}

#[test]
fn every_clip_is_keyed_from_the_poses_of_the_stick_figure() {
    use srr::front::stick::clip_poses;
    for clip in Clip::ALL {
        let (seconds, keys) = clip_poses(clip);
        assert!(seconds > 0.0 && seconds <= 2.0, "{clip:?}");
        assert!(keys.len() >= 3, "{clip:?}");
        assert_eq!(keys[0].0, 0.0, "{clip:?} starts at the start");
        assert_eq!(keys.last().unwrap().0, 1.0, "{clip:?} ends at the end");
        assert!(
            keys.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "{clip:?}"
        );
        // The solved keys differ where the poses do: a clip that moves is not one pose.
        let rigs: Vec<puppet::Rig> = keys.iter().map(|(_, pose)| puppet::solve(pose)).collect();
        let moves = rigs.windows(2).any(|pair| pair[0] != pair[1]);
        let holds = matches!(
            clip,
            Clip::Crouch
                | Clip::Block
                | Clip::Throw
                | Clip::Scissor
                | Clip::Mount
                | Clip::Submission
                | Clip::ScarfHold
                | Clip::Armbar
        );
        assert_eq!(moves, !holds, "{clip:?}");
    }
    // A strike winds up at the end of the start-up and touches at the contact: the keys sit
    // where the engine scrubs to.
    let (_, punch) = clip_poses(Clip::Punch);
    let times: Vec<f32> = punch.iter().map(|key| key.0).collect();
    assert_eq!(times[1], STRIKE_WINDUP);
    assert_eq!(times[2], STRIKE_CONTACT);
    let contact = &punch[2].1;
    let guard = &punch[0].1;
    assert!(
        contact.hands[0].x > guard.hands[0].x + 20.0,
        "the fist is out at contact"
    );
    // The two walks are the same stride run in opposite directions.
    let (_, forward) = clip_poses(Clip::WalkF);
    let (_, backward) = clip_poses(Clip::WalkB);
    for (a, b) in forward.iter().zip(backward.iter().rev()) {
        assert_eq!(a.1.feet[0], b.1.feet[0]);
    }
    // Each ground position has its own pose, none the guard's.
    let rigs: HashSet<Vec<u32>> = Move::ALL
        .into_iter()
        .map(|movement| {
            let rig = puppet::solve(&pose_at(movement));
            rig.pitch.iter().map(|p| p.to_bits()).collect()
        })
        .collect();
    assert!(rigs.len() >= 28, "{} distinct poses", rigs.len());
}

#[test]
fn every_skin_of_the_puppet_is_the_atlas_in_its_palette() {
    let mut palettes = HashSet::new();
    for variant in Variant::ALL {
        let png = puppet::skin(variant);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..24], [0, 0, 1, 32, 0, 0, 0, 192], "288x192 pixels");
        assert_eq!(png[25], 2, "opaque RGB");
        palettes.insert(png);
    }
    assert_eq!(
        palettes.len(),
        Variant::ALL.len(),
        "every variant has its colours"
    );
    assert!(
        !palettes.contains(&puppet::texture(&puppet::Paints::chalk())),
        "the chalk's own paint is none of theirs"
    );
}

#[test]
fn the_committed_placeholder_is_the_puppet() {
    let root = std::path::Path::new("assets");
    let read =
        |path: String| std::fs::read(root.join(&path)).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert!(
        read(model_path(Character::Chalk)) == puppet::glb(),
        "assets/models/chalk/chalk.glb is stale: \
         cargo run --no-default-features --example models -- --write chalk"
    );
    for variant in Variant::ALL {
        assert!(
            read(skin_path(Character::Chalk, variant)) == puppet::skin(variant),
            "{variant:?}"
        );
    }
}

#[test]
fn a_model_without_the_new_clips_plays_what_it_always_played() {
    let old = |clip: Clip| clip.is_basic();
    for clip in Clip::ALL {
        let played = clip.resolve(&old);
        assert!(played.is_basic(), "{clip:?} -> {played:?}");
        if clip.is_basic() {
            assert_eq!(played, clip);
            assert_eq!(clip.fallback(), None);
        } else {
            assert_eq!(
                Some(played),
                clip.fallback(),
                "{clip:?} has a basic fallback"
            );
        }
    }
    // The grapple's ground game keeps its old look: the top crouches, the bottom lies.
    for clip in [Clip::Mount, Clip::Submission, Clip::ScarfHold, Clip::Armbar] {
        assert_eq!(clip.resolve(&old), Clip::Crouch);
    }
    for clip in [Clip::Swept, Clip::Thrown, Clip::Pinned, Clip::Caught] {
        assert_eq!(clip.resolve(&old), Clip::Ko);
    }
    // And a model with none of them stands still: the idle, or nothing if it lacks that too.
    assert_eq!(Clip::Punch.resolve(&|clip| clip == Clip::Idle), Clip::Idle);
    assert_eq!(Clip::Punch.resolve(&|_| false), Clip::Idle);
    assert_eq!(
        Clip::Mount.resolve(&|clip| clip == Clip::Mount),
        Clip::Mount
    );
}

#[test]
fn every_movement_has_a_clip_and_a_cell() {
    for (index, movement) in Move::ALL.into_iter().enumerate() {
        assert_eq!(movement.index(), index);
        // The first twenty-two are the manga poses, in their order.
        if index < srr::front::skin::Pose::ALL.len() {
            assert_eq!(
                movement.pose(),
                srr::front::skin::Pose::ALL[index],
                "{movement:?}"
            );
        }
        // The clip of a movement keys the pose of that movement.
        let (_, keys) = srr::front::stick::clip_poses(movement.clip());
        assert!(keys.len() >= 3);
    }
    // Every clip is some movement's, but for the ones the states reach other ways.
    let clips: HashSet<Clip> = Move::ALL.into_iter().map(Move::clip).collect();
    for clip in Clip::ALL {
        assert!(
            clips.contains(&clip) || matches!(clip, Clip::WalkB | Clip::HitHeavy | Clip::Sweep),
            "{clip:?}"
        );
    }
    for (limb, strength, movement) in [
        (Limb::Punch, Strength::Low, Move::PunchLow),
        (Limb::Punch, Strength::Medium, Move::PunchMid),
        (Limb::Punch, Strength::High, Move::PunchHigh),
        (Limb::Kick, Strength::Low, Move::KickLow),
        (Limb::Kick, Strength::Medium, Move::KickMid),
        (Limb::Kick, Strength::High, Move::KickHigh),
    ] {
        assert_eq!(Move::strike(limb, strength), movement);
    }
}
