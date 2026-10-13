//! Grappling: the grab, the clinch / mount / submission ladder, escapes and the reversal glitch.
//!
//! Rules under test (numbers from `common::GRAPPLE`):
//!
//! * a grab (keypad 0) runs 2 start-up + 1 active + 6 recovery steps from the floor, with the
//!   priority attack > grab > jump > walk. Grabs resolve after strikes: a strike landing on a
//!   grabber interrupts it. A grab cannot be blocked; it catches the first fighter in roster
//!   order that is in front within 50 units, on the floor, not knocked out, not stunned (throw
//!   invulnerability), not grappling and that it may hurt. A whiff runs the full recovery;
//! * on connect the grabber is on top and the target at the bottom of a clinch, 40 apart,
//!   facing each other; neither moves, jumps, blocks or strikes normally;
//! * grappling reacts to fresh presses only (held now, not on the previous step). The first
//!   grapple tick is the step after the connect. The top advances (clinch -> mount ->
//!   submission) with a fresh grab once the position has been held 5 ticks; the takedown deals
//!   7. The top strikes with a fresh punch or kick, at most once every 3 ticks: 2 in the clinch,
//!   3 in the mount, none in the submission, which drains 4 every 2 ticks;
//! * the bottom gets one escape point per tick with any fresh press; 2 / 3 / 4 points escape the
//!   clinch / mount / submission: both are set 100 apart, the top is stunned 6 ticks. A
//!   position held 100 ticks is stood up by the referee (nobody stunned). A hit from a third
//!   fighter or a knockout ends the grapple for both;
//! * `Glitch::Reversal`: a fresh grab of the bottom on its first grapple tick swaps the roles.
//!
//! Timeline of `clinch()`: player 0 (at 580) presses grab on step 1 next to player 1 (at 620);
//! the grab is active and connects on step 3. "Tick n" below is the n-th grapple tick, i.e. the
//! n-th step after the connect.

mod common;

use common::*;
use srr::game::{
    Attack, AttackPhase, Facing, FighterState, Glitch, GlitchSet, Grapple, GrappleData,
    GrapplePosition, GrappleRole, Input, Match, MatchConfig, Outcome, Phase, PlayerId, Roster,
    Strength, TeamId,
};

const CLINCH: FighterState = FighterState::Grappling(GrappleRole::Top, GrapplePosition::Clinch);

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

fn cfg_grapple(change: impl FnOnce(&mut GrappleData)) -> MatchConfig {
    let mut config = cfg();
    change(&mut config.grapple);
    config
}

fn with_glitch(glitch: Glitch) -> MatchConfig {
    MatchConfig {
        glitches: GlitchSet::none().with(glitch),
        ..cfg()
    }
}

/// Player 0 at 580 grabs player 1 at 620 on step 1; returns the match right after the connect
/// (step 3).
fn clinch(config: MatchConfig) -> Match {
    let mut m = duel_at(config, 580, 620);
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    m
}

/// [`clinch`], then the top takes the bottom down on tick 5.
fn mount(config: MatchConfig) -> Match {
    let mut m = clinch(config);
    advance(&mut m, 0);
    m
}

/// [`mount`], then the top locks the submission 5 ticks later.
fn submission(config: MatchConfig) -> Match {
    let mut m = mount(config);
    advance(&mut m, 0);
    m
}

/// Waits 4 ticks, then `top` freshly presses grab: the position advances on the 5th tick.
fn advance(m: &mut Match, top: u8) {
    run(m, 4, &no_inputs());
    m.step(&only(top, press_grab()));
}

fn grapple_of(m: &Match, id: u8) -> Grapple {
    fighter(m, id)
        .grapple()
        .unwrap_or_else(|| panic!("player {id} is not grappling"))
}

fn position_of(m: &Match, id: u8) -> GrapplePosition {
    grapple_of(m, id).position
}

fn is_grappling(m: &Match, id: u8) -> bool {
    fighter(m, id).grapple().is_some()
}

/// One step where `id` presses `input`, then one step where nobody presses anything: one fresh
/// press every second tick.
fn tap(m: &mut Match, id: u8, input: Input) {
    m.step(&only(id, input));
    m.step(&no_inputs());
}

fn jab_with_range(range: i32) -> MatchConfig {
    cfg_with(|d| d.range = range)
}

// ---------------------------------------------------------------------------------------------
// The grab attempt
// ---------------------------------------------------------------------------------------------

#[test]
fn a_whiffed_grab_runs_its_whole_timeline() {
    let mut m = duel_at(cfg(), 100, 1100);
    let mut states = Vec::new();
    m.step(&only(0, press_grab()));
    states.push(state_of(&m, 0));
    for _ in 0..9 {
        m.step(&no_inputs());
        states.push(state_of(&m, 0));
    }
    let startup = FighterState::Grabbing(AttackPhase::Startup);
    let active = FighterState::Grabbing(AttackPhase::Active);
    let recovery = FighterState::Grabbing(AttackPhase::Recovery);
    assert_eq!(
        states,
        [
            startup,
            startup,
            active,
            recovery,
            recovery,
            recovery,
            recovery,
            recovery,
            recovery,
            FighterState::Idle
        ]
    );
    assert_eq!(attack_of(&m, 0), None, "a grab is not an attack");
}

#[test]
fn a_fighter_cannot_act_during_the_whiff_recovery_so_it_can_be_punished() {
    let mut m = duel_at(cfg(), 580, 700);
    m.step(&only(0, press_grab()));
    run(&mut m, 3, &only(0, jab())); // steps 2..=4: the jab is ignored
    let walk = Input {
        right: true,
        jump: true,
        ..Input::NONE
    };
    run(&mut m, 5, &only(0, walk)); // steps 5..=9: still recovering
    assert_eq!(
        state_of(&m, 0),
        FighterState::Grabbing(AttackPhase::Recovery)
    );
    assert_eq!((x_of(&m, 0), y_of(&m, 0)), (580, 0));
    m.step(&only(0, jab())); // step 10: free again
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Startup)
    );
}

#[test]
fn the_whiff_recovery_cannot_be_animation_cancelled() {
    let mut m = duel_at(with_glitch(Glitch::AnimationCancel), 100, 1100);
    m.step(&only(0, press_grab()));
    run(&mut m, 4, &no_inputs());
    m.step(&only(0, press_jump())); // step 6: in recovery
    assert_eq!(
        state_of(&m, 0),
        FighterState::Grabbing(AttackPhase::Recovery)
    );
    assert_eq!(y_of(&m, 0), 0);
}

