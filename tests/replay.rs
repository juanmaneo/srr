//! Replays (`srr::game::Replay`): recording the inputs of a match tick by tick and playing
//! them again into a fresh match, which must end in exactly the same state (the simulation is
//! deterministic).

mod common;

use std::collections::HashMap;

use common::*;
use srr::game::{
    Attack, ConfigError, Input, Match, MatchConfig, PlayerId, Replay, ReplayError, Roster,
    SetupError, Strength,
};

/// Tiny linear congruential generator (Knuth's MMIX constants), so the inputs need no
/// dependencies and are fully reproducible.
struct Lcg(u64);

impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn input(&mut self) -> Input {
        Input::from_bits(self.next_u64() as u16)
    }
}

/// Records `ticks` ticks of a free-for-all of `players` fighters, stepping with random inputs.
fn recorded(players: usize, ticks: usize, seed: u64) -> (Replay, Match) {
    let roster = Roster::free_for_all(players).unwrap();
    let mut replay = Replay::new(roster.clone(), cfg());
    let mut game = Match::new(roster, cfg()).unwrap();
    let mut rng = Lcg(seed);
    for _ in 0..ticks {
        let inputs: Inputs = (0..players as u8)
            .map(|id| (PlayerId(id), rng.input()))
            .collect();
        game.step(&inputs);
        replay.record(&inputs);
    }
    (replay, game)
}

#[test]
fn an_empty_replay_has_no_ticks() {
    let replay = Replay::new(Roster::duel(), cfg());
    assert_eq!(replay.len(), 0);
    assert!(replay.is_empty());
    assert_eq!(replay.frame(0), None, "nothing recorded");
}

#[test]
fn the_recorded_frames_are_the_inputs_of_every_player() {
    let mut replay = Replay::new(Roster::duel(), cfg());
    let walk = Input {
        right: true,
        ..Input::NONE
    };
    // Player 1 walks; player 0 is absent, so presses nothing.
    replay.record(&HashMap::from([(PlayerId(1), walk)]));
    // An entry for a player outside the roster is dropped.
    replay.record(&HashMap::from([(PlayerId(7), walk)]));
    assert_eq!(replay.len(), 2);
    assert!(!replay.is_empty());
    assert_eq!(replay.frame(0).unwrap()[&PlayerId(1)], walk);
    assert_eq!(replay.frame(0).unwrap()[&PlayerId(0)], Input::NONE);
    assert_eq!(replay.frame(1).unwrap()[&PlayerId(1)], Input::NONE);
    assert_eq!(replay.frame(2), None, "past the recording");
}

#[test]
fn playing_a_replay_rebuilds_the_match_exactly() {
    for (players, seed) in [(2, 1), (3, 2), (10, 3)] {
        let (replay, game) = recorded(players, 500, seed);
        assert_eq!(
            replay.play().unwrap(),
            game,
            "{players} players, seed {seed}"
        );
        // A clone plays the same way.
        assert_eq!(replay.clone().play().unwrap(), game);
    }
}

#[test]
fn stepping_a_replay_tick_by_tick_rebuilds_the_match_exactly() {
    let (replay, game) = recorded(4, 300, 4);
    let mut again = Match::new(Roster::free_for_all(4).unwrap(), cfg()).unwrap();
    for tick in 0..replay.len() {
        // A replay plays through `Match::step`, one recorded frame per tick.
        again.step(replay.frame(tick as u32).unwrap());
        assert_eq!(
            again.fighter(PlayerId(0)).unwrap().x(),
            game.fighter(PlayerId(0)).unwrap().x(),
            "same place at tick {tick}"
        );
    }
    assert_eq!(replay.frame(300), None, "the recording is over");
    assert_eq!(again, game);
}

#[test]
fn a_replay_can_start_where_the_match_started_with_explicit_spawns() {
    let mut replay = Replay::with_spawns(Roster::duel(), cfg(), vec![100, 1100]);
    let mut game = Match::with_spawns(Roster::duel(), cfg(), &[100, 1100]).unwrap();
    let kick = Input::pressing(Attack::kick(Strength::Medium));
    for _ in 0..50 {
        let inputs = HashMap::from([(PlayerId(0), kick)]);
        game.step(&inputs);
        replay.record(&inputs);
    }
    assert_eq!(replay.play().unwrap(), game);
}

#[test]
fn an_invalid_setup_is_reported_when_playing() {
    let broken = MatchConfig {
        gravity: 0,
        ..cfg()
    };
    let replay = Replay::new(Roster::duel(), broken);
    assert_eq!(
        replay.play().unwrap_err(),
        ReplayError::Setup(SetupError::InvalidConfig(ConfigError::NonPositiveGravity))
    );
    let mut wrong = Replay::with_spawns(Roster::duel(), cfg(), vec![10]);
    wrong.record(&Inputs::new());
    assert!(matches!(
        wrong.play().unwrap_err(),
        ReplayError::Setup(SetupError::SpawnCountMismatch {
            expected: 2,
            actual: 1
        })
    ));
}

#[test]
fn replay_errors_explain_themselves() {
    let error: Box<dyn std::error::Error> =
        Box::new(ReplayError::Setup(SetupError::SpawnOutOfBounds {
            player: PlayerId(1),
            x: 2000,
        }));
    assert_eq!(
        error.to_string(),
        "invalid replay setup: player 1 spawns outside the arena at x = 2000"
    );
}
