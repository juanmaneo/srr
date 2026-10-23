//! Characters: their numbers, their special moves and the fireball.

mod common;

use common::*;
use srr::game::{Attack, ProjectileData};
use srr::game::{
    AttackPhase, Character, ConfigError, Facing, FighterState, Input, Match, MatchConfig, Outcome,
    Phase, PlayerId, PlayerSlot, Roster, SHOULDER_RUSH, Strength, TeamId,
};

fn special() -> Input {
    Input {
        special: true,
        ..Input::NONE
    }
}

/// [`cfg`] with player 0 and player 1 playing the given characters.
fn cast(first: Character, second: Character) -> MatchConfig {
    cfg()
        .with_character(PlayerId(0), first)
        .with_character(PlayerId(1), second)
}

fn special_state(phase: AttackPhase) -> FighterState {
    FighterState::Special(phase)
}

/// Steps once with player 0 holding `input` and nobody else pressing anything.
fn step0(m: &mut Match, input: Input) {
    m.step(&only(0, input));
}

// ---------------------------------------------------------------------------------------------
// The roster of characters
// ---------------------------------------------------------------------------------------------

#[test]
fn there_are_four_characters_and_chalk_is_the_default() {
    assert_eq!(
        Character::ALL,
        [
            Character::Chalk,
            Character::Torch,
            Character::Brick,
            Character::Zip
        ]
    );
    assert_eq!(Character::default(), Character::Chalk);
    let names: Vec<String> = Character::ALL.iter().map(ToString::to_string).collect();
    assert_eq!(names, ["Chalk", "Torch", "Brick", "Zip"]);
    for character in Character::ALL {
        let data = character.data();
        assert_eq!(data.name, character.name());
        assert!(!data.special_name.is_empty() && !data.tagline.is_empty());
        assert_eq!(character.next().previous(), character);
    }
    assert_eq!(Character::Torch.data().special_name, "Fireball");
}

#[test]
fn chalk_follows_the_configuration_exactly() {
    let base = MatchConfig::default();
    assert_eq!(Character::Chalk.rules(&base), base);
    assert_eq!(base.special, SHOULDER_RUSH);
    let custom = cfg();
    assert_eq!(Character::Chalk.rules(&custom), custom);
}

#[test]
fn the_other_characters_scale_the_configuration() {
    let base = MatchConfig::default();
    let brick = Character::Brick.rules(&base);
    assert_eq!(brick.max_health, 130);
    assert_eq!(brick.walk_speed, 5, "75 % of 7, rounded down");
    assert_eq!(brick.jump_velocity, 18);
    assert_eq!(brick.punches[0].damage, 5, "115 % of 5");
    assert_eq!(brick.kicks[2].damage, 19, "115 % of 17");
    assert_eq!(brick.kicks[2].range, 131);
    assert_eq!(brick.grapple.range, 68);
    assert_eq!(brick.techniques[1].range, 81);
    assert!(brick.special.armored);
    assert_eq!(brick.arena_width, base.arena_width, "the shared rules stay");

    let zip = Character::Zip.rules(&base);
    assert_eq!(
        (zip.max_health, zip.walk_speed, zip.sidestep_speed),
        (80, 9, 6)
    );
    assert!(zip.special.low_profile);

    let torch = Character::Torch.rules(&base);
    assert_eq!(torch.max_health, 90);
    assert!(torch.special.projectile.is_some());

    for character in Character::ALL {
        assert_eq!(character.rules(&base).validate(), Ok(()), "{character}");
    }
}

#[test]
fn a_character_always_keeps_some_health() {
    let frail = MatchConfig {
        max_health: 1,
        ..cfg()
    };
    assert_eq!(Character::Zip.rules(&frail).max_health, 1);
}

#[test]
fn every_player_has_a_character() {
    let config = MatchConfig::default().with_character(PlayerId(9), Character::Zip);
    assert_eq!(config.character(PlayerId(9)), Character::Zip);
    assert_eq!(config.characters[9], Character::Zip);
    let unchanged = config
        .clone()
        .with_character(PlayerId(10), Character::Brick);
    assert_eq!(unchanged, config, "no seat 10");
    assert_eq!(config.character(PlayerId(10)), Character::Chalk);
    assert_eq!(config.rules_of(PlayerId(9)), Character::Zip.rules(&config));
}

