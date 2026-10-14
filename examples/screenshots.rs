//! Plays scripted games headlessly and saves screenshots: no window, no GPU, no sound. This is
//! the game's "server-side rendering" (see `srr::front::render`). The games: practice against
//! the training dummy, a round against the computer, an online 1v1v1 and an online brawl of
//! ten whose other players are simulated by handing the client the frames of inputs the server
//! would send (one of the ten lagging). The strikes and the free-for-all are shot in both views
//! (`Tab`: the third-person 3D view).
//!
//! ```sh
//! cargo xtask screenshots                       # or:
//! cargo run --example screenshots --no-default-features -- [OUT_DIR] [WIDTH] [HEIGHT]
//! ```
//!
//! PNG files go to `OUT_DIR` (default `target/screenshots`), at `WIDTH` x `HEIGHT` (default
//! 1280 x 720). The fight screen is also printed as text on the standard output.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use srr::front::intro::{COMPLETE_AT, SHATTER_AT};
use srr::front::{Game, Headless};
use srr::game::{Attack, Input, Key, MatchConfig, PlayerId, Strength};
use srr::net::{Client, PlayerSet, ServerMessage};

/// An online game seated as player 1 in a free-for-all of `count` players that has just
/// started, on the intro.
fn free_for_all(count: usize, width: usize, height: usize) -> Headless {
    let players = PlayerSet::first(count);
    let mut client = Client::online(MatchConfig::default(), 0);
    client.handle(ServerMessage::Welcome {
        player: PlayerId(0),
    });
    client.handle(ServerMessage::Lobby {
        seated: players,
        ready: players,
    });
    client.handle(ServerMessage::Start {
        session: 1,
        players,
    });
    Headless::new(Game::new(client)).with_size(width, height)
}

