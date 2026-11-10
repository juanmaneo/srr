# Player manual: Shattered Realm: Rampage

The full rules and controls. The [README](../README.md) has the short version; the numbers behind every move are in the [tuning guide](tuning-guide.md).

## Controls

Each player plays on their own keyboard and can pick their own layout on the controls page
(press `C` on the title screen, or `P` to pause during a fight, see
[Keyboard layout](#keyboard-layout)). The default (**WASD**) puts the attacks on the letters;
the **Standard** preset moves them onto the arrows and the numeric keypad.

| Action                | Standard preset | WASD (default) |
| --------------------- | ------------------ | ----------- |
| Walk left / right     | `←` / `→`          | `A` / `D`   |
| Sidestep to the wall / the street | keypad `7` / `2` | `Q` / `E` |
| Jump                  | `↑`                | `W`         |
| Crouch                | `↓` (hold)         | `S` (hold)  |
| Turn around           | `↓` + `←` / `↓` + `→` | `S` + `A` / `S` + `D` |
| Low / medium / high punch (fist) | keypad `1` / `5` / `6` | `B` / `H` / `J` |
| Low / medium / high kick         | keypad `4` / `8` / `9` | `G` / `Y` / `U` |
| Grab / advance a grapple         | keypad `0`             | `Space`         |
| Judo sweep → hon-gesa-gatame      | keypad `5` + `9` together | `H` + `U` together |
| Judo throw → ude-hishigi-juji-gatame | keypad `8` + `9` together | `Y` + `U` together |
| Special move (your [character](#characters)'s power) | keypad `3`  | `F`             |
| Counter a judo technique         | the same two keys, while being thrown | same |
| Escape a grapple                 | mash any key           | same |
| Controls page (title screen)     | `C`                    | same |
| Pause the fight / change the controls | `P`               | same |
| Switch the view (side / third-person 3D) | `Tab`          | same |
| Skip the title screen   | `Enter` or `Esc` | same |
| Ready for the next match (lobby, or after a round) | `Enter` | same |
| Practice: go practising (title screen), start a practice fight while waiting online, switch the rival (dummy / computer) | `T` | same |
| Leave a practice fight played while online | `Esc` | same |

* **Low, Medium, High** are damage tiers. A stronger attack hits harder and pushes further,
  but takes longer to come out and to recover from, so it is easier to punish. Kicks reach
  further and hit a little harder than punches of the same tier, but are slower.
  With several attack keys held, the strongest wins (a kick beats a punch of the same tier).
* **Your fighter never turns by itself: press `↓` + `←` or `↓` + `→` to face that way.**
  Fighters start facing each other. Jump over the opponent (or let it walk around you) and
  you keep looking the old way, with your back to it, until you turn. The two keys can come
  in either order (holding `↓` then pressing a direction, or the other way round); holding
  them does nothing more, and `←` + `→` together face nowhere. Turning is instant, works on
  the ground and in the air (where you keep drifting), and can be combined with an attack on
  the same tick (the turn comes first); it is ignored, not delayed, while attacking,
  grabbing, stunned or grappling.
* **Hold `↓` to crouch.** A crouching fighter squats on the spot: it does not walk and does
  not block, but **high punches and high kicks pass over its head**; low and medium attacks
  still hit it with full damage. Attacking, grabbing, the judo techniques and jumping work
  from a crouch (they take priority over it), and landing with `↓` held lands crouching.
* **There is no block button: you block by stepping back**, Tekken style: holding the
  direction *away* from where you face walks backwards with the guard up. A hit landing while
  you step back away from it is blocked: it does a fraction of the damage, no stun and no
  push. Standing still, walking forward, jumping or attacking does not block. Facing the
  wrong way, "back" is towards the opponent, so turn first.
* **Fighters are solid**: you cannot walk through the opponent. Walking into it pushes both
  of you (each moves half the overlap), and against a wall it does not budge. You can still
  **jump over** it: a fighter high enough above the other passes over its body, and lands
  on the other side if it got past the middle.
  Knocked-out and grappling fighters have no push box.
* **Sidestep to dodge**, Tekken style: keypad `7` steps into the alley, towards the wall,
  keypad `2` out, towards the street (`Q` / `E` in WASD). Everybody starts on the middle line.
  Strikes, grabs and judo techniques only reach a little way across, so **a short sidestep
  (150 ms) takes you out of their line**: too slow to dodge a jab on reaction, enough for a
  high kick seen coming. Fighters far enough apart across walk past each other. A grab or a technique pulls its catch onto the catcher's line.
  Sidestepping works on the floor only, together with walking (and stepping back still
  blocks); jumping, crouching and attacking take priority over it. The side view shows the
  depth like the belt scrollers: a fighter further back stands higher and is drawn behind.
* **Kick from the air.** A kick pressed while jumping is a flying kick (punches, grabs and the
  judo techniques need the floor). It keeps the kick's numbers and flies on with the jump,
  steering included; strikes only reach a little way up or down, so **press it near the top
  of the jump to come down active** during the last steps of the fall. Landing ends the kick
  whatever its phase: pressed too late, it never comes out (a key still held kicks again
  from the floor). A flying kick comes from above, so even a high one hits a crouching
  fighter; stepping back still blocks it.
* **Combos: chain into a stronger strike.** Once a strike has landed (neither blocked nor
  whiffed), holding a stronger attack key cancels the rest of it into that strike at once:
  low → medium → high, punch or kick in any mix (keypad `1` → `5` → `9`), skipping a tier
  if you like; in the air only a stronger kick chains. Nothing chains after a high strike,
  and a blocked first hit leaves you in the full recovery: block the jab and punish. With the
  default numbers every next-tier link lands inside the stun of the previous hit, so jab,
  straight, high kick is a true 3-hit combo as long as the knockback has
  not pushed the opponent out of reach (start close: the punch chain needs you within 58
  units, the kick chain within 83), while skipping straight to the high kick lets the
  opponent recover just in time. A flying kick that connects opens the combo: you land free
  and jab at once. The hits a fighter takes without recovering are counted under its bar
  (`3 HITS`), whoever lands them.
* The figure turns white (with stars around the head) while stunned, and grey once knocked
  out; blocking raises the hands. The striking limb stretches to the attack's range in the
  colour of its strength (yellow, orange, red). The yellow marker shows which fighter is
  yours. A grab shows as green arms, a judo technique as blue limbs, a submission as a red
  locked limb.

## Playing on a phone or a tablet

Open the same address in the phone's browser (`http://<host-ip>:8080`, or `?practice` to play
alone) and **turn the phone sideways**. As soon as you touch the screen, on-screen controls
appear (a keyboard player never sees them):

* on the title: **START** and **PRACTICE**;
* on the select screen: **tap a character's card** to choose it (tap it again to fight),
  **OK** and **BACK**; in the pause menu: a directional pad (the arrows), **OK** and
  **BACK**;
* in the fight: the **floating stick** on the left (left / right to walk, up to jump,
  down to crouch; push it diagonally for down + a direction to turn around, or up + a
  direction to jump sideways), **WALL** / **STREET** to sidestep next to it; on the right
  the six attacks (**LK MK HK** above, **LP MP HP** below), **GRAB** and **SPECIAL**; at
  the top **BACK**, **READY** (between rounds), **RIVAL** (practice), **VIEW** (3D) and
  **PAUSE**.

The controls are translucent and the stick is a smooth joystick (a ring with a knob that
follows the thumb), so the fight is always in view. Like GeForce Now's virtual controller,
they can be hidden and brought back: the small **TOGGLE** button (or the `X` key) flips
them; while they are hidden only that button stays in the corner.

Several fingers press several buttons, so the judo techniques work as on a keyboard (**MP** +
**HK** for the sweep, **MK** + **HK** for the throw). The buttons press the keys of your
layout, so phones and keyboards can play in the same room. Added to the home screen, the page
opens full screen like an app. The controls are drawn by the game itself (`src/front/touch.rs`),
like every other screen, and tested headlessly.

## Playing with a PS4 / PS5 controller

Plug in a **DualShock 4** (PS4) or a **DualSense** (PS5) — or pair it over Bluetooth — and it
just works, natively and in the browser (the browser's Gamepad API: press any button once so
the page sees the pad). Any standard pad does: an Xbox controller maps the same way, with
A B X Y instead of the shapes. The mapping (`src/front/gamepad.rs`) follows the on-screen
controls of a phone:

| Action                     | PS4 / PS5 (or Xbox) pad                    |
| -------------------------- | ------------------------------------------ |
| Walk left / right           | left stick or D-pad `◀` / `▶` (Xbox: `◀` / `▶`) |
| Jump                       | left stick or D-pad `▲`                    |
| Crouch / turn around       | left stick or D-pad `▼` (diagonals turn)    |
| Low / medium / high punch  | `Square` / `Triangle` / `R1` (Xbox: `X` / `Y` / `RB`) |
| Low / medium / high kick   | `Cross` / `Circle` / `R2` (Xbox: `A` / `B` / `RT`) |
| Grab / advance a grapple   | `L1` (Xbox: `LB`)                           |
| Sidestep to the wall / the street | `L3` / `R3` (the stick clicks)        |
| Special move               | `L2` (Xbox: `LT`)                           |
| Judo sweep / throw         | `Triangle` + `R2` / `Circle` + `R2`         |
| Confirm / ready            | `Cross` (Xbox: `A`)                          |
| Back / leave a practice fight | `Circle` (Xbox: `B`); `Share` on the fight screen |
| Pause / resume             | `Options` (Xbox: `Menu`)                    |
| Controls page (title screen) / practice | `Triangle` / `Square` on the title (Xbox: `Y` / `X`) |

A thumb resting on the stick presses nothing (a worn stick does not drift the fighter
around), every pad connected steers the same fighter, and the buttons press the keys of your
layout, so a pad and a keyboard can play in the same room. `Tab` (switching the view) stays on
the keyboard; the PS button belongs to the console / the browser / the OS.

## Skins (the styles and the variants)

Every fighter can wear a **skin**: how it is drawn, nothing to do with the rules. The styles
are two, and a match can mix them freely — a chalk stick figure against a full-colour
heroine, or a brawl of ten fighters all dressed differently:

* **Stick (chalk)**, the default: the game's own ink on the alley wall.
* **Manga**: the ten fighters as girls of the KonoSuba school — bright cel shading, big
  glossy eyes, bold ink outlines, twintails, braids and manes, fighting dresses and
  thigh-high stockings (in the broad tradition of sprite brawlers like Phantom Breaker:
  Battle Grounds; nothing copied from either: every face, haircut and outfit is the
  game's own).

Each player wears their **own** variant (the ten players of a brawl each look different, like
the accessories of the chalk ink), picked on the **Skin** row of the controls page / pause
menu (`←` / `→`); the choice takes effect **immediately**: locally at once, online the server
tells every browser in the same round, so the whole room sees the change together. Every
fighter keeps the game's poses, marks and effects whatever it wears — a skin is a layer over
the ink, never a replacement.

**The styles on offer are decided by the server hosting the game** (`--styles stick,manga`
on the command line of `srr-server`, all of them by default; the chalk is always allowed). A
style the server does not allow never reaches the menus.

Each manga fighter is **one PNG sprite sheet** in `assets/skins/manga/`: every movement of
the character — the guard, the two walk frames, the punch, the kick, the crouch, the jump,
the hit recoil, the knockout — in one 3 x 3 grid in a fixed order, the same cell layout for
all ten. The game decodes the PNG itself (a small DEFLATE decoder, no dependency) and slices
the grid deterministically (`src/front/skin.rs`), so both engines and the headless tests draw
the same art; every cell is normalised to the same box with the fighter seated on the same
floor, the reason a manga girl and a chalk stick can trade blows at the same on-screen size.
One image per character is the point: a model asked for one pose at a time drifts (hair
colour, hem length, broken legs), one sheet keeps the fighter identical across its
movements. The 3D view (`Tab`) reads the same palettes for its cel-shaded mannequins.

## Characters

Every player picks a character on the **select screen** that follows the title (`Left` /
`Right` to choose, `Enter` to fight, `Esc` back to the title), and can change it at any time
from the **Character** row of the controls page / pause menu. Each character has its own
numbers ([tuning guide](tuning-guide.md#characters-percentages-and-special-moves)) and one
**special move** on keypad `3` (`F` in WASD):

| Character | Special move |
| --------- | ------------ |
| **Chalk**, the all-rounder | **Shoulder Rush**: a lunging charge across the alley |
| **Torch**, the zoner | **Fireball**: a flaming shot that flies across the arena, with a cooldown |
| **Brick**, the heavy (more health, slower) | **Iron Palm**: a slow, heavy palm that shrugs off hits while it winds up |
| **Zip**, the rushdown (less health, faster) | **Slide Tackle**: a low slide that passes under high attacks |

* A special move works from the floor only, and like a strike it can be **blocked by
  stepping back** and is **cancelled by a hit** — except the Iron Palm, which takes the
  hits of its start-up without flinching (they still hurt; a grab still beats it).
* **Combos**: a strike that connected can be cancelled into the special move, like into a
  stronger strike (jab → straight → shoulder rush). Nothing chains after a special move.
* **The fireball** flies along Torch's line: **step back** to block it, **sidestep** out of
  its line or **jump** over it. It vanishes on its first hit, and two fireballs meeting
  head-on put each other out. Until Torch can throw the next one, the orange gauge along
  the bottom of its health bar refills.
* **The slide** keeps Zip under high punches and high kicks until it recovers; low and
  medium attacks still catch it.
* The bars, the name tags (`P1 Torch`) and the health count follow each character; at
  time-up, fighters with different health compare their health *shares*, so a full Brick
  and a full Zip draw.
* Online, your pick is sent to the server when you get ready (`Enter`) and every browser
  plays the characters the match started with; picking another one while ready tells the
  server at once. In practice the round restarts at once with the new character, and the
  computer plays another character every round.

A new character is one more entry in `src/game/character.rs`.

## Keyboard layout

Press `C` on the title screen to open the controls page, or `P` at any time during a fight:
the fight **pauses** and the same page opens as the pause menu (titled PAUSED), so the layout
can be changed whenever you want, not only before playing.

* `↑` / `↓` choose a row (the arrows always drive this page, whatever the layout);
* on the **Preset** row, `←` / `→` (or `Enter`) switch between **WASD (letters + space)**
  (the default) and **Standard (arrows + keypad)**;
* on an action row, `Enter` then the new key remaps it (`Esc` cancels). A key already used by
  another action swaps with it, so a key never does two things. `Enter`, `Esc`, `P`, `Tab` and
  `T` cannot be bound (they drive the menus, the readiness for the next match, the pause, the
  view and practice). The preset row then reads **Custom**;
* on the **Character** row, `←` / `→` (or `Enter`) choose your
  [character](#characters): alone the round restarts at once with it, online it is used
  for the next match;
* on the **Practice** row, `←` / `→` (or `Enter`) choose who to practise against: **the
  computer**, **the patient computer**, **the aggressive computer** or **the training
  dummy** (and, online, **off**: no practice fight, the room only); the change is carried
  out at once, even in the middle of a round. See
  [Playing alone](#playing-alone-the-training-dummy-or-the-computers);
* `Esc` (or `Enter` on "Back to the title") goes back to the title; from the pause menu, `Esc`,
  `P` or `Enter` on "Resume the fight" goes back to the fight.

Pausing freezes a practice fight. An online match cannot wait for one player: it goes on while
the pause menu is open, with your fighter standing still (the others see it idle), so pause
between rounds when you can.

The layout is local to your machine: each player picks their own, and the in-fight controls
reminder and the `COUNTER` prompts name your keys. Keys are **physical positions**, so the
WASD preset sits in the same place on an AZERTY keyboard (`Z` `Q` `S` `D` there). The layout
is saved in the browser and comes back after a reload.

## Title screen

The game opens on an animated ASCII-art title: "SHATTERED" and "REALM:" are spray-painted in
cold neon on a basement brick wall, then "RAMPAGE" in hot, slanted, dripping letters, Streets
of Rage style. The title then *shatters* (flash, glitching rows, chromatic split, cracks that
stay), the tagline is typed and "PRESS ENTER" blinks; a glitch burst keeps the wall alive
every few seconds. Press `Enter` or `Esc` to choose your [character](#characters), then
the fight, or `C` for the controls page
(hinted in the top-left corner; the sound status is in the top-right one). Online, a match
starts once every player in the room is on the fight screen and has pressed `Enter`.

## Third-person 3D view (`Tab`)

`Tab` switches the fight, at any moment, between the side view (chalk stick figures on the
alley wall) and a third-person view over the shoulder of your fighter, like the brawls of
Tekken's story mode. The camera glides behind you and looks towards the others, so a crowded
free-for-all stays readable.

The 3D is drawn by the game itself, the old-school way (`src/front/space.rs`,
`src/front/tps.rs`): integer fixed-point maths, a quaternion camera, the painter's algorithm
and everything snapped to a 2-pixel grain, for the look of a 90s console. The figures become
wooden mannequins (every bone a tube, every joint a ball) in a comic-book style: flat two-tone
shading and dark ink outlines, in a colour palette evoking the Streets of Rage title screen
(a nod to its mood; no palette or asset is copied from it). Like every screen,
the result is only rectangles and labels, so both engines draw it unchanged. The rules are
the same in both views; this is where sidesteps read best: the mannequins stand on the line
they stepped to, and the camera follows yours.

**Cel-shaded 3D fighters (`--features mode3d`, Bevy only).** With the `mode3d` feature, a
character that has a rigged glTF model in `assets/models/<name>/<name>.glb` is drawn as that
model instead of its mannequin. The model uses a cel-shading material (crisp, cool-tinted
shadow bands, a stepped highlight and a rim light) and a coloured ink outline, and wears the texture of the fighter's manga
variant (`skins/<variant>.png`). A manga girl who has a model of her own
(`assets/models/manga/<variant>.glb`, made from her sprite by TRELLIS) is drawn as her, in
her own texture; the others are that puppet in her colours. Its animation follows the fighter's state: named clips
(`idle`, `walk_f`, `punch`, `ko`...) blend into each other over 0.1 s, and the attack clips
are scrubbed by the move's frame data, so the contact pose lands on the active frames. The
models are drawing only. The rules, the hitboxes, the replays and online play are
unchanged, and a missing or broken model only brings that character's mannequin back. The
models come from an AI image-to-3D pipeline fed with the manga skins
([docs/3d-asset-pipeline.md](3d-asset-pipeline.md)). Until then, Chalk is a procedurally
generated wooden puppet, Pinocchio style, tubes and balls with a grain texture
(`src/front/puppet.rs`), which is also the CI fixture. It moves exactly like the stick figure
of the arena: every keyframe of its 25 clips is a pose of the stick figure, the ground
positions of a grapple included, and the stick figure's sprite sheet
(`assets/skins/chalk/chalk.png`, `src/front/chalk.rs`) shows the same thirty movements in
the grid of the manga sheets.

## HUD

Like in every versus game: in a 1v1, player 1's health bar on the left, player 2's on the
right, both emptying towards the round timer in the middle. In bigger matches the bars get
thinner and stack, the first half of the players on the left and the rest on the right (up
to five a side in a brawl of ten, tinier beyond four players). Recent damage stays
visible as a red trail that drains after half a second, so a combo reads at a glance; the
bar blinks orange below 25 %. Under each bar: name, character and HP (`P1 (you) Torch`,
`P3 Brick`, or `CPU Zip` for the computer), round wins as pips, the combo counter (`2 HITS`, `3 HITS`..., hotter with every
hit: the hits that fighter took without recovering, kept for three quarters of a second
after it recovers), and during a grapple either the position ladder
(`[CLINCH] > mount > submission`, or `[IPPON-SEOI-NAGE] > juji-gatame` for a judo technique)
for the fighter on top, or for the one underneath the escape gauge (in the air: the counter
to enter, e.g. `IPPON-SEOI-NAGE!  COUNTER 8+9`). Big graffiti words announce READY, FIGHT!,
KO and TIME; between matches the banner shows the lobby (who is here, who is ready, who
left). Online, a bar reads `LAG` while the server stands in for that player's late inputs
(their fighter repeats its moves) and `LEFT` once they left the match (their fighter stands
still); when the server itself is not heard from, the banner says `Waiting for the server...`.

## Grappling (BJJ / MMA)

Press keypad `0` next to the opponent to grab; a whiff leaves you open. A connected grab
locks both fighters in a **clinch** and the fighter who grabbed is on **top**. The top climbs
the ladder **clinch → mount (takedown) → submission** with a fresh press of grab once it has
held the position long enough, and strikes with a fresh punch or kick press. The submission
drains health until the tap-out (a KO). The numbers are in the
[tuning guide](tuning-guide.md#grappling-the-ladder).

* The bottom escapes by mashing: every tick with any fresh key press scores a point, and each
  rung of the ladder needs more points. Breaking free pushes the fighters apart and stuns the
  top.
* The referee stands both fighters up after a while in the same position, and a hit from a
  third fighter breaks the hold.
* Grabs are unblockable (stepping back does not help), but a strike beats a grab: getting hit
  during the start-up cancels it. Stunned or airborne fighters cannot be grabbed.

## Judo techniques (grab and counter-grab)

Two keypad keys pressed **simultaneously** (at most 3 ticks, 50 ms, apart) throw the
opponent. The first key usually starts its own attack; the second one, arriving while that
attack still winds up, turns it into the technique. If one press completes both
combinations, an attack already started turns into the technique that uses its key;
otherwise the sweep wins.

| Keys | Technique | Projection (in the air) | Finish on the ground |
| ---- | --------- | ----------------------- | -------------------- |
| `5` + `9` (medium punch + high kick) | aerial foot sweep | a flying **juji-gatame** leg kick scissors the opponent and sweeps it off its feet | **Hon-gesa-gatame** (scarf-hold pin) |
| `8` + `9` (medium kick + high kick)  | grab and projection | **Ippon-seoi-nage**, the one-arm shoulder throw (te-waza) | **Ude-hishigi-juji-gatame** (cross armbar) |

The sweep is quicker and reaches further; the throw hits harder and is harder to escape
([numbers](tuning-guide.md#grappling-the-ladder)).

* Like a grab, a technique is unblockable, a strike during its start-up cancels it, and it
  only catches fighters on the floor that are neither stunned nor grappling. A whiff cannot
  be cancelled: it is very punishable.
* Nobody can escape during the projection. On landing, the thrower holds the finish: it
  drains like the submission until the tap-out, an escape (mash any key), or the referee's
  stand-up.
* **Counter-grabbing needs the exact same combination**: `5` + `9` counters `5` + `9`, `8` +
  `9` counters `8` + `9`. Either enter it during the first ticks of the projection, or be
  winding up the same technique when the opponent's connects: the roles are reversed and you
  throw the thrower. A counter cannot be countered again. Another combination, a grab or
  mashing does nothing in the air. If both fighters' same technique becomes active on the same
  tick, they clash and bounce apart.

## Music

The soundtrack is synthesised live (no audio file), in pure Rust, and follows the fight:

* a distorted 909-style kick with an off-beat sub rumble, hats and claps, a dark drone, and a
  303-style acid line (sawtooth into a resonant 4-pole ladder filter, with accents and
  slides) in A phrygian;
* 138 BPM on the title screen, 145 BPM in the fight (the tempos of the reference mix); the
  acid pattern mutates a few steps every 8 bars, so it evolves but stays recognisable;
* the filter opens and the acid line gets more resonance, accents and drive as the round gets
  tense (low health, a grapple heading for a submission, the last ten seconds), and the
  music drops to a breakdown between rounds.

The native build starts the music with the title screen. Browsers only allow sound after a
user gesture, so in the browser the music starts on the first key press or click in the
page. The top-right corner of the title screen tells where the sound is at:
`sound: press a key or click`, `sound: on`, or `sound: no audio output` (the reason is printed
on stderr natively, in the browser console on the web). No sound? Check that the tab is not
muted, click into the page, and look at the console.

## Rounds

The frame data of every attack (60 ticks per second) is in the
[tuning guide](tuning-guide.md#the-six-attacks); those are
Chalk's numbers, and the other [characters](#characters) scale damage and range.

A round lasts 99 seconds after a 2-second countdown. It ends when only one fighter is still
standing, or at time-up (most health wins). Every player then presses `Enter` to get ready
("Waiting for P2, P3 to get ready..."); the next round starts when everybody in the room is
ready, including players who arrived meanwhile ("P4 joins the next round"). Round wins are
counted on the scoreboard as long as the same players keep fighting.

## Playing alone: the training dummy or the computers

A practice fight is a 1v1 against a **rival** played by the game, as player 2:

* **the training dummy** (`P2`), which never moves: try moves on it;
* **the computer** (`CPU`), the bot. It walks in, sidesteps back onto your line when you
  dodge, picks strikes, grabs and both judo techniques, chains every strike that lands into
  the next tier (a combo), kicks on the way down when it jumps in, steps back to block or
  crouches under high attacks (never under a flying kick), turns around with `↓` + direction
  like you, mashes out of grapples, climbs to the submission, and sometimes counters a judo
  throw with the same two keys. It is deterministic (seeded, no real randomness), so it is
  fully tested;
* **the patient computer** and **the aggressive computer**: the same brawler with another
  tuning (`BrawlerConfig`, see the [tuning guide](tuning-guide.md)). The patient one
  guards nearly everything, waits for its openings and escapes grapples with method, but
  jumps rarely and never rushes in; the aggressive one decides twice as often, presses hard
  in the grapples and jumps at you, but guards only one strike in five.

All four are bots behind the same `Bot` trait (`src/game/bot.rs`), listed by `Rival`;
switching the rival just swaps the bot, in the middle of the round if you like, and a new
strategy is a new implementation.

* **Practice mode** (`cargo run`, or `?practice` in the browser) is nothing but a practice
  fight: `Enter` (or `T`) on the title screen starts it against the training dummy. The
  notice under the banner says who you fight and that **`T` switches to the next rival**;
  the **Practice** row of the pause menu (`P`) does the same.
* **Online**, while waiting in the room (alone, or for the others to get ready), `T` starts a
  practice fight against the computer; `T` again goes through the patient and aggressive
  computers and the dummy, then back to the room.
  `T` on the title screen goes straight to it. The fight runs only in your browser while the
  room stays open: when someone joins, the notice says so ("P2 is here - Esc: leave the
  practice"); `Esc` brings you back to the room, where `Enter` gets you ready for the real
  match, and a real match that starts takes over anyway. The pause menu's Practice row offers
  the same choices (the three computers, the dummy, off). During an online match the row reads
  "not during a match" and `T` does nothing.

After a round alone, `Enter` starts the next one at once.

The intentional glitches are on by default — each one is a learned move, not an
accident, and each can be switched off in the configuration:

| Glitch | What it does |
| --- | --- |
| **Recovery cancel** | a jump pressed during an attack's *recovery* throws the rest of it away and takes off at once |
| **Super jump** | a jump pressed again on the very tick you land re-launches you, higher than a normal jump (held down, you keep bouncing) |
| **Reversal** | pressing grab on the very first tick you are grabbed swaps the roles (not during a judo projection, which has its own counter) |
| **Echo barrage** | a punch that lands unblocked desyncs the attacker: lag copies of it appear in a ring around the victim and rain extra hits, one per tick, from every direction — they cannot be blocked, but they only hurt: no push, no stun |
| **Stretch strike** | your arms lengthen: every punch reaches further than its frame data says, and a punch that whiffs its way to the end of its timeline breaks its fist off at full stretch. The floating fist keeps existing and can still hit — blockable, like the punch that lost it |
| **Quick draw** | a punch pressed while a punch is in its recovery restarts a fresh punch at once, whichever strength is held: held down, the button keeps the fists flying — a flurry no recovery can slow down |
| **Crater slam** | a judo throw that lands does not just hurt the thrown fighter: the impact quakes the ground, and everybody standing near the landing point — allies included — loses health, no push, no stun: the floor does the damage |
| **Phantom warp** | an unblocked hit taken in the air desyncs your model: besides the knockback, you snap further along the direction the hit travels |

**What the glitches look like** (`src/front/glitch.rs`, the same in the arena and in the 3D
view, whatever the fighters wear, and only while the rules of the glitch are on):

* **Echo barrage**: the victim is ringed by lag copies of itself, ghosts flickering between
  cyan and magenta, turning round it while the echoes rain; the ring thins out as they run
  out.
* **Phantom warp**: the model leaves fading afterimages between the place it stood and the
  place the hit snapped it to.
* **Stretch strike**: the stick figure's arm reaches as far as the rules say, a sprite or a 3D
  model gets a rubber arm over it, and the fist that breaks off at full stretch floats where
  it snapped.
* **Crater slam**: the landing of a judo throw leaves a crater in the street, a dark pit with
  glowing cracks and a shock ring running out to the edge of the quake, for about two thirds
  of a second.

The rules expose just enough for that, read-only (`Fighter::echoes`, `Fighter::warp`,
`Match::craters`, like the shots in flight): the looks never feed back into the simulation,
so replays and online matches stay exactly as deterministic.


## Spectating: the game on a TV

Open the page with `?spectate` on the machine hooked to the big screen
(`http://<host-ip>:8080/?spectate`): it takes no seat, so a full party of ten can play while
the tenth machine shows the game to the room. The spectator's page skips the title and the
select screens and shows the fights as they are — the arena, the versus HUD, the banners, the
music — with new matches showing up on their own, so nobody has to touch the TV once it is
set up:

* the page connects to the server's `/watch` endpoint instead of `/ws`, and the server marks
  it a spectator: it is told everything the players are told (the room, the matches, every
  frame, the skins, the departures) and nothing it sends is listened to;
* it plays the frames of the server exactly, never guessing anything: the fight on the TV is
  always the confirmed one, so it never rolls back or hesitates;
* several pages may spectate at once (say, one per screen of the room) — they cost the
  server one connection each, like a player;
* a spectator arriving in the middle of a match sees it from the frames on (`Tab` switches
  to the third-person view, which stands behind a fighter still standing, since a spectator
  owns nobody).

A knocked-out player's third-person camera does the same: it leaves their fighter lying on
the pavement and follows the fight that goes on, so being eliminated from a brawl means
watching the rest of it from the corner, not staring at the floor.
