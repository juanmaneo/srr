//! Match configuration and the glitch registry.
//!
//! README: the game is built around glitches that exist *on purpose* ("BERP" style), so they are
//! first-class, individually switchable parts of the configuration.

mod common;

use common::{DATA, GRAPPLE, SWEEP, THROW, cfg};
use srr::game::{
    Attack, AttackData, ConfigError, Glitch, GlitchSet, GrappleData, GrapplePosition, Limb,
    MatchConfig, Strength, TICKS_PER_SECOND, Technique, TechniqueData,
};

/// Applies `change` to the test config and validates the result.
fn validate_changed(change: impl FnOnce(&mut MatchConfig)) -> Result<(), ConfigError> {
    let mut config = cfg();
    change(&mut config);
    config.validate()
}

// ---------------------------------------------------------------------------------------------
// MatchConfig
// ---------------------------------------------------------------------------------------------

#[test]
fn the_default_config_is_valid() {
    assert_eq!(MatchConfig::default().validate(), Ok(()));
}

#[test]
fn the_test_config_is_valid() {
    assert_eq!(cfg().validate(), Ok(()));
}

#[test]
fn attack_looks_up_the_frame_data_of_each_attack() {
    let config = MatchConfig::default();
    for strength in Strength::ALL {
        assert_eq!(
            config.attack(Attack::punch(strength)),
            &config.punches[strength.index()]
        );
        assert_eq!(
            config.attack(Attack::kick(strength)),
            &config.kicks[strength.index()]
        );
    }
}

#[test]
fn the_total_length_of_an_attack_adds_its_three_phases() {
    assert_eq!(DATA.total(), 9);
    let huge = AttackData {
        startup: u32::MAX,
        ..DATA
    };
    assert_eq!(huge.total(), u32::MAX, "saturates instead of overflowing");
}

#[test]
fn by_default_stronger_attacks_hit_harder_but_start_slower() {
    let config = MatchConfig::default();
    for table in [&config.punches, &config.kicks] {
        for pair in table.windows(2) {
            let (weaker, stronger) = (&pair[0], &pair[1]);
            assert!(stronger.damage > weaker.damage, "{table:?}");
            assert!(stronger.startup > weaker.startup, "{table:?}");
            assert!(stronger.total() > weaker.total(), "{table:?}");
        }
    }
}

#[test]
fn by_default_kicks_reach_further_than_punches() {
    let config = MatchConfig::default();
    let longest_punch = config.punches.iter().map(|d| d.range).max().unwrap();
    let shortest_kick = config.kicks.iter().map(|d| d.range).min().unwrap();
    assert!(shortest_kick > longest_punch);
    for strength in Strength::ALL {
        let punch = config.attack(Attack::punch(strength));
        let kick = config.attack(Attack::kick(strength));
        assert!(kick.range > punch.range, "{strength:?}");
        assert!(kick.damage > punch.damage, "{strength:?}");
        assert!(kick.startup > punch.startup, "{strength:?}");
    }
}

#[test]
fn by_default_every_attack_can_hit() {
    let config = MatchConfig::default();
    for attack in Attack::ALL {
        let data = config.attack(attack);
        assert!(data.active > 0, "{attack}");
        assert!(data.range > 0, "{attack}");
        assert!(data.damage > 0, "{attack}");
    }
}

#[test]
fn by_default_allies_cannot_hurt_each_other() {
    assert!(!MatchConfig::default().friendly_fire);
}

#[test]
fn by_default_every_glitch_is_enabled() {
    let glitches = MatchConfig::default().glitches;
    for glitch in Glitch::ALL {
        assert!(glitches.contains(glitch), "{glitch:?} should be on");
    }
}

#[test]
fn configs_can_be_cloned_and_compared() {
    assert_eq!(cfg().clone(), cfg());
    assert_ne!(
        cfg(),
        MatchConfig {
            max_health: 1,
            ..cfg()
        }
    );
}

#[test]
fn arena_width_must_be_positive() {
    assert_eq!(
        validate_changed(|c| c.arena_width = 0),
        Err(ConfigError::NonPositiveArenaWidth)
    );
    assert_eq!(
        validate_changed(|c| c.arena_width = -10),
        Err(ConfigError::NonPositiveArenaWidth)
    );
    assert_eq!(validate_changed(|c| c.arena_width = 1), Ok(()));
}