#[test]
fn fighters_follow_the_rules_of_their_character() {
    let mut m = duel_at(cast(Character::Brick, Character::Zip), 300, 800);
    assert_eq!(health_of(&m, 0), 130);
    assert_eq!(health_of(&m, 1), 80);
    assert_eq!(m.config().max_health, 100, "the base configuration");
    assert_eq!(m.rules(PlayerId(0)).unwrap().max_health, 130);
    m.step(&inputs_of(&[(0, press_right()), (1, press_left())]));
    assert_eq!(x_of(&m, 0), 307, "75 % of the walk speed");
    assert_eq!(x_of(&m, 1), 787, "135 % of the walk speed");
}

#[test]
fn the_special_move_is_validated() {
    let mut config = cfg();
    config.special.attack.active = 0;
    assert_eq!(config.validate(), Err(ConfigError::ZeroSpecialActiveFrames));
    assert_eq!(
        ConfigError::ZeroSpecialActiveFrames.to_string(),
        "the special move needs at least one active tick"
    );
    config.special.attack.active = 1;
    config.special.projectile = Some(ProjectileData {
        speed: 5,
        lifetime: 0,
        size: 5,
    });
    assert_eq!(config.validate(), Err(ConfigError::ZeroProjectileLifetime));
    assert_eq!(
        ConfigError::ZeroProjectileLifetime.to_string(),
        "the projectile needs a lifetime of at least one tick"
    );
}

// ---------------------------------------------------------------------------------------------
// The special move: Chalk's Shoulder Rush
// ---------------------------------------------------------------------------------------------

#[test]
fn the_shoulder_rush_lunges_through_its_timeline() {
    let mut m = duel_at(cfg(), 100, 1100);
    step0(&mut m, special());
    assert_eq!(state_of(&m, 0), special_state(AttackPhase::Startup));
    assert!(fighter(&m, 0).is_special());
    assert_eq!(attack_of(&m, 0), None, "not one of the six attacks");
    assert_eq!(x_of(&m, 0), 112, "rushes forward from the first tick");
    run(&mut m, 7, &no_inputs());
    assert_eq!(state_of(&m, 0), special_state(AttackPhase::Startup));
    run(&mut m, 1, &no_inputs());
    assert_eq!(state_of(&m, 0), special_state(AttackPhase::Active));
    run(&mut m, 5, &no_inputs());
    assert_eq!(x_of(&m, 0), 100 + 14 * 12);
    run(&mut m, 1, &no_inputs());
    assert_eq!(state_of(&m, 0), special_state(AttackPhase::Recovery));
    assert_eq!(x_of(&m, 0), 268, "no lunge while recovering");
    run(&mut m, 17, &no_inputs());
    assert!(fighter(&m, 0).is_special());
    run(&mut m, 1, &no_inputs());
    assert_eq!(state_of(&m, 0), FighterState::Idle);
}

#[test]
fn the_shoulder_rush_hits_like_a_strike() {
    let mut m = duel_at(cfg(), 100, 300);
    step0(&mut m, special());
    run(&mut m, 11, &no_inputs());
    assert_eq!(health_of(&m, 1), 100 - 12);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(x_of(&m, 1), 348, "pushed back");
    assert!(fighter(&m, 0).attack_connected());
}

#[test]
fn stepping_back_blocks_the_shoulder_rush() {
    // Player 1 stands against the right wall: stepping back keeps it there, guard up.
    let mut m = duel_at(cfg(), 1000, 1200);
    step0(&mut m, press_turn(Facing::Right));
    step0(&mut m, Input::NONE);
    m.step(&inputs_of(&[(0, special()), (1, press_right())]));
    for _ in 0..14 {
        m.step(&only(1, press_right()));
    }
    assert_eq!(health_of(&m, 1), 100 - 3, "a quarter of the damage");
    assert_ne!(state_of(&m, 1), FighterState::HitStun);
}

#[test]
fn the_special_move_needs_the_floor() {
    let mut m = duel_at(cfg(), 100, 1100);
    step0(&mut m, press_jump());
    step0(&mut m, special());
    assert_eq!(state_of(&m, 0), FighterState::Airborne);
}

