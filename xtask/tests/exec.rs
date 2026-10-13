//! Executing plans and the command line runner, against a recording fake executor.
//!
//! `run_cli(args, root, executor, out, err)` returns the process exit code:
//! 0 = success (or help), 1 = a step failed, 2 = usage error.

use std::path::{Path, PathBuf};

use xtask::{execute, plan, run_cli, usage, CommandSpec, ExecError, Executor, Step, Task};

const ROOT: &str = "/project";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Run(String),
    RemoveDir(PathBuf),
    CreateDir(PathBuf),
    Copy(PathBuf, PathBuf),
}

/// Records every call; optionally fails on the n-th one (0-based).
#[derive(Default)]
struct Recorder {
    calls: Vec<Call>,
    fail_on_call: Option<usize>,
}

impl Recorder {
    fn failing_on(index: usize) -> Self {
        Recorder {
            calls: Vec::new(),
            fail_on_call: Some(index),
        }
    }

    fn record(&mut self, call: Call) -> Result<(), ExecError> {
        self.calls.push(call);
        if self.fail_on_call == Some(self.calls.len() - 1) {
            Err(ExecError::CommandFailed {
                command: "boom".to_string(),
                code: Some(3),
            })
        } else {
            Ok(())
        }
    }
}

impl Executor for Recorder {
    fn run_command(&mut self, command: &CommandSpec) -> Result<(), ExecError> {
        self.record(Call::Run(command.display()))
    }

    fn remove_dir(&mut self, path: &Path) -> Result<(), ExecError> {
        self.record(Call::RemoveDir(path.to_path_buf()))
    }

    fn create_dir(&mut self, path: &Path) -> Result<(), ExecError> {
        self.record(Call::CreateDir(path.to_path_buf()))
    }

    fn copy_file(&mut self, from: &Path, to: &Path) -> Result<(), ExecError> {
        self.record(Call::Copy(from.to_path_buf(), to.to_path_buf()))
    }
}

fn calls_for(task: &Task) -> Vec<Call> {
    plan(task, Path::new(ROOT))
        .into_iter()
        .map(|step| match step {
            Step::Run(command) => Call::Run(command.display()),
            Step::RemoveDir(path) => Call::RemoveDir(path),
            Step::CreateDir(path) => Call::CreateDir(path),
            Step::Copy { from, to } => Call::Copy(from, to),
        })
        .collect()
}