#[test]
fn max_health_must_be_positive() {
    assert_eq!(
        validate_changed(|c| c.max_health = 0),
        Err(ConfigError::NonPositiveMaxHealth)
    );
    assert_eq!(
        validate_changed(|c| c.max_health = -1),
        Err(ConfigError::NonPositiveMaxHealth)
    );
    assert_eq!(validate_changed(|c| c.max_health = 1), Ok(()));
}

#[test]
fn gravity_must_be_positive_or_jumps_never_end() {
    assert_eq!(
        validate_changed(|c| c.gravity = 0),
        Err(ConfigError::NonPositiveGravity)
    );
    assert_eq!(
        validate_changed(|c| c.gravity = -3),
        Err(ConfigError::NonPositiveGravity)
    );
    assert_eq!(validate_changed(|c| c.gravity = 1), Ok(()));
}

/// The frame data of `attack` in `config`, mutably.
fn data_mut(config: &mut MatchConfig, attack: Attack) -> &mut AttackData {
    let table = match attack.limb {
        Limb::Punch => &mut config.punches,
        Limb::Kick => &mut config.kicks,
    };
    &mut table[attack.strength.index()]
}

#[test]
fn every_attack_needs_at_least_one_active_tick() {
    for attack in Attack::ALL {
        assert_eq!(
            validate_changed(|c| data_mut(c, attack).active = 0),
            Err(ConfigError::ZeroActiveFrames(attack)),
            "{attack}"
        );
        assert_eq!(validate_changed(|c| data_mut(c, attack).active = 1), Ok(()));
    }
}

#[test]
fn attack_distances_damage_and_knockback_must_not_be_negative() {
    type Change = fn(&mut AttackData);
    let fields: [(&str, Change); 3] = [
        ("range", |d| d.range = -1),
        ("damage", |d| d.damage = -1),
        ("knockback", |d| d.knockback = -1),
    ];
    for attack in Attack::ALL {
        for (field, change) in fields {
            assert_eq!(
                validate_changed(|c| change(data_mut(c, attack))),
                Err(ConfigError::NegativeAttackValue {
                    attack,
                    field,
                    value: -1
                }),
                "{field} of the {attack}"
            );
        }
    }
}

#[test]
fn attack_values_may_be_zero() {
    for attack in Attack::ALL {
        assert_eq!(
            validate_changed(|c| {
                *data_mut(c, attack) = AttackData {
                    startup: 0,
                    active: 1,
                    recovery: 0,
                    range: 0,
                    damage: 0,
                    knockback: 0,
                    hitstun: 0,
                }
            }),
            Ok(()),
            "{attack}"
        );
    }
}

#[test]
fn negative_walk_speed_attack_height_and_super_jump_bonus_are_rejected() {
    assert_eq!(
        validate_changed(|c| c.walk_speed = -1),
        Err(ConfigError::NegativeValue {
            field: "walk_speed",
            value: -1
        })
    );
    assert_eq!(
        validate_changed(|c| c.attack_height = -2),
        Err(ConfigError::NegativeValue {
            field: "attack_height",
            value: -2
        })
    );
    assert_eq!(
        validate_changed(|c| c.super_jump_bonus = -3),
        Err(ConfigError::NegativeValue {
            field: "super_jump_bonus",
            value: -3
        })
    );
    assert_eq!(
        validate_changed(|c| {
            c.walk_speed = 0;
            c.attack_height = 0;
            c.super_jump_bonus = 0;
        }),
        Ok(())
    );
}