#[test]
fn holding_grab_chains_a_new_grab_after_the_recovery() {
    let mut m = duel_at(cfg(), 100, 1100);
    run(&mut m, 10, &only(0, press_grab()));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Grabbing(AttackPhase::Startup)
    );
}

#[test]
fn an_attack_wins_over_a_grab_held_at_the_same_time() {
    let mut m = duel_at(cfg(), 100, 1100);
    m.step(&only(
        0,
        Input {
            grab: true,
            ..jab()
        },
    ));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Startup)
    );
}

#[test]
fn a_grab_wins_over_jump_and_walk() {
    let mut m = duel_at(cfg(), 100, 1100);
    let everything = Input {
        right: true,
        jump: true,
        grab: true,
        ..Input::NONE
    };
    m.step(&only(0, everything));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Grabbing(AttackPhase::Startup)
    );
    assert_eq!((x_of(&m, 0), y_of(&m, 0)), (100, 0));
}

#[test]
fn grabbing_needs_the_floor() {
    let mut m = duel_at(cfg(), 100, 1100);
    m.step(&only(0, press_jump()));
    m.step(&only(0, press_grab()));
    assert_eq!(state_of(&m, 0), FighterState::Airborne);
}

#[test]
fn a_grab_without_start_up_connects_on_its_first_step() {
    let mut m = duel_at(cfg_grapple(|g| g.startup = 0), 580, 620);
    m.step(&only(0, press_grab()));
    assert_eq!(state_of(&m, 0), CLINCH);
}

#[test]
fn a_grab_connects_on_any_of_its_active_steps() {
    let walk_in = Input {
        left: true,
        ..Input::NONE
    };
    let mut m = duel_at(cfg_grapple(|g| g.active = 3), 580, 680);
    m.step(&inputs_of(&[(0, press_grab()), (1, walk_in)]));
    run(&mut m, 3, &only(1, walk_in)); // steps 2..=4: player 1 at 650, 640
    assert!(!is_grappling(&m, 0));
    m.step(&only(1, walk_in)); // step 5: last active step, player 1 at 630
    assert_eq!(state_of(&m, 0), CLINCH);
}

// ---------------------------------------------------------------------------------------------
// Who can be grabbed
// ---------------------------------------------------------------------------------------------

#[test]
fn a_grab_connects_and_starts_a_clinch() {
    let m = clinch(cfg());
    assert_eq!(state_of(&m, 0), CLINCH);
    assert_eq!(
        state_of(&m, 1),
        FighterState::Grappling(GrappleRole::Bottom, GrapplePosition::Clinch)
    );
    assert_eq!(
        grapple_of(&m, 0),
        Grapple {
            partner: PlayerId(1),
            role: GrappleRole::Top,
            position: GrapplePosition::Clinch,
            escape: 0,
            escape_needed: 2,
            held_ticks: 0,
        }
    );
    assert_eq!(
        grapple_of(&m, 1),
        Grapple {
            partner: PlayerId(0),
            role: GrappleRole::Bottom,
            ..grapple_of(&m, 0)
        }
    );
    assert_eq!(health_of(&m, 1), 100, "the grab itself does no damage");
}

#[test]
fn a_target_exactly_at_grab_range_is_caught_and_one_unit_further_is_missed() {
    let mut at_range = duel_at(cfg(), 580, 630);
    at_range.step(&only(0, press_grab()));
    run(&mut at_range, 2, &no_inputs());
    assert!(is_grappling(&at_range, 1));

    let mut beyond = duel_at(cfg(), 580, 631);
    beyond.step(&only(0, press_grab()));
    run(&mut beyond, 3, &no_inputs());
    assert!(!is_grappling(&beyond, 1));
    assert_eq!(
        state_of(&beyond, 0),
        FighterState::Grabbing(AttackPhase::Recovery)
    );
}

#[test]
fn a_target_on_the_same_x_is_caught() {
    let mut m = duel_at(cfg(), 600, 600);
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    assert_eq!(state_of(&m, 0), CLINCH);
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (600, 640));
}

#[test]
fn a_grab_catches_the_first_fighter_in_roster_order_in_front() {
    // Player 0 faces player 2 (nearest, at 630); player 1 is behind it at 560.
    let mut m = brawl_at(cfg(), &[600, 560, 630]);
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    assert_eq!(grapple_of(&m, 0).partner, PlayerId(2));
    assert!(!is_grappling(&m, 1));

    // Both in front: roster order wins over distance.
    let mut m = brawl_at(cfg(), &[600, 640, 620]);
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    assert_eq!(grapple_of(&m, 0).partner, PlayerId(1));
    assert!(!is_grappling(&m, 2));
}

#[test]
fn grabs_cannot_be_blocked_by_stepping_back() {
    // Both spawn in the right half, facing left: player 0 turns to face player 1.
    let mut m = duel_at(cfg(), 1160, 1200);
    let back = press_right();
    m.step(&inputs_of(&[
        (0, turning(press_grab(), Facing::Right)),
        (1, back),
    ]));
    m.step(&only(1, back));
    assert_eq!(
        state_of(&m, 1),
        FighterState::Blocking,
        "a strike would be blocked"
    );
    m.step(&only(1, back));
    assert_eq!(state_of(&m, 0), CLINCH);
    assert!(is_grappling(&m, 1));
}

#[test]
fn a_grab_catches_a_fighter_walking_in() {
    let forward = press_left();
    let mut m = duel_at(cfg(), 580, 620);
    m.step(&inputs_of(&[(0, press_grab()), (1, forward)]));
    run(&mut m, 2, &only(1, forward));
    assert_eq!(state_of(&m, 0), CLINCH);
    assert_eq!(x_of(&m, 1), 620, "snapped back to the grapple distance");
}

