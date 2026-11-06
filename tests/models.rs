//! The 3D fighters as the front end describes them (`srr::front::model`), and the cube man that
//! stands in for a real model (`srr::front::cubeman`):
//!
//! * the toon band math the shader mirrors;
//! * the clip of every state the rules can hold, and how it runs: the strikes, the special move,
//!   the grab and the techniques are scrubbed through their frame data, so the swing reaches its
//!   contact pose on the active ticks exactly;
//! * the stage of the third-person view: one actor per fighter with a model, left out of the
//!   software mannequins, never fed back into the rules;
//! * the layout of `assets/models/` and its audit, the glTF reading behind it;
//! * the cube man, byte for byte the committed placeholder of `assets/models/chalk/`.

mod common;

use std::collections::HashSet;

use srr::front::cubeman;
use srr::front::model::{
    self, Actor, Audit, BAND_EDGES, BAND_LEVELS, CROSSFADE_SECONDS, Clip, HEAVY_HIT_COMBO,
    HOLD_POSE, MODEL_HEIGHT, Models, OUTLINE_WIDTH, Playback, RIM, STRIKE_CONTACT, STRIKE_WINDUP,
    Severity, audit, band, clip_of, clips_of, generator_of, glb_json, model_dir, model_path,
    playback_of, rim, skin_path, strike_fraction,
};
use srr::front::skin::SPRITE_NAMES;
use srr::front::space::{Camera, Vec3};
use srr::front::{Game, Headless, Screen, VIEW_KEY, View};
use srr::game::{
    Attack, AttackPhase, Character, Fighter, FighterState, GrapplePosition, GrappleRole, Input,
    Key, Limb, Match, MatchConfig, PlayerId, Skin, Strength, Style, Variant,
};
use srr::net::Client;

// ---------------------------------------------------------------------------------------------
// Tuning and the toon bands
// ---------------------------------------------------------------------------------------------

#[test]
fn the_toon_bands_step_at_their_edges() {
    assert_eq!(band(-1.0), BAND_LEVELS[0]);
    assert_eq!(band(BAND_EDGES[0] - 0.001), BAND_LEVELS[0]);
    assert_eq!(
        band(BAND_EDGES[0]),
        BAND_LEVELS[1],
        "an edge belongs to the band above"
    );
    assert_eq!(band(BAND_EDGES[1] - 0.001), BAND_LEVELS[1]);
    assert_eq!(band(BAND_EDGES[1]), BAND_LEVELS[2]);
    assert_eq!(band(1.0), BAND_LEVELS[2]);
    // Flat bands: three steps, darker to lighter, the lit band unshaded.
    const { assert!(BAND_EDGES[0] < BAND_EDGES[1]) };
    const { assert!(BAND_LEVELS[0] < BAND_LEVELS[1] && BAND_LEVELS[1] < BAND_LEVELS[2]) };
    assert_eq!(BAND_LEVELS[2], 1.0);
    // The rim shows on the silhouette only.
    assert_eq!(rim(RIM.0 - 0.01), RIM.1);
    assert_eq!(rim(RIM.0), 0.0);
    assert_eq!(rim(1.0), 0.0);
}

#[test]
fn the_tuning_knobs_are_in_range() {
    const {
        assert!(CROSSFADE_SECONDS > 0.0 && CROSSFADE_SECONDS <= 0.25);
        assert!(0.0 < STRIKE_WINDUP && STRIKE_WINDUP < STRIKE_CONTACT && STRIKE_CONTACT < 1.0);
        assert!(STRIKE_WINDUP < HOLD_POSE && HOLD_POSE < STRIKE_CONTACT);
        assert!(HEAVY_HIT_COMBO >= 2);
        assert!(MODEL_HEIGHT > 0.0 && OUTLINE_WIDTH > 0.0);
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
            "special"
        ]
    );
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
        Attacking(_) if fighter.attack().unwrap().limb == Limb::Kick => (Clip::Kick, Kind::Scrub),
        Attacking(_) | Grabbing(_) => (Clip::Punch, Kind::Scrub),
        Special(_) => (Clip::Special, Kind::Scrub),
        Technique(..) => (Clip::Sweep, Kind::Scrub),
        // Both sides of a clinch hold a pose: the top's grip, the bottom's flinch.
        Grappling(GrappleRole::Bottom, GrapplePosition::Clinch) => (Clip::HitLight, Kind::Scrub),
        Grappling(GrappleRole::Bottom, _) => (Clip::Ko, Kind::Once),
        Grappling(GrappleRole::Top, GrapplePosition::Clinch) => (Clip::Punch, Kind::Scrub),
        Grappling(GrappleRole::Top, GrapplePosition::Sweep | GrapplePosition::Throw) => {
            (Clip::Sweep, Kind::Loop)
        }
        Grappling(GrappleRole::Top, _) => (Clip::Crouch, Kind::Loop),
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
    }
}

/// The files of a complete model of Chalk: the cube man and every skin.
fn complete_chalk() -> Vec<String> {
    let mut files = vec![model_path(Character::Chalk)];
    files.extend(Variant::ALL.map(|variant| skin_path(Character::Chalk, variant)));
    files
}

