# Tuning guide: the frame data of Shattered Realm: Rampage

The whole balance of the game is one struct: `MatchConfig`
(`src/game/config.rs`), and every number is in **ticks** — one tick is 1/60 s.
This guide explains what the fields do and how they interact, so a balance patch is a
small, safe change. The tests never use the defaults (they build their own explicit
`MatchConfig` in `tests/common/mod.rs`), so retuning `MatchConfig::default()` breaks
no test — only the feel of the game.

## The one formula to remember

An attack is **start-up + active + recovery** ticks (`AttackData::total()`), and that
sum is the **total commitment** of a press:

* **start-up**: the wind-up. Nothing happens yet; being hit here is being interrupted.
* **active**: the hitbox is live. A whiff still spends these ticks.
* **recovery**: you cannot act (blocking included). Being punished means somebody hit
  you in here, or right after.

If your attack connects, the target is stunned for **hitstun** ticks; a strike that
landed can be cancelled into a stronger tier of the same kind (`punch → punch harder`),
which is how combos chain. The techniques have a window of their own:
`combo_window` (3) is the most ticks between the two presses of a technique's key
combination (`5` + `9`, `8` + `9`; zero would mean the very same tick).

## The defaults, in the open

`MatchConfig::default()` (60 ticks a second, an arena 1200 wide and 150 deep):

| What                    | Field                     | Value                          |
| ----------------------- | ------------------------- | ------------------------------ |
| Health                  | `max_health`              | 100                            |
| Walk / sidestep         | `walk_speed` / `sidestep_speed` | 7 / 5 (a sidestep leaves an attack's line in 9 ticks) |
| Jump                    | `jump_velocity`, `gravity`, `super_jump_bonus` | 22, 2, 10 (jump cancels; the glitch super-jump adds the bonus) |
| Push box                | `body_width/height/depth` | 50 × 90 × 40                   |
| Reach (height / depth)  | `attack_height`, `attack_depth` | 60 / 40                  |
| Blocking                | `block_reduction_percent` | 75 (a blocked strike deals a quarter) |
| Round / countdown       | `round_time_ticks`, `countdown_ticks` | 99 s / 2 s            |

## The intentional glitches

Every exploit is a `Glitch` in `MatchConfig::glitches` (`GlitchSet`, a bit per
switch), on by default; below the numbers each one uses. Turn one off and the
corresponding behaviour disappears completely — the tests of `tests/glitches.rs`
check exactly that.

| Glitch | Numbers (field / default) | What they do |
| --- | --- | --- |
| Animation cancel | — | a jump pressed in an attack's recovery cancels it |
| Super jump | `super_jump_bonus` / 10 | extra take-off velocity of the bounce |
| Reversal | — | grab on the first tick of being grabbed swaps the roles |
| Echo barrage | `echo_punches` / 5, `echo_damage` / 2 | a landed unblocked punch rings that many copies around the victim, each hitting once per tick for that much — unblockable, no push, no stun |
| Stretch strike | `stretch_reach` / 60, `fist_lifetime` / 15, `fist_damage` / 6 | punches reach that much further, and a fully whiffed one breaks its fist off, which floats for that many ticks and still hits for that much (blockable) |
| Quick draw | — | a punch pressed in a punch's recovery restarts the punch at once |
| Crater slam | `slam_radius` / 150, `slam_damage` / 8 | a landed judo throw quakes the ground: everybody standing within the radius of the landing point loses that much health |
| Phantom warp | `warp_distance` / 80 | an unblocked hit taken in the air snaps the victim that far along the hit's direction, on top of the knockback |

Tuning tips: the echo barrage rewards landing a single clean punch, so keep its total
(`echo_punches` × `echo_damage`) below a heavy attack's damage; the stretch reach turns
pokes into long-range threats, so mind its sum with `range` against `walk_speed`;
the slam turns a throw into area denial — raise `slam_radius` and a throw into a crowd
becomes the play.

The six attacks (start-up, active, recovery, range, damage, knockback, hitstun):

| Attack          | start-up | active | recovery | **total** | range | damage | knockback | hitstun |
| --------------- | --------: | -----: | -------: | --------: | ----: | -----: | --------: | ------: |
| Low punch       | 3         | 2      | 6        | **11**    | 80    | 5      | 12        | 10      |
| Medium punch    | 5         | 3      | 10       | **18**    | 85    | 9      | 20        | 14      |
| High punch      | 8         | 3      | 16       | **27**    | 90    | 14     | 32        | 18      |
| Low kick        | 5         | 3      | 9        | **17**    | 105   | 7      | 16        | 12      |
| Medium kick     | 7         | 3      | 13       | **23**    | 115   | 11     | 26        | 16      |
| High kick       | 11        | 4      | 20       | **35**    | 125   | 17     | 40        | 22      |

Read it as the trade the player is offered: **stronger = slower and more committed**,
and a kick reaches further and hits harder than a punch of the same tier but pays for
it in wind-up — the low kick (17 total) and the high punch (27) sit on purpose at the
two ends a brawler actually uses.

## Grappling: the ladder

`GrappleData::default()`: the grab itself is 4 + 2 ticks at range 55, and a whiff
costs 20 recovery ticks — long enough to be the big read it is meant to be. A connected
grab locks both fighters 40 apart (`grapple_distance`) and starts the ladder:

* Positions advance every `advance_ticks` (45 = 0.75 s): clinch → mount → submission.
* The top strikes every `strike_cooldown` (15 ticks) for 3 damage in the clinch, 4 on
  the ground (`clinch_strike_damage`, `ground_strike_damage`); grabbing again climbs
  and the takedown costs 8 (`takedown_damage`).
* A submission (`submission_damage` 2, every `submission_interval` 12 ticks) drains
  until the tap-out — a KO.
* The bottom escapes by mashing **fresh presses**: `escape_presses` [4, 7, 10] points
  for the clinch, mount and submission. Breaking free pushes the fighters
  `break_distance` (90) apart and stuns the top for `break_stun` (20) ticks: a
  well-earned turn.
* The referee stands both up after `max_hold_ticks` (5 s in the same position), and any
  hit from a third fighter breaks the hold.

The judo techniques (`techniques`): the sweep is 6 + 3 (+ 24 recovery) at range 75 for
10 damage, countered in the first 12 ticks of its 24-tick projection, escaped with 9
presses; the throw is 8 + 2 (+ 28) at range 65 for 14 damage, countered in the first
12 of 30, escaped with 11. Both whiffs are the most punishable moments in the game —
that is the point of a read-based technique.

## Characters: percentages and special moves

Every fighter plays a `Character` (`src/game/character.rs`). A character never touches the
shared rules (arena, timer, blocking, the grappling ladder): `Character::rules` scales the
fighter's own numbers by the percentages of its `CharacterData`, rounding down, and gives it
its special move. Chalk is 100 % everywhere and keeps `MatchConfig::special`, so a match of
Chalks is exactly the configuration.

| Character | Health | Speed (walk, sidestep) | Jump | Power (strike damage) | Reach (strike range) | Grab reach (grab, techniques) |
| --------- | -----: | ---------------------: | ---: | --------------------: | -------------------: | ----------------------------: |
| Chalk     | 100 %  | 100 %                  | 100 % | 100 %                | 100 %                | 100 %                         |
| Torch     | 90 %   | 95 %                   | 100 % | 95 %                 | 100 %                | 100 %                         |
| Brick     | 130 %  | 75 %                   | 85 %  | 115 %                | 105 %                | 125 %                         |
| Zip       | 80 %   | 135 %                  | 112 % | 85 %                 | 95 %                 | 90 %                          |

The special moves (`SpecialData`) carry absolute numbers, not percentages:

| Move (character)       | Start-up / active / recovery | Range | Damage | Knockback | Hit-stun | What makes it special |
| ---------------------- | ---------------------------: | ----: | -----: | --------: | -------: | --------------------- |
| Shoulder Rush (Chalk)  | 8 / 6 / 18                   | 60    | 12     | 48        | 20       | lunges 12 per tick until it recovers (168 units) |
| Fireball (Torch)       | 12 / 1 / 22                  | —     | 10     | 24        | 18       | throws a shot: 11 per tick for 70 ticks, 20 wide; 75-tick cooldown |
| Iron Palm (Brick)      | 14 / 4 / 26                  | 95    | 20     | 64        | 26       | armored start-up: hits hurt but do not stun or push |
| Slide Tackle (Zip)     | 4 / 8 / 18                   | 70    | 9      | 30        | 28       | lunges 15 per tick; high attacks pass over it until it recovers |

Balance levers, from the gentlest: a special's `cooldown` (only the fireball has one, so it
cannot be spammed), its recovery (what a blocked or dodged special costs), then the
percentages. The fireball's 1-tick active phase is only the throw: the shot itself lives
70 ticks. At time-up, characters with different health compare their health *shares*, so
Brick's bigger bar is not a time-out win in itself.