#[test]
fn a_grab_catches_an_attacker_in_every_phase_of_its_attack() {
    // Short jabs whiff at 40 units, so player 1 attacks in the void.
    let cases = [
        (0, AttackPhase::Startup),
        (1, AttackPhase::Active),
        (3, AttackPhase::Recovery),
    ];
    for (delay, phase) in cases {
        let config = jab_with_range(10);

        // Where player 1's attack stands on the step the grab is active, without the grab.
        let mut alone = duel_at(config.clone(), 580, 620);
        alone.step(&only(1, jab()));
        run(&mut alone, delay + 2, &no_inputs());
        assert_eq!(state_of(&alone, 1), FighterState::Attacking(phase));

        let mut m = duel_at(config, 580, 620);
        m.step(&only(1, jab()));
        run(&mut m, delay, &no_inputs());
        m.step(&only(0, press_grab()));
        run(&mut m, 2, &no_inputs());
        assert_eq!(state_of(&m, 0), CLINCH, "{phase:?}");
        assert_eq!(attack_of(&m, 1), None, "the attack is cancelled");
    }
}

#[test]
fn a_grab_beats_a_strike_still_in_start_up() {
    let mut m = duel_at(cfg(), 580, 620);
    m.step(&inputs_of(&[(0, press_grab()), (1, jab())]));
    run(&mut m, 4, &no_inputs());
    assert_eq!(state_of(&m, 0), CLINCH);
    assert_eq!(health_of(&m, 0), 100, "the jab never came out");
}

#[test]
fn a_strike_landing_during_the_start_up_interrupts_the_grab() {
    let mut m = duel_at(cfg(), 580, 620);
    m.step(&only(1, jab())); // active on step 4
    m.step(&no_inputs());
    m.step(&only(0, press_grab())); // step 3: start-up on steps 3 and 4
    assert_eq!(
        state_of(&m, 0),
        FighterState::Grabbing(AttackPhase::Startup)
    );
    m.step(&no_inputs());
    assert_eq!(state_of(&m, 0), FighterState::HitStun);
    run(&mut m, 10, &no_inputs());
    assert!(!is_grappling(&m, 0) && !is_grappling(&m, 1));
    assert_eq!(health_of(&m, 0), 80);
}

#[test]
fn strikes_resolve_before_grabs_so_a_grabber_hit_on_its_active_step_does_not_connect() {
    let mut m = duel_at(cfg(), 580, 620);
    m.step(&only(1, jab())); // active on step 4
    m.step(&only(0, press_grab())); // step 2: active on step 4 too
    m.step(&no_inputs());
    m.step(&no_inputs());
    assert_eq!(state_of(&m, 0), FighterState::HitStun);
    assert!(!is_grappling(&m, 1));
}

#[test]
fn stunned_fighters_cannot_be_grabbed() {
    // A long stun without push-back keeps player 1 in reach and helpless.
    let config = cfg_with(|d| {
        d.hitstun = 30;
        d.knockback = 0;
    });
    let mut m = duel_at(config, 580, 620);
    m.step(&only(0, jab())); // hits on step 4: player 1 stunned on steps 5..=34
    run(&mut m, 8, &no_inputs());
    m.step(&only(0, press_grab())); // step 10: active on step 12
    run(&mut m, 2, &no_inputs());
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert!(!is_grappling(&m, 1), "throw invulnerability");
    m.step(&no_inputs());
    assert_eq!(
        state_of(&m, 0),
        FighterState::Grabbing(AttackPhase::Recovery)
    );

    run(&mut m, 30, &no_inputs()); // both free again
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    assert_eq!(state_of(&m, 0), CLINCH);
}

#[test]
fn airborne_fighters_cannot_be_grabbed() {
    let mut m = duel_at(cfg(), 580, 620);
    m.step(&inputs_of(&[(0, press_grab()), (1, press_jump())]));
    run(&mut m, 2, &no_inputs());
    assert!(y_of(&m, 1) > 0);
    assert!(!is_grappling(&m, 1));
    m.step(&no_inputs());
    assert_eq!(
        state_of(&m, 0),
        FighterState::Grabbing(AttackPhase::Recovery)
    );
}

#[test]
fn knocked_out_fighters_cannot_be_grabbed() {
    // Player 2 knocks player 1 out and pushes it next to player 0, whose grab then whiffs.
    let config = MatchConfig {
        max_health: 20,
        ..cfg()
    };
    let mut m = brawl_at(config, &[500, 540, 640]);
    m.step(&only(2, jab())); // knocks player 1 out on step 4, pushed to 510
    run(&mut m, 3, &no_inputs());
    assert!(fighter(&m, 1).is_knocked_out());
    assert_eq!(x_of(&m, 1), 510);
    m.step(&only(0, press_grab()));
    run(&mut m, 3, &no_inputs());
    assert!(!is_grappling(&m, 1));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Grabbing(AttackPhase::Recovery)
    );
}

#[test]
fn grappling_fighters_cannot_be_grabbed_again() {
    // Player 0 holds player 1; player 2 grabs at player 1 a step later and whiffs.
    let mut m = brawl_at(cfg(), &[500, 540, 580]);
    m.step(&only(0, press_grab()));
    m.step(&only(2, press_grab()));
    m.step(&no_inputs()); // step 3: player 0 connects
    m.step(&no_inputs()); // step 4: player 2's grab is active
    assert_eq!(grapple_of(&m, 1).partner, PlayerId(0));
    assert!(!is_grappling(&m, 2));
    m.step(&no_inputs());
    assert_eq!(
        state_of(&m, 2),
        FighterState::Grabbing(AttackPhase::Recovery)
    );
}

#[test]
fn a_fighter_grabbed_by_two_on_the_same_step_goes_to_the_first_in_roster_order() {
    let mut m = brawl_at(cfg(), &[500, 540, 580]);
    m.step(&inputs_of(&[(0, press_grab()), (2, press_grab())]));
    run(&mut m, 2, &no_inputs());
    assert_eq!(grapple_of(&m, 1).partner, PlayerId(0));
    assert_eq!(grapple_of(&m, 0).partner, PlayerId(1));
    assert!(!is_grappling(&m, 2));
}

#[test]
fn simultaneous_grabs_go_to_the_first_in_roster_order() {
    let mut m = duel_at(cfg(), 580, 620);
    m.step(&inputs_of(&[(0, press_grab()), (1, press_grab())]));
    run(&mut m, 2, &no_inputs());
    assert_eq!(grapple_of(&m, 0).role, GrappleRole::Top);
    assert_eq!(grapple_of(&m, 1).role, GrappleRole::Bottom);
}