/// One tick of the free-for-all: the local player holds `keys`, players 2, 3... hold `others`,
/// and the inputs of the `late` players are marked late. The server's frames confirm every tick
/// up to the one about to be played, so the client never has to guess.
fn brawl_tick(game: &mut Headless, keys: &[Key], others: &[Input], late: PlayerSet) {
    let pressed: HashSet<Key> = keys.iter().copied().collect();
    let mut inputs = [Input::NONE; PlayerSet::CAPACITY];
    inputs[0] = game.game().bindings().to_input(&pressed);
    inputs[1..=others.len()].copy_from_slice(others);
    let client = game.game_mut().client_mut();
    let session = client.session().expect("the match runs");
    let (id, players) = (session.id(), session.players());
    for tick in session.confirmed_tick()..=session.tick() {
        client.handle(ServerMessage::Frame {
            session: id,
            tick,
            players,
            late,
            inputs,
        });
    }
    game.press(keys);
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().unwrap_or_else(|| "target/screenshots".into()));
    let mut size = args.map(|arg| arg.parse::<usize>());
    let (width, height) = match (size.next(), size.next()) {
        (None, _) => (1280, 720),
        (Some(Ok(width)), Some(Ok(height))) if width > 0 && height > 0 => (width, height),
        _ => {
            eprintln!("usage: screenshots [OUT_DIR] [WIDTH HEIGHT]");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = fs::create_dir_all(&out) {
        eprintln!("cannot create {}: {error}", out.display());
        return ExitCode::FAILURE;
    }

    let mut game = Headless::practice().with_size(width, height);
    let mut shots = Vec::new();
    let mut shoot = |game: &Headless, name: &str| shots.push((name.to_string(), game.screenshot()));

    game.idle(60);
    shoot(&game, "01-intro-spray");
    game.idle(SHATTER_AT + 4 - 60);
    shoot(&game, "02-intro-shatter");
    game.idle(COMPLETE_AT - SHATTER_AT - 4);
    shoot(&game, "03-intro-title");

    game.tap(&[Key::C]);
    game.tap(&[Key::ArrowDown]);
    shoot(&game, "04-controls");
    game.tap(&[Key::Escape]);

    game.tap(&[Key::Enter]);
    // P: the pause menu, with the practice row; P again resumes against the training dummy.
    game.tap(&[Key::P]);
    shoot(&game, "05-pause-menu");
    game.tap(&[Key::P]);
    game.idle(60);
    shoot(&game, "06-fight-countdown");
    game.idle(60);
    game.hold(&[Key::ArrowDown], 10);
    shoot(&game, "07-fight-crouch");
    game.hold(&[Key::ArrowRight], 70);
    for _ in 0..4 {
        game.tap(&[Key::Numpad5]);
        game.idle(20);
    }
    shoot(&game, "08-fight-strikes");
    // Tab: the same moment over the shoulder; the camera takes a few ticks to glide in.
    game.tap(&[Key::Tab]);
    game.idle(40);
    shoot(&game, "08-fight-strikes-3d");
    game.tap(&[Key::Tab]);
    // Step back, jump in, and kick near the top of the jump so that the kick comes down
    // active on the dummy; land, then jab and chain the straight: a 3-hit combo.
    game.hold(&[Key::ArrowLeft], 10);
    game.hold(&[Key::ArrowUp, Key::ArrowRight], 1);
    game.hold(&[Key::ArrowRight], 13);
    game.hold(&[Key::ArrowRight, Key::Numpad4], 6);
    shoot(&game, "08-fight-flying-kick");
    game.hold(&[Key::ArrowRight], 3);
    game.hold(&[Key::Numpad1], 1);
    game.idle(3);
    game.hold(&[Key::Numpad5], 1);
    game.idle(5);
    shoot(&game, "08-fight-combo");
    // The strikes pushed the dummy back: step in (collisions stop at body contact), then throw.
    game.hold(&[Key::ArrowRight], 20);
    game.hold(&[Key::Numpad8, Key::Numpad9], 1);
    game.idle(20);
    shoot(&game, "09-fight-ippon-seoi-nage");
    game.idle(40);
    shoot(&game, "10-fight-juji-gatame");

    // T: the computer takes over the dummy and comes for the (idle) player.
    let mut cpu = Headless::practice().with_size(width, height);
    cpu.skip_intro();
    cpu.tap(&[Key::T]);
    cpu.idle(MatchConfig::default().countdown_ticks + 75);
    shoot(&cpu, "11-versus-cpu");

    let mut brawl = free_for_all(3, width, height);
    brawl.skip_intro();
    let walk_left = Input {
        left: true,
        ..Input::NONE
    };
    let walk_right = Input {
        right: true,
        ..Input::NONE
    };
    let kick = Input::pressing(Attack::kick(Strength::Medium));
    let nobody = PlayerSet::EMPTY;
    for tick in 0..MatchConfig::default().countdown_ticks + 70 {
        let p3 = if tick % 25 == 0 { kick } else { walk_left };
        brawl_tick(&mut brawl, &[Key::ArrowRight], &[Input::NONE, p3], nobody);
    }
    shoot(&brawl, "12-free-for-all");
    brawl.tap(&[Key::Tab]);
    for _ in 0..40 {
        brawl_tick(&mut brawl, &[], &[Input::NONE, Input::NONE], nobody);
    }
    shoot(&brawl, "12-free-for-all-3d");

    // Ten players: everybody walks towards a neighbour and kicks now and then; P7 lags.
    let mut ten = free_for_all(10, width, height);
    ten.skip_intro();
    let lagging = PlayerSet::EMPTY.with(PlayerId(6));
    for tick in 0..MatchConfig::default().countdown_ticks + 60 {
        let others: Vec<Input> = (1..10u32)
            .map(|player| match (tick + player * 7) % 30 {
                0 => kick,
                _ if player % 2 == 0 => walk_left,
                _ => walk_right,
            })
            .collect();
        brawl_tick(&mut ten, &[Key::ArrowRight], &others, lagging);
    }
    shoot(&ten, "13-brawl-of-ten");
    ten.tap(&[Key::Tab]);
    for _ in 0..30 {
        brawl_tick(&mut ten, &[], &[Input::NONE; 9], lagging);
    }
    shoot(&ten, "13-brawl-of-ten-3d");

    for (name, image) in &shots {
        let path = out.join(format!("{name}.png"));
        if let Err(error) = fs::write(&path, image.to_png()) {
            eprintln!("cannot write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        println!("wrote {}", path.display());
    }
    println!("\n{}", game.text_snapshot(160, 45));
    ExitCode::SUCCESS
}