#[test]
fn a_technique_wins_over_the_special_move_which_wins_over_an_attack() {
    let mut m = duel_at(cfg(), 100, 1100);
    step0(
        &mut m,
        Input {
            special: true,
            ..jab()
        },
    );
    assert_eq!(state_of(&m, 0), special_state(AttackPhase::Startup));

    let mut m = duel_at(cfg(), 100, 1100);
    let sweep = press(Attack::punch(Strength::Medium)).with(Attack::kick(Strength::High));
    step0(
        &mut m,
        Input {
            special: true,
            ..sweep
        },
    );
    assert!(matches!(state_of(&m, 0), FighterState::Technique(..)));
}

#[test]
fn a_strike_that_connected_cancels_into_the_special_move() {
    let mut m = duel_at(cfg(), 100, 150);
    step0(&mut m, jab());
    run(&mut m, 3, &no_inputs());
    assert!(fighter(&m, 0).attack_connected());
    step0(&mut m, special());
    assert_eq!(state_of(&m, 0), special_state(AttackPhase::Startup));
}

#[test]
fn a_whiffed_strike_does_not_cancel_into_the_special_move() {
    let mut m = duel_at(cfg(), 100, 1100);
    step0(&mut m, jab());
    run(&mut m, 3, &no_inputs());
    step0(&mut m, special());
    assert!(matches!(state_of(&m, 0), FighterState::Attacking(_)));
}

#[test]
fn nothing_chains_after_the_special_move() {
    let mut m = duel_at(cfg(), 100, 300);
    step0(&mut m, special());
    run(&mut m, 11, &no_inputs());
    assert!(fighter(&m, 0).attack_connected());
    step0(
        &mut m,
        Input {
            special: true,
            ..press(Attack::kick(Strength::High))
        },
    );
    assert!(fighter(&m, 0).is_special(), "the rush goes on");
    assert_eq!(attack_of(&m, 0), None);
}

// ---------------------------------------------------------------------------------------------
// Torch's fireball
// ---------------------------------------------------------------------------------------------

/// Torch (player 0) against Chalk (player 1).
fn torch_duel(p0_x: i32, p1_x: i32) -> Match {
    duel_at(cast(Character::Torch, Character::Chalk), p0_x, p1_x)
}

#[test]
fn the_fireball_leaves_on_the_first_active_tick_and_flies() {
    let mut m = torch_duel(100, 1100);
    step0(&mut m, special());
    assert_eq!(fighter(&m, 0).special_cooldown(), 75);
    run(&mut m, 11, &no_inputs());
    assert!(m.projectiles().is_empty(), "still winding up");
    assert_eq!(x_of(&m, 0), 100, "no lunge");
    run(&mut m, 1, &no_inputs());
    let shot = m.projectiles()[0];
    assert_eq!(shot.owner(), PlayerId(0));
    assert_eq!(shot.facing(), Facing::Right);
    assert_eq!((shot.x(), shot.y(), shot.z()), (111, 0, 0));
    assert_eq!(shot.remaining(), 69);
    assert_eq!(shot.size(), 20);
    run(&mut m, 1, &no_inputs());
    assert_eq!(m.projectiles()[0].x(), 122);
}

#[test]
fn the_fireball_hits_from_across_the_arena_and_vanishes() {
    let mut m = torch_duel(100, 600);
    step0(&mut m, special());
    let mut ticks = 1;
    while health_of(&m, 1) == 100 {
        m.step(&no_inputs());
        ticks += 1;
        assert!(ticks < 100, "the fireball never arrived");
    }
    assert_eq!(health_of(&m, 1), 90);
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(x_of(&m, 1), 624);
    assert!(m.projectiles().is_empty());
    assert_eq!(
        health_of(&m, 0),
        90,
        "Torch's own bar: 90 % health, untouched"
    );
}

#[test]
fn the_body_of_the_fireball_thrower_hits_nobody() {
    let mut m = torch_duel(100, 110);
    step0(&mut m, special());
    run(&mut m, 11, &no_inputs());
    // The shot is thrown at point blank: it hits, the hand does not hit on top of it.
    run(&mut m, 1, &no_inputs());
    assert_eq!(health_of(&m, 1), 90);
    assert!(!fighter(&m, 0).attack_connected());
}

#[test]
fn stepping_back_blocks_the_fireball() {
    let mut m = torch_duel(1000, 1200);
    step0(&mut m, press_turn(Facing::Right));
    step0(&mut m, Input::NONE);
    m.step(&inputs_of(&[(0, special()), (1, press_right())]));
    for _ in 0..40 {
        m.step(&only(1, press_right()));
    }
    assert_eq!(health_of(&m, 1), 100 - 2, "a quarter of 10, rounded down");
    assert!(m.projectiles().is_empty(), "a blocked shot vanishes too");
}