#[test]
fn the_faster_grab_wins() {
    let mut m = duel_at(cfg(), 580, 620);
    m.step(&only(1, press_grab()));
    m.step(&only(0, press_grab()));
    m.step(&no_inputs()); // player 1's grab is active, player 0's still starting up
    assert_eq!(grapple_of(&m, 1).role, GrappleRole::Top);
    assert_eq!(grapple_of(&m, 0).role, GrappleRole::Bottom);
}

#[test]
fn allies_cannot_be_grabbed_without_friendly_fire() {
    let mut m = Match::with_spawns(Roster::two_versus_one(), cfg(), &[500, 540, 900]).unwrap();
    m.step(&only(0, press_grab()));
    run(&mut m, 3, &no_inputs());
    assert!(!is_grappling(&m, 1));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Grabbing(AttackPhase::Recovery)
    );
}

#[test]
fn allies_can_be_grabbed_with_friendly_fire() {
    let config = MatchConfig {
        friendly_fire: true,
        ..cfg()
    };
    let mut m = Match::with_spawns(Roster::two_versus_one(), config, &[500, 540, 900]).unwrap();
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    assert_eq!(grapple_of(&m, 0).partner, PlayerId(1));
}

// ---------------------------------------------------------------------------------------------
// Snapping
// ---------------------------------------------------------------------------------------------

#[test]
fn the_target_is_pulled_to_the_grapple_distance_and_both_face_each_other() {
    let mut m = duel_at(cfg(), 560, 580);
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (560, 600));
    assert_eq!(
        (facing_of(&m, 0), facing_of(&m, 1)),
        (Facing::Right, Facing::Left)
    );
}

#[test]
fn a_left_facing_grabber_pulls_the_target_to_its_left() {
    let mut m = duel_at(cfg(), 600, 620);
    m.step(&only(1, press_grab()));
    run(&mut m, 2, &no_inputs());
    assert_eq!((x_of(&m, 1), x_of(&m, 0)), (620, 580));
    assert_eq!(
        (facing_of(&m, 1), facing_of(&m, 0)),
        (Facing::Left, Facing::Right)
    );
}

#[test]
fn at_a_wall_the_grabber_steps_back_to_keep_the_distance() {
    let mut right_wall = duel_at(cfg(), 1170, 1200);
    right_wall.step(&only(0, turning(press_grab(), Facing::Right)));
    run(&mut right_wall, 2, &no_inputs());
    assert_eq!((x_of(&right_wall, 0), x_of(&right_wall, 1)), (1160, 1200));

    let mut left_wall = duel_at(cfg(), 0, 10);
    left_wall.step(&only(1, turning(press_grab(), Facing::Left)));
    run(&mut left_wall, 2, &no_inputs());
    assert_eq!((x_of(&left_wall, 1), x_of(&left_wall, 0)), (40, 0));
}

#[test]
fn in_an_arena_narrower_than_the_grapple_distance_both_stay_inside() {
    let config = MatchConfig {
        arena_width: 30,
        ..cfg()
    };
    let mut m = duel_at(config, 0, 30);
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (0, 30));
}

// ---------------------------------------------------------------------------------------------
// Inside the grapple
// ---------------------------------------------------------------------------------------------

#[test]
fn grappling_fighters_neither_walk_jump_turn_nor_attack() {
    let mut m = clinch(cfg());
    let mash = Input {
        left: true,
        right: true,
        jump: true,
        ..Input::pressing(Attack::kick(Strength::High))
    };
    let walk_away = Input {
        right: true,
        ..Input::NONE
    };
    run(&mut m, 20, &inputs_of(&[(0, mash), (1, walk_away)]));
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (580, 620));
    assert_eq!((y_of(&m, 0), y_of(&m, 1)), (0, 0));
    assert_eq!(
        (facing_of(&m, 0), facing_of(&m, 1)),
        (Facing::Right, Facing::Left)
    );
    assert_eq!(attack_of(&m, 0), None);
    assert_eq!(state_of(&m, 0), CLINCH);
    // Holding buttons is not pressing them: one strike and one escape point, on tick 1.
    assert_eq!(health_of(&m, 1), 98);
    assert_eq!(grapple_of(&m, 1).escape, 1);
    assert_eq!(grapple_of(&m, 1).held_ticks, 20);
}

#[test]
fn both_fighters_see_the_same_grapple() {
    let mut m = clinch(cfg());
    m.step(&only(1, press_left()));
    run(&mut m, 3, &no_inputs());
    let (top, bottom) = (grapple_of(&m, 0), grapple_of(&m, 1));
    assert_eq!((top.partner, bottom.partner), (PlayerId(1), PlayerId(0)));
    assert_eq!(
        (top.role, bottom.role),
        (GrappleRole::Top, GrappleRole::Bottom)
    );
    assert_eq!(top.position, bottom.position);
    assert_eq!((top.escape, top.escape_needed, top.held_ticks), (1, 2, 4));
    assert_eq!(
        (bottom.escape, bottom.escape_needed, bottom.held_ticks),
        (1, 2, 4)
    );
}

// ---------------------------------------------------------------------------------------------
// Advancing
// ---------------------------------------------------------------------------------------------

#[test]
fn the_top_takes_the_bottom_down_into_the_mount() {
    let m = mount(cfg());
    assert_eq!(
        state_of(&m, 0),
        FighterState::Grappling(GrappleRole::Top, GrapplePosition::Mount)
    );
    assert_eq!(
        state_of(&m, 1),
        FighterState::Grappling(GrappleRole::Bottom, GrapplePosition::Mount)
    );
    assert_eq!(health_of(&m, 1), 93, "the takedown hurts");
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (580, 620));
    assert_eq!((y_of(&m, 0), y_of(&m, 1)), (0, 0));
}

#[test]
fn the_mount_advances_into_a_painless_submission() {
    let m = submission(cfg());
    assert_eq!(position_of(&m, 0), GrapplePosition::Submission);
    assert_eq!(position_of(&m, 1), GrapplePosition::Submission);
    assert_eq!(health_of(&m, 1), 93);
}

