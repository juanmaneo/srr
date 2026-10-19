//! The computer opponents (`srr::game::Bot`): the training dummy, which never moves, and the
//! brawler: deterministic, fair, and good enough to beat an idle player, escape and counter
//! grapples, turn around, guard and use every move.
//!
//! Most tests put a fighter in a precise situation by driving the match with scripted inputs,
//! then hand it to the bot. Behaviours that rely on the bot's dice are checked over many seeds or
//! many decisions.

use std::collections::HashMap;

use std::collections::HashSet;

use srr::game::{
    Attack, AttackData, Bot, Brawler, BrawlerConfig, Dummy, Facing, FighterState, GlitchSet,
    GrapplePosition, GrappleRole, Input, Limb, Match, MatchConfig, Outcome, Phase, PlayerId, Rival,
    Roster, Strength, TeamId, Technique,
};

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

const HUMAN: PlayerId = PlayerId(0);
const CPU: PlayerId = PlayerId(1);

fn config() -> MatchConfig {
    MatchConfig {
        countdown_ticks: 0,
        ..MatchConfig::default()
    }
}

/// A duel with the fighters at `human` and `cpu`.
fn duel_at(config: MatchConfig, human: i32, cpu: i32) -> Match {
    Match::with_spawns(Roster::duel(), config, &[human, cpu]).unwrap()
}

fn step(game: &mut Match, human: Input, cpu: Input) {
    game.step(&HashMap::from([(HUMAN, human), (CPU, cpu)]));
}

fn state(game: &Match, player: PlayerId) -> FighterState {
    game.fighter(player).unwrap().state()
}

fn walking(left: bool) -> Input {
    Input {
        left,
        right: !left,
        ..Input::NONE
    }
}

fn technique(technique: Technique) -> Input {
    let [first, second] = technique.buttons();
    Input::pressing(first).with(second)
}

const GRAB: Input = Input {
    grab: true,
    ..Input::NONE
};

/// Steps `game` with `human` / `cpu` (the first tick only) until the bot's fighter is in
/// `wanted`.
fn until(game: &mut Match, human: Input, cpu: Input, wanted: FighterState) {
    for tick in 0..20 {
        if state(game, CPU) == wanted {
            return;
        }
        let press = |input: Input| if tick == 0 { input } else { Input::NONE };
        step(game, press(human), press(cpu));
    }
    panic!("never reached {wanted:?}");
}

/// The human grabs the idle bot from 50 units: the clinch, with the bot underneath.
fn grabbed_bot(config: MatchConfig) -> Match {
    let mut game = duel_at(config, 600, 650);
    let clinch = FighterState::Grappling(GrappleRole::Bottom, GrapplePosition::Clinch);
    until(&mut game, GRAB, Input::NONE, clinch);
    game
}

/// The idle human is grabbed by the bot's fighter: the clinch, with the bot on top.
fn bot_on_top(config: MatchConfig) -> Match {
    let mut game = duel_at(config, 600, 650);
    let clinch = FighterState::Grappling(GrappleRole::Top, GrapplePosition::Clinch);
    until(&mut game, Input::NONE, GRAB, clinch);
    game
}

/// The human throws the idle bot (ippon-seoi-nage): the projection has just started.
fn thrown_bot() -> Match {
    let mut game = duel_at(config(), 600, 650);
    let thrown = FighterState::Grappling(GrappleRole::Bottom, GrapplePosition::Throw);
    until(&mut game, technique(Technique::Throw), Input::NONE, thrown);
    game
}

/// What the bot answers to the same situation `calls` times in a row.
fn answers(bot: &mut dyn Bot, game: &Match, calls: usize) -> Vec<Input> {
    (0..calls).map(|_| bot.think(game)).collect()
}

// ---------------------------------------------------------------------------------------------
// The rivals: the dummy and the brawler, behind the same trait
// ---------------------------------------------------------------------------------------------

#[test]
fn the_training_dummy_never_presses_anything() {
    let mut dummy = Dummy::new(CPU);
    assert_eq!(dummy.player(), CPU);
    assert_eq!(Dummy::new(HUMAN), Dummy::new(HUMAN));
    let fighting = duel_at(config(), 600, 650);
    assert!(
        answers(&mut dummy, &fighting, 50)
            .iter()
            .all(|&i| i == Input::NONE)
    );
    let mut game = fighting.clone();
    for _ in 0..30 {
        let input = dummy.think(&game);
        step(
            &mut game,
            Input::pressing(Attack::punch(Strength::Low)),
            input,
        );
    }
    assert!(
        game.fighter(CPU).unwrap().health() < config().max_health,
        "it takes the punches"
    );
    assert_eq!(
        game.fighter(HUMAN).unwrap().health(),
        config().max_health,
        "and never hits back"
    );
}

