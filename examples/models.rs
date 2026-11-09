//! Audits the 3D models of `assets/models/` and writes the puppet placeholder: no engine,
//! no window (see `docs/3d-asset-pipeline.md`).
//!
//! ```sh
//! cargo xtask models                                                # audit, or:
//! cargo run --example models --no-default-features                  # the same
//! cargo run --example models --no-default-features -- --write chalk # the placeholder
//! cargo run --example models --no-default-features -- --clips       # the movements as JSON
//! ```
//!
//! The audit lists, for every character, its model (`models/<name>/<name>.glb`), the clips it
//! carries and the skin textures of `models/<name>/skins/`; it exits with 1 when a model is
//! broken, misses a clip or has a stray skin (a missing model or skin is only noted: the
//! mannequin, or the model's own texture, stands in).
//!
//! `--clips` prints the skeleton and the keyframes of every clip as JSON, the movements that
//! `tools/trellis/rig.py` puts on a model of the image-to-3D pipeline.
//!
//! `--write` followed by character names (or none, for every character without a real model)
//! writes the puppet as their model and the ten manga palettes as its skins. A real model
//! (anything the puppet did not generate) is never overwritten.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use srr::front::model::{self, Severity};
use srr::front::puppet;
use srr::game::{Character, Variant};

/// The asset root: `assets/` of the project.
fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets")
}

/// Every file under `assets/models/`, as paths relative to the asset root with `/`.
fn list(dir: &Path, prefix: &str, files: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{prefix}{name}");
        if entry.path().is_dir() {
            list(&entry.path(), &format!("{path}/"), files);
        } else {
            files.push(path);
        }
    }
}

fn audit() -> ExitCode {
    let root = assets();
    let mut files = Vec::new();
    list(&root.join("models"), "models/", &mut files);
    let paths: Vec<&str> = files.iter().map(String::as_str).collect();
    let audit = model::audit(&paths, |path| fs::read(root.join(path)).ok());
    for (severity, line) in &audit.lines {
        let tag = match severity {
            Severity::Info => "      ",
            Severity::Problem => "ERROR ",
        };
        println!("{tag}{line}");
    }
    if audit.passed() {
        println!("models: OK");
        ExitCode::SUCCESS
    } else {
        println!("models: problems found (see ERROR lines above)");
        ExitCode::FAILURE
    }
}

/// Writes the puppet and its skins for `character`, unless a real model is there.
fn write(character: Character) -> Result<(), String> {
    let root = assets();
    let model = root.join(model::model_path(character));
    if let Ok(existing) = fs::read(&model)
        && !model::generator_of(&existing)
            .as_deref()
            .is_some_and(puppet::made_here)
    {
        return Err(format!(
            "{}: a real model is there, not overwriting it",
            model.display()
        ));
    }
    let io = |error: std::io::Error| format!("{}: {error}", model.display());
    fs::create_dir_all(root.join(model::model_dir(character)).join("skins")).map_err(io)?;
    fs::write(&model, puppet::glb()).map_err(io)?;
    println!("wrote {}", model.display());
    for variant in Variant::ALL {
        let skin = root.join(model::skin_path(character, variant));
        fs::write(&skin, puppet::skin(variant))
            .map_err(|error| format!("{}: {error}", skin.display()))?;
        println!("wrote {}", skin.display());
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((flag, names)) = args.split_first() else {
        return audit();
    };
    if flag == "--clips" {
        println!("{}", puppet::clips_json());
        return ExitCode::SUCCESS;
    }
    if flag != "--write" {
        eprintln!("usage: models [--write [character...] | --clips]");
        return ExitCode::from(2);
    }
    let characters: Vec<Character> = if names.is_empty() {
        Character::ALL
            .into_iter()
            .filter(|&character| {
                fs::read(assets().join(model::model_path(character)))
                    .ok()
                    .and_then(|bytes| model::generator_of(&bytes))
                    .is_none_or(|generator| puppet::made_here(&generator))
            })
            .collect()
    } else {
        let mut characters = Vec::new();
        for name in names {
            match Character::ALL
                .into_iter()
                .find(|character| character.name().eq_ignore_ascii_case(name))
            {
                Some(character) => characters.push(character),
                None => {
                    eprintln!("unknown character `{name}`");
                    return ExitCode::from(2);
                }
            }
        }
        characters
    };
    for character in characters {
        if let Err(error) = write(character) {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    }
    audit()
}