#[test]
fn advancing_needs_the_position_held_long_enough() {
    let mut m = clinch(cfg());
    run(&mut m, 3, &no_inputs());
    m.step(&only(0, press_grab())); // tick 4: too early
    assert_eq!(position_of(&m, 0), GrapplePosition::Clinch);
    m.step(&only(0, press_grab())); // tick 5: held, not pressed
    assert_eq!(position_of(&m, 0), GrapplePosition::Clinch);
    m.step(&no_inputs());
    m.step(&only(0, press_grab())); // tick 7: fresh press, long enough
    assert_eq!(position_of(&m, 0), GrapplePosition::Mount);
}

#[test]
fn holding_grab_never_advances() {
    let mut m = duel_at(cfg(), 580, 620);
    run(&mut m, 60, &only(0, press_grab()));
    assert_eq!(state_of(&m, 0), CLINCH);
    assert_eq!(grapple_of(&m, 0).held_ticks, 57);
}

#[test]
fn entering_a_position_resets_the_escape_and_the_hold() {
    let mut m = clinch(cfg());
    m.step(&only(1, press_left())); // tick 1: one escape point
    assert_eq!(grapple_of(&m, 0).escape, 1);
    run(&mut m, 3, &no_inputs());
    m.step(&only(0, press_grab())); // tick 5: takedown
    let grapple = grapple_of(&m, 0);
    assert_eq!(grapple.position, GrapplePosition::Mount);
    assert_eq!(
        (grapple.escape, grapple.escape_needed, grapple.held_ticks),
        (0, 3, 0)
    );

    m.step(&only(1, press_left()));
    advance(&mut m, 0);
    let grapple = grapple_of(&m, 1);
    assert_eq!(grapple.position, GrapplePosition::Submission);
    assert_eq!(
        (grapple.escape, grapple.escape_needed, grapple.held_ticks),
        (0, 4, 0)
    );
}

#[test]
fn the_submission_is_the_last_position() {
    let mut m = submission(cfg());
    let health = health_of(&m, 1);
    advance(&mut m, 0);
    assert_eq!(position_of(&m, 0), GrapplePosition::Submission);
    assert_eq!(grapple_of(&m, 0).held_ticks, 5);
    assert_eq!(
        health_of(&m, 1),
        health - 2 * 4,
        "only the submission drain"
    );
}

#[test]
fn advancing_wins_over_striking_on_the_same_tick() {
    let mut m = clinch(cfg());
    run(&mut m, 4, &no_inputs());
    m.step(&only(
        0,
        Input {
            grab: true,
            ..jab()
        },
    ));
    assert_eq!(position_of(&m, 0), GrapplePosition::Mount);
    assert_eq!(health_of(&m, 1), 93, "takedown only");
}

// ---------------------------------------------------------------------------------------------
// Strikes
// ---------------------------------------------------------------------------------------------

#[test]
fn every_punch_and_kick_lands_in_the_clinch() {
    for attack in Attack::ALL {
        let mut m = clinch(cfg());
        m.step(&only(0, press(attack)));
        assert_eq!(health_of(&m, 1), 98, "{attack}");
        assert_eq!(health_of(&m, 0), 100, "{attack}");
    }
}

#[test]
fn ground_and_pound_hits_harder_in_the_mount() {
    let mut m = mount(cfg());
    m.step(&only(0, press(Attack::kick(Strength::Low))));
    assert_eq!(health_of(&m, 1), 93 - 3);
}

#[test]
fn holding_a_strike_lands_it_once() {
    let mut m = clinch(cfg());
    run(&mut m, 20, &only(0, jab()));
    assert_eq!(health_of(&m, 1), 98);
}

#[test]
fn strikes_have_a_cooldown() {
    let mut m = clinch(cfg());
    m.step(&only(0, jab())); // tick 1: lands
    m.step(&no_inputs());
    m.step(&only(0, jab())); // tick 3: cooling down
    assert_eq!(health_of(&m, 1), 98);
    m.step(&only(0, Input::pressing(Attack::punch(Strength::High)))); // tick 4: lands
    assert_eq!(health_of(&m, 1), 96);
}

#[test]
fn mashing_lands_one_strike_per_cooldown() {
    let mut m = clinch(cfg_grapple(|g| g.strike_cooldown = 0));
    for _ in 0..5 {
        tap(&mut m, 0, jab());
    }
    assert_eq!(health_of(&m, 1), 90, "no cooldown: every fresh press lands");

    let mut m = clinch(cfg());
    for _ in 0..6 {
        tap(&mut m, 0, jab()); // presses on ticks 1, 3, 5, 7, 9, 11
    }
    assert_eq!(health_of(&m, 1), 94, "lands on ticks 1, 5 and 9");
}

#[test]
fn there_are_no_strikes_in_the_submission() {
    let mut quiet = submission(cfg());
    let mut striking = quiet.clone();
    quiet.step(&no_inputs());
    striking.step(&only(0, jab()));
    assert_eq!(health_of(&striking, 1), health_of(&quiet, 1));
}

#[test]
fn the_bottom_cannot_strike() {
    let mut m = clinch(cfg());
    m.step(&only(1, jab()));
    assert_eq!(health_of(&m, 0), 100);
    assert_eq!(attack_of(&m, 1), None);
}

#[test]
fn a_strike_can_knock_out_in_the_grapple() {
    let config = MatchConfig {
        max_health: 2,
        ..cfg()
    };
    let mut m = clinch(config);
    m.step(&only(0, jab()));
    assert!(fighter(&m, 1).is_knocked_out());
    assert_eq!(state_of(&m, 1), FighterState::Knockout);
    assert_eq!(fighter(&m, 0).grapple(), None);
    assert_eq!(m.phase(), Phase::Finished(Outcome::Winner(TeamId(0))));
}

#[test]
fn the_takedown_can_knock_out() {
    let config = MatchConfig {
        max_health: 7,
        ..cfg()
    };
    let m = mount(config);
    assert!(fighter(&m, 1).is_knocked_out());
    assert_eq!(m.outcome(), Some(Outcome::Winner(TeamId(0))));
}

// ---------------------------------------------------------------------------------------------
// Submission
// ---------------------------------------------------------------------------------------------

#[test]
fn the_submission_drains_health_at_its_interval() {
    let mut m = submission(cfg());
    let mut health = Vec::new();
    for _ in 0..6 {
        m.step(&no_inputs());
        health.push(health_of(&m, 1));
    }
    assert_eq!(health, [93, 89, 89, 85, 85, 81]);
    assert_eq!(health_of(&m, 0), 100);
}