#[test]
fn the_fireball_cools_down_before_the_next_one() {
    let mut m = torch_duel(100, 1100);
    step0(&mut m, special());
    run(&mut m, 40, &no_inputs());
    assert_eq!(state_of(&m, 0), FighterState::Idle);
    assert_eq!(fighter(&m, 0).special_cooldown(), 35);
    step0(&mut m, special());
    assert_eq!(state_of(&m, 0), FighterState::Idle, "not ready yet");
    run(&mut m, 33, &no_inputs());
    assert_eq!(fighter(&m, 0).special_cooldown(), 1);
    step0(&mut m, special());
    assert_eq!(
        state_of(&m, 0),
        special_state(AttackPhase::Startup),
        "75 ticks after the first one"
    );
}

#[test]
fn a_connected_strike_does_not_cancel_into_a_cooling_fireball() {
    let mut m = torch_duel(100, 150);
    step0(&mut m, special());
    run(&mut m, 11, &no_inputs());
    run(&mut m, 30, &no_inputs());
    step0(&mut m, jab());
    run(&mut m, 3, &no_inputs());
    assert!(fighter(&m, 0).attack_connected());
    step0(&mut m, special());
    assert!(!fighter(&m, 0).is_special());
}

#[test]
fn the_fireball_burns_out() {
    let mut config = cast(Character::Torch, Character::Chalk);
    config.arena_width = 3000;
    let mut m = duel_at(config, 100, 2900);
    step0(&mut m, special());
    run(&mut m, 11, &no_inputs());
    run(&mut m, 69, &no_inputs());
    assert_eq!(m.projectiles().len(), 1);
    assert_eq!(m.projectiles()[0].remaining(), 1);
    run(&mut m, 1, &no_inputs());
    assert!(m.projectiles().is_empty(), "70 ticks of flight, no more");
    assert_eq!(health_of(&m, 1), 100);
}

#[test]
fn the_fireball_leaves_the_arena_at_the_wall() {
    let mut m = torch_duel(1150, 100);
    // Player 0 faces left (right half): turn it towards the right wall.
    step0(&mut m, press_turn(Facing::Right));
    step0(&mut m, Input::NONE);
    step0(&mut m, special());
    run(&mut m, 12, &no_inputs());
    assert_eq!(m.projectiles().len(), 1);
    run(&mut m, 4, &no_inputs());
    assert!(m.projectiles().is_empty(), "past the right wall");
}

#[test]
fn a_sidestep_or_a_jump_dodges_the_fireball() {
    let sidestep = Input {
        sidestep_in: true,
        ..Input::NONE
    };
    let mut m = torch_duel(100, 400);
    step0(&mut m, special());
    for _ in 0..50 {
        m.step(&only(1, sidestep));
    }
    assert_eq!(health_of(&m, 1), 100, "out of its line");
    assert!(m.projectiles()[0].x() > 400, "it flew past");

    // A high jump: the shot takes five ticks to pass under the fighter.
    let config = MatchConfig {
        jump_velocity: 30,
        ..cast(Character::Torch, Character::Chalk)
    };
    let mut m = duel_at(config, 100, 400);
    step0(&mut m, special());
    while m.projectiles().first().is_none_or(|shot| shot.x() < 330) {
        m.step(&no_inputs());
    }
    m.step(&only(1, press_jump()));
    run(&mut m, 20, &no_inputs());
    assert_eq!(health_of(&m, 1), 100, "over it");
}

