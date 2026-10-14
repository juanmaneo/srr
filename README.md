Shattered Realm: Rampage (SRR): Emphasizes a chaotic, broken world built for intense combat encounters.

This is a game created to test and push the limits of "vibe coding".

It MUST be entirely written in Rust.
It MUST run in a browser in WASM.
It MUST have 100% test coverage.
It MUST have a `cargo xtask` pattern for build, tests, coverage, and distribution.
It MUST have a Docker setup to be able to build on any machine (via Docker).

It is a versus type of game that can be played 1v1 (2 players) or 1v1v1 (3 players) or 2v1 (3 players).
The goal is to be able to play up to 10 players online (on a local network in the first iterations) with any kind of alliance or as full brawler.

The lore is inspired by games like Street Fighter, Tekken, Mortal Kombat but the idea of on purpose "glitches" to exploit comes from BERP (BErseRk online Passion), a fictional game in the manga/anime "Shangri-La Frontier".

The street look comes from **Streets of Rage**: like its cover art, the title is splashed over
the city as graffiti, the fights happen in back alleys, and the soundtrack is club music — here
a 90s dark acid techno groove generated live, as if played in a New York basement. The
grappling borrows from Brazilian jiu-jitsu and MMA (clinch, takedown, mount, submission) and
from judo: two-key throws that end in a pin or an armlock.

**Art direction:** the fighters are single-line stick figures in the style of **XKCD** and
Osvaldo Cavandoli's **"La Linea"**: a hollow round head, one line for the body, two-segment
arms and legs, in chalk on the alley wall. Every player wears an XKCD-ish accessory in their
colour (P1 Black Hat Guy's top hat, P2 Beret Guy's beret, P3 a ponytail, P4 a headband), so
the figures stay apart with the same ink. The graphics will be reworked later; the stick
figures already have a pose for every state (guard, walk, crouch, jump, each strike, grabs,
the judo throws and every grapple position).

---

## Current iteration: 1v1, 1v1v1 or 1v1v1v1 on the local network

2 to 4 players, each on their own machine, each in a web browser, on the same local network,
**everybody for themselves** (free-for-all). One machine runs the small SRR server, which
serves the game and relays the inputs; every player opens its address in a browser. A match
starts when everybody in the room has pressed `Enter`: two players make a 1v1, three a
1v1v1, four a 1v1v1v1. The rules engine already supports up to 10 players and alliances
(2v1…), which later iterations will open up.

