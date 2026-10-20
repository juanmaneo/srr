//! What each xtask task does, as data.
//!
//! `plan(task, root)` is a pure function from a task to the ordered steps that implement it, so
//! the whole workflow can be tested without running a single process. Paths in steps are relative
//! to the project root, which is where the executor runs them. `root` is only needed to
//! bind-mount the project into the Docker container (`<root>:/work`).
//!
//! Choices pinned here: the WASM build is `cargo build --release --target
//! wasm32-unknown-unknown` followed by `wasm-bindgen --target web --out-dir <dir>`; `dist/`
//! gets `web/index.html` plus the wasm-bindgen `.js` and `.wasm` output.

use std::path::{Path, PathBuf};

use xtask::{CommandSpec, DOCKER_IMAGE, DOCKER_TOOLCHAIN_STAGE, SERVE_PORT, Step, Task, plan};

const ROOT: &str = "/project";

fn steps(task: Task) -> Vec<Step> {
    plan(&task, Path::new(ROOT))
}

fn commands(task: Task) -> Vec<CommandSpec> {
    steps(task)
        .into_iter()
        .filter_map(|step| match step {
            Step::Run(command) => Some(command),
            _ => None,
        })
        .collect()
}

fn copies(task: Task) -> Vec<(PathBuf, PathBuf)> {
    steps(task)
        .into_iter()
        .filter_map(|step| match step {
            Step::Copy { from, to } => Some((from, to)),
            _ => None,
        })
        .collect()
}

/// True when `second` directly follows `first` in `args`.
fn has_pair(args: &[String], first: &str, second: &str) -> bool {
    args.windows(2).any(|w| w[0] == first && w[1] == second)
}

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

fn bindgen_out_dir() -> String {
    let all = commands(Task::Build);
    let bindgen = all
        .iter()
        .find(|c| c.program == "wasm-bindgen")
        .expect("a wasm-bindgen step");
    value_after(&bindgen.args, "--out-dir")
        .expect("wasm-bindgen --out-dir")
        .to_string()
}

// ---------------------------------------------------------------------------------------------
// CommandSpec
// ---------------------------------------------------------------------------------------------

#[test]
fn command_specs_keep_program_and_arguments() {
    let command = CommandSpec::new("cargo", &["test", "--workspace"]);
    assert_eq!(command.program, "cargo");
    assert_eq!(
        command.args,
        vec!["test".to_string(), "--workspace".to_string()]
    );
}

#[test]
fn command_specs_display_like_a_shell_command() {
    assert_eq!(
        CommandSpec::new("cargo", &["test", "--workspace"]).display(),
        "cargo test --workspace"
    );
    assert_eq!(CommandSpec::new("docker", &[]).display(), "docker");
}

// ---------------------------------------------------------------------------------------------
// build
// ---------------------------------------------------------------------------------------------

#[test]
fn build_compiles_the_game_for_wasm_in_release_mode() {
    let all = commands(Task::Build);
    let cargo = all
        .iter()
        .find(|c| c.program == "cargo")
        .expect("a cargo step");
    assert_eq!(cargo.args[0], "build");
    assert!(cargo.args.iter().any(|a| a == "--release"));
    assert!(has_pair(&cargo.args, "--target", "wasm32-unknown-unknown"));
}

#[test]
fn build_runs_wasm_bindgen_for_the_web_on_the_release_artifact() {
    let all = commands(Task::Build);
    let bindgen = all
        .iter()
        .find(|c| c.program == "wasm-bindgen")
        .expect("a wasm-bindgen step");
    assert!(has_pair(&bindgen.args, "--target", "web"));
    assert!(value_after(&bindgen.args, "--out-dir").is_some());
    assert!(
        bindgen
            .args
            .last()
            .unwrap()
            .contains("wasm32-unknown-unknown/release/srr.wasm")
    );
}

#[test]
fn build_compiles_before_it_binds() {
    let all = commands(Task::Build);
    let cargo = all.iter().position(|c| c.program == "cargo").unwrap();
    let bindgen = all
        .iter()
        .position(|c| c.program == "wasm-bindgen")
        .unwrap();
    assert!(cargo < bindgen);
}