#[test]
fn validation_reports_the_first_problem_in_the_documented_order() {
    // Everything broken at once: each fix reveals the next problem in line.
    let mut config = MatchConfig {
        arena_width: 0,
        max_health: 0,
        gravity: 0,
        round_time_ticks: 0,
        block_reduction_percent: 101,
        walk_speed: -1,
        attack_height: -1,
        super_jump_bonus: -1,
        body_width: -1,
        body_height: -1,
        arena_depth: -1,
        sidestep_speed: -1,
        attack_depth: -1,
        body_depth: -1,
        techniques: [Technique::Sweep, Technique::Throw].map(|technique| TechniqueData {
            active: 0,
            escape_presses: 0,
            range: -1,
            damage: -1,
            ..technique_data(technique)
        }),
        grapple: GrappleData {
            active: 0,
            submission_interval: 0,
            escape_presses: [0; 3],
            range: -1,
            grapple_distance: -1,
            break_distance: -1,
            clinch_strike_damage: -1,
            ground_strike_damage: -1,
            takedown_damage: -1,
            submission_damage: -1,
            ..GRAPPLE
        },
        ..cfg()
    };
    for attack in Attack::ALL {
        *data_mut(&mut config, attack) = AttackData {
            active: 0,
            range: -1,
            damage: -1,
            knockback: -1,
            ..DATA
        };
    }

    let mut expected = vec![
        ConfigError::NonPositiveArenaWidth,
        ConfigError::NonPositiveMaxHealth,
        ConfigError::NonPositiveGravity,
        ConfigError::ZeroRoundTime,
        ConfigError::InvalidBlockReduction(101),
    ];
    for attack in Attack::ALL {
        expected.push(ConfigError::ZeroActiveFrames(attack));
        for field in ["range", "damage", "knockback"] {
            expected.push(ConfigError::NegativeAttackValue {
                attack,
                field,
                value: -1,
            });
        }
    }
    for field in [
        "walk_speed",
        "attack_height",
        "super_jump_bonus",
        "body_width",
        "body_height",
        "arena_depth",
        "sidestep_speed",
        "attack_depth",
        "body_depth",
    ] {
        expected.push(ConfigError::NegativeValue { field, value: -1 });
    }
    expected.push(ConfigError::ZeroGrabActiveFrames);
    expected.push(ConfigError::ZeroSubmissionInterval);
    for position in GrapplePosition::LADDER {
        expected.push(ConfigError::ZeroEscapePresses(position));
    }
    for field in GRAPPLE_FIELDS {
        expected.push(ConfigError::NegativeValue { field, value: -1 });
    }
    for technique in Technique::ALL {
        expected.push(ConfigError::ZeroTechniqueActiveFrames(technique));
        expected.push(ConfigError::ZeroEscapePresses(technique.finish()));
        for field in technique_fields(technique) {
            expected.push(ConfigError::NegativeValue { field, value: -1 });
        }
    }

    let mut reported = Vec::new();
    while let Err(error) = config.validate() {
        reported.push(error);
        fix(&mut config, error);
    }
    assert_eq!(reported, expected);
}

/// Repairs the single value `error` complains about.
fn fix(config: &mut MatchConfig, error: ConfigError) {
    match error {
        ConfigError::NonPositiveArenaWidth => config.arena_width = 1200,
        ConfigError::NonPositiveMaxHealth => config.max_health = 100,
        ConfigError::NonPositiveGravity => config.gravity = 5,
        ConfigError::ZeroRoundTime => config.round_time_ticks = 600,
        ConfigError::InvalidBlockReduction(_) => config.block_reduction_percent = 75,
        ConfigError::ZeroActiveFrames(attack) => data_mut(config, attack).active = 2,
        ConfigError::NegativeAttackValue { attack, field, .. } => {
            let data = data_mut(config, attack);
            match field {
                "range" => data.range = 100,
                "damage" => data.damage = 20,
                _ => data.knockback = 30,
            }
        }
        ConfigError::NegativeValue { field, .. } => *non_negative_mut(config, field) = 10,
        ConfigError::ZeroGrabActiveFrames => config.grapple.active = 1,
        ConfigError::ZeroSubmissionInterval => config.grapple.submission_interval = 2,
        ConfigError::ZeroTechniqueActiveFrames(technique) => {
            config.techniques[technique.index()].active = 1
        }
        ConfigError::ZeroEscapePresses(position) => match position.technique() {
            Some(technique) => config.techniques[technique.index()].escape_presses = 3,
            None => config.grapple.escape_presses[position.index()] = 3,
        },
    }
}

/// The rules [`cfg`] gives to `technique`.
fn technique_data(technique: Technique) -> TechniqueData {
    match technique {
        Technique::Sweep => SWEEP,
        Technique::Throw => THROW,
    }
}

/// The values of `technique` that must not be negative, in validation order.
fn technique_fields(technique: Technique) -> [&'static str; 2] {
    match technique {
        Technique::Sweep => ["sweep.range", "sweep.damage"],
        Technique::Throw => ["throw.range", "throw.damage"],
    }
}