Alone? The game **offers** a match against the computer (the bot) and waits for your answer
(`Y` / `N`); see [Playing alone](#playing-alone-the-training-dummy-or-the-computer).

### Controls

Each player plays on their own keyboard and can pick their own layout on the controls page
(press `C` on the title screen, or `P` to pause during a fight, see
[Keyboard layout](#keyboard-layout)). The default (**Standard**) puts the attacks on the
numeric keypad; the **WASD** preset is for keyboards without one.

| Action                | Standard (default) | WASD preset |
| --------------------- | ------------------ | ----------- |
| Walk left / right     | `←` / `→`          | `A` / `D`   |
| Jump                  | `↑`                | `W`         |
| Crouch                | `↓` (hold)         | `S` (hold)  |
| Turn around           | `↓` + `←` / `↓` + `→` | `S` + `A` / `S` + `D` |
| Low / medium / high punch (fist) | keypad `1` / `5` / `6` | `B` / `H` / `J` |
| Low / medium / high kick         | keypad `4` / `8` / `9` | `G` / `Y` / `U` |
| Grab / advance a grapple         | keypad `0`             | `Space`         |
| Judo sweep → hon-gesa-gatame      | keypad `5` + `9` together | `H` + `U` together |
| Judo throw → ude-hishigi-juji-gatame | keypad `8` + `9` together | `Y` + `U` together |
| Counter a judo technique         | the same two keys, while being thrown | same |
| Escape a grapple                 | mash any key           | same |
| Controls page (title screen)     | `C`                    | same |
| Pause the fight / change the controls | `P`               | same |
| Switch the view (side / third-person 3D) | `Tab`          | same |
| Skip the title screen   | `Enter` or `Esc` | same |
| Ready for the next match (lobby, or after a round) | `Enter` | same |
| Answer "Fight the bot?" (alone) | `Y` yes / `N` no | same |
| Leave an online match against the computer | `Esc` | same |

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
  you step back away from it is blocked: it does a quarter of the damage, no stun and no
  push. Standing still, walking forward, jumping or attacking does not block. Facing the
  wrong way, "back" is towards the opponent, so turn first.
* **Fighters are solid**: you cannot walk through the opponent. Walking into it pushes both
  of you (each moves half the overlap), and against a wall it does not budge. You can still
  **jump over** it: a fighter high enough above the other passes over its body (push box
  50 × 90 units by default), and lands on the other side if it got past the middle.
  Knocked-out and grappling fighters have no push box.
* Attacks only work from the ground; you can steer left/right in the air.
* The figure turns white (with stars around the head) while stunned, and grey once knocked
  out; blocking raises the hands. The striking limb stretches to the attack's range in the
  colour of its strength (yellow, orange, red). The yellow marker shows which fighter is
  yours. A grab shows as green arms, a judo technique as blue limbs, a submission as a red
  locked limb.

### Keyboard layout

Press `C` on the title screen to open the controls page, or `P` at any time during a fight:
the fight **pauses** and the same page opens as the pause menu (titled PAUSED), so the layout
can be changed whenever you want, not only before playing.

* `↑` / `↓` choose a row (the arrows always drive this page, whatever the layout);
* on the **Preset** row, `←` / `→` (or `Enter`) switch between **Standard (arrows + keypad)**
  and **WASD (letters + space)**;
* on an action row, `Enter` then the new key remaps it (`Esc` cancels). A key already used by
  another action swaps with it, so a key never does two things. `Enter`, `Esc`, `P` and `Tab`
  cannot be bound (they drive the menus, the readiness for the next match, the pause and the
  view; `Y` / `N`
  only answer the bot offer while it is shown, so they stay bindable). The preset row then
  reads **Custom**;
* `Esc` (or `Enter` on "Back to the title") goes back to the title; from the pause menu, `Esc`,
  `P` or `Enter` on "Resume the fight" goes back to the fight.

Pausing freezes a match against the dummy or the computer. An online match cannot wait for
one player: it goes on while the pause menu is open, with your fighter standing still (the
others see it idle), so pause between rounds when you can.

The layout is local to your machine: each player picks their own, and the in-fight controls
reminder and the `COUNTER` prompts name your keys. Keys are **physical positions**, so the
WASD preset sits in the same place on an AZERTY keyboard (`Z` `Q` `S` `D` there). The layout
lasts until the page is reloaded (it is not saved yet).

### Title screen

The game opens on an animated ASCII-art title: "SHATTERED" and "REALM:" are spray-painted in
cold neon on a basement brick wall, then "RAMPAGE" in hot, slanted, dripping letters, Streets
of Rage style. The title then *shatters* (flash, glitching rows, chromatic split, cracks that
stay), the tagline is typed and "PRESS ENTER" blinks; a glitch burst keeps the wall alive
every few seconds. Press `Enter` or `Esc` to go to the fight, or `C` for the controls page
(hinted in the top-left corner; the sound status is in the top-right one). Online, a match
starts once every player in the room is on the fight screen and has pressed `Enter`.

### Third-person 3D view (`Tab`)

`Tab` switches the fight, at any moment, between the side view (chalk stick figures on the
alley wall) and a third-person view over the shoulder of your fighter, like the brawls of
Tekken's story mode. The camera glides behind you and looks towards the others, so a crowded
free-for-all stays readable.

The 3D is drawn by the game itself, the old-school way (`src/front/space.rs`,
`src/front/tps.rs`): integer fixed-point maths, a quaternion camera, the painter's algorithm
and everything snapped to a 2-pixel grain, for the look of a 90s console. The figures become
wooden mannequins (every bone a tube, every joint a ball) in a comic-book style: flat two-tone
shading and dark ink outlines, in the Streets of Rage colours of the title. Like every screen,
the result is only rectangles and labels, so both engines draw it unchanged. The rules are
the same in both views: fighters still move along one line (sidestepping is not in the rules
yet).

### HUD

Like in every versus game: in a 1v1, player 1's health bar on the left, player 2's on the
right, both emptying towards the round timer in the middle. In a 1v1v1 or 1v1v1v1 the bars
get thinner and stack: P1 and P2 on the left, P3 (and P4) on the right. Recent damage stays
visible as a red trail that drains after half a second, so a combo reads at a glance; the
bar blinks orange below 25 %. Under each bar: name and HP (`P1 (you)`, `P3`, or `CPU` for
the computer), round wins as pips, and during a grapple either the position ladder
(`[CLINCH] > mount > submission`, or `[IPPON-SEOI-NAGE] > juji-gatame` for a judo technique)
for the fighter on top, or for the one underneath the escape gauge (in the air: the counter
to enter, e.g. `IPPON-SEOI-NAGE!  COUNTER 8+9`). Big graffiti words announce READY, FIGHT!,
KO and TIME; between matches the banner shows the lobby (who is here, who is ready, who
left), and a late input names the player the match waits for (`Waiting for P3...`).

### Grappling (BJJ / MMA)

Press keypad `0` next to the opponent to grab (start-up 4 ticks, active 2, range 55; a whiff
leaves you open for 20 ticks). A connected grab locks both fighters in a **clinch** and the
fighter who grabbed is on **top**:

| Position   | Top (attacker)                                             | Bottom (defender)          |
| ---------- | ---------------------------------------------------------- | -------------------------- |
| Clinch     | strikes: 3 damage; grab again → takedown (8 damage)         | escape: 4 points            |
| Mount      | ground strikes: 4 damage; grab again → submission          | escape: 7 points            |
| Submission | drains 2 health every 12 ticks until the tap-out (a KO)   | escape: 10 points           |

* The top advances with a fresh press of grab once it held the position for 0.75 s, and
  strikes with a fresh punch or kick press (at most one every 15 ticks).
* The bottom escapes by mashing: every tick with any fresh key press scores a point. Breaking
  free pushes the fighters apart and stuns the top for 20 ticks.
* The referee stands both fighters up after 5 s in the same position, and a hit from a third
  fighter breaks the hold.
* Grabs are unblockable (stepping back does not help), but a strike beats a grab: getting hit
  during the start-up cancels it. Stunned or airborne fighters cannot be grabbed.

### Judo techniques (grab and counter-grab)

Two keypad keys pressed **simultaneously** (at most 3 ticks, 50 ms, apart) throw the
opponent. The first key usually starts its own attack; the second one, arriving while that
attack still winds up, turns it into the technique. If one press completes both
combinations, an attack already started turns into the technique that uses its key;
otherwise the sweep wins.

| Keys | Technique | Projection (in the air) | Finish on the ground |
| ---- | --------- | ----------------------- | -------------------- |
| `5` + `9` (medium punch + high kick) | aerial foot sweep | a flying **juji-gatame** leg kick scissors the opponent and sweeps it off its feet | **Hon-gesa-gatame** (scarf-hold pin) |
| `8` + `9` (medium kick + high kick)  | grab and projection | **Ippon-seoi-nage**, the one-arm shoulder throw (te-waza) | **Ude-hishigi-juji-gatame** (cross armbar) |

| Default numbers | Sweep `5`+`9` | Throw `8`+`9` |
| --------------- | ------------: | ------------: |
| Start-up / active / whiff recovery (ticks) | 6 / 3 / 24 | 8 / 2 / 28 |
| Range | 75 | 65 |
| Projection (counter window) | 24 ticks (first 12) | 30 ticks (first 12) |
| Landing damage | 10 | 14 |
| Escape from the finish | 9 points | 11 points |

* Like a grab, a technique is unblockable, a strike during its start-up cancels it, and it
  only catches fighters on the floor that are neither stunned nor grappling. A whiff cannot
  be cancelled: it is very punishable.
* Nobody can escape during the projection. On landing, the thrower holds the finish: it
  drains like the submission (2 health every 12 ticks) until the tap-out, an escape (mash any
  key), or the referee's stand-up after 5 s.
* **Counter-grabbing needs the exact same combination**: `5` + `9` counters `5` + `9`, `8` +
  `9` counters `8` + `9`. Either enter it during the first ticks of the projection, or be
  winding up the same technique when the opponent's connects: the roles are reversed and you
  throw the thrower. A counter cannot be countered again. Another combination, a grab or
  mashing does nothing in the air. If both fighters' same technique becomes active on the same
  tick, they clash and bounce apart.

### Music

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

Default frame data (60 ticks per second, arena 1200 units wide, 100 health):

| Attack       | Start-up | Active | Recovery | Range | Damage | Knockback | Hit-stun |
| ------------ | -------: | -----: | -------: | ----: | -----: | --------: | -------: |
| Low punch    | 3        | 2      | 6        | 80    | 5      | 12        | 10       |
| Medium punch | 5        | 3      | 10       | 85    | 9      | 20        | 14       |
| High punch   | 8        | 3      | 16       | 90    | 14     | 32        | 18       |
| Low kick     | 5        | 3      | 9        | 105   | 7      | 16        | 12       |
| Medium kick  | 7        | 3      | 13       | 115   | 11     | 26        | 16       |
| High kick    | 11       | 4      | 20       | 125   | 17     | 40        | 22       |

A round lasts 99 seconds after a 2-second countdown. It ends when only one fighter is still
standing, or at time-up (most health wins). Every player then presses `Enter` to get ready
("Waiting for P2, P3 to get ready..."); the next round starts when everybody in the room is
ready, including players who arrived meanwhile ("P4 joins the next round"). Round wins are
counted on the scoreboard as long as the same players keep fighting.

### Playing alone: the training dummy or the computer

When you are alone (practice mode, or nobody else in the online room), the fight screen asks
**"Nobody else is here. Fight the bot?  Y: yes  N: no"**, and nothing happens until you
answer: the computer never starts a fight without your confirmation.

* `Y`: the computer (`CPU`) plays player 2. It walks in, picks strikes, grabs and both judo
  techniques, steps back to block or crouches under high attacks, turns around with
  `↓` + direction like you, mashes out of grapples, climbs to the submission, and sometimes
  counters a judo throw with the same two keys. It is deterministic (seeded, no real
  randomness), so it is fully tested.
* `N` in practice: you keep the training dummy, which never moves.
* `N` online: you keep waiting in the room; `Y` still starts a match against the computer
  later. That match runs only in your browser while the room stays open: when someone joins,
  a notice says so ("P2 is here - Esc: leave the bot"); `Esc` brings you back to the room,
  where `Enter` gets you ready for the real match.

After a round alone, `Enter` starts the next one at once.

The intentional glitches are on: cancelling an attack's recovery with a jump, a super
jump when jumping again on the very tick you land, and the grapple **reversal**: pressing
grab on the very first tick you are grabbed swaps the roles (not during a judo projection,
which has its own counter).

### Play on your local network (2 to 4 players)

On the machine that hosts the game (it can also be one of the players):

```sh
cargo xtask serve            # or, with only Docker installed: cargo xtask docker serve
```

Without Rust at all (only Docker, e.g. on NixOS), plain `docker` does the same:

```sh
docker build -t srr .
docker run --rm -p 8080:8080 srr
```

This builds the web version into `dist/` and starts the server on port 8080. It prints the
address to use, for example:

```text
Shattered Realm: Rampage - serving dist
  this machine:  http://localhost:8080
  local network: http://192.168.1.42:8080   <- open this on every machine
```

1. Open `http://<host-ip>:8080` in a browser on each machine, up to four (the host can use
   `http://localhost:8080`). Click into the page so it gets the keyboard.
2. Leave the title screen (`Enter`). On the fight screen, press `Enter` when you are ready:
   the banner shows who is in the room and who is ready. The match starts as soon as
   everybody in the room (at least two) is ready: 1v1, 1v1v1 or 1v1v1v1.
3. Somebody who opens the page during a match waits in the room and joins the next round
   once everybody presses `Enter` again. A fifth browser is told the room is full.
4. If a player leaves, the match they were in stops for the others ("P3 left"), who go back
   to the room and press `Enter` for a new match. Whoever is left alone is offered the bot.

To try it on a single computer, open several browser windows (or a private window, or
another browser) on `http://localhost:8080`; each window is a player.

Notes:

* Allow incoming TCP connections on port 8080 in the host's firewall
  (e.g. `sudo ufw allow 8080/tcp`).
* With `cargo xtask docker serve` or `docker run`, the address printed is the container's;
  use the host machine's LAN address instead (`ip addr` / `ipconfig`).
* With the default layout, keep Num Lock on to be safe. Without a numeric keypad (laptops),
  pick the WASD preset on the controls page (`C` on the title screen).
* Keep the game tab in the foreground: a hidden tab stops sending inputs, and the match
  waits for it ("Waiting for P2...").
* Practice alone, without the server's room, by opening `http://<host-ip>:8080/?practice`:
  you fight the training dummy, or the computer if you answer `Y`. `cargo run` (native
  window) also plays alone.

How the online match works: every browser runs the same deterministic simulation in
lock-step. Each one sends its input for every tick (60 per second) to the server, which only
forwards it to the other players of the match; a tick is simulated once the inputs of
everyone are known. Inputs are scheduled 3 ticks (50 ms) ahead, which hides the latency of a
local network.

The protocol is a handful of binary WebSocket frames (integers little-endian; a set of
players is one byte, bit `i` for player `i`):

| Tag | Message        | Direction        | Fields                                        |
| --: | -------------- | ---------------- | --------------------------------------------- |
| 1   | `Welcome`      | server → browser | your seat / player `u8`                       |
| 2   | `Start`        | server → browser | session `u8`, players `u8` (set)              |
| 3   | `Input`        | browser → server | session `u8`, tick `u32`, input `u16`         |
| 3   | `Input`        | server → browser | session `u8`, player `u8`, tick `u32`, input `u16` |
| 4   | `PlayerLeft`   | server → browser | player `u8`                                   |
| 5   | `RoomFull`     | server → browser | –                                             |
| 6   | `Lobby`        | server → browser | seated `u8` (set), ready `u8` (set)           |
| 7   | `Ready`        | browser → server | –                                             |

### The server

`srr-server` (package in `server/`) is a dependency-free, std-only HTTP + WebSocket server:

```sh
cargo run --release -p srr-server -- --dir dist --port 8080
```

| Option        | Default | Meaning                               |
| ------------- | ------- | ------------------------------------- |
| `--port PORT` | `8080`  | TCP port, listened on all interfaces  |
| `--dir DIR`   | `dist`  | directory with the web build to serve |
| `-h, --help`  |         | show the usage                        |

It serves the files of `DIR` on `GET` and the room (up to 4 players) on the WebSocket at
`/ws`.

---

## Building and testing

### Layout

* `src/game/` – the engine-agnostic rules: roster and alliances, fixed-step deterministic match
  simulation, the six attacks and their frame data, blocking, crouching, push boxes, manual
  turning, the key bindings and their presets, grappling, the judo techniques and their counters, intentional glitches,
  and the computer opponent (`bot.rs`, seeded).
  No engine, clock, thread or randomness, so it runs the same in the browser and on every
  machine of an online match.
* `src/net/` – the pure networking logic: the binary protocol, sets of players, the N-player
  lock-step session and the client state machine (lobby, readiness, playing alone against the
  dummy or the bot); no sockets, so it is fully unit-tested.
* `src/front/` – the screens as a pure state machine (`Game`): the graffiti ASCII intro, the
  controls page (`controls.rs`), the arena with the XKCD stick figures (`stick.rs`), the
  third-person 3D view (`tps.rs`, on the fixed-point 3D maths of `space.rs`), the
  versus HUD and the music cues. Every frame is described as plain rectangles and
  monospace labels on a 1280×720 virtual screen (`Frame`), so it is fully unit-tested.
  `front/render.rs` is a software renderer for those frames (pixels, PNG, text) and
  `front/headless.rs` plays the `Game` without any engine (see [Headless rendering](#headless-rendering-tests-and-screenshots)).
* `src/audio/` – the acid techno synthesiser (pure DSP, deterministic, fully tested).
* `src/platform/` – engine-independent client glue: the browser WebSocket and the sound card
  output (`cpal`).
* `src/engine/` – the engine backends (`bevy.rs`, `fyrox.rs`): open the window, map the
  keyboard, tick the game 60 times per second and draw its frames.
* `assets/fonts/` – Fira Mono (SIL OFL), the monospace font of the Fyrox backend (Bevy embeds
  the same font).
* `server/` – `srr-server`: static files + the WebSocket room and relay for LAN matches of 2
  to 4 players.
* `xtask/` – the `cargo xtask` build automation.
* `web/index.html` – the page that loads the WASM build.

### Native (development)

```sh
cargo run                                   # alone: the training dummy, or the bot (Y)
cargo run --features bevy/dynamic_linking   # faster incremental builds (native only)
```

On Linux the native build needs the ALSA headers for the music (`libasound2-dev` and
`pkg-config` on Debian / Ubuntu).

### Engines: Bevy or Fyrox

The game is engine-agnostic: everything but `src/engine/<engine>.rs` is shared, and a backend
only implements the small `Engine` trait (open a window, call `Shell::tick` at 60 Hz with the
keys held, draw `Shell::frame`). The engine is chosen at compile time:

| Engine          | Command                                              | Web build |
| --------------- | ---------------------------------------------------- | --------- |
| Bevy (default)  | `cargo run`                                          | yes       |
| Fyrox           | `cargo run --no-default-features --features fyrox`   | native only for now |

When both features are enabled, Bevy wins. The relay server never compiles an engine. `cpal`
is pinned to 0.17.1 because Fyrox's audio links the same `alsa-sys` version (Cargo allows a
single crate per `links = "alsa"`).

### `cargo xtask`

| Command                        | What it does                                                   |
| ------------------------------ | -------------------------------------------------------------- |
| `cargo xtask build`            | compile to `wasm32-unknown-unknown` and run `wasm-bindgen`     |
| `cargo xtask test`             | run every test of the workspace                                |
| `cargo xtask coverage`         | run the tests with `cargo llvm-cov`, fail below 100 % of lines |
| `cargo xtask dist`             | build and assemble the playable web build in `dist/`           |
| `cargo xtask serve`            | `dist`, then run `srr-server` on port 8080 for LAN matches     |
| `cargo xtask screenshots`      | play a scripted game headlessly, save PNGs in `target/screenshots` |
| `cargo xtask docker-image`     | build the Docker image with the whole toolchain                |
| `cargo xtask docker <task>`    | run `build`, `test`, `coverage`, `dist` or `serve` in that image |

Native requirements for the non-Docker tasks: the `wasm32-unknown-unknown` target,
`wasm-bindgen-cli` (same version as the `wasm-bindgen` crate in `Cargo.lock`) and
`cargo-llvm-cov`. With Docker only Docker itself is needed, e.g. `cargo xtask docker dist`.
`cargo xtask docker serve` publishes port 8080 of the container. The container runs as the
owner of the mounted project, so the `target/` and `dist/` it writes belong to you, not to
root, and the native tasks keep working afterwards.

The `Dockerfile` has two targets. `cargo xtask docker` uses its `toolchain` stage (the Rust
toolchain and the tools above, nothing else) and mounts the project into it. Its default
target is a ready-to-run image for machines without Rust: it copies the project in, runs
`cargo xtask dist`, compiles `srr-server`, and serves `dist/` on port 8080:

```sh
docker build -t srr .                 # compiles the game, long the first time
docker run --rm -p 8080:8080 srr      # same as `cargo xtask serve`
```

`dist/` must be served over HTTP (WASM cannot be loaded from `file://`); `srr-server` does it,
and only it provides the `/ws` endpoint needed to play online. Any other static server is fine
for `?practice`.

The engine backends (`src/engine/`), the platform glue (`src/platform/`) and the thin
`main.rs` files (game and server) are excluded from the coverage gate: they only draw, read
the keyboard, play samples, or open sockets and print, and are checked by playing. Everything else, including the server's HTTP / WebSocket handling over
real sockets, is covered by tests.

### Headless rendering (tests and screenshots)

The tests never show anything on screen: no window opens, no sound plays, and the xtask
process tests discard the output of the commands they run (`SystemExecutor::quiet`).
`tests/project_layout.rs` enforces it.

Screens are still tested as a player sees them, the way
[Blitz](https://github.com/DioxusLabs/blitz) renders Dioxus apps server-side without a
browser:

* `srr::front::render` is a software renderer for the same `Frame` that Bevy and Fyrox draw.
  It follows the same contract: background, letterboxing with `Viewport`, rectangles then
  labels with alpha blending, a built-in 5×7 font on the monospace grid. It outputs
  * a `Pixmap`, with `to_png()` (a dependency-free PNG encoder),
  * or a `text_snapshot` (shaded characters, readable in a terminal or in an assertion).
* `srr::front::Headless` is a third, headless "engine". It ticks the real `Game` with scripted
  keys and renders it in memory.

```rust
use srr::front::Headless;
use srr::game::Key;

let mut game = Headless::practice();
game.skip_intro();
game.tap(&[Key::N]);                        // keep the training dummy
game.hold(&[Key::ArrowRight], 60);
game.press(&[Key::Numpad8, Key::Numpad9]); // Ippon-seoi-nage
let png = game.screenshot().to_png();       // what a 1280x720 window would show
println!("{}", game.text_snapshot(160, 45)); // the same screen as text
```

`cargo xtask screenshots` runs `examples/screenshots.rs`. It plays the intro, opens the
controls page, shows the bot offer, plays a short fight against the dummy (crouch, strikes,
ippon-seoi-nage, juji-gatame), a round against the computer and an online 1v1v1 (the other
two players simulated), headlessly, and writes PNGs to `target/screenshots/`. It is built with `--no-default-features`,
so no engine, window or GPU is needed: it works in CI or over SSH.