#[test]
fn the_rivals_spawn_their_bots() {
    let game = duel_at(config(), 100, 1100);
    let mut dummy = Rival::Dummy.spawn(CPU, 9);
    assert_eq!(dummy.player(), CPU);
    assert_eq!(dummy.think(&game), Input::NONE);
    // Every computer is the brawler with its own tuning.
    for (rival, tuning) in [
        (Rival::Brawler, BrawlerConfig::default()),
        (Rival::Patient, BrawlerConfig::PATIENT),
        (Rival::Aggressive, BrawlerConfig::AGGRESSIVE),
    ] {
        let mut bot = rival.spawn(CPU, 9);
        assert_eq!(bot.player(), CPU);
        assert_eq!(
            bot.think(&game),
            Brawler::with_config(CPU, 9, tuning).think(&game),
            "{rival:?}: the brawler, with that tuning"
        );
    }
    let mut brawler = Rival::Brawler.spawn(CPU, 9);
    assert!(brawler.think(&game).left, "walks in");
    assert_eq!(format!("{dummy:?}"), "Dummy { player: PlayerId(1) }");
}

#[test]
fn the_rivals_go_round_in_menu_order_and_have_names() {
    assert_eq!(
        Rival::ALL,
        [
            Rival::Brawler,
            Rival::Patient,
            Rival::Aggressive,
            Rival::Dummy
        ]
    );
    for rival in Rival::ALL {
        assert_eq!(rival.next().previous(), rival);
        assert_eq!(rival.previous().next(), rival);
        assert_ne!(rival.next(), rival, "four rivals: always another one");
        assert!(rival.name().starts_with("the "), "{}", rival.name());
        assert_eq!(rival.fights(), rival != Rival::Dummy);
    }
    assert_eq!(Rival::Brawler.name(), "the computer");
    assert_eq!(Rival::Patient.name(), "the patient computer");
    assert_eq!(Rival::Aggressive.name(), "the aggressive computer");
    assert_eq!(Rival::Dummy.name(), "the training dummy");
    assert!(!Rival::Dummy.fights(), "a punching bag");
}

#[test]
fn a_boxed_bot_is_cloned_with_its_state() {
    let game = duel_at(config(), 600, 650);
    let mut original: Box<dyn Bot> = Rival::Brawler.spawn(CPU, 11);
    for _ in 0..40 {
        original.think(&game);
    }
    let mut copy = original.clone();
    assert_eq!(
        answers(copy.as_mut(), &game, 200),
        answers(original.as_mut(), &game, 200),
        "the copy goes on from the same dice"
    );
    let mut fresh = Rival::Brawler.spawn(CPU, 11);
    assert_ne!(
        answers(fresh.as_mut(), &game, 200),
        answers(copy.as_mut(), &game, 200)
    );
    let dummy: Box<dyn Bot> = Rival::Dummy.spawn(CPU, 0);
    assert_eq!(dummy.clone().player(), CPU);
}

// ---------------------------------------------------------------------------------------------
// The brawler's tuning (`BrawlerConfig`)
// ---------------------------------------------------------------------------------------------

#[test]
fn the_default_config_is_the_standard_computer() {
    assert_eq!(BrawlerConfig::default().mash_percent, 25);
    assert_eq!(BrawlerConfig::default().guard_percent, 55);
    assert_eq!(BrawlerConfig::default().decision_pause, (8, 14));
    assert_eq!(
        Brawler::new(CPU, 9),
        Brawler::with_config(CPU, 9, BrawlerConfig::default()),
        "new is with_config with the default"
    );
}