/// The grappling values that must not be negative, in validation order.
const GRAPPLE_FIELDS: [&str; 7] = [
    "grapple.range",
    "grapple.grapple_distance",
    "grapple.break_distance",
    "grapple.clinch_strike_damage",
    "grapple.ground_strike_damage",
    "grapple.takedown_damage",
    "grapple.submission_damage",
];

/// The field a `NegativeValue` error names.
fn non_negative_mut<'a>(config: &'a mut MatchConfig, field: &str) -> &'a mut i32 {
    let grapple = &mut config.grapple;
    match field {
        "walk_speed" => &mut config.walk_speed,
        "attack_height" => &mut config.attack_height,
        "super_jump_bonus" => &mut config.super_jump_bonus,
        "body_width" => &mut config.body_width,
        "body_height" => &mut config.body_height,
        "arena_depth" => &mut config.arena_depth,
        "sidestep_speed" => &mut config.sidestep_speed,
        "attack_depth" => &mut config.attack_depth,
        "body_depth" => &mut config.body_depth,
        "sweep.range" => &mut config.techniques[0].range,
        "sweep.damage" => &mut config.techniques[0].damage,
        "throw.range" => &mut config.techniques[1].range,
        "throw.damage" => &mut config.techniques[1].damage,
        "grapple.range" => &mut grapple.range,
        "grapple.grapple_distance" => &mut grapple.grapple_distance,
        "grapple.break_distance" => &mut grapple.break_distance,
        "grapple.clinch_strike_damage" => &mut grapple.clinch_strike_damage,
        "grapple.ground_strike_damage" => &mut grapple.ground_strike_damage,
        "grapple.takedown_damage" => &mut grapple.takedown_damage,
        "grapple.submission_damage" => &mut grapple.submission_damage,
        other => panic!("unknown field {other}"),
    }
}

// ---------------------------------------------------------------------------------------------
// GrappleData
// ---------------------------------------------------------------------------------------------

#[test]
fn the_default_config_uses_the_default_grappling_rules() {
    assert_eq!(MatchConfig::default().grapple, GrappleData::default());
}

#[test]
fn the_default_grappling_numbers() {
    let data = GrappleData::default();
    assert_eq!((data.startup, data.active, data.recovery), (4, 2, 20));
    assert_eq!(
        (data.range, data.grapple_distance, data.break_distance),
        (55, 40, 90)
    );
    assert_eq!(
        (
            data.advance_ticks,
            data.max_hold_ticks,
            data.strike_cooldown
        ),
        (45, 5 * TICKS_PER_SECOND, 15)
    );
    assert_eq!(
        (
            data.clinch_strike_damage,
            data.ground_strike_damage,
            data.takedown_damage
        ),
        (3, 4, 8)
    );
    assert_eq!((data.submission_damage, data.submission_interval), (2, 12));
    assert_eq!(data.escape_presses, [4, 7, 10]);
    assert_eq!(data.break_stun, 20);
}

#[test]
fn by_default_a_grab_is_short_ranged_and_its_whiff_is_punishable() {
    let config = MatchConfig::default();
    let grab = config.grapple;
    let shortest_attack = Attack::ALL
        .into_iter()
        .map(|attack| config.attack(attack).range)
        .min()
        .unwrap();
    assert!(
        grab.range < shortest_attack,
        "grabs reach less than any strike"
    );
    assert!(
        grab.recovery > config.attack(Attack::punch(Strength::Low)).total(),
        "a jab fits in the whiff recovery"
    );
    assert!(grab.grapple_distance <= grab.range);
}

#[test]
fn by_default_escaping_gets_harder_up_the_ladder() {
    let presses = GrappleData::default().escape_presses;
    assert!(
        presses.windows(2).all(|pair| pair[0] < pair[1]),
        "{presses:?}"
    );
}

#[test]
fn the_total_length_of_a_grab_adds_its_three_phases() {
    assert_eq!(GRAPPLE.total(), 9);
    let huge = GrappleData {
        recovery: u32::MAX,
        ..GRAPPLE
    };
    assert_eq!(huge.total(), u32::MAX, "saturates instead of overflowing");
}

#[test]
fn the_grab_needs_at_least_one_active_tick() {
    assert_eq!(
        validate_changed(|c| c.grapple.active = 0),
        Err(ConfigError::ZeroGrabActiveFrames)
    );
    assert_eq!(validate_changed(|c| c.grapple.active = 1), Ok(()));
}

