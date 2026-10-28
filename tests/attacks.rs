//! The six attacks: each one follows its own frame data, and the input decides which one is
//! thrown.
//!
//! An attack is a limb (punch or kick) thrown at a strength (low, medium or high). The frame
//! data of every attack lives in `MatchConfig::punches` / `MatchConfig::kicks`, indexed by
//! `Strength::index`. When several attack buttons are held, the strongest attack wins, and a
//! kick wins over a punch of the same strength.

mod common;

use common::*;
use srr::game::{
    Attack, AttackData, AttackPhase, FighterState, GlitchSet, Input, Limb, MatchConfig, Strength,
};

const LOW_PUNCH: Attack = Attack::punch(Strength::Low);
const HIGH_KICK: Attack = Attack::kick(Strength::High);

/// The frame data of `attack` in `config`, mutably, looked up independently of
/// `MatchConfig::attack`.
fn data_mut(config: &mut MatchConfig, attack: Attack) -> &mut AttackData {
    let table = match attack.limb {
        Limb::Punch => &mut config.punches,
        Limb::Kick => &mut config.kicks,
    };
    &mut table[attack.strength.index()]
}

/// `cfg()` where the attack at position `i` of `Attack::ALL` gets `change(i, data)`.
fn per_attack(change: impl Fn(usize, &mut AttackData)) -> MatchConfig {
    let mut config = cfg();
    for (index, attack) in Attack::ALL.into_iter().enumerate() {
        change(index, data_mut(&mut config, attack));
    }
    config
}

/// Player 0 at `attacker_x` throws `attack` once against player 1 at `target_x`; returns the
/// match after the attack is over.
fn after_one(
    config: MatchConfig,
    attack: Attack,
    attacker_x: i32,
    target_x: i32,
) -> srr::game::Match {
    let total = config.attack(attack).total() as usize;
    let mut m = duel_at(config, attacker_x, target_x);
    m.step(&only(0, press(attack)));
    run(&mut m, total, &no_inputs());
    m
}

// ---------------------------------------------------------------------------------------------
// Frame data
// ---------------------------------------------------------------------------------------------

#[test]
fn each_attack_deals_its_own_damage() {
    let config = per_attack(|index, d| {
        d.damage = 2 + 3 * index as i32;
        d.knockback = 0;
    });
    for (index, attack) in Attack::ALL.into_iter().enumerate() {
        let m = after_one(config.clone(), attack, 560, 640);
        assert_eq!(
            health_of(&m, 1),
            100 - (2 + 3 * index as i32),
            "{attack} should deal its own damage"
        );
    }
}

#[test]
fn a_high_kick_reaches_further_than_a_low_punch_when_configured_so() {
    let mut config = cfg();
    *data_mut(&mut config, LOW_PUNCH) = AttackData {
        range: 50,
        damage: 5,
        ..DATA
    };
    *data_mut(&mut config, HIGH_KICK) = AttackData {
        range: 150,
        damage: 30,
        ..DATA
    };

    // 150 apart: only the high kick connects.
    assert_eq!(
        health_of(&after_one(config.clone(), HIGH_KICK, 450, 600), 1),
        70
    );
    assert_eq!(
        health_of(&after_one(config.clone(), LOW_PUNCH, 450, 600), 1),
        100
    );
    // 151 apart: nothing connects.
    assert_eq!(
        health_of(&after_one(config.clone(), HIGH_KICK, 449, 600), 1),
        100
    );
    // 50 apart: both connect, each with its own damage.
    assert_eq!(
        health_of(&after_one(config.clone(), HIGH_KICK, 550, 600), 1),
        70
    );
    assert_eq!(
        health_of(&after_one(config.clone(), LOW_PUNCH, 550, 600), 1),
        95
    );
    // 51 apart: the low punch misses.
    assert_eq!(health_of(&after_one(config, LOW_PUNCH, 549, 600), 1), 100);
}

#[test]
fn each_attack_follows_its_own_timeline() {
    let config = per_attack(|index, d| {
        let n = index as u32;
        d.startup = n;
        d.active = 1 + n % 2;
        d.recovery = 6 - n;
    });
    for attack in Attack::ALL {
        let data = *config.attack(attack);
        let mut m = duel_at(config.clone(), 100, 1100);
        m.step(&only(0, press(attack)));
        let mut states = vec![state_of(&m, 0)];
        for _ in 1..=data.total() {
            m.step(&no_inputs());
            states.push(state_of(&m, 0));
        }

        let phase = |count: u32, phase: AttackPhase| {
            std::iter::repeat_n(FighterState::Attacking(phase), count as usize)
        };
        let expected: Vec<FighterState> = phase(data.startup, AttackPhase::Startup)
            .chain(phase(data.active, AttackPhase::Active))
            .chain(phase(data.recovery, AttackPhase::Recovery))
            .chain([FighterState::Idle])
            .collect();
        assert_eq!(states, expected, "timeline of the {attack}");
    }
}