fn run(args: &[&str], recorder: &mut Recorder) -> (i32, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_cli(
        args.iter().copied(),
        Path::new(ROOT),
        recorder,
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

// ---------------------------------------------------------------------------------------------
// execute
// ---------------------------------------------------------------------------------------------

#[test]
fn execute_does_nothing_for_an_empty_plan() {
    let mut recorder = Recorder::default();
    assert_eq!(execute(&[], &mut recorder), Ok(()));
    assert!(recorder.calls.is_empty());
}

#[test]
fn execute_maps_every_step_kind_to_its_executor_call_in_order() {
    let steps = vec![
        Step::RemoveDir(PathBuf::from("dist")),
        Step::CreateDir(PathBuf::from("dist")),
        Step::Run(CommandSpec::new("cargo", &["build"])),
        Step::Copy {
            from: PathBuf::from("a"),
            to: PathBuf::from("dist/a"),
        },
    ];
    let mut recorder = Recorder::default();
    assert_eq!(execute(&steps, &mut recorder), Ok(()));
    assert_eq!(
        recorder.calls,
        vec![
            Call::RemoveDir(PathBuf::from("dist")),
            Call::CreateDir(PathBuf::from("dist")),
            Call::Run("cargo build".to_string()),
            Call::Copy(PathBuf::from("a"), PathBuf::from("dist/a")),
        ]
    );
}

#[test]
fn execute_stops_at_the_first_failure_and_returns_its_error() {
    let steps = vec![
        Step::Run(CommandSpec::new("one", &[])),
        Step::Run(CommandSpec::new("two", &[])),
        Step::Run(CommandSpec::new("three", &[])),
    ];
    let mut recorder = Recorder::failing_on(1);
    let result = execute(&steps, &mut recorder);
    assert_eq!(
        result,
        Err(ExecError::CommandFailed {
            command: "boom".to_string(),
            code: Some(3)
        })
    );
    assert_eq!(recorder.calls.len(), 2, "the third step must not run");
}

// ---------------------------------------------------------------------------------------------
// run_cli
// ---------------------------------------------------------------------------------------------

#[test]
fn run_cli_executes_the_plan_of_the_requested_task() {
    let mut recorder = Recorder::default();
    let (code, _, err) = run(&["dist"], &mut recorder);
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(recorder.calls, calls_for(&Task::Dist));
}

#[test]
fn run_cli_reports_each_command_it_runs() {
    let mut recorder = Recorder::default();
    let (_, out, _) = run(&["build"], &mut recorder);
    for step in plan(&Task::Build, Path::new(ROOT)) {
        if let Step::Run(command) = step {
            assert!(out.contains(&command.display()), "{out}");
        }
    }
}

#[test]
fn run_cli_supports_docker_tasks() {
    let mut recorder = Recorder::default();
    let (code, _, _) = run(&["docker", "test"], &mut recorder);
    assert_eq!(code, 0);
    assert_eq!(recorder.calls, calls_for(&Task::Docker(Box::new(Task::Test))));
}

#[test]
fn help_prints_the_usage_and_runs_nothing() {
    for args in [&[][..], &["help"][..], &["--help"][..]] {
        let mut recorder = Recorder::default();
        let (code, out, _) = run(args, &mut recorder);
        assert_eq!(code, 0, "{args:?}");
        assert!(out.contains(usage().as_str()), "{args:?}");
        assert!(recorder.calls.is_empty(), "{args:?}");
    }
}

#[test]
fn usage_errors_exit_with_two_and_explain_themselves_on_stderr() {
    let mut recorder = Recorder::default();
    let (code, out, err) = run(&["frobnicate"], &mut recorder);
    assert_eq!(code, 2);
    assert!(err.contains("frobnicate"), "{err}");
    assert!(err.contains(usage().as_str()), "{err}");
    assert!(out.is_empty());
    assert!(recorder.calls.is_empty());
}

#[test]
fn a_failing_step_exits_with_one_names_the_command_and_stops() {
    let mut recorder = Recorder::failing_on(0);
    let (code, _, err) = run(&["build"], &mut recorder);
    assert_eq!(code, 1);
    assert!(err.contains("boom"), "{err}");
    assert_eq!(recorder.calls.len(), 1);
}

// ---------------------------------------------------------------------------------------------
// ExecError
// ---------------------------------------------------------------------------------------------

#[test]
fn exec_errors_have_informative_messages() {
    let failed = ExecError::CommandFailed {
        command: "cargo test".to_string(),
        code: Some(101),
    };
    assert!(failed.to_string().contains("cargo test"));
    assert!(failed.to_string().contains("101"));

    let killed = ExecError::CommandFailed {
        command: "cargo test".to_string(),
        code: None,
    };
    assert!(killed.to_string().contains("cargo test"));

    let spawn = ExecError::SpawnFailed {
        command: "wasm-bindgen".to_string(),
        message: "No such file".to_string(),
    };
    assert!(spawn.to_string().contains("wasm-bindgen"));
    assert!(spawn.to_string().contains("No such file"));

    let io = ExecError::Io {
        path: PathBuf::from("dist/index.html"),
        message: "denied".to_string(),
    };
    assert!(io.to_string().contains("dist/index.html"));
    assert!(io.to_string().contains("denied"));
}

#[test]
fn exec_error_is_a_std_error() {
    fn assert_error<E: std::error::Error>() {}
    assert_error::<ExecError>();
}