#[test]
fn the_submission_interval_must_be_positive() {
    assert_eq!(
        validate_changed(|c| c.grapple.submission_interval = 0),
        Err(ConfigError::ZeroSubmissionInterval)
    );
    assert_eq!(
        validate_changed(|c| c.grapple.submission_interval = 1),
        Ok(())
    );
}

#[test]
fn every_escape_needs_at_least_one_press() {
    for position in GrapplePosition::LADDER {
        assert_eq!(
            validate_changed(|c| c.grapple.escape_presses[position.index()] = 0),
            Err(ConfigError::ZeroEscapePresses(position)),
            "{position}"
        );
    }
    assert_eq!(
        validate_changed(|c| c.grapple.escape_presses = [1; 3]),
        Ok(())
    );
}

#[test]
fn grappling_distances_and_damage_must_not_be_negative() {
    for field in GRAPPLE_FIELDS {
        assert_eq!(
            validate_changed(|c| *non_negative_mut(c, field) = -5),
            Err(ConfigError::NegativeValue { field, value: -5 }),
            "{field}"
        );
        assert_eq!(
            validate_changed(|c| *non_negative_mut(c, field) = 0),
            Ok(()),
            "{field}"
        );
    }
}

#[test]
fn grappling_durations_may_be_zero() {
    let grapple = GrappleData {
        startup: 0,
        recovery: 0,
        advance_ticks: 0,
        max_hold_ticks: 0,
        strike_cooldown: 0,
        break_stun: 0,
        ..GRAPPLE
    };
    assert_eq!(validate_changed(|c| c.grapple = grapple), Ok(()));
}

#[test]
fn grappling_errors_explain_themselves() {
    assert_eq!(
        ConfigError::ZeroGrabActiveFrames.to_string(),
        "the grab needs at least one active tick"
    );
    assert_eq!(
        ConfigError::ZeroSubmissionInterval.to_string(),
        "the submission interval must be at least one tick"
    );
    assert_eq!(
        ConfigError::ZeroEscapePresses(GrapplePosition::Mount).to_string(),
        "escaping the mount needs at least one press"
    );
    assert_eq!(
        ConfigError::NegativeValue {
            field: "grapple.range",
            value: -1,
        }
        .to_string(),
        "`grapple.range` must not be negative, got -1"
    );
}

// ---------------------------------------------------------------------------------------------
// TechniqueData, push boxes and the combination window
// ---------------------------------------------------------------------------------------------

#[test]
fn technique_looks_up_the_rules_of_each_technique() {
    let config = cfg();
    assert_eq!(config.technique(Technique::Sweep), &SWEEP);
    assert_eq!(config.technique(Technique::Throw), &THROW);
    for technique in Technique::ALL {
        assert_eq!(
            config.technique(technique),
            &config.techniques[technique.index()]
        );
    }
}

#[test]
fn the_default_technique_numbers() {
    let config = MatchConfig::default();
    let sweep = config.technique(Technique::Sweep);
    assert_eq!((sweep.startup, sweep.active, sweep.recovery), (6, 3, 24));
    assert_eq!(
        (sweep.range, sweep.damage, sweep.escape_presses),
        (75, 10, 9)
    );
    assert_eq!((sweep.projection_ticks, sweep.counter_window), (24, 12));
    let throw = config.technique(Technique::Throw);
    assert_eq!((throw.startup, throw.active, throw.recovery), (8, 2, 28));
    assert_eq!(
        (throw.range, throw.damage, throw.escape_presses),
        (65, 14, 11)
    );
    assert_eq!((throw.projection_ticks, throw.counter_window), (30, 12));
    assert_eq!(
        (config.body_width, config.body_height, config.combo_window),
        (50, 90, 3)
    );
}

#[test]
fn by_default_techniques_outreach_the_push_box_and_can_be_countered_in_the_air() {
    let config = MatchConfig::default();
    for technique in Technique::ALL {
        let data = config.technique(technique);
        assert!(data.range > config.body_width, "{technique}");
        assert!(data.counter_window < data.projection_ticks, "{technique}");
        assert!(
            data.recovery > config.attack(Attack::punch(Strength::Low)).total(),
            "a whiffed {technique} is punishable"
        );
    }
    assert!(config.grapple.range > config.body_width, "grabs reach too");
}

