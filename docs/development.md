# Development: internals, building and testing

How SRR is built, tested and served. The [README](../README.md) has the quick start.

## How the online match works

Every browser runs the same deterministic simulation from the same inputs; no game state is
ever sent. With ten players, somebody always has a bad connection, so nobody waits for
anybody:

* **Server frames.** Each browser sends its input for every tick (60 per second), scheduled
  2 ticks (33 ms) ahead. The server gathers the inputs of everybody into one frame per tick,
  sent as soon as all of them are there. When an input is late, the server waits a short
  grace period (8 ticks), then repeats that player's previous input and marks them late
  (`LAG`): only the laggard's fighter hesitates, and everybody gets the same frame.
* **Rollback.** A browser does not wait for the frames: it shows the match ahead of them,
  guessing that the others keep doing what they did, and when a frame proves a guess wrong it
  replays the ticks since then with the right inputs (up to 30 ticks, half a second; beyond
  that it waits, `Waiting for the server...`). A browser left behind (a paused tab) jumps
  straight to the frames.
* **Timing.** Every quarter of a second the server tells each browser how early its inputs
  arrive, at worst. The browser then runs a few ticks twice, or skips a few, to keep about
  two ticks of margin, whatever its round trip to the server and the drift of its clock.
* **Adaptive input delay.** Far from the server, running further ahead would mean guessing
  the others for too long (big rollbacks); the browser schedules its inputs further ahead
  instead (up to 30 ticks) and gives that delay back when the connection gets better.
* **The server never blocks.** Each connection has its own writer with a bounded queue: a
  connection that stops reading is cut off instead of stalling the room for the others.

`server/tests/network.rs` checks all of this with a deterministic simulation of ten browsers
behind cables, Wi-Fi, far links, freezing tabs and drifting clocks: everybody's confirmed
match is the same, and only the bad connections are ever late.

The protocol is a handful of binary WebSocket frames (integers little-endian; a set of
players is a `u16`, bit `i` for player `i`):

| Tag | Message        | Direction        | Fields                                        |
| --: | -------------- | ---------------- | --------------------------------------------- |
| 1   | `Welcome`      | server → browser | your seat / player `u8`                       |
| 2   | `Start`        | server → browser | session `u8`, players (set), then one character `u8` per player, by id |
| 3   | `Input`        | browser → server | session `u8`, tick `u32`, input `u16`         |
| 4   | `PlayerLeft`   | server → browser | player `u8`                                   |
| 5   | `RoomFull`     | server → browser | –                                             |
| 6   | `Lobby`        | server → browser | seated (set), ready (set)                     |
| 7   | `Ready`        | browser → server | character `u8` (0 Chalk, 1 Torch, 2 Brick, 3 Zip) |
| 8   | `Frame`        | server → browser | session `u8`, tick `u32`, players (set), late (set), then one input `u16` per player, by id |
| 9   | `Timing`       | server → browser | tick `u32`, lead `i8` (ticks of margin of the input for that tick) |

## The server

`srr-server` (package in `server/`) is a dependency-free, std-only HTTP + WebSocket server:

```sh
cargo run --release -p srr-server -- --dir dist --port 8080
```

| Option        | Default | Meaning                               |
| ------------- | ------- | ------------------------------------- |
| `--port PORT` | `8080`  | TCP port, listened on all interfaces  |
| `--dir DIR`   | `dist`  | directory with the web build to serve |
| `-h, --help`  |         | show the usage                        |

It serves the files of `DIR` on `GET`, the room (up to 10 players) on the WebSocket at
`/ws`, and the spectators on the WebSocket at `/watch` (the page opened with `?spectate`),
and runs the room's clock (60 ticks per second) that sends the frames of late players.
`Ctrl-C` stops it cleanly: the browsers are told with a close frame, the sockets are flushed,
and the process exits 0.

## Layout

* `src/game/` – the engine-agnostic rules: roster and alliances, the characters and their
  special moves (`character.rs`) and the fireballs in flight (`projectile.rs`), fixed-step
  deterministic match simulation, the six attacks and their frame data, blocking, crouching, push boxes, manual
  turning, the key bindings and their presets, grappling, the judo techniques and their counters, intentional glitches,
  the computer opponents (`bot.rs`: the `Bot` trait, the training dummy and the seeded
  brawler behind three tunings, listed by `Rival`) and the replays (`replay.rs`: the input
  frames of a match are the match).
  No engine, clock, thread or randomness, so it runs the same in the browser and on every
  machine of an online match.
* `src/net/` – the pure networking logic: the binary protocol, sets of players, the
  rollback session (`rollback.rs`) and the client state machine (lobby, readiness, timing and
  input delay, playing alone against a rival bot); no sockets, so it is fully
  unit-tested.