fn audit_of(files: &[String], glb: Option<Vec<u8>>) -> Audit {
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    audit(&files, |path| {
        assert_eq!(path, model_path(Character::Chalk), "only the model is read");
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
    // Nothing at all: four mannequins, nothing wrong.
    let empty = audit_of(&[], None);
    assert!(empty.passed());
    assert_eq!(empty.lines.len(), Character::ALL.len());
    assert!(empty.lines[0].1.contains("no model"));

    // The committed layout: the cube man and every skin.
    let complete = audit_of(&complete_chalk(), Some(cubeman::glb()));
    assert!(complete.passed(), "{:?}", complete.lines);
    assert!(
        complete
            .lines
            .iter()
            .any(|(_, line)| line.contains(&format!("13 clips ({})", cubeman::GENERATOR)))
    );

    // A missing skin is only noted: the model's own texture stands in.
    let mut files = complete_chalk();
    files.pop();
    let partial = audit_of(&files, Some(cubeman::glb()));
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
    let stray = audit_of(&files, Some(cubeman::glb()));
    assert_eq!(problems(&stray).len(), 1, "{:?}", stray.lines);
    assert!(problems(&stray)[0].contains("purple.png"));

    // A model that cannot be read, or that is not a glTF binary.
    let unreadable = audit_of(&complete_chalk(), None);
    assert!(!unreadable.passed());
    assert!(problems(&unreadable)[0].contains("truncated"));

    // A model missing clips: one problem per clip, the generator unknown.
    let json = r#"{"asset":{"version":"2.0"},"animations":[{"name":"idle"},{"name":"kick"}]}"#;
    let gaps = audit_of(&complete_chalk(), Some(cubeman::pack(json, &[])));
    assert_eq!(
        problems(&gaps).len(),
        Clip::ALL.len() - 2,
        "{:?}",
        gaps.lines
    );
    assert!(problems(&gaps).iter().any(|line| line.contains("`walk_f`")));
    assert!(
        gaps.lines
            .iter()
            .any(|(_, line)| line.contains("2 clips (unknown generator)"))
    );
}

/// The cube man's header with `patch` applied.
fn patched(patch: impl Fn(&mut Vec<u8>)) -> Vec<u8> {
    let mut glb = cubeman::pack("{}", &[]);
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
    assert_eq!(glb_json(&cubeman::pack("{}", &[1, 2, 3])).unwrap(), "{}  ");
    assert!(clips_of(&[]).is_err());
    assert_eq!(generator_of(&[]), None);
}

fn clips_in(json: &str) -> Vec<String> {
    clips_of(&cubeman::pack(json, &[])).unwrap()
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
    let generator = |json: &str| generator_of(&cubeman::pack(json, &[]));
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
// The cube man
// ---------------------------------------------------------------------------------------------

#[test]
fn the_cube_man_has_every_clip_and_is_deterministic() {
    let glb = cubeman::glb();
    assert_eq!(glb, cubeman::glb());
    assert_eq!(glb.len() % 4, 0);
    let names: Vec<&str> = Clip::ALL.iter().map(|clip| clip.name()).collect();
    assert_eq!(clips_of(&glb).unwrap(), names);
    assert_eq!(generator_of(&glb).as_deref(), Some(cubeman::GENERATOR));
    let json = glb_json(&glb).unwrap();
    for needle in [
        "\"skins\"",
        "\"inverseBindMatrices\"",
        "\"JOINTS_0\"",
        "\"images\"",
    ] {
        assert!(json.contains(needle), "{needle}");
    }
    const { assert!(cubeman::HEIGHT > 1.0) };
}

#[test]
fn the_cube_man_packs_padded_chunks() {
    let glb = cubeman::pack("{\"a\":1}", &[1, 2, 3, 4, 5]);
    assert_eq!(glb.len() % 4, 0);
    assert_eq!(glb_json(&glb).unwrap().trim_end(), "{\"a\":1}");
}

#[test]
fn every_skin_of_the_cube_man_is_a_four_cell_palette() {
    let mut palettes = HashSet::new();
    for variant in Variant::ALL {
        let png = cubeman::skin(variant);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..24], [0, 0, 0, 4, 0, 0, 0, 1], "4x1 pixels");
        assert_eq!(png[25], 2, "opaque RGB");
        palettes.insert(png);
    }
    assert_eq!(
        palettes.len(),
        Variant::ALL.len(),
        "every variant has its colours"
    );
}

#[test]
fn the_committed_placeholder_is_the_cube_man() {
    let root = std::path::Path::new("assets");
    let read =
        |path: String| std::fs::read(root.join(&path)).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert!(
        read(model_path(Character::Chalk)) == cubeman::glb(),
        "assets/models/chalk/chalk.glb is stale: \
         cargo run --no-default-features --example models -- --write chalk"
    );
    for variant in Variant::ALL {
        assert!(
            read(skin_path(Character::Chalk, variant)) == cubeman::skin(variant),
            "{variant:?}"
        );
    }
}