#[test]
fn the_total_length_of_a_technique_adds_its_three_phases() {
    assert_eq!(SWEEP.total(), 9);
    assert_eq!(THROW.total(), 10);
    let huge = TechniqueData {
        recovery: u32::MAX,
        ..SWEEP
    };
    assert_eq!(huge.total(), u32::MAX, "saturates instead of overflowing");
}

#[test]
fn techniques_need_an_active_tick_and_an_escapable_finish() {
    for technique in Technique::ALL {
        let index = technique.index();
        assert_eq!(
            validate_changed(|c| c.techniques[index].active = 0),
            Err(ConfigError::ZeroTechniqueActiveFrames(technique))
        );
        assert_eq!(
            validate_changed(|c| c.techniques[index].escape_presses = 0),
            Err(ConfigError::ZeroEscapePresses(technique.finish()))
        );
        for field in technique_fields(technique) {
            assert_eq!(
                validate_changed(|c| *non_negative_mut(c, field) = -2),
                Err(ConfigError::NegativeValue { field, value: -2 })
            );
        }
        let minimal = TechniqueData {
            startup: 0,
            active: 1,
            recovery: 0,
            range: 0,
            projection_ticks: 0,
            counter_window: 0,
            damage: 0,
            escape_presses: 1,
        };
        assert_eq!(validate_changed(|c| c.techniques[index] = minimal), Ok(()));
    }
    assert_eq!(
        ConfigError::ZeroTechniqueActiveFrames(Technique::Throw).to_string(),
        "the ippon-seoi-nage needs at least one active tick"
    );
    assert_eq!(
        ConfigError::ZeroEscapePresses(GrapplePosition::ScarfHold).to_string(),
        "escaping the hon-gesa-gatame needs at least one press"
    );
}

#[test]
fn push_boxes_must_not_be_negative_and_may_be_off() {
    for field in ["body_width", "body_height"] {
        assert_eq!(
            validate_changed(|c| *non_negative_mut(c, field) = -1),
            Err(ConfigError::NegativeValue { field, value: -1 })
        );
    }
    assert_eq!(
        validate_changed(|c| {
            c.body_width = 0;
            c.body_height = 0;
            c.combo_window = 0;
        }),
        Ok(())
    );
}

#[test]
fn the_depth_rules_must_not_be_negative_and_may_be_off() {
    for field in [
        "arena_depth",
        "sidestep_speed",
        "attack_depth",
        "body_depth",
    ] {
        assert_eq!(
            validate_changed(|c| *non_negative_mut(c, field) = -1),
            Err(ConfigError::NegativeValue { field, value: -1 })
        );
        assert_eq!(
            validate_changed(|c| *non_negative_mut(c, field) = 0),
            Ok(())
        );
    }
}

#[test]
fn by_default_a_sidestep_leaves_the_line_of_an_attack_in_nine_ticks() {
    let config = MatchConfig::default();
    assert_eq!(
        (config.attack_depth / config.sidestep_speed) + 1,
        9,
        "first tick past attack_depth"
    );
    assert!(config.arena_depth > config.attack_depth, "room to dodge");
    assert_eq!(config.body_depth, config.attack_depth);
}

#[test]
fn rounds_need_a_positive_time_limit() {
    assert_eq!(
        validate_changed(|c| c.round_time_ticks = 0),
        Err(ConfigError::ZeroRoundTime)
    );
    assert_eq!(validate_changed(|c| c.round_time_ticks = 1), Ok(()));
}

#[test]
fn block_reduction_is_a_percentage() {
    assert_eq!(
        validate_changed(|c| c.block_reduction_percent = -1),
        Err(ConfigError::InvalidBlockReduction(-1))
    );
    assert_eq!(
        validate_changed(|c| c.block_reduction_percent = 101),
        Err(ConfigError::InvalidBlockReduction(101))
    );
    assert_eq!(validate_changed(|c| c.block_reduction_percent = 0), Ok(()));
    assert_eq!(
        validate_changed(|c| c.block_reduction_percent = 100),
        Ok(())
    );
}

#[test]
fn the_countdown_may_be_zero() {
    assert_eq!(validate_changed(|c| c.countdown_ticks = 0), Ok(()));
}

