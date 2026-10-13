//! Determinism and robustness of the simulation.
//!
//! README: online play for up to 10 players is a goal. That needs a simulation that is a pure
//! function of (state, inputs): same inputs => same state, a cloned snapshot continues
//! identically (rollback), and no input sequence may break the game's invariants or panic.
//!
//! Random inputs come from a small seeded generator defined here, so the tests need no
//! dependencies and are fully reproducible.

mod common;

use common::*;
use srr::game::{
    Attack, FighterState, GlitchSet, Input, Match, MatchConfig, Outcome, Phase, PlayerId, Roster,
    Strength,
};

/// Tiny linear congruential generator (Knuth's MMIX constants).
struct Lcg(u64);

impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    /// Random movement buttons; at most one attack button, held a quarter of the time, so that
    /// fighters also walk, step back and jump instead of attacking nearly every tick.
    fn input(&mut self) -> Input {
        let bits = self.next_u64();
        // Truncation is intended: only the low movement bits are kept.
        let movement = Input::from_bits((bits & 0b111) as u16);
        if (bits >> 3) & 0b11 == 0 {
            let attack = Attack::ALL[((bits >> 5) % 6) as usize];
            movement.with(attack)
        } else {
            movement
        }
    }
}

fn scripted_inputs(players: usize, steps: usize, seed: u64) -> Vec<Inputs> {
    let mut rng = Lcg(seed);
    let mut script = Vec::with_capacity(steps);
    for _ in 0..steps {
        let mut frame = Inputs::new();
        for id in 0..players {
            frame.insert(PlayerId(id as u8), rng.input());
        }
        script.push(frame);
    }
    script
}

/// The six attacks keep the standard (all different) frame data, so random brawls mix them.
fn brawl_config(friendly_fire: bool) -> MatchConfig {
    let standard = MatchConfig::default();
    MatchConfig {
        punches: standard.punches,
        kicks: standard.kicks,
        max_health: 60,
        round_time_ticks: 1500,
        countdown_ticks: 5,
        friendly_fire,
        glitches: GlitchSet::all(),
        ..cfg()
    }
}

fn rosters() -> Vec<Roster> {
    vec![
        Roster::duel(),
        Roster::two_versus_one(),
        Roster::free_for_all(5).unwrap(),
        Roster::from_team_sizes(&[3, 3, 2, 2]).unwrap(),
        Roster::free_for_all(10).unwrap(),
    ]
}

// ---------------------------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------------------------

#[test]
fn identical_inputs_produce_identical_states_at_every_step() {
    let script = scripted_inputs(3, 800, 42);
    let roster = Roster::free_for_all(3).unwrap();
    let mut a = Match::new(roster.clone(), brawl_config(false)).unwrap();
    let mut b = Match::new(roster, brawl_config(false)).unwrap();
    for (step, frame) in script.iter().enumerate() {
        a.step(frame);
        b.step(frame);
        assert_eq!(a, b, "diverged at step {step}");
    }
}

#[test]
fn a_cloned_snapshot_continues_exactly_like_the_original() {
    let script = scripted_inputs(4, 600, 7);
    let roster = Roster::from_team_sizes(&[2, 1, 1]).unwrap();
    let mut original = Match::new(roster, brawl_config(true)).unwrap();
    for frame in &script[..200] {
        original.step(frame);
    }
    let mut snapshot = original.clone();
    for (step, frame) in script[200..].iter().enumerate() {
        original.step(frame);
        snapshot.step(frame);
        assert_eq!(
            original, snapshot,
            "diverged {step} steps after the snapshot"
        );
    }
}

#[test]
fn inputs_for_players_that_are_not_in_the_match_never_change_the_result() {
    let script = scripted_inputs(5, 300, 99);
    let mut clean = Match::new(Roster::duel(), brawl_config(false)).unwrap();
    let mut noisy = Match::new(Roster::duel(), brawl_config(false)).unwrap();
    for frame in &script {
        let relevant: Inputs = frame
            .iter()
            .filter(|(id, _)| id.0 < 2)
            .map(|(id, input)| (*id, *input))
            .collect();
        clean.step(&relevant);
        noisy.step(frame);
    }
    assert_eq!(clean, noisy);
}

// ---------------------------------------------------------------------------------------------
// Invariants
// ---------------------------------------------------------------------------------------------

fn assert_invariants(m: &Match) {
    let max_health = m.config().max_health;
    let width = m.config().arena_width;

    for f in m.fighters() {
        assert!(
            (0..=max_health).contains(&f.health()),
            "{:?} has health {}",
            f.player(),
            f.health()
        );
        assert!(
            (0..=width).contains(&f.x()),
            "{:?} is at x = {}",
            f.player(),
            f.x()
        );
        assert!(f.y() >= 0, "{:?} is below the floor", f.player());
        assert_eq!(f.is_knocked_out(), f.health() == 0);
        assert_eq!(f.state() == FighterState::Knockout, f.health() == 0);
    }

    assert!(m.remaining_ticks() <= m.config().round_time_ticks);

    if let Phase::Finished(Outcome::Winner(team)) = m.phase() {
        assert!(
            m.fighters()
                .iter()
                .any(|f| !f.is_knocked_out() && m.roster().team_of(f.player()) == Some(team)),
            "winning team {team:?} has nobody left standing"
        );
    }
}

#[test]
fn random_brawls_never_break_the_invariants_and_always_end() {
    for (index, roster) in rosters().into_iter().enumerate() {
        for friendly_fire in [false, true] {
            let players = roster.player_count();
            let script = scripted_inputs(players, 1600, 1000 + index as u64);
            let mut m = Match::new(roster.clone(), brawl_config(friendly_fire)).unwrap();

            let mut previous_health: Vec<i32> = m.fighters().iter().map(|f| f.health()).collect();
            let mut previous_phase = m.phase();

            for frame in &script {
                m.step(frame);
                assert_invariants(&m);

                // Health never recovers and knocked-out fighters stay knocked out.
                for (before, f) in previous_health.iter().zip(m.fighters()) {
                    assert!(f.health() <= *before, "{:?} healed", f.player());
                }
                previous_health = m.fighters().iter().map(|f| f.health()).collect();

                // A finished round stays finished with the same outcome.
                if let Phase::Finished(outcome) = previous_phase {
                    assert_eq!(m.phase(), Phase::Finished(outcome));
                }
                previous_phase = m.phase();
            }

            assert!(
                matches!(m.phase(), Phase::Finished(_)),
                "{players}-player brawl (friendly fire: {friendly_fire}) did not finish"
            );
        }
    }
}

#[test]
fn glitch_exploiting_input_never_leaves_the_arena() {
    // Everybody mashes jump + right + the slowest attack: the worst case for the glitch code
    // paths.
    let mash = Input {
        right: true,
        jump: true,
        ..Input::pressing(Attack::kick(Strength::High))
    };
    let mut m = Match::new(Roster::free_for_all(4).unwrap(), brawl_config(true)).unwrap();
    let frame: Inputs = (0..4).map(|id| (PlayerId(id), mash)).collect();
    for _ in 0..1200 {
        m.step(&frame);
        assert_invariants(&m);
    }
}