* `src/front/` – the screens as a pure state machine (`Game`): the graffiti ASCII intro, the
  character select screen (`select.rs`), the controls page (`controls.rs`), the on-screen
  touch controls for phones (`touch.rs`), the arena with the XKCD stick figures (`stick.rs`), the
  third-person 3D view (`tps.rs`, on the fixed-point 3D maths of `space.rs`), the stage of
  its glTF fighters (`model.rs`: clips, frame-data scrubbing, toon bands, the model audit),
  the glitches' looks (`glitch.rs`), the movements they share (`moves.rs`: the thirty things
  a fighter can be seen doing, for
  the stick figure, the sprite sheets and the clips), the chalk sprite sheet (`chalk.rs`) and
  the puppet placeholder (`puppet.rs`), the
  versus HUD and the music cues. Every frame is described as plain rectangles and
  monospace labels on a 1280×720 virtual screen (`Frame`), so it is fully unit-tested.
  `front/render.rs` is a software renderer for those frames (pixels, PNG, text) and
  `front/headless.rs` plays the `Game` without any engine (see [Headless rendering](#headless-rendering-tests-and-screenshots)).
* `src/audio/` – the acid techno synthesiser (pure DSP, deterministic, fully tested).
* `src/platform/` – engine-independent client glue: the browser WebSocket and the sound card
  output (`cpal`).
* `src/engine/` – the engine backends (`bevy.rs`, `fyrox.rs`): open the window, map the
  keyboard, tick the game 60 times per second and draw its frames. `bevy/stage.rs` (with
  `toon.wgsl` and `outline.wgsl`) renders the glTF fighters of the `mode3d` feature.
* `assets/models/` – the 3D fighters: one rigged `.glb` per character plus its skin textures,
  and one per manga girl in `manga/<variant>.glb` (see
  [docs/3d-asset-pipeline.md](3d-asset-pipeline.md)); `chalk/` holds the generated
  puppet.
* `tools/trellis/` – the one place that is **not Rust**: the offline scripts (Python, and
  Blender for the previews) that turn a manga sprite into a rigged `.glb` through the public
  TRELLIS Space and give it the puppet's movements. They only *make* the files of
  `assets/models/manga/`. The game, the build, `cargo xtask`, the tests and CI never run or
  need them, and what they produce is audited like every other model
  (`cargo xtask models`).
* `assets/skins/` – the sprite sheets: one hand-painted PNG per manga character
  (`manga/`), and the generated chalk (`chalk/chalk.png`, written by
  `cargo run --no-default-features --example sheets -- --write`).
* `assets/fonts/` – Fira Mono (SIL OFL), the monospace font of the Fyrox backend (Bevy embeds
  the same font).
* `server/` – `srr-server`: static files + the WebSocket room (`room.rs`, pure: seats,
  frames, timing reports) for LAN matches of 2 to 10 players; `server/tests/network.rs`
  simulates ten browsers over bad networks.
* `xtask/` – the `cargo xtask` build automation.
* `benches/` – the `criterion` benchmarks: the simulation (`Match::step` at 2, 5 and 10
  players), the music renderer and the protocol round-trip (`cargo bench`).
* `docs/` – the [tuning guide](tuning-guide.md): the frame data of the whole game
  (attacks, grappling, techniques), the bot tunings and the 3D view's knobs, explained for
  balance work; the [3D asset pipeline](3d-asset-pipeline.md).
* `web/index.html` – the page that loads the WASM build.

## Native (development)

```sh
cargo run                                   # alone: the training dummy, or the bot (T)
cargo run --features bevy/dynamic_linking   # faster incremental builds (native only)
cargo run --features mode3d                 # the glTF cel-shaded fighters in the 3D view (Tab)
```

On Linux the native build needs the ALSA headers for the music (`libasound2-dev` and
`pkg-config` on Debian / Ubuntu).

## Engines: Bevy or Fyrox

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

## `cargo xtask`

| Command                        | What it does                                                   |
| ------------------------------ | -------------------------------------------------------------- |
| `cargo xtask build`            | compile to `wasm32-unknown-unknown` and run `wasm-bindgen`     |
| `cargo xtask test`             | run every test of the workspace                                |
| `cargo xtask coverage`         | run the tests with `cargo llvm-cov`, fail below 100 % of the lines, regions or functions |
| `cargo xtask dev-check`        | everything a push must pass: `fmt --check`, clippy with `-D warnings`, the tests, then `coverage` |
| `cargo xtask dist`             | build and assemble the playable web build in `dist/`           |
| `cargo xtask serve`            | `dist`, then run `srr-server` on port 8080 for LAN matches     |
| `cargo xtask screenshots`      | play a scripted game headlessly, save PNGs in `target/screenshots` |
| `cargo xtask models`           | audit `assets/models/`: every GLB, its named clips and its skins |
| `cargo xtask docker-image`     | build the Docker image with the whole toolchain                |
| `cargo xtask docker <task>`    | run `build`, `test`, `coverage`, `dev-check`, `dist` or `serve` in that image |

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

## Podman (Buildah) instead of Docker

The `Dockerfile` builds with Podman (which uses Buildah) as well as with Docker; it needs
Buildah 1.33 / Podman 4.8 or newer, because the toolchain stage uses a heredoc. The base
images are fully qualified (`docker.io/library/...`), so no `registries.conf` short-name setup
is needed. Rootless Podman works: the project is owned by root inside the container, which is
you on the host, so `target/` and `dist/` still belong to you.

The playable image:

```sh
podman build -t srr .                       # or: buildah build -t srr .
podman run --rm -p 0.0.0.0:8080:8080 srr    # then open http://localhost:8080
```

With rootless Podman, publish the port on IPv4 (`0.0.0.0:`), as above. `srr-server` only
listens on IPv4. With a plain `-p 8080:8080`, Podman also accepts connections on IPv6
(`::1`) and forwards them to the container, where nobody is listening. `localhost` then
fails with "connection reset", while `http://127.0.0.1:8080` and the LAN address keep working.

`cargo xtask docker <task>` runs the `docker` command. Make `docker` run Podman (the
`podman-docker` package, or a `docker` symlink to `podman` on your `PATH`) and it works as
is. Or run the same commands yourself:

```sh
podman build --target toolchain -t srr-builder .
podman run --rm -v "$PWD":/work:z -v srr-cargo-registry:/usr/local/cargo/registry \
    -w /work srr-builder cargo xtask dist        # or build, test, coverage
podman run --rm --init -p 0.0.0.0:8080:8080 \
    -v "$PWD":/work:z -v srr-cargo-registry:/usr/local/cargo/registry \
    -w /work srr-builder cargo xtask serve
```

The `:z` suffix relabels the mount for SELinux (Fedora, RHEL); without SELinux, Podman
ignores it. `cargo xtask docker` does not add it, so on an SELinux host use the commands above.

### FreeBSD

On FreeBSD, Podman runs the image's Linux binaries directly through the Linuxulator (Linux
binary compatibility). You don't need a Linux VM under bhyve. Podman on FreeBSD runs as root.
One-time setup:

```sh
pkg install podman-suite
mount -t fdescfs fdesc /dev/fd                # and add it to /etc/fstab:
                                              # fdesc  /dev/fd  fdescfs  rw  0  0
sysrc linux_enable=YES && service linux start
cp /usr/local/etc/containers/pf.conf.sample /etc/pf.conf   # set your network interface in it
sysrc pf_enable=YES && service pf start
# Storage: on ZFS,
zfs create -o mountpoint=/var/db/containers zroot/containers
# otherwise set driver = "vfs" in /usr/local/etc/containers/storage.conf
```

Then build and run the Linux image with `--os=linux`:

```sh
podman build --os=linux -t srr .
podman run --rm --os=linux -p 8080:8080 srr
```

Published ports go through `pf`'s redirections, so other machines on the LAN reach
`http://<host-ip>:8080`. To reach them from the FreeBSD host itself, read the comments in
`pf.conf.sample` (`net.pf.filter_local`). The image builds and runs with Docker and with
rootless Podman on Linux. The FreeBSD steps follow the FreeBSD Podman documentation but have
not been tested with this project.

The engine backends (`src/engine/`), the platform glue (`src/platform/`) and the thin
`main.rs` files (game and server) are excluded from the coverage gate: they only draw, read
the keyboard, play samples, or open sockets and print, and are checked by playing. Everything else, including the server's HTTP / WebSocket handling over
real sockets, is covered by tests.

## Headless rendering (tests and screenshots)

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
game.skip_intro();                          // the training dummy is waiting (T: the computer)
game.hold(&[Key::ArrowRight], 60);
game.press(&[Key::Numpad8, Key::Numpad9]); // Ippon-seoi-nage
let png = game.screenshot().to_png();       // what a 1280x720 window would show
println!("{}", game.text_snapshot(160, 45)); // the same screen as text
```

`cargo xtask screenshots` runs `examples/screenshots.rs`. It plays the intro, opens the
controls page, shows the character select screen and the pause menu, plays a short fight against the dummy (crouch, strikes,
a flying kick into a 3-hit combo, ippon-seoi-nage, juji-gatame), a round against the
computer, an online 1v1v1 and a brawl of
ten (the other players simulated, one of the ten lagging), then each special move (Torch's
fireball in both views, Brick's palm, Zip's slide) and the phone's touch controls, headlessly, and writes PNGs to `target/screenshots/`. It is built with `--no-default-features`,
so no engine, window or GPU is needed: it works in CI or over SSH.

## Continuous integration

`.github/workflows/ci.yml` runs on every push and pull request:

- **dev-check**: the whole gate in one command (`cargo xtask dev-check`): `cargo fmt
  --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test
  --workspace`, then the coverage ratchet (`cargo xtask coverage`): 100 % of the lines,
  the regions and the functions. Mesa (llvmpipe/lavapipe) is installed first so the
  headless engine test of `tests/render.rs` renders for real on the software rasterizer
  instead of skipping
- **no-default-features**: `cargo test -p srr --no-default-features`, proving the rules,
  the protocol and the headless renderer build and pass without compiling any engine
  (the relay server's configuration)