#[test]
fn a_tap_out_is_a_knockout_that_ends_the_round() {
    let config = MatchConfig {
        max_health: 20,
        ..cfg()
    };
    let mut m = submission(config); // 13 health left after the takedown
    run(&mut m, 7, &no_inputs());
    assert_eq!(health_of(&m, 1), 1);
    assert_eq!(m.phase(), Phase::Fighting);
    m.step(&no_inputs());
    assert_eq!(health_of(&m, 1), 0);
    assert!(fighter(&m, 1).is_knocked_out());
    assert_eq!(state_of(&m, 1), FighterState::Knockout);
    assert_eq!(fighter(&m, 1).grapple(), None);
    assert_eq!(fighter(&m, 0).grapple(), None);
    assert_eq!(state_of(&m, 0), FighterState::Idle, "the winner lets go");
    assert_eq!(m.phase(), Phase::Finished(Outcome::Winner(TeamId(0))));
}

// ---------------------------------------------------------------------------------------------
// Escapes
// ---------------------------------------------------------------------------------------------

#[test]
fn escaping_gets_harder_up_the_ladder() {
    type Enter = fn(MatchConfig) -> Match;
    let positions: [(Enter, u32); 3] = [(clinch, 2), (mount, 3), (submission, 4)];
    for (enter, needed) in positions {
        let mut m = enter(cfg());
        let position = position_of(&m, 0);
        assert_eq!(grapple_of(&m, 1).escape_needed, needed, "{position}");
        for point in 1..needed {
            tap(&mut m, 1, press_left());
            assert_eq!(grapple_of(&m, 1).escape, point, "{position}");
        }
        m.step(&only(1, press_left()));
        assert!(!is_grappling(&m, 0) && !is_grappling(&m, 1), "{position}");
    }
}

#[test]
fn every_button_counts_as_a_struggle() {
    for bit in 0..Input::BITS {
        let mut m = clinch(cfg());
        m.step(&only(1, Input::from_bits(1 << bit)));
        assert_eq!(grapple_of(&m, 1).escape, 1, "bit {bit}");
    }
}

#[test]
fn several_fresh_presses_on_one_tick_count_once() {
    let mut m = mount(cfg());
    m.step(&only(1, Input::from_bits(u16::MAX)));
    assert_eq!(grapple_of(&m, 1).escape, 1);
}

#[test]
fn a_new_button_counts_even_while_others_are_held() {
    let mut m = mount(cfg());
    let left = press_left();
    let left_and_jump = Input { jump: true, ..left };
    m.step(&only(1, left));
    m.step(&only(1, left));
    m.step(&only(1, left_and_jump));
    assert_eq!(grapple_of(&m, 1).escape, 2);
}

#[test]
fn an_escape_separates_the_fighters_and_stuns_the_top() {
    let mut m = clinch(cfg());
    tap(&mut m, 1, press_left());
    m.step(&only(1, press_left())); // tick 3: escape
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (550, 650));
    assert_eq!(state_of(&m, 0), FighterState::HitStun);
    assert_eq!(state_of(&m, 1), FighterState::Idle);

    // The bottom is free at once (it jumps away); the top cannot act for 6 steps.
    m.step(&inputs_of(&[(0, jab()), (1, press_jump())]));
    assert_eq!(state_of(&m, 1), FighterState::Airborne);
    let mut stunned_steps = 1;
    while state_of(&m, 0) == FighterState::HitStun {
        m.step(&only(0, jab()));
        stunned_steps += 1;
    }
    assert_eq!(stunned_steps, 6);
    m.step(&only(0, Input::pressing(Attack::kick(Strength::Low))));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Startup)
    );
}

#[test]
fn without_break_stun_the_top_is_free_at_once() {
    let mut m = clinch(cfg_grapple(|g| g.break_stun = 0));
    tap(&mut m, 1, press_left());
    m.step(&only(1, press_left()));
    assert_eq!(state_of(&m, 0), FighterState::Idle);
}

#[test]
fn an_escape_from_a_left_facing_top_keeps_the_sides() {
    let mut m = duel_at(cfg(), 600, 620);
    m.step(&only(1, press_grab()));
    run(&mut m, 2, &no_inputs()); // player 1 on top at 620, player 0 at 580
    tap(&mut m, 0, press_right());
    m.step(&only(0, press_right()));
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (550, 650));
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
}

#[test]
fn breaking_at_a_wall_keeps_both_inside_the_arena() {
    let mut right_wall = duel_at(cfg(), 1170, 1200);
    right_wall.step(&only(0, turning(press_grab(), Facing::Right)));
    run(&mut right_wall, 2, &no_inputs());
    tap(&mut right_wall, 1, press_left());
    right_wall.step(&only(1, press_left()));
    assert_eq!((x_of(&right_wall, 0), x_of(&right_wall, 1)), (1100, 1200));

    let mut left_wall = duel_at(cfg(), 0, 10);
    left_wall.step(&only(1, turning(press_grab(), Facing::Left)));
    run(&mut left_wall, 2, &no_inputs());
    tap(&mut left_wall, 0, press_left());
    left_wall.step(&only(0, press_left()));
    assert_eq!((x_of(&left_wall, 0), x_of(&left_wall, 1)), (0, 100));
}

#[test]
fn breaking_in_an_arena_narrower_than_the_break_distance_stays_inside() {
    let config = MatchConfig {
        arena_width: 30,
        ..cfg()
    };
    let mut m = duel_at(config, 0, 30);
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    tap(&mut m, 1, press_left());
    m.step(&only(1, press_left()));
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (0, 30));
}

// ---------------------------------------------------------------------------------------------
// Referee
// ---------------------------------------------------------------------------------------------

#[test]
fn the_referee_stands_a_stalled_position_up() {
    let mut m = clinch(cfg());
    run(&mut m, 99, &no_inputs());
    assert_eq!(grapple_of(&m, 0).held_ticks, 99);
    m.step(&no_inputs());
    assert!(!is_grappling(&m, 0) && !is_grappling(&m, 1));
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (550, 650));
    assert_eq!(state_of(&m, 0), FighterState::Idle, "nobody is stunned");
    assert_eq!(state_of(&m, 1), FighterState::Idle);
    m.step(&only(0, jab()));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Startup)
    );
}