#[test]
fn a_configured_bot_obeys_its_config() {
    let game = duel_at(config(), 100, 1100);

    // Never jumps.
    let no_jumps = BrawlerConfig {
        jump_percent: 0,
        ..BrawlerConfig::default()
    };
    let inputs = answers(&mut Brawler::with_config(CPU, 9, no_jumps), &game, 1000);
    assert!(inputs.iter().all(|i| !i.jump), "never a jump");

    // Always jumps while it walks in.
    let always_jumps = BrawlerConfig {
        jump_percent: 100,
        ..BrawlerConfig::default()
    };
    let inputs = answers(&mut Brawler::with_config(CPU, 9, always_jumps), &game, 100);
    assert!(inputs.iter().all(|i| i.jump), "a jump every tick");

    // Never guards.
    let no_guard = BrawlerConfig {
        guard_percent: 0,
        ..BrawlerConfig::default()
    };
    let mut game = duel_at(config(), 600, 660);
    step(
        &mut game,
        Input::pressing(Attack::punch(Strength::Low)),
        Input::NONE,
    );
    let mut bot = Brawler::with_config(CPU, 3, no_guard);
    assert_ne!(
        bot.think(&game),
        walking(false),
        "never steps back to block"
    );

    // Always guards, so it crouches under a high kick from the floor.
    let always_guards = BrawlerConfig {
        guard_percent: 100,
        ..BrawlerConfig::default()
    };
    let mut game = duel_at(config(), 600, 660);
    step(
        &mut game,
        Input::pressing(Attack::kick(Strength::High)),
        Input::NONE,
    );
    let mut bot = Brawler::with_config(CPU, 3, always_guards);
    assert_eq!(
        bot.think(&game),
        Input {
            down: true,
            ..Input::NONE
        },
        "crouches under the high kick"
    );
}

#[test]
fn any_decision_pause_is_accepted_even_reversed_or_extreme() {
    let game = duel_at(config(), 600, 650);
    let pause = |decision_pause| BrawlerConfig {
        decision_pause,
        ..BrawlerConfig::default()
    };
    // A reversed pause is the same range: the bot plays exactly like with the ordered one.
    assert_eq!(
        answers(
            &mut Brawler::with_config(CPU, 5, pause((14, 8))),
            &game,
            300
        ),
        answers(
            &mut Brawler::with_config(CPU, 5, pause((8, 14))),
            &game,
            300
        ),
    );
    // The widest range and an empty one neither overflow nor panic.
    for decision_pause in [(0, u32::MAX), (u32::MAX, 0), (0, 0), (u32::MAX, u32::MAX)] {
        answers(
            &mut Brawler::with_config(CPU, 5, pause(decision_pause)),
            &game,
            300,
        );
    }
}

#[test]
fn the_decision_pause_sets_the_pace_of_the_choices_in_reach() {
    let game = duel_at(config(), 600, 650);
    // The committed moves (grabs, techniques, strikes) happen on decision ticks only.
    let pace = |pause: (u32, u32)| {
        let config = BrawlerConfig {
            decision_pause: pause,
            ..BrawlerConfig::default()
        };
        let inputs = answers(&mut Brawler::with_config(CPU, 11, config), &game, 3000);
        inputs
            .iter()
            .filter(|i| i.grab || i.attack().is_some())
            .count()
    };
    let quick = pace((1, 1));
    let slow = pace((20, 20));
    assert!(
        quick > 1000 && slow < 1000,
        "a decision every other tick: {quick}, every twenty-one: {slow}"
    );
}

#[test]
fn a_configured_bot_is_still_deterministic() {
    let fight = |bot: &mut Brawler| {
        let mut game = Match::new(Roster::duel(), config()).unwrap();
        let mut inputs = Vec::new();
        while game.outcome().is_none() {
            let input = bot.think(&game);
            step(&mut game, Input::NONE, input);
            inputs.push(input);
        }
        (inputs, game)
    };
    let config = BrawlerConfig {
        mash_percent: 50,
        guard_percent: 10,
        ..BrawlerConfig::default()
    };
    let (inputs, game) = fight(&mut Brawler::with_config(CPU, 7, config));
    let (again, replay) = fight(&mut Brawler::with_config(CPU, 7, config));
    assert_eq!(inputs, again);
    assert_eq!(game, replay);
    // The jump chance governs the walk towards the enemy, so another value is another fight
    // (the guard and the mash never come up against an idle player).
    let (other_config, _) = fight(&mut Brawler::with_config(
        CPU,
        7,
        BrawlerConfig {
            jump_percent: 100,
            ..config
        },
    ));
    assert_ne!(inputs, other_config, "another tuning, another fight");
}

// ---------------------------------------------------------------------------------------------
// The rivals' difficulty tunings
// ---------------------------------------------------------------------------------------------

