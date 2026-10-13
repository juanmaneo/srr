//! Project-level requirements taken from the README and the plan.
//!
//! README musts: entirely Rust, runs in a browser as WASM, `cargo xtask` for build / test /
//! coverage / distribution, a Docker setup that builds on any machine.
//! Plan: Bevy for the first draft, with the game rules in an engine-agnostic `game` module.
//!
//! These tests only read files, so they run (and fail) before any implementation exists.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read `{relative}`: {error}"))
}

fn exists(relative: &str) -> bool {
    root().join(relative).exists()
}

fn code_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if matches!(name.as_str(), "target" | "dist" | ".git" | "node_modules") {
                continue;
            }
            collect_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Crate structure
// ---------------------------------------------------------------------------------------------

#[test]
fn the_crate_has_a_library_target_so_the_game_logic_is_testable() {
    assert!(exists("src/lib.rs"), "src/lib.rs is missing");
}

#[test]
fn the_game_module_exists() {
    assert!(
        exists("src/game/mod.rs") || exists("src/game.rs"),
        "the engine-agnostic `game` module is missing"
    );
}

#[test]
fn the_game_module_is_engine_and_platform_agnostic() {
    let mut files = Vec::new();
    collect_files(&root().join("src/game"), &mut files);
    let single = root().join("src/game.rs");
    if single.is_file() {
        files.push(single);
    }
    files.retain(|path| path.extension().is_some_and(|ext| ext == "rs"));
    assert!(
        !files.is_empty(),
        "no Rust sources found for the game module"
    );

    // Bevy would couple the rules to one engine; the rest is unavailable or non-deterministic
    // in the browser / in a lock-step online simulation.
    let forbidden = [
        "bevy::",
        "use bevy",
        "extern crate bevy",
        "std::time::Instant",
        "SystemTime",
        "std::thread",
        "std::fs",
        "std::net",
        "std::process",
        "std::env",
        "rand::",
        "thread_rng",
    ];
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        for pattern in forbidden {
            assert!(
                !text.contains(pattern),
                "{} must not use `{pattern}`",
                path.display()
            );
        }
    }
}

#[test]
fn only_the_engine_backends_know_the_engines() {
    // Everything outside `src/engine/` is shared by the Bevy and the Fyrox builds.
    let mut files = Vec::new();
    collect_files(&root().join("src"), &mut files);
    files.retain(|path| path.extension().is_some_and(|ext| ext == "rs"));
    let engine_dir = root().join("src/engine");
    for path in files.iter().filter(|path| !path.starts_with(&engine_dir)) {
        let text = fs::read_to_string(path).unwrap();
        for pattern in ["bevy::", "use bevy", "fyrox::", "use fyrox"] {
            assert!(
                !text.contains(pattern),
                "{} must not use `{pattern}`: only src/engine/ may",
                path.display()
            );
        }
    }
    assert!(exists("src/engine/bevy.rs"), "the Bevy backend is missing");
    assert!(
        exists("src/engine/fyrox.rs"),
        "the Fyrox backend is missing"
    );
}

#[test]
fn the_tests_run_headless() {
    // Tests look at the screens through the software renderer (`srr::front::render`, driven by
    // `srr::front::Headless`): no window, no sound card, nothing printed by child processes.
    let mut files = Vec::new();
    collect_files(&root().join("tests"), &mut files);
    let this_file = root().join("tests/project_layout.rs");
    files.retain(|path| path.extension().is_some_and(|ext| ext == "rs") && *path != this_file);
    assert!(!files.is_empty());
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        for pattern in ["srr::engine", "srr::platform", "Shell::", "engine::run"] {
            assert!(
                !text.contains(pattern),
                "{} must not use `{pattern}`: tests run headless",
                path.display()
            );
        }
    }
    assert!(
        read("xtask/tests/system.rs").contains("SystemExecutor::quiet(self.0.clone())"),
        "the xtask process tests must discard the output of the commands they run"
    );
    assert!(exists("src/front/render.rs") && exists("src/front/headless.rs"));
}