#[test]
fn the_fireball_spares_allies_and_knocked_out_fighters() {
    let mut config = cfg().with_character(PlayerId(0), Character::Torch);
    config.max_health = 10;
    let slot = |player, team| PlayerSlot {
        player: PlayerId(player),
        team: TeamId(team),
    };
    // Torch and its ally against two enemies, everybody facing right, one fireball a kill.
    let roster = Roster::new(vec![slot(0, 0), slot(1, 0), slot(2, 1), slot(3, 1)]).unwrap();
    let mut m = Match::with_spawns(roster, config, &[100, 300, 500, 700]).unwrap();
    m.step(&only(0, special()));
    run(&mut m, 60, &no_inputs());
    assert_eq!(health_of(&m, 1), 10, "the shot flew past the ally");
    assert!(
        fighter(&m, 2).is_knocked_out(),
        "the first enemy in its way"
    );
    assert_eq!(health_of(&m, 3), 10, "the shot vanished on its first hit");
    assert!(m.projectiles().is_empty());

    // The next one flies over the knocked-out fighter to the last enemy.
    run(&mut m, 20, &no_inputs());
    m.step(&only(0, special()));
    run(&mut m, 70, &no_inputs());
    assert!(fighter(&m, 3).is_knocked_out());
    assert_eq!(m.phase(), Phase::Finished(Outcome::Winner(TeamId(0))));
}

#[test]
fn the_fireball_hits_the_nearest_fighter_first() {
    let config = cfg().with_character(PlayerId(0), Character::Torch);
    let mut m = brawl_at(config, &[100, 400, 395]);
    m.step(&only(0, special()));
    run(&mut m, 40, &no_inputs());
    assert_eq!(health_of(&m, 2), 90, "nearer");
    assert_eq!(health_of(&m, 1), 100);
}

#[test]
fn fireballs_meeting_head_on_put_each_other_out() {
    let config = cast(Character::Torch, Character::Torch);
    let mut m = duel_at(config, 100, 1100);
    m.step(&inputs_of(&[(0, special()), (1, special())]));
    run(&mut m, 12, &no_inputs());
    assert_eq!(m.projectiles().len(), 2);
    let mut ticks = 0;
    while !m.projectiles().is_empty() {
        m.step(&no_inputs());
        ticks += 1;
        assert!(ticks < 60);
    }
    assert_eq!(ticks, 43, "they met in the middle");
    run(&mut m, 60, &no_inputs());
    assert_eq!(health_of(&m, 0), 90);
    assert_eq!(health_of(&m, 1), 90, "nobody was hit");
}

#[test]
fn fireballs_on_different_lines_or_flying_the_same_way_fly_on() {
    let sidestep = Input {
        sidestep_in: true,
        ..Input::NONE
    };
    let config = cast(Character::Torch, Character::Torch);
    let mut m = duel_at(config, 100, 1100);
    for _ in 0..20 {
        m.step(&only(1, sidestep));
    }
    m.step(&inputs_of(&[(0, special()), (1, special())]));
    run(&mut m, 12, &no_inputs());
    assert_eq!(m.projectiles().len(), 2);
    run(&mut m, 60, &no_inputs());
    assert_eq!(m.projectiles().len(), 2, "they crossed without meeting");
    assert_eq!((health_of(&m, 0), health_of(&m, 1)), (90, 90));

    // Both face right (left half of the arena): the shots chase each other.
    let mut m = duel_at(cast(Character::Torch, Character::Torch), 100, 300);
    m.step(&inputs_of(&[(0, special()), (1, special())]));
    run(&mut m, 12, &no_inputs());
    assert_eq!(m.projectiles().len(), 2);
    run(&mut m, 2, &no_inputs());
    assert_eq!(m.projectiles().len(), 2, "same way: no clash");
}

#[test]
fn fireballs_flying_apart_never_meet() {
    let config = cast(Character::Torch, Character::Torch);
    let mut m = duel_at(config, 500, 700);
    // Turn both around, back to back.
    m.step(&inputs_of(&[
        (0, press_turn(Facing::Left)),
        (1, press_turn(Facing::Right)),
    ]));
    m.step(&no_inputs());
    m.step(&inputs_of(&[(0, special()), (1, special())]));
    run(&mut m, 13, &no_inputs());
    assert_eq!(m.projectiles().len(), 2);
    assert_eq!(m.projectiles()[0].facing(), Facing::Left);
}

// ---------------------------------------------------------------------------------------------
// Brick's Iron Palm (armored) and Zip's Slide Tackle (low profile)
// ---------------------------------------------------------------------------------------------