#[test]
fn advancing_restarts_the_referee_count() {
    let mut m = mount(cfg()); // 5 ticks in the clinch
    run(&mut m, 99, &no_inputs());
    assert_eq!(position_of(&m, 0), GrapplePosition::Mount);
    m.step(&no_inputs());
    assert!(!is_grappling(&m, 0));
}

#[test]
fn the_referee_also_breaks_a_submission() {
    let mut m = submission(cfg_grapple(|g| g.submission_damage = 0));
    run(&mut m, 100, &no_inputs());
    assert!(!is_grappling(&m, 1));
    assert_eq!(health_of(&m, 1), 93);
}

#[test]
fn an_escape_on_the_referee_tick_still_stuns_the_top() {
    let mut m = clinch(cfg());
    m.step(&only(1, press_left()));
    run(&mut m, 98, &no_inputs());
    m.step(&only(1, press_left())); // tick 100
    assert_eq!(state_of(&m, 0), FighterState::HitStun);
}

// ---------------------------------------------------------------------------------------------
// Third parties and knockouts
// ---------------------------------------------------------------------------------------------

#[test]
fn a_third_fighter_hitting_the_bottom_frees_the_top() {
    // Player 0 holds player 1 (at 540); player 2 jabs player 1 from 640.
    let mut m = brawl_at(cfg(), &[500, 540, 640]);
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    assert!(is_grappling(&m, 1));
    // The bottom steps back the way the jab travels: grapplers cannot block.
    m.step(&inputs_of(&[(2, jab()), (1, press_left())]));
    run(&mut m, 2, &only(1, press_left()));
    assert!(is_grappling(&m, 0));
    m.step(&only(1, press_left())); // step 7: the jab lands
    assert_eq!(health_of(&m, 1), 80, "not blocked");
    assert_eq!(state_of(&m, 1), FighterState::HitStun);
    assert_eq!(x_of(&m, 1), 510);
    assert_eq!(fighter(&m, 0).grapple(), None);
    assert_eq!(state_of(&m, 0), FighterState::Idle);
    m.step(&only(0, jab()));
    assert_eq!(
        state_of(&m, 0),
        FighterState::Attacking(AttackPhase::Startup)
    );
}

#[test]
fn a_third_fighter_hitting_the_top_frees_the_bottom() {
    // Player 0 (at 640) holds player 1 (at 600); player 2 jabs player 0 from 740.
    let mut m = brawl_at(cfg(), &[640, 600, 740]);
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    assert_eq!(grapple_of(&m, 0).partner, PlayerId(1));
    m.step(&only(2, jab()));
    run(&mut m, 3, &no_inputs());
    assert_eq!(state_of(&m, 0), FighterState::HitStun);
    assert_eq!(health_of(&m, 1), 100);
    assert_eq!(fighter(&m, 1).grapple(), None);
    assert_eq!(state_of(&m, 1), FighterState::Idle);
}

#[test]
fn a_participant_knocked_out_by_a_third_fighter_frees_the_other() {
    let config = MatchConfig {
        max_health: 20,
        ..cfg()
    };
    let mut m = brawl_at(config, &[500, 540, 640]);
    m.step(&only(0, press_grab()));
    run(&mut m, 2, &no_inputs());
    m.step(&only(2, jab()));
    run(&mut m, 3, &no_inputs());
    assert!(fighter(&m, 1).is_knocked_out());
    assert_eq!(fighter(&m, 0).grapple(), None);
    assert_eq!(state_of(&m, 0), FighterState::Idle);
    assert_eq!(m.phase(), Phase::Fighting, "players 0 and 2 still stand");
}

#[test]
fn a_harmless_ally_does_not_break_the_grapple() {
    // 2v1: player 2 holds player 0; player 1 (player 0's ally) cannot hurt player 0.
    let mut m = Match::with_spawns(Roster::two_versus_one(), cfg(), &[600, 700, 640]).unwrap();
    m.step(&only(2, press_grab()));
    run(&mut m, 2, &no_inputs());
    assert_eq!(grapple_of(&m, 2).partner, PlayerId(0));
    m.step(&only(1, jab()));
    run(&mut m, 4, &no_inputs());
    assert_eq!(health_of(&m, 2), 80, "the enemy top is hit and lets go");
    assert_eq!(health_of(&m, 0), 100);
    assert_eq!(state_of(&m, 0), FighterState::Idle);
}

// ---------------------------------------------------------------------------------------------
// Reversal glitch
// ---------------------------------------------------------------------------------------------

#[test]
fn reversal_is_enabled_by_default() {
    assert!(GlitchSet::all().contains(Glitch::Reversal));
    assert!(MatchConfig::default().glitches.contains(Glitch::Reversal));
}

#[test]
fn a_grab_pressed_on_the_first_grapple_tick_reverses_the_roles() {
    let mut m = clinch(with_glitch(Glitch::Reversal));
    m.step(&only(1, press_grab()));
    assert_eq!(
        grapple_of(&m, 1),
        Grapple {
            partner: PlayerId(0),
            role: GrappleRole::Top,
            position: GrapplePosition::Clinch,
            escape: 0,
            escape_needed: 2,
            held_ticks: 0,
        }
    );
    assert_eq!(grapple_of(&m, 0).role, GrappleRole::Bottom);
    assert_eq!(state_of(&m, 1), CLINCH);
    assert_eq!((x_of(&m, 0), x_of(&m, 1)), (580, 620));

    // The new top runs the grapple; the old one cannot reverse back.
    m.step(&only(0, press_grab()));
    assert_eq!(grapple_of(&m, 1).role, GrappleRole::Top);
    assert_eq!(grapple_of(&m, 0).escape, 1);
    run(&mut m, 3, &no_inputs());
    m.step(&only(1, press_grab())); // 5th step held after the reversal
    assert_eq!(position_of(&m, 1), GrapplePosition::Mount);
    assert_eq!(health_of(&m, 0), 93);
}

#[test]
fn reversal_only_works_on_the_first_tick() {
    let mut m = clinch(with_glitch(Glitch::Reversal));
    m.step(&no_inputs());
    m.step(&only(1, press_grab()));
    assert_eq!(grapple_of(&m, 1).role, GrappleRole::Bottom);
    assert_eq!(grapple_of(&m, 1).escape, 1, "just a struggle");
}

