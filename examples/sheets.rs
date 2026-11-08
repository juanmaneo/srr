//! Writes the chalk sprite sheet: the stick figure of the arena in every movement of the
//! game, one PNG in the grid of the manga sheets (see `src/front/chalk.rs`).
//!
//! ```sh
//! cargo run --no-default-features --example sheets            # what the sheet holds
//! cargo run --no-default-features --example sheets -- --write # writes assets/skins/chalk/chalk.png
//! ```

use std::path::Path;
use std::process::ExitCode;

use srr::front::chalk::{self, CELL_HEIGHT, CELL_WIDTH};
use srr::front::moves::Move;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => {
            println!(
                "{} moves, {CELL_WIDTH}x{CELL_HEIGHT} cells, {}x{} sheet",
                Move::ALL.len(),
                chalk::SHEET_WIDTH,
                chalk::SHEET_HEIGHT
            );
            ExitCode::SUCCESS
        }
        [flag] if flag == "--write" => {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/skins/chalk/chalk.png");
            let png = chalk::png();
            let written = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&path, &png));
            match written {
                Ok(()) => {
                    println!("wrote {} ({} bytes)", path.display(), png.len());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("{}: {error}", path.display());
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: sheets [--write]");
            ExitCode::from(2)
        }
    }
}
