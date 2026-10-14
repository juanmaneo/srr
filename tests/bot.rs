//! The computer opponent (`srr::game::Bot`): deterministic, fair, and good enough to beat an idle
//! player, escape and counter grapples, turn around, guard and use every move.
//!
//! Most tests put a fighter in a precise situation by driving the match with scripted inputs,
//! then hand it to the bot. Behaviours that rely on the bot's dice are checked over many seeds or
//! many decisions.

use std::collections::HashMap;

use srr::game::{
    Attack, AttackData, Bot, Facing, FighterState, GlitchSet, GrapplePosition, GrappleRole, Input,
    Match, MatchConfig, Outcome, Phase, PlayerId, Roster, Strength, TeamId, Technique,
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
fn answers(bot: &mut Bot, game: &Match, calls: usize) -> Vec<Input> {
    (0..calls).map(|_| bot.think(game)).collect()
}

// ---------------------------------------------------------------------------------------------
// Determinism and silence
// ---------------------------------------------------------------------------------------------

/// Two bots fight a whole round; returns every input of both, and the final match.
fn bot_versus_bot(seeds: (u64, u64)) -> (Vec<(Input, Input)>, Match) {
    let mut game = Match::new(Roster::duel(), config()).unwrap();
    let mut first = Bot::new(HUMAN, seeds.0);
    let mut second = Bot::new(CPU, seeds.1);
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
    assert_eq!(Bot::new(PlayerId(3), 0).player(), PlayerId(3));
    assert_eq!(Bot::new(CPU, 7), Bot::new(CPU, 7));
    assert_ne!(Bot::new(CPU, 7), Bot::new(CPU, 8));
}

#[test]
fn the_bot_presses_nothing_outside_the_fight() {
    let countdown = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
    assert!(matches!(countdown.phase(), Phase::Countdown { .. }));
    let mut bot = Bot::new(CPU, 0);
    assert!(
        answers(&mut bot, &countdown, 50)
            .iter()
            .all(|&i| i == Input::NONE)
    );

    let mut outsider = Bot::new(PlayerId(5), 0);
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
    let mut bot = Bot::new(CPU, 0);
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
    let mut bot = Bot::new(CPU, 0);
    assert_eq!(bot.think(&game), Input::NONE);
}

// ---------------------------------------------------------------------------------------------
// Fighting
// ---------------------------------------------------------------------------------------------

#[test]
fn the_bot_beats_an_idle_player() {
    for seed in 0..4 {
        let mut game = Match::new(Roster::duel(), MatchConfig::default()).unwrap();
        let mut bot = Bot::new(CPU, seed);
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
    let mut bot = Bot::new(CPU, 9);
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
    let mut bot = Bot::new(CPU, 0);
    assert_eq!(bot.think(&game), walking(true));
}

#[test]
fn the_bot_turns_around_with_a_fresh_press() {
    // Both spawn facing right: the human is behind the bot.
    let mut game = duel_at(config(), 100, 300);
    assert_eq!(game.fighter(CPU).unwrap().facing(), Facing::Right);
    let mut bot = Bot::new(CPU, 0);
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
    let mut bot = Bot::new(CPU, 0);
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
            let mut bot = Bot::new(CPU, seed);
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
        let mut bot = Bot::new(CPU, seed);
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
    let inputs = answers(&mut Bot::new(CPU, 9), &far, 200);
    assert!(
        inputs
            .iter()
            .all(|i| *i == towards_the_wall(walking(true)) || i.jump),
        "{inputs:?}"
    );
    // 45 across, in reach: nothing would connect, it only sidesteps.
    let close = human_off_the_line(600, 650, 9);
    let inputs = answers(&mut Bot::new(CPU, 9), &close, 50);
    assert!(inputs.iter().all(|&i| i == towards_the_wall(Input::NONE)));
    // 20 across, half the attack depth: close enough, no sidestep.
    let near = human_off_the_line(100, 1100, 4);
    let inputs = answers(&mut Bot::new(CPU, 9), &near, 200);
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
        let input = Bot::new(CPU, seed).think(&game);
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
    let input = Bot::new(CPU, 0).think(&game);
    assert!(!input.down, "no turn towards player 0: {input:?}");
}

#[test]
fn up_close_the_bot_uses_every_move() {
    let game = duel_at(config(), 600, 650);
    let mut bot = Bot::new(CPU, 11);
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
    let mut bot = Bot::new(CPU, 12);
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
    let mut bot = Bot::new(CPU, 13);
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
        let mut bot = Bot::new(CPU, seed);
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
        let mut bot = Bot::new(CPU, seed);
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
        let mut bot = Bot::new(CPU, seed);
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
        let mut bot = Bot::new(CPU, seed);
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