#[test]
fn the_patient_computer_guards_and_the_aggressive_one_rushes_in() {
    // Against a jab coming: the patient one steps back to block, the aggressive one rarely does.
    let blocks = |rival: Rival| {
        (0..40)
            .filter(|&seed| {
                let mut game = duel_at(config(), 600, 660);
                step(
                    &mut game,
                    Input::pressing(Attack::punch(Strength::Low)),
                    Input::NONE,
                );
                let mut bot = rival.spawn(CPU, seed);
                bot.think(&game) == walking(false)
            })
            .count()
    };
    let patient = blocks(Rival::Patient);
    let aggressive = blocks(Rival::Aggressive);
    assert!(
        patient > 25 && aggressive < 15,
        "patient guards {patient} times in 40, aggressive {aggressive}"
    );

    // In reach, the aggressive one commits to a move about twice as often.
    let game = duel_at(config(), 600, 650);
    let commits = |rival: Rival| {
        let mut bot = rival.spawn(CPU, 11);
        answers(bot.as_mut(), &game, 3000)
            .iter()
            .filter(|i| i.grab || i.attack().is_some())
            .count()
    };
    assert!(
        commits(Rival::Aggressive) > commits(Rival::Patient),
        "the aggressive computer decides more often"
    );
}

#[test]
fn every_computer_beats_an_idle_player() {
    for rival in [Rival::Brawler, Rival::Patient, Rival::Aggressive] {
        let mut game = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
        let mut bot = rival.spawn(CPU, 0);
        while game.outcome().is_none() {
            let input = bot.think(&game);
            step(&mut game, Input::NONE, input);
        }
        assert_eq!(
            game.outcome(),
            Some(Outcome::Winner(TeamId(1))),
            "{rival:?}: wins by knockout"
        );
        assert!(game.fighter(HUMAN).unwrap().is_knocked_out(), "{rival:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// Determinism and silence
// ---------------------------------------------------------------------------------------------

/// Two bots fight a whole round; returns every input of both, and the final match.
fn bot_versus_bot(seeds: (u64, u64)) -> (Vec<(Input, Input)>, Match) {
    let mut game = Match::new(Roster::duel(), config()).unwrap();
    let mut first = Brawler::new(HUMAN, seeds.0);
    let mut second = Brawler::new(CPU, seeds.1);
    let mut inputs = Vec::new();
    while game.outcome().is_none() {
        let pair = (first.think(&game), second.think(&game));
        step(&mut game, pair.0, pair.1);
        inputs.push(pair);
    }
    (inputs, game)
}

#[test]
fn the_same_seed_plays_the_same_match() {
    let (inputs, game) = bot_versus_bot((1, 2));
    let (again, replay) = bot_versus_bot((1, 2));
    assert_eq!(inputs, again);
    assert_eq!(game, replay);
    let (other, _) = bot_versus_bot((3, 2));
    assert_ne!(inputs, other, "another seed, another fight");
    assert!(game.outcome().is_some(), "two bots finish a round");
}

#[test]
fn a_bot_knows_its_player() {
    assert_eq!(Brawler::new(PlayerId(3), 0).player(), PlayerId(3));
    assert_eq!(Brawler::new(CPU, 7), Brawler::new(CPU, 7));
    assert_ne!(Brawler::new(CPU, 7), Brawler::new(CPU, 8));
}

#[test]
fn the_bot_presses_nothing_outside_the_fight() {
    let countdown = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
    assert!(matches!(countdown.phase(), Phase::Countdown { .. }));
    let mut bot = Brawler::new(CPU, 0);
    assert!(
        answers(&mut bot, &countdown, 50)
            .iter()
            .all(|&i| i == Input::NONE)
    );

    let mut outsider = Brawler::new(PlayerId(5), 0);
    let fighting = Match::new(Roster::duel(), config()).unwrap();
    assert_eq!(outsider.think(&fighting), Input::NONE, "not in the match");

    let (_, finished) = bot_versus_bot((4, 5));
    assert!(
        answers(&mut bot, &finished, 50)
            .iter()
            .all(|&i| i == Input::NONE)
    );
}

#[test]
fn a_knocked_out_bot_presses_nothing_while_the_others_fight_on() {
    let config = MatchConfig {
        max_health: 5,
        ..config()
    };
    let roster = Roster::free_for_all(3).unwrap();
    let mut game = Match::with_spawns(roster, config, &[100, 160, 900]).unwrap();
    let jab = Input::pressing(Attack::punch(Strength::Low));
    let mut ticks = 0;
    while !game.fighter(CPU).unwrap().is_knocked_out() {
        let first = if ticks == 0 { jab } else { Input::NONE };
        game.step(&HashMap::from([(HUMAN, first)]));
        ticks += 1;
        assert!(ticks < 30, "the jab must land");
    }
    assert_eq!(game.phase(), Phase::Fighting, "two are still standing");
    let mut bot = Brawler::new(CPU, 0);
    assert!(
        answers(&mut bot, &game, 20)
            .iter()
            .all(|&i| i == Input::NONE)
    );
}

#[test]
fn a_busy_bot_presses_nothing() {
    let mut game = duel_at(config(), 300, 600);
    step(
        &mut game,
        Input::NONE,
        Input::pressing(Attack::kick(Strength::High)),
    );
    assert!(matches!(state(&game, CPU), FighterState::Attacking(_)));
    let mut bot = Brawler::new(CPU, 0);
    assert_eq!(bot.think(&game), Input::NONE);
}

// ---------------------------------------------------------------------------------------------
// Fighting
// ---------------------------------------------------------------------------------------------

#[test]
fn the_bot_beats_an_idle_player() {
    for seed in 0..4 {
        let mut game = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
        let mut bot = Brawler::new(CPU, seed);
        while game.outcome().is_none() {
            let input = bot.think(&game);
            step(&mut game, Input::NONE, input);
        }
        assert_eq!(
            game.outcome(),
            Some(Outcome::Winner(TeamId(1))),
            "seed {seed}"
        );
        assert!(
            game.fighter(HUMAN).unwrap().is_knocked_out(),
            "seed {seed}: by knockout"
        );
    }
}

#[test]
fn the_bot_walks_in_and_jumps_now_and_then() {
    let game = duel_at(config(), 100, 1100);
    let mut bot = Brawler::new(CPU, 9);
    let inputs = answers(&mut bot, &game, 1000);
    assert!(
        inputs.iter().all(|i| i.left && !i.right),
        "towards the human"
    );
    let jumps = inputs.iter().filter(|i| i.jump).count();
    assert!((5..60).contains(&jumps), "{jumps} jumps in 1000 ticks");
}

#[test]
fn the_bot_drifts_towards_its_target_in_the_air() {
    let mut game = duel_at(config(), 100, 700);
    step(
        &mut game,
        Input::NONE,
        Input {
            jump: true,
            ..Input::NONE
        },
    );
    assert_eq!(state(&game, CPU), FighterState::Airborne);
    let mut bot = Brawler::new(CPU, 0);
    assert_eq!(bot.think(&game), walking(true));
}

#[test]
fn the_bot_kicks_when_it_flies_within_reach() {
    let mut game = duel_at(config(), 600, 700);
    step(
        &mut game,
        Input::NONE,
        Input {
            jump: true,
            ..Input::NONE
        },
    );
    assert_eq!(state(&game, CPU), FighterState::Airborne);
    let mut strengths = HashSet::new();
    for seed in 0..30 {
        let input = Brawler::new(CPU, seed).think(&game);
        let kick = input.kick().expect("a kick, in reach");
        assert_eq!(input.attack(), Some(kick), "seed {seed}: never a punch");
        assert!(
            input.left && !input.right,
            "still drifting towards the human"
        );
        strengths.insert(kick.strength);
    }
    assert_eq!(strengths.len(), 3, "every strength comes up");
}

#[test]
fn the_bot_chains_a_strike_that_connected_up_to_the_high_tier() {
    let mut limbs = HashSet::new();
    for seed in 0..8 {
        // 50 apart: even two punches of knockback leave the human in the high punch's reach.
        let mut game = duel_at(config(), 600, 650);
        let mut bot = Brawler::new(CPU, seed);
        step(
            &mut game,
            Input::NONE,
            Input::pressing(Attack::punch(Strength::Low)),
        );
        let mut pressed = Vec::new();
        let mut longest = 0;
        for _ in 0..36 {
            let input = bot.think(&game);
            if let Some(attack) = input.attack() {
                assert_eq!(input, Input::pressing(attack), "one button");
                pressed.push(attack);
                limbs.insert(attack.limb);
            }
            step(&mut game, Input::NONE, input);
            longest = longest.max(game.fighter(HUMAN).unwrap().combo_hits());
        }
        let tiers: Vec<Strength> = pressed.iter().map(|attack| attack.strength).collect();
        assert_eq!(
            tiers,
            [Strength::Medium, Strength::High],
            "seed {seed}: {pressed:?}"
        );
        assert_eq!(longest, 3, "seed {seed}: a three-hit combo");
    }
    assert_eq!(limbs, HashSet::from([Limb::Punch, Limb::Kick]));
}

#[test]
fn the_bot_steps_back_rather_than_crouching_under_a_jump_kick() {
    let mut guards = 0;
    for seed in 0..40 {
        let mut game = duel_at(config(), 600, 660);
        step(
            &mut game,
            Input {
                jump: true,
                ..Input::NONE
            },
            Input::NONE,
        );
        step(
            &mut game,
            Input::pressing(Attack::kick(Strength::High)),
            Input::NONE,
        );
        let human = game.fighter(HUMAN).unwrap();
        assert!(human.is_airborne() && human.attack().is_some());
        let first = Brawler::new(CPU, seed).think(&game);
        assert!(
            !first.down,
            "seed {seed}: a kick from above cannot be crouched under"
        );
        if first == walking(false) {
            guards += 1;
        }
    }
    assert!(guards > 5, "{guards} guards");
}

#[test]
fn the_bot_turns_around_with_a_fresh_press() {
    // Both spawn facing right: the human is behind the bot.
    let mut game = duel_at(config(), 100, 300);
    assert_eq!(game.fighter(CPU).unwrap().facing(), Facing::Right);
    let mut bot = Brawler::new(CPU, 0);
    let turn = Input {
        down: true,
        ..walking(true)
    };
    assert_eq!(bot.think(&game), turn);
    assert_eq!(
        bot.think(&game),
        Input::NONE,
        "released, to press it afresh"
    );
    assert_eq!(bot.think(&game), turn);
    step(&mut game, Input::NONE, turn);
    assert_eq!(game.fighter(CPU).unwrap().facing(), Facing::Left);
    assert!(!bot.think(&game).down, "facing the human now");
}

#[test]
fn the_bot_goes_for_the_nearest_enemy() {
    let roster = Roster::free_for_all(3).unwrap();
    let game = Match::with_spawns(roster, config(), &[400, 500, 800]).unwrap();
    assert_eq!(game.fighter(CPU).unwrap().facing(), Facing::Right);
    let mut bot = Brawler::new(CPU, 0);
    let input = bot.think(&game);
    assert!(
        input.down && input.left,
        "turns to player 1, the closest: {input:?}"
    );
}

#[test]
fn the_bot_guards_against_strikes_and_crouches_under_high_ones() {
    let mut blocks = 0;
    let mut crouches = 0;
    let mut ignored = 0;
    for seed in 0..40 {
        for (attack, high) in [
            (Attack::punch(Strength::Low), false),
            (Attack::kick(Strength::High), true),
        ] {
            let mut game = duel_at(config(), 600, 660);
            step(&mut game, Input::pressing(attack), Input::NONE);
            assert!(matches!(state(&game, HUMAN), FighterState::Attacking(_)));
            let mut bot = Brawler::new(CPU, seed);
            let first = bot.think(&game);
            let crouch = Input {
                down: true,
                ..Input::NONE
            };
            if first == walking(false) || first == crouch {
                assert_eq!(first == crouch, high, "crouch under high strikes only");
                assert_eq!(bot.think(&game), first, "decided once per strike");
                if high {
                    crouches += 1;
                } else {
                    blocks += 1;
                }
            } else {
                ignored += 1;
            }
        }
    }
    assert!(
        blocks > 5 && crouches > 5 && ignored > 5,
        "{blocks} {crouches} {ignored}"
    );
}

#[test]
fn a_guard_holds_until_the_strike_is_over() {
    for seed in 0..20 {
        let mut game = duel_at(config(), 600, 660);
        step(
            &mut game,
            Input::pressing(Attack::punch(Strength::Low)),
            Input::NONE,
        );
        let mut bot = Brawler::new(CPU, seed);
        let mut guarded = Vec::new();
        for _ in 0..30 {
            let input = bot.think(&game);
            guarded.push(input == walking(false));
            step(&mut game, Input::NONE, input);
        }
        // Either it stepped back while the jab came, or never; then it fights.
        let first = guarded[0];
        assert!(
            guarded[..3].iter().all(|&g| g == first),
            "seed {seed}: {guarded:?}"
        );
    }
}

/// A duel with the fighters at `human` and `cpu`, the human `steps` sidesteps towards the wall.
fn human_off_the_line(human: i32, cpu: i32, steps: usize) -> Match {
    let mut game = duel_at(config(), human, cpu);
    let sidestep = Input {
        sidestep_in: true,
        ..Input::NONE
    };
    for _ in 0..steps {
        step(&mut game, sidestep, Input::NONE);
    }
    game
}

#[test]
fn the_bot_sidesteps_back_onto_the_line_of_its_target() {
    let towards_the_wall = |input: Input| Input {
        sidestep_in: true,
        ..input
    };
    // 25 across, far away: it walks in and sidesteps at once.
    let far = human_off_the_line(100, 1100, 5);
    let inputs = answers(&mut Brawler::new(CPU, 9), &far, 200);
    assert!(
        inputs
            .iter()
            .all(|i| *i == towards_the_wall(walking(true)) || i.jump),
        "{inputs:?}"
    );
    // 45 across, in reach: nothing would connect, it only sidesteps.
    let close = human_off_the_line(600, 650, 9);
    let inputs = answers(&mut Brawler::new(CPU, 9), &close, 50);
    assert!(inputs.iter().all(|&i| i == towards_the_wall(Input::NONE)));
    // 20 across, half the attack depth: close enough, no sidestep.
    let near = human_off_the_line(100, 1100, 4);
    let inputs = answers(&mut Brawler::new(CPU, 9), &near, 200);
    assert!(inputs.iter().all(|i| !i.sidestep_in && !i.sidestep_out));
}

#[test]
fn the_bot_does_not_guard_against_a_strike_off_its_line() {
    for seed in 0..20 {
        let mut game = human_off_the_line(600, 660, 9);
        step(
            &mut game,
            Input::pressing(Attack::kick(Strength::High)),
            Input::NONE,
        );
        assert!(matches!(state(&game, HUMAN), FighterState::Attacking(_)));
        let input = Brawler::new(CPU, seed).think(&game);
        assert_eq!(
            input,
            Input {
                sidestep_in: true,
                ..Input::NONE
            },
            "seed {seed}: no guard, back onto the line"
        );
    }
}

#[test]
fn the_bot_goes_for_the_nearest_enemy_across_the_arena_too() {
    let roster = Roster::free_for_all(3).unwrap();
    let mut game = Match::with_spawns(roster, config(), &[400, 500, 650]).unwrap();
    // Player 0 is 100 along but 150 across: player 2, 150 along, is nearer.
    let sidestep = Input {
        sidestep_in: true,
        ..Input::NONE
    };
    for _ in 0..30 {
        game.step(&HashMap::from([(HUMAN, sidestep)]));
    }
    assert_eq!(game.fighter(HUMAN).unwrap().z(), 150);
    let input = Brawler::new(CPU, 0).think(&game);
    assert!(!input.down, "no turn towards player 0: {input:?}");
}

#[test]
fn up_close_the_bot_uses_every_move() {
    let game = duel_at(config(), 600, 650);
    let mut bot = Brawler::new(CPU, 11);
    let inputs = answers(&mut bot, &game, 3000);
    let count = |wanted: Input| inputs.iter().filter(|&&i| i == wanted).count();
    let strikes = inputs
        .iter()
        .filter(|i| i.attack().is_some() && !i.grab)
        .count()
        - count(technique(Technique::Sweep))
        - count(technique(Technique::Throw));
    for (name, seen) in [
        ("grab", count(GRAB)),
        ("sweep", count(technique(Technique::Sweep))),
        ("throw", count(technique(Technique::Throw))),
        ("strike", strikes),
    ] {
        assert!(seen > 10, "{name}: {seen}");
    }
    // Between two decisions it waits 8 to 14 ticks.
    let decisions = inputs.iter().filter(|&&i| i != Input::NONE).count();
    assert!(
        (3000 / 15..=3000 / 9 + 1).contains(&decisions),
        "{decisions} decisions"
    );
}

#[test]
fn at_kick_range_the_bot_kicks_steps_in_or_backs_off() {
    let config = config();
    let game = duel_at(config.clone(), 600, 700);
    let mut bot = Brawler::new(CPU, 12);
    let inputs = answers(&mut bot, &game, 3000);
    let mut kinds = [0; 3];
    for input in &inputs {
        if let Some(attack) = input.attack() {
            assert!(config.attack(attack).range >= 100, "{attack} cannot reach");
            assert_eq!(*input, Input::pressing(attack), "one button");
            kinds[0] += 1;
        } else if *input == walking(true) {
            kinds[1] += 1;
        } else if *input == walking(false) {
            kinds[2] += 1;
        } else {
            assert_eq!(*input, Input::NONE);
        }
    }
    assert!(
        kinds.iter().all(|&k| k > 20),
        "kicks, steps in, steps back: {kinds:?}"
    );
}

#[test]
fn without_any_strike_in_reach_the_bot_grabs() {
    let short = AttackData {
        range: 10,
        ..MatchConfig::default().punches[0]
    };
    let config = MatchConfig {
        punches: [short; 3],
        kicks: [short; 3],
        ..config()
    };
    let game = duel_at(config, 600, 650);
    let mut bot = Brawler::new(CPU, 13);
    let inputs = answers(&mut bot, &game, 2000);
    assert!(
        inputs
            .iter()
            .all(|i| i.attack().is_none() || i.punch_medium || i.kick_medium)
    );
    assert!(inputs.iter().filter(|&&i| i == GRAB).count() > 40);
}

// ---------------------------------------------------------------------------------------------
// Grappling
// ---------------------------------------------------------------------------------------------

#[test]
fn underneath_the_bot_mashes_its_way_out() {
    let config = MatchConfig {
        glitches: GlitchSet::none(),
        ..config()
    };
    for seed in 0..5 {
        let mut game = grabbed_bot(config.clone());
        let mut bot = Brawler::new(CPU, seed);
        let mut presses = Vec::new();
        let mut ticks = 0;
        while game.fighter(CPU).unwrap().grapple().is_some() {
            let input = bot.think(&game);
            presses.push(input);
            step(&mut game, Input::NONE, input);
            ticks += 1;
            assert!(ticks < 120, "seed {seed}: still stuck");
        }
        assert_eq!(
            state(&game, HUMAN),
            FighterState::HitStun,
            "escaped: the top is stunned"
        );
        // Fresh presses only: never the same button two ticks in a row.
        assert!(
            presses
                .windows(2)
                .all(|w| w[0] == Input::NONE || w[1] == Input::NONE)
        );
        assert!(presses.iter().any(|i| i.punch_low) && presses.iter().any(|i| i.kick_low));
    }
}

#[test]
fn a_fresh_clinch_tempts_the_bot_into_the_reversal_glitch() {
    let mut reversals = 0;
    for seed in 0..30 {
        let mut game = grabbed_bot(config());
        let mut bot = Brawler::new(CPU, seed);
        let input = bot.think(&game);
        step(&mut game, Input::NONE, input);
        if state(&game, CPU) == FighterState::Grappling(GrappleRole::Top, GrapplePosition::Clinch) {
            assert_eq!(input, GRAB);
            reversals += 1;
        }
    }
    assert!((3..25).contains(&reversals), "{reversals} reversals in 30");
}

#[test]
fn on_top_the_bot_strikes_then_advances_to_the_submission() {
    let config = config();
    let advance = config.grapple.advance_ticks;
    let mut struck = false;
    for seed in 0..5 {
        let mut game = bot_on_top(config.clone());
        let mut bot = Brawler::new(CPU, seed);
        let health = game.fighter(HUMAN).unwrap().health();
        for _ in 0..advance - 1 {
            let input = bot.think(&game);
            assert!(!input.grab, "too early to advance");
            step(&mut game, Input::NONE, input);
        }
        struck |= game.fighter(HUMAN).unwrap().health() < health;
        let mut ticks = 0;
        while state(&game, CPU)
            != FighterState::Grappling(GrappleRole::Top, GrapplePosition::Submission)
        {
            let input = bot.think(&game);
            step(&mut game, Input::NONE, input);
            ticks += 1;
            assert!(
                ticks <= advance + 5,
                "seed {seed}: the bot must advance at once"
            );
        }
        // Nothing more to climb: it keeps the hold (and taps the bottom out eventually).
        for _ in 0..200 {
            let input = bot.think(&game);
            assert!(!input.grab);
            step(&mut game, Input::NONE, input);
        }
    }
    assert!(struck, "the top strikes while it waits");
}

#[test]
fn thrown_the_bot_sometimes_counters_with_the_same_technique() {
    let mut countered = 0;
    for seed in 0..30 {
        let mut game = thrown_bot();
        let mut bot = Brawler::new(CPU, seed);
        let window = config().technique(Technique::Throw).counter_window;
        for _ in 0..window + 10 {
            let input = bot.think(&game);
            assert!(
                input == Input::NONE || input == technique(Technique::Throw),
                "{input:?}"
            );
            step(&mut game, Input::NONE, input);
            if state(&game, CPU)
                == FighterState::Grappling(GrappleRole::Top, GrapplePosition::Throw)
            {
                countered += 1;
                break;
            }
        }
    }
    assert!((5..28).contains(&countered), "{countered} counters in 30");
}