#[test]
fn config_errors_have_human_readable_messages() {
    let errors = [
        ConfigError::NonPositiveArenaWidth,
        ConfigError::NonPositiveMaxHealth,
        ConfigError::NonPositiveGravity,
        ConfigError::ZeroActiveFrames(Attack::punch(Strength::Low)),
        ConfigError::ZeroRoundTime,
        ConfigError::InvalidBlockReduction(101),
        ConfigError::NegativeValue {
            field: "walk_speed",
            value: -1,
        },
        ConfigError::NegativeAttackValue {
            attack: Attack::kick(Strength::Medium),
            field: "range",
            value: -1,
        },
        ConfigError::ZeroGrabActiveFrames,
        ConfigError::ZeroSubmissionInterval,
        ConfigError::ZeroEscapePresses(GrapplePosition::Clinch),
    ];
    for error in errors {
        assert!(!error.to_string().is_empty(), "{error:?}");
    }
    assert!(
        ConfigError::InvalidBlockReduction(101)
            .to_string()
            .contains("101")
    );
}

#[test]
fn attack_errors_name_the_attack_the_field_and_the_value() {
    assert_eq!(
        ConfigError::ZeroActiveFrames(Attack::kick(Strength::High)).to_string(),
        "the high kick needs at least one active tick"
    );
    assert_eq!(
        ConfigError::ZeroActiveFrames(Attack::punch(Strength::Low)).to_string(),
        "the low punch needs at least one active tick"
    );
    assert_eq!(
        ConfigError::NegativeAttackValue {
            attack: Attack::punch(Strength::Medium),
            field: "damage",
            value: -7,
        }
        .to_string(),
        "the damage of the medium punch must not be negative, got -7"
    );
    assert_eq!(
        ConfigError::NegativeValue {
            field: "attack_height",
            value: -4,
        }
        .to_string(),
        "`attack_height` must not be negative, got -4"
    );
}

#[test]
fn config_error_is_a_std_error() {
    fn assert_error<E: std::error::Error>() {}
    assert_error::<ConfigError>();
}

// ---------------------------------------------------------------------------------------------
// GlitchSet
// ---------------------------------------------------------------------------------------------

#[test]
fn there_are_three_glitches_in_declaration_order() {
    assert_eq!(
        Glitch::ALL,
        [Glitch::AnimationCancel, Glitch::SuperJump, Glitch::Reversal]
    );
}

#[test]
fn each_glitch_is_switched_independently() {
    for glitch in Glitch::ALL {
        let only = GlitchSet::none().with(glitch);
        for other in Glitch::ALL {
            assert_eq!(
                only.contains(other),
                other == glitch,
                "{glitch:?} / {other:?}"
            );
        }
        assert!(
            !GlitchSet::all().without(glitch).contains(glitch),
            "{glitch:?}"
        );
    }
}

#[test]
fn an_empty_set_contains_no_glitch() {
    for glitch in Glitch::ALL {
        assert!(!GlitchSet::none().contains(glitch));
    }
}

#[test]
fn the_full_set_contains_every_glitch() {
    for glitch in Glitch::ALL {
        assert!(GlitchSet::all().contains(glitch));
    }
}

#[test]
fn none_and_all_differ() {
    assert_ne!(GlitchSet::none(), GlitchSet::all());
}

#[test]
fn with_adds_exactly_one_glitch() {
    let set = GlitchSet::none().with(Glitch::AnimationCancel);
    assert!(set.contains(Glitch::AnimationCancel));
    assert!(!set.contains(Glitch::SuperJump));
}

#[test]
fn with_is_idempotent() {
    let once = GlitchSet::none().with(Glitch::SuperJump);
    assert_eq!(once.with(Glitch::SuperJump), once);
}

#[test]
fn without_removes_exactly_one_glitch() {
    let set = GlitchSet::all().without(Glitch::SuperJump);
    assert!(!set.contains(Glitch::SuperJump));
    assert!(set.contains(Glitch::AnimationCancel));
}

#[test]
fn without_on_a_missing_glitch_changes_nothing() {
    assert_eq!(
        GlitchSet::none().without(Glitch::SuperJump),
        GlitchSet::none()
    );
}

#[test]
fn adding_every_glitch_builds_the_full_set_in_any_order() {
    let forward = Glitch::ALL
        .iter()
        .copied()
        .fold(GlitchSet::none(), |set, glitch| set.with(glitch));
    let backward = Glitch::ALL
        .iter()
        .rev()
        .copied()
        .fold(GlitchSet::none(), |set, glitch| set.with(glitch));
    assert_eq!(forward, GlitchSet::all());
    assert_eq!(backward, GlitchSet::all());
}