#[test]
fn build_does_not_touch_the_dist_folder() {
    for step in steps(Task::Build) {
        match step {
            Step::RemoveDir(path) | Step::CreateDir(path) => assert!(!path.starts_with("dist")),
            Step::Copy { to, .. } => assert!(!to.starts_with("dist")),
            Step::Run(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// test and coverage
// ---------------------------------------------------------------------------------------------

#[test]
fn test_runs_every_test_of_the_workspace() {
    let all = commands(Task::Test);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].program, "cargo");
    assert_eq!(all[0].args[0], "test");
    assert!(all[0].args.iter().any(|a| a == "--workspace"));
    assert_eq!(
        steps(Task::Test).len(),
        1,
        "no steps other than the command"
    );
}

#[test]
fn coverage_fails_below_one_hundred_percent() {
    let all = commands(Task::Coverage);
    let gate = all
        .iter()
        .find(|c| has_pair(&c.args, "--fail-under-lines", "100"))
        .expect("a step that enforces 100 % line coverage");
    // The ratchet holds every measure, not only the lines: a branch never taken on a covered
    // line (the error of a `?`, the `else` of an `if let`) fails it too.
    for measure in ["--fail-under-regions", "--fail-under-functions"] {
        assert!(has_pair(&gate.args, measure, "100"), "{measure}");
    }
    assert_eq!(gate.program, "cargo");
    assert_eq!(gate.args[0], "llvm-cov");
    assert!(gate.args.iter().any(|a| a == "--workspace"));
}

// ---------------------------------------------------------------------------------------------
// dist
// ---------------------------------------------------------------------------------------------

#[test]
fn dist_starts_with_the_whole_build() {
    let build = steps(Task::Build);
    let dist = steps(Task::Dist);
    assert!(dist.len() > build.len());
    assert_eq!(&dist[..build.len()], &build[..]);
}

#[test]
fn dist_recreates_the_dist_folder_before_copying_into_it() {
    let dist = steps(Task::Dist);
    let remove = dist
        .iter()
        .position(|s| *s == Step::RemoveDir(PathBuf::from("dist")))
        .expect("dist is cleaned");
    let create = dist
        .iter()
        .position(|s| *s == Step::CreateDir(PathBuf::from("dist")))
        .expect("dist is created");
    assert!(remove < create);
    for (index, step) in dist.iter().enumerate() {
        if matches!(step, Step::Copy { .. }) {
            assert!(index > create, "copy at {index} happens before dist exists");
        }
    }
}

#[test]
fn dist_ships_the_web_page() {
    assert!(copies(Task::Dist).contains(&(
        PathBuf::from("web/index.html"),
        PathBuf::from("dist/index.html")
    )));
}

#[test]
fn dist_ships_the_wasm_binary_and_its_js_loader_from_the_bindgen_output() {
    let out_dir = bindgen_out_dir();
    let all = copies(Task::Dist);
    for extension in ["wasm", "js"] {
        assert!(
            all.iter().any(|(from, to)| {
                from.starts_with(&out_dir)
                    && to.starts_with("dist")
                    && to.extension().is_some_and(|e| e == extension)
            }),
            "no .{extension} file is copied from {out_dir} to dist"
        );
    }
}

#[test]
fn dist_only_writes_inside_the_dist_folder() {
    for (_, to) in copies(Task::Dist) {
        assert!(to.starts_with("dist"), "{to:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// serve
// ---------------------------------------------------------------------------------------------

#[test]
fn serve_builds_the_whole_dist_folder_first() {
    let dist = steps(Task::Dist);
    let serve = steps(Task::Serve);
    assert_eq!(serve.len(), dist.len() + 1);
    assert_eq!(&serve[..dist.len()], &dist[..]);
}

#[test]
fn serve_ends_by_running_the_lan_server_on_the_dist_folder() {
    let Some(Step::Run(server)) = steps(Task::Serve).pop() else {
        panic!("serve must end with a command");
    };
    assert_eq!(server.program, "cargo");
    assert_eq!(server.args[0], "run");
    assert!(server.args.iter().any(|a| a == "--release"));
    assert!(has_pair(&server.args, "--package", "srr-server"));
    let options = server
        .args
        .iter()
        .position(|a| a == "--")
        .expect("server options");
    let options = &server.args[options..];
    assert!(has_pair(options, "--dir", "dist"));
    assert!(has_pair(options, "--port", SERVE_PORT));
}

// ---------------------------------------------------------------------------------------------
// docker
// ---------------------------------------------------------------------------------------------

#[test]
fn docker_image_builds_the_dockerfile_of_the_project() {
    let all = commands(Task::DockerImage);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].program, "docker");
    assert_eq!(all[0].args[0], "build");
    assert!(has_pair(&all[0].args, "-t", DOCKER_IMAGE));
    assert!(
        has_pair(&all[0].args, "--target", DOCKER_TOOLCHAIN_STAGE),
        "only the toolchain stage: the default target compiles the whole game"
    );
    assert_eq!(
        all[0].args.last().unwrap(),
        ".",
        "the project is the build context"
    );
    assert_eq!(steps(Task::DockerImage).len(), 1);
}

#[test]
fn docker_tasks_build_the_image_then_run_the_inner_task_in_it() {
    let image_steps = steps(Task::DockerImage);
    for inner in [
        Task::Build,
        Task::Test,
        Task::Coverage,
        Task::Dist,
        Task::Serve,
    ] {
        let all = steps(Task::Docker(Box::new(inner.clone())));
        assert_eq!(all.len(), image_steps.len() + 1, "{inner:?}");
        assert_eq!(&all[..image_steps.len()], &image_steps[..], "{inner:?}");

        let Step::Run(run) = all.last().unwrap() else {
            panic!("the last step of docker {inner:?} must run a command");
        };
        assert_eq!(run.program, "docker");
        assert_eq!(run.args[0], "run");
        assert!(
            run.args.iter().any(|a| a == "--rm"),
            "containers are throw-away"
        );

        let image = run
            .args
            .iter()
            .position(|a| a.as_str() == DOCKER_IMAGE)
            .expect("image");
        let cargo = run.args.iter().position(|a| a == "cargo").expect("cargo");
        assert!(
            image < cargo,
            "the image comes before the container command"
        );

        let tail: Vec<&str> = run.args[run.args.len() - 3..]
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(tail, vec!["cargo", "xtask", inner.name()]);
    }
}

#[test]
fn docker_mounts_the_project_so_that_dist_lands_on_the_host() {
    let all = commands(Task::Docker(Box::new(Task::Dist)));
    let run = all.last().unwrap();
    assert!(
        has_pair(&run.args, "-v", "/project:/work"),
        "{:?}",
        run.args
    );
}

#[test]
fn docker_serve_publishes_the_server_port_and_forwards_ctrl_c() {
    let all = commands(Task::Docker(Box::new(Task::Serve)));
    let run = all.last().unwrap();
    let publish = format!("{SERVE_PORT}:{SERVE_PORT}");
    assert!(has_pair(&run.args, "-p", &publish), "{:?}", run.args);
    assert!(run.args.iter().any(|a| a == "--init"), "{:?}", run.args);
}

#[test]
fn only_docker_serve_publishes_a_port() {
    for inner in [Task::Build, Task::Test, Task::Coverage, Task::Dist] {
        let all = commands(Task::Docker(Box::new(inner.clone())));
        let run = all.last().unwrap();
        assert!(
            !run.args.iter().any(|a| a == "-p"),
            "{inner:?}: {:?}",
            run.args
        );
    }
}

#[test]
fn docker_mounts_whatever_root_it_is_given() {
    let all = plan(
        &Task::Docker(Box::new(Task::Test)),
        Path::new("/somewhere/else"),
    );
    let Some(Step::Run(run)) = all.last() else {
        panic!("expected a final command");
    };
    assert!(
        has_pair(&run.args, "-v", "/somewhere/else:/work"),
        "{:?}",
        run.args
    );
}

// ---------------------------------------------------------------------------------------------
// help and purity
// ---------------------------------------------------------------------------------------------

#[test]
fn screenshots_run_the_headless_example_without_any_engine() {
    let all = steps(Task::Screenshots);
    assert_eq!(all.len(), 1);
    let Step::Run(command) = &all[0] else {
        panic!("expected a command, got {all:?}");
    };
    assert_eq!(command.program, "cargo");
    assert_eq!(command.args[0], "run");
    assert!(has_pair(&command.args, "--example", "screenshots"));
    assert!(has_pair(&command.args, "--package", "srr"));
    // No Bevy, no Fyrox: the software renderer needs neither a window nor a GPU.
    assert!(command.args.contains(&"--no-default-features".to_string()));
    assert!(!command.args.iter().any(|arg| arg == "--features"));
}

#[test]
fn help_does_nothing() {
    assert!(steps(Task::Help).is_empty());
}

#[test]
fn planning_is_deterministic() {
    for task in [
        Task::Build,
        Task::Test,
        Task::Coverage,
        Task::Dist,
        Task::Serve,
        Task::Screenshots,
        Task::DockerImage,
        Task::Help,
    ] {
        assert_eq!(steps(task.clone()), steps(task));
    }
}
