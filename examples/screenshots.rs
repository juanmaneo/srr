//! Plays scripted games headlessly and saves screenshots: no window, no GPU, no sound. This is
//! the game's "server-side rendering" (see `srr::front::render`). The games: practice against
//! the training dummy, a round against the computer, and an online 1v1v1 whose other two
//! players are simulated by handing the client their inputs, as the relay server would. The
//! strikes and the free-for-all are shot in both views (`Tab`: the third-person 3D view).
//!
//! ```sh
//! cargo xtask screenshots                       # or:
//! cargo run --example screenshots --no-default-features -- [OUT_DIR] [WIDTH] [HEIGHT]
//! ```
//!
//! PNG files go to `OUT_DIR` (default `target/screenshots`), at `WIDTH` x `HEIGHT` (default
//! 1280 x 720). The fight screen is also printed as text on the standard output.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use srr::front::intro::{COMPLETE_AT, SHATTER_AT};
use srr::front::{Game, Headless};
use srr::game::{Attack, Input, Key, MatchConfig, PlayerId, Strength};
use srr::net::{Client, PlayerSet, ServerMessage};

/// An online game seated as player 1 in a 1v1v1 that has just started, on the intro.
fn free_for_all(width: usize, height: usize) -> Headless {
    let players = PlayerSet::first(3);
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

/// One tick of the 1v1v1: players 2 and 3 hold `others`, the local player `keys`.
fn brawl_tick(game: &mut Headless, keys: &[Key], others: [Input; 2]) {
    let client = game.game_mut().client_mut();
    let tick = client.session().map_or(0, |session| session.tick());
    for (player, input) in (1..).zip(others) {
        client.handle(ServerMessage::Input {
            session: 1,
            player: PlayerId(player),
            tick,
            input,
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
    shoot(&game, "05-fight-bot-offer");
    // N: keep the training dummy for the scripted moves.
    game.tap(&[Key::N]);
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
    // The strikes pushed the dummy back: step in (collisions stop at body contact), then throw.
    game.hold(&[Key::ArrowRight], 20);
    game.hold(&[Key::Numpad8, Key::Numpad9], 1);
    game.idle(20);
    shoot(&game, "09-fight-ippon-seoi-nage");
    game.idle(40);
    shoot(&game, "10-fight-juji-gatame");

    // Y: the computer steps in and comes for the (idle) player.
    let mut cpu = Headless::practice().with_size(width, height);
    cpu.skip_intro();
    cpu.tap(&[Key::Y]);
    cpu.idle(MatchConfig::default().countdown_ticks + 75);
    shoot(&cpu, "11-versus-cpu");

    let mut brawl = free_for_all(width, height);
    brawl.skip_intro();
    let walk_left = Input {
        left: true,
        ..Input::NONE
    };
    let kick = Input::pressing(Attack::kick(Strength::Medium));
    for tick in 0..MatchConfig::default().countdown_ticks + 70 {
        let p3 = if tick % 25 == 0 { kick } else { walk_left };
        brawl_tick(&mut brawl, &[Key::ArrowRight], [Input::NONE, p3]);
    }
    shoot(&brawl, "12-free-for-all");
    brawl.tap(&[Key::Tab]);
    for _ in 0..40 {
        brawl_tick(&mut brawl, &[], [Input::NONE, Input::NONE]);
    }
    shoot(&brawl, "12-free-for-all-3d");

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