#[test]
fn the_game_is_written_entirely_in_rust() {
    let mut files = Vec::new();
    collect_files(&root(), &mut files);
    let offenders: Vec<_> = files
        .iter()
        .filter(|path| {
            let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            matches!(extension, "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx")
                || name == "package.json"
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "non-Rust sources found: {offenders:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Bevy and WASM
// ---------------------------------------------------------------------------------------------

#[test]
fn bevy_is_a_dependency_and_is_pinned() {
    let manifest = read("Cargo.toml");
    let lines = code_lines(&manifest);
    assert!(
        lines.iter().any(|l| l.starts_with("bevy =")
            || l.starts_with("bevy=")
            || *l == "[dependencies.bevy]"),
        "Cargo.toml has no `bevy` dependency"
    );
    assert!(
        !lines
            .iter()
            .any(|l| l.starts_with("bevy") && l.contains("\"*\"")),
        "bevy must be pinned to a version, not `*`"
    );
}

#[test]
fn dynamic_linking_is_not_enabled_in_the_manifest_because_it_breaks_wasm() {
    let manifest = read("Cargo.toml");
    assert!(
        !code_lines(&manifest)
            .iter()
            .any(|line| line.contains("dynamic_linking")),
        "enable `bevy/dynamic_linking` from the command line for native dev builds only"
    );
}

#[test]
fn dependencies_are_optimised_in_dev_builds() {
    let manifest = read("Cargo.toml");
    assert!(manifest.contains("[profile.dev.package.\"*\"]"));
    assert!(manifest.contains("opt-level = 3"));
}

#[test]
fn a_web_page_loads_the_wasm_build() {
    assert!(exists("web/index.html"), "web/index.html is missing");
}

// ---------------------------------------------------------------------------------------------
// cargo xtask
// ---------------------------------------------------------------------------------------------

#[test]
fn xtask_is_a_workspace_member() {
    let manifest = read("Cargo.toml");
    assert!(manifest.contains("[workspace]"), "no [workspace] section");
    assert!(
        manifest.contains("\"xtask\""),
        "xtask is not a workspace member"
    );
}

#[test]
fn the_xtask_crate_has_a_library_with_the_logic_and_a_thin_main() {
    assert!(read("xtask/Cargo.toml").contains("name = \"xtask\""));
    assert!(exists("xtask/src/lib.rs"));
    assert!(exists("xtask/src/main.rs"));
}

#[test]
fn cargo_xtask_is_an_alias_for_running_the_xtask_package() {
    let config = read(".cargo/config.toml");
    assert!(config.contains("[alias]"));
    let alias = config
        .lines()
        .find(|line| line.trim_start().starts_with("xtask"))
        .expect("no `xtask` alias");
    assert!(alias.contains("run"), "{alias}");
    assert!(
        alias.contains("--package xtask") || alias.contains("-p xtask"),
        "{alias}"
    );
}

#[test]
fn the_readme_documents_the_xtask_workflow() {
    let readme = read("README.md");
    for command in [
        "cargo xtask build",
        "cargo xtask test",
        "cargo xtask coverage",
        "cargo xtask dist",
        "cargo xtask docker",
    ] {
        assert!(
            readme.contains(command),
            "README does not mention `{command}`"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Docker and ignore files
// ---------------------------------------------------------------------------------------------

#[test]
fn the_dockerfile_can_build_test_cover_and_package_the_game() {
    let dockerfile = read("Dockerfile");
    assert!(dockerfile.contains("FROM "));
    assert!(
        dockerfile.contains("wasm32-unknown-unknown"),
        "wasm target missing"
    );
    assert!(dockerfile.contains("wasm-bindgen"), "wasm-bindgen missing");
    assert!(dockerfile.contains("llvm-cov"), "coverage tooling missing");
    assert!(
        dockerfile.contains("WORKDIR /work"),
        "xtask mounts the project at /work"
    );
}

#[test]
fn the_dockerfile_serves_the_game_by_default_and_has_a_toolchain_stage_for_xtask() {
    let dockerfile = read("Dockerfile");
    assert!(
        dockerfile.contains(" AS toolchain"),
        "`cargo xtask docker` builds the `toolchain` stage"
    );
    assert!(
        dockerfile.contains("cargo xtask dist"),
        "the default image must build the web build"
    );
    assert!(
        dockerfile.contains("srr-server") && dockerfile.contains("EXPOSE 8080"),
        "the default image must run the LAN server on port 8080"
    );
    assert!(
        dockerfile.contains("COPY . ."),
        "the default image builds from a copy of the project, not from a mount"
    );
}

#[test]
fn the_toolchain_image_writes_to_the_mounted_project_as_its_owner_not_as_root() {
    let dockerfile = read("Dockerfile");
    assert!(
        dockerfile.contains("stat -c %u /work") && dockerfile.contains("setpriv --reuid="),
        "the entrypoint must drop to the uid/gid owning /work, or target/ and dist/ end up root-owned on the host"
    );
    assert!(
        dockerfile.contains("chmod -R a+rwX ${CARGO_HOME}"),
        "the crate cache must be writable by that user"
    );
}

#[test]
fn build_outputs_stay_out_of_git_and_docker_contexts() {
    let is_target =
        |line: &str| matches!(line.trim(), "target" | "/target" | "target/" | "/target/");
    let gitignore = read(".gitignore");
    assert!(
        gitignore.lines().any(is_target),
        ".gitignore must exclude target"
    );
    assert!(
        gitignore
            .lines()
            .any(|l| matches!(l.trim(), "dist" | "/dist" | "dist/" | "/dist/")),
        ".gitignore must exclude dist"
    );
    assert!(
        read(".dockerignore").lines().any(is_target),
        ".dockerignore must exclude target"
    );
}