#[test]
fn reversal_needs_a_fresh_press() {
    // Player 1 grabs too but a step late: its grab button is still held when caught.
    let mut m = duel_at(with_glitch(Glitch::Reversal), 580, 620);
    m.step(&only(0, press_grab()));
    m.step(&only(1, press_grab()));
    m.step(&only(1, press_grab())); // caught during its start-up
    m.step(&only(1, press_grab())); // first grapple tick, grab held
    assert_eq!(grapple_of(&m, 1).role, GrappleRole::Bottom);
    assert_eq!(grapple_of(&m, 1).escape, 0);
}

#[test]
fn without_the_glitch_a_first_tick_grab_is_just_a_struggle() {
    let mut m = clinch(cfg());
    m.step(&only(1, press_grab()));
    assert_eq!(grapple_of(&m, 1).role, GrappleRole::Bottom);
    assert_eq!(grapple_of(&m, 1).escape, 1);
}

// ---------------------------------------------------------------------------------------------
// Observability
// ---------------------------------------------------------------------------------------------

#[test]
fn positions_are_listed_and_indexed_in_ladder_order() {
    assert_eq!(
        GrapplePosition::LADDER,
        [
            GrapplePosition::Clinch,
            GrapplePosition::Mount,
            GrapplePosition::Submission
        ]
    );
    for (index, position) in GrapplePosition::LADDER.into_iter().enumerate() {
        assert_eq!(position.index(), index);
        assert_eq!(
            position.next(),
            GrapplePosition::LADDER.get(index + 1).copied()
        );
        assert_eq!(position.technique(), None);
        assert!(!position.is_projection());
    }
    let names: Vec<String> = GrapplePosition::LADDER
        .iter()
        .map(|p| p.to_string())
        .collect();
    assert_eq!(names, ["clinch", "mount", "submission"]);
}

#[test]
fn every_position_is_listed_and_indexed_ladder_first() {
    assert_eq!(&GrapplePosition::ALL[..3], &GrapplePosition::LADDER[..]);
    assert_eq!(
        &GrapplePosition::ALL[3..],
        &[
            GrapplePosition::Sweep,
            GrapplePosition::ScarfHold,
            GrapplePosition::Throw,
            GrapplePosition::Armbar
        ]
    );
    for (index, position) in GrapplePosition::ALL.into_iter().enumerate() {
        assert_eq!(position.index(), index);
    }
    let names: Vec<String> = GrapplePosition::ALL[3..]
        .iter()
        .map(|p| p.to_string())
        .collect();
    assert_eq!(
        names,
        [
            "juji-gatame sweep",
            "hon-gesa-gatame",
            "ippon-seoi-nage",
            "ude-hishigi-juji-gatame"
        ]
    );
}

#[test]
fn roles_and_facings_have_opposites() {
    assert_eq!(GrappleRole::Top.opposite(), GrappleRole::Bottom);
    assert_eq!(GrappleRole::Bottom.opposite(), GrappleRole::Top);
    assert_eq!(Facing::Left.opposite(), Facing::Right);
    assert_eq!(Facing::Right.opposite(), Facing::Left);
}

#[test]
fn free_fighters_report_no_grapple() {
    let m = duel_at(cfg(), 580, 620);
    assert_eq!(fighter(&m, 0).grapple(), None);
    assert_eq!(fighter(&m, 1).grapple(), None);
}

// ---------------------------------------------------------------------------------------------
// Determinism and invariants
// ---------------------------------------------------------------------------------------------

/// Knuth's MMIX linear congruential generator.
struct Lcg(u64);

impl Lcg {
    /// Random buttons, all ten of them, so grabs, struggles and reversals all happen.
    fn input(&mut self) -> Input {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // Truncation is intended: only the low bits are kept.
        let bits = (self.0 >> 33) as u16;
        // Mostly grabs and directions, now and then an attack.
        let mask = if bits & 0x400 == 0 {
            0b10_0000_0111
        } else {
            0x3FF
        };
        Input::from_bits(bits & mask)
    }
}

fn assert_grapple_invariants(m: &Match) {
    for f in m.fighters() {
        assert!((0..=m.config().arena_width).contains(&f.x()));
        assert!((0..=m.config().max_health).contains(&f.health()));
        let Some(grapple) = f.grapple() else {
            assert!(!matches!(f.state(), FighterState::Grappling(..)));
            continue;
        };
        assert_eq!(f.y(), 0, "grappling happens on the floor");
        assert_eq!(
            f.state(),
            FighterState::Grappling(grapple.role, grapple.position)
        );
        let partner = m.fighter(grapple.partner).unwrap().grapple().unwrap();
        assert_eq!(partner.partner, f.player());
        assert_eq!(partner.role, grapple.role.opposite());
        assert_eq!(
            (partner.position, partner.escape, partner.held_ticks),
            (grapple.position, grapple.escape, grapple.held_ticks)
        );
        if grapple.position.is_projection() {
            assert_eq!((grapple.escape, grapple.escape_needed), (0, 0));
        } else {
            assert!(grapple.escape < grapple.escape_needed);
        }
        assert!(grapple.held_ticks < m.config().grapple.max_hold_ticks);
    }
}

#[test]
fn random_grappling_brawls_keep_the_invariants_and_replay_identically() {
    let config = MatchConfig {
        glitches: GlitchSet::all(),
        friendly_fire: true,
        round_time_ticks: 3000,
        ..cfg()
    };
    let mut grapples_seen = 0;
    for (seed, players) in [(1, 2), (2, 3), (3, 4), (4, 10)] {
        let mut rng = Lcg(seed);
        let roster = Roster::free_for_all(players).unwrap();
        let mut m = Match::new(roster, config.clone()).unwrap();
        let mut replay = m.clone();
        for _ in 0..3000 {
            let frame: Inputs = (0..players as u8)
                .map(|id| (PlayerId(id), rng.input()))
                .collect();
            m.step(&frame);
            replay.step(&frame);
            assert_eq!(m, replay);
            assert_grapple_invariants(&m);
            grapples_seen += m
                .fighters()
                .iter()
                .filter(|f| f.grapple().is_some())
                .count();
        }
        assert!(m.outcome().is_some(), "{players} players");
    }
    assert!(grapples_seen > 0, "the random inputs never grappled");
}