#[test]
fn the_iron_palm_shrugs_off_hits_while_it_winds_up() {
    let mut m = duel_at(cast(Character::Brick, Character::Chalk), 100, 150);
    step0(&mut m, special());
    let mut jabbing = only(1, turning(jab(), Facing::Left));
    jabbing.insert(PlayerId(0), Input::NONE);
    m.step(&jabbing);
    run(&mut m, 3, &no_inputs());
    assert_eq!(health_of(&m, 0), 130 - 20, "the hit still hurts");
    assert_eq!(
        state_of(&m, 0),
        special_state(AttackPhase::Startup),
        "no stun"
    );
    assert_eq!(x_of(&m, 0), 100, "no push");
    assert_eq!(fighter(&m, 0).combo_hits(), 0);
    run(&mut m, 10, &no_inputs());
    assert_eq!(health_of(&m, 1), 100 - 20, "and the palm lands");
}

#[test]
fn the_iron_palm_is_not_armored_once_it_is_out() {
    // Player 1 stands just beyond the palm's reach (95) but inside its jab's (100).
    let mut m = duel_at(cast(Character::Brick, Character::Chalk), 100, 198);
    step0(&mut m, special());
    run(&mut m, 10, &no_inputs());
    m.step(&only(1, turning(jab(), Facing::Left)));
    run(&mut m, 2, &no_inputs());
    assert_eq!(state_of(&m, 0), special_state(AttackPhase::Startup));
    // Next tick the palm comes out (and misses), and the jab lands on it.
    run(&mut m, 1, &no_inputs());
    assert_eq!(
        state_of(&m, 0),
        FighterState::HitStun,
        "the jab stuns it now"
    );
    assert_eq!(health_of(&m, 1), 100);
}

#[test]
fn the_armor_does_not_save_from_a_knockout() {
    let mut config = cast(Character::Brick, Character::Chalk);
    config.max_health = 10;
    let mut m = duel_at(config, 100, 150);
    step0(&mut m, special());
    m.step(&only(1, turning(jab(), Facing::Left)));
    run(&mut m, 3, &no_inputs());
    assert!(fighter(&m, 0).is_knocked_out());
}

#[test]
fn a_grab_beats_the_armor() {
    let mut m = duel_at(cast(Character::Brick, Character::Chalk), 100, 140);
    step0(&mut m, special());
    m.step(&only(1, turning(press_grab(), Facing::Left)));
    run(&mut m, 3, &no_inputs());
    assert!(fighter(&m, 0).grapple().is_some());
}

#[test]
fn the_slide_tackle_passes_under_high_attacks() {
    // The slide reaches 95 units from player 1 on its 7th tick, when player 1's strike
    // pressed on the 4th tick comes out.
    let strike_on_fourth_tick = |attack| {
        let mut m = duel_at(cast(Character::Zip, Character::Chalk), 100, 300);
        step0(&mut m, special());
        run(&mut m, 2, &no_inputs());
        m.step(&only(1, turning(press(attack), Facing::Left)));
        run(&mut m, 3, &no_inputs());
        m
    };
    let m = strike_on_fourth_tick(Attack::punch(Strength::High));
    assert_eq!(health_of(&m, 0), 80, "the high punch passed over the slide");
    let m = strike_on_fourth_tick(Attack::punch(Strength::Low));
    assert!(health_of(&m, 0) < 80, "a low punch catches it");
}

#[test]
fn the_slide_is_hittable_high_while_it_recovers() {
    // The slide stops at 280, its tip at 350; player 1 waits at 360.
    let mut m = duel_at(cast(Character::Zip, Character::Chalk), 100, 360);
    step0(&mut m, special());
    run(&mut m, 12, &no_inputs());
    assert_eq!(x_of(&m, 0), 280);
    assert_eq!(state_of(&m, 0), special_state(AttackPhase::Recovery));
    assert_eq!(health_of(&m, 1), 100, "the slide fell short");
    m.step(&only(
        1,
        turning(press(Attack::punch(Strength::High)), Facing::Left),
    ));
    run(&mut m, 3, &no_inputs());
    assert!(
        health_of(&m, 0) < 80,
        "standing up again: a high punch hits"
    );
}

// ---------------------------------------------------------------------------------------------
// Time-up with characters of different health
// ---------------------------------------------------------------------------------------------

#[test]
fn at_time_up_fresh_fighters_of_different_characters_draw() {
    let mut config = cast(Character::Brick, Character::Zip);
    config.round_time_ticks = 1;
    let mut m = duel_at(config, 100, 1100);
    m.step(&no_inputs());
    assert_eq!(
        m.phase(),
        Phase::Finished(Outcome::Draw),
        "full bars, whatever their size"
    );
}