## Tuning the computer opponents

The bots share the `Bot` trait (`src/game/bot.rs`); the three computers are one
implementation, `Brawler`, tuned by `BrawlerConfig` (percentages are per relevant tick,
so "mash 25" means about 15 presses a second):

| Field                 | Standard | Patient | Aggressive | What it is                          |
| --------------------- | -------: | ------: | ---------: | ----------------------------------- |
| `mash_percent`        | 25       | 20      | 40         | grapple escapes (fresh presses)      |
| `counter_percent`     | 8        | 15      | 5          | countering a projection              |
| `reversal_percent`    | 30       | 20      | 40         | reversal glitch in a fresh clinch    |
| `top_strike_percent`  | 12       | 8       | 20         | striking while holding top position  |
| `guard_percent`       | 55       | 85      | 20         | guarding an incoming strike          |
| `jump_percent`        | 2        | 1       | 4          | jumping while walking in             |
| `decision_pause`      | 8–14     | 16–26   | 4–7        | ticks between two decisions in reach |

The presets live next to the struct (`BrawlerConfig::default`, `::PATIENT`,
`::AGGRESSIVE`); the rivals are listed by `Rival` and picked in the practice menu. A
new difficulty is a new const there — no strategy code to touch.

Every computer uses its character's special move: when it is ready, a decision in reach
whose roll falls in 60–69 picks it if it reaches (the lunge counts), and a fireball
thrower walking in throws one on 4 % of the ticks while on the enemy's line. In practice
the rival plays the next character every round (Chalk, Torch, Brick, Zip...).

## How to try a change

1. Edit the numbers in `MatchConfig::default()` (or the `BrawlerConfig` presets, or a
   character's `CharacterData` and special move in `src/game/character.rs`).
2. `cargo run` and fight it; `cargo test --workspace` must stay green (the game tests
   carry their own frame data, they are guarding the *rules*, not this balance).
3. Keep the contract in mind: totals are what players feel, singles are what tests
   count. When a number feeds a rule (an escape ladder, a counter window), check the
   tests named after it first (`tests/grappling.rs`, `tests/techniques.rs`).