#[test]
fn each_attack_hits_on_its_own_first_active_step() {
    let config = per_attack(|index, d| d.startup = index as u32);
    for attack in Attack::ALL {
        let startup = config.attack(attack).startup as usize;
        let mut m = close_duel(config.clone());
        m.step(&only(0, press(attack)));
        run(&mut m, startup, &no_inputs());
        assert_eq!(health_of(&m, 1), 80, "{attack} after its start-up");

        let mut early = close_duel(config.clone());
        early.step(&only(0, press(attack)));
        run(&mut early, startup.saturating_sub(1), &no_inputs());
        if startup > 0 {
            assert_eq!(health_of(&early, 1), 100, "{attack} during its start-up");
        }
    }
}

#[test]
fn each_attack_has_its_own_knockback_and_hit_stun() {
    let config = per_attack(|index, d| {
        d.knockback = 10 * index as i32;
        d.hitstun = 1 + index as u32;
    });
    for (index, attack) in Attack::ALL.into_iter().enumerate() {
        let data = *config.attack(attack);
        let mut m = close_duel(config.clone());
        m.step(&only(0, press(attack)));
        run(&mut m, data.startup as usize, &no_inputs());
        assert_eq!(
            x_of(&m, 1),
            640 + 10 * index as i32,
            "knockback of the {attack}"
        );
        run(&mut m, data.hitstun as usize - 1, &no_inputs());
        assert_eq!(
            state_of(&m, 1),
            FighterState::HitStun,
            "{attack}: still stunned"
        );
        m.step(&no_inputs());
        assert_eq!(state_of(&m, 1), FighterState::Idle, "{attack}: stun over");
    }
}

#[test]
fn the_default_attacks_hit_exactly_within_their_range_for_their_damage() {
    let config = MatchConfig {
        countdown_ticks: 0,
        glitches: GlitchSet::none(),
        ..MatchConfig::default()
    };
    for attack in Attack::ALL {
        let data = *config.attack(attack);
        let hit = after_one(config.clone(), attack, 300, 300 + data.range);
        assert_eq!(
            health_of(&hit, 1),
            config.max_health - data.damage,
            "{attack} at its range"
        );
        let miss = after_one(config.clone(), attack, 300, 301 + data.range);
        assert_eq!(
            health_of(&miss, 1),
            config.max_health,
            "{attack} beyond its range"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Choosing the attack
// ---------------------------------------------------------------------------------------------

#[test]
fn each_button_throws_its_own_attack() {
    for attack in Attack::ALL {
        let mut m = duel_at(cfg(), 100, 1100);
        m.step(&only(0, press(attack)));
        assert_eq!(attack_of(&m, 0), Some(attack));
    }
}

#[test]
fn the_strongest_held_attack_is_thrown() {
    let mut m = duel_at(cfg(), 100, 1100);
    let mash = Input::pressing(Attack::punch(Strength::High)).with(Attack::kick(Strength::Low));
    m.step(&only(0, mash));
    assert_eq!(attack_of(&m, 0), Some(Attack::punch(Strength::High)));
}

#[test]
fn a_kick_wins_over_a_punch_of_the_same_strength() {
    let mut m = duel_at(cfg(), 100, 1100);
    let both =
        Input::pressing(Attack::punch(Strength::Medium)).with(Attack::kick(Strength::Medium));
    m.step(&only(0, both));
    assert_eq!(attack_of(&m, 0), Some(Attack::kick(Strength::Medium)));
}

#[test]
fn pressing_another_attack_during_an_attack_does_not_change_it() {
    let mut m = duel_at(cfg(), 100, 1100);
    m.step(&only(0, jab()));
    run(&mut m, 8, &only(0, press(HIGH_KICK)));
    assert_eq!(attack_of(&m, 0), Some(JAB));
    m.step(&only(0, press(HIGH_KICK)));
    assert_eq!(attack_of(&m, 0), Some(HIGH_KICK), "chained once free again");
}
