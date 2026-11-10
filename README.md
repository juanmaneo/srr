[![CI](https://github.com/juanmaneo/srr/actions/workflows/ci.yml/badge.svg)](https://github.com/juanmaneo/srr/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

# Shattered Realm: Rampage (SRR)

A versus fighting game in a broken world, made to push the limits of "vibe coding". It must:

* be written entirely in Rust;
* run in a browser (WASM);
* have 100 % test coverage;
* build, test, cover and package through `cargo xtask`;
* build anywhere through Docker.

**The glitches are the game.** In the spirit of BERP (the fictional trash game of
*Shangri-La Frontier*), the broken physics is a skill ceiling: every exploit is a move you
learn and combo, like a fireball. The glitches are documented, tested and switchable, and
on by default.

**Look and sound:** street brawls in back alleys and a live-generated 90s acid techno
soundtrack, in homage to *Streets of Rage*. The fighters are chalk stick figures in the
style of XKCD and *La Linea*, or optional manga skins. Street Fighter, Tekken and Mortal
Kombat are tone references only: no art, music or code is taken from any game.

---

## Playing

2 to 10 players on a local network, free-for-all, each in their own browser. One machine
hosts the game:

```sh
cargo xtask serve                          # or, with only Docker: cargo xtask docker serve
docker build -t srr . && docker run --rm -p 8080:8080 srr      # without Rust
```

Open the printed address (`http://<host-ip>:8080`) on every machine, then press `Enter`
on the fight screen when you are ready. The match starts when everybody in the room is
ready. A bad connection only slows its own fighter.

* **Alone:** `cargo run`, or `?practice` in the browser. Fight the training dummy or one of
  three computer opponents (`T` switches).
* **On a TV:** open `?spectate` to watch without taking a seat.
* **Phone or tablet:** turn it sideways, and on-screen controls appear.
* **PS4 / PS5 / Xbox pad:** plug it in and it works.

### Controls (default WASD layout)

| Action | Keys |
| --- | --- |
| Walk / jump / crouch | `A` `D` / `W` / `S` |
| Turn around | `S` + `A` or `S` + `D` (you never turn by yourself) |
| Block | walk backwards |
| Sidestep | `Q` / `E` |
| Punch low / medium / high | `B` / `H` / `J` |
| Kick low / medium / high | `G` / `Y` / `U` |
| Grab, then climb the grapple | `Space` |
| Judo sweep / throw | `H` + `U` / `Y` + `U` |
| Special move | `F` |
| Switch view (side / 3D) | `Tab` |
| Pause, change the controls | `P` (or `C` on the title screen) |

The **Standard** preset moves the attacks onto the arrows and the numeric keypad. Every key
can be remapped.

### Fighting

* **Strikes:** stronger attacks hit harder but are slower. A strike that lands chains
  into a stronger one (low → medium → high).
* **Grappling:** a grab leads to clinch → mount → submission. The fighter underneath
  escapes by mashing.
* **Judo:** a two-key throw ends in a pin or an armlock. The same two keys counter it.
* **Characters**, each with one special move:

  | Character | Style | Special move |
  | --- | --- | --- |
  | Chalk | all-rounder | Shoulder Rush |
  | Torch | zoner | Fireball |
  | Brick | heavy | Iron Palm (shrugs off hits) |
  | Zip | rushdown | Slide Tackle (under high attacks) |

* **Glitches:** recovery cancel, super jump, grab reversal, echo barrage (lag copies rain
  hits), stretch strike (rubber arms, flying fists), quick draw, crater slam and phantom
  warp.

### Looks

* **Skins:** chalk stick figures (the default) or ten manga girls drawn from sprite
  sheets. Each player picks their own, and the hosting server decides which styles are
  allowed (`--styles`).
* **3D view (`Tab`):** a third-person camera over your shoulder, drawn in software as
  wooden mannequins. With `--features mode3d` (Bevy only), it draws rigged, cel-shaded glTF
  fighters instead (see the [3D asset pipeline](docs/3d-asset-pipeline.md)).

Every rule, key and number: the [player manual](docs/manual.md) and the
[tuning guide](docs/tuning-guide.md).

---

## Building and testing

```sh
cargo run                        # native, alone against the dummy or the bot
cargo run --features mode3d      # with the glTF fighters in the 3D view
cargo xtask dev-check            # everything a push must pass
```

On Linux, the native build needs `libasound2-dev` and `pkg-config` for the music.

| Command | What it does |
| --- | --- |
| `cargo xtask build` | compile to WASM and run `wasm-bindgen` |
| `cargo xtask test` | run every test of the workspace |
| `cargo xtask coverage` | fail below 100 % of the lines, regions or functions |
| `cargo xtask dev-check` | `fmt`, clippy `-D warnings`, the tests, then coverage |
| `cargo xtask dist` / `serve` | the web build in `dist/` / served for LAN play |
| `cargo xtask screenshots` | play a scripted game headlessly and save PNGs |
| `cargo xtask models` | audit the 3D models in `assets/models/` |
| `cargo xtask docker <task>` | run a task in the toolchain image (`docker-image` builds it) |

The rules (`src/game/`), the netcode (`src/net/`) and the screens (`src/front/`) never touch
an engine. Bevy (the default) or Fyrox (`--no-default-features --features fyrox`) only draws
the frames, so everything is tested headlessly, without a window or a GPU.

CI (`.github/workflows/ci.yml`) runs `cargo xtask dev-check` and a build without default
features on every push.

The layout, the netcode and its protocol, the server, Podman and FreeBSD, and headless
rendering are covered in [docs/development.md](docs/development.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option. The bundled Fira Mono font is under the SIL
Open Font License 1.1 ([LICENSE-FiraMono.txt](assets/fonts/LICENSE-FiraMono.txt)).

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed
as above, without any additional terms or conditions.
