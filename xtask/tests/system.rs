//! The real executor: processes and file system operations.
//!
//! `SystemExecutor::new(root)` runs commands with `root` as working directory and resolves every
//! relative path against `root`. File operations are exercised in throw-away temp directories;
//! the process tests only use `cargo`, which is always present when these tests run.
//!
//! The tests use `SystemExecutor::quiet`, which discards the output of the commands: running
//! the tests prints nothing on the terminal.

use std::fs;
use std::path::{Path, PathBuf};

use xtask::{CommandSpec, ExecError, Executor, Output, SystemExecutor};

/// A unique temp directory that is removed when dropped.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("srr-xtask-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TempRoot(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn executor(&self) -> SystemExecutor {
        SystemExecutor::quiet(self.0.clone())
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------------------------

#[test]
fn the_normal_executor_shows_the_output_and_the_quiet_one_hides_it() {
    assert_eq!(SystemExecutor::new(".".into()).output(), Output::Inherit);
    assert_eq!(SystemExecutor::quiet(".".into()).output(), Output::Discard);
}

#[test]
fn a_successful_command_is_ok() {
    let temp = TempRoot::new("run-ok");
    let mut executor = temp.executor();
    assert_eq!(
        executor.run_command(&CommandSpec::new("cargo", &["--version"])),
        Ok(())
    );
}

#[test]
fn a_failing_command_reports_its_exit_code_and_text() {
    let temp = TempRoot::new("run-fail");
    let mut executor = temp.executor();
    let result = executor.run_command(&CommandSpec::new("cargo", &["--definitely-not-a-flag"]));
    match result {
        Err(ExecError::CommandFailed { command, code }) => {
            assert!(command.contains("--definitely-not-a-flag"), "{command}");
            assert!(matches!(code, Some(c) if c != 0), "{code:?}");
        }
        other => panic!("expected CommandFailed, got {other:?}"),
    }
}

#[test]
fn a_missing_program_is_a_spawn_failure() {
    let temp = TempRoot::new("run-missing");
    let mut executor = temp.executor();
    let result = executor.run_command(&CommandSpec::new("srr-definitely-not-a-program", &["x"]));
    match result {
        Err(ExecError::SpawnFailed { command, message }) => {
            assert!(
                command.contains("srr-definitely-not-a-program"),
                "{command}"
            );
            assert!(!message.is_empty());
        }
        other => panic!("expected SpawnFailed, got {other:?}"),
    }
}

#[test]
fn commands_run_in_the_executors_root_directory() {
    // `cargo locate-project` succeeds only when a Cargo.toml is found from the working directory.
    let temp = TempRoot::new("run-cwd");
    let locate = CommandSpec::new("cargo", &["locate-project"]);

    let mut without_manifest = temp.executor();
    assert!(matches!(
        without_manifest.run_command(&locate),
        Err(ExecError::CommandFailed { .. })
    ));

    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    let mut with_manifest = temp.executor();
    assert_eq!(with_manifest.run_command(&locate), Ok(()));
}

// ---------------------------------------------------------------------------------------------
// File system
// ---------------------------------------------------------------------------------------------

#[test]
fn create_dir_creates_nested_directories_relative_to_the_root() {
    let temp = TempRoot::new("mkdir");
    let mut executor = temp.executor();
    assert_eq!(executor.create_dir(Path::new("a/b/c")), Ok(()));
    assert!(temp.path().join("a/b/c").is_dir());
}

#[test]
fn create_dir_is_idempotent() {
    let temp = TempRoot::new("mkdir-twice");
    let mut executor = temp.executor();
    executor.create_dir(Path::new("dist")).unwrap();
    assert_eq!(executor.create_dir(Path::new("dist")), Ok(()));
}

#[test]
fn remove_dir_removes_a_directory_tree() {
    let temp = TempRoot::new("rmdir");
    fs::create_dir_all(temp.path().join("dist/nested")).unwrap();
    fs::write(temp.path().join("dist/nested/file.txt"), "x").unwrap();
    let mut executor = temp.executor();
    assert_eq!(executor.remove_dir(Path::new("dist")), Ok(()));
    assert!(!temp.path().join("dist").exists());
}

#[test]
fn removing_a_missing_directory_is_not_an_error() {
    let temp = TempRoot::new("rmdir-missing");
    let mut executor = temp.executor();
    assert_eq!(executor.remove_dir(Path::new("never-existed")), Ok(()));
}

#[test]
fn copy_file_copies_the_contents() {
    let temp = TempRoot::new("copy");
    fs::write(temp.path().join("source.txt"), "hello wasm").unwrap();
    fs::create_dir_all(temp.path().join("dist")).unwrap();
    let mut executor = temp.executor();
    assert_eq!(
        executor.copy_file(Path::new("source.txt"), Path::new("dist/copy.txt")),
        Ok(())
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("dist/copy.txt")).unwrap(),
        "hello wasm"
    );
}

#[test]
fn copying_a_missing_file_is_an_io_error_naming_the_file() {
    let temp = TempRoot::new("copy-missing");
    fs::create_dir_all(temp.path().join("dist")).unwrap();
    let mut executor = temp.executor();
    let result = executor.copy_file(Path::new("missing.txt"), Path::new("dist/missing.txt"));
    match result {
        Err(ExecError::Io { path, message }) => {
            assert!(path.ends_with("missing.txt"), "{path:?}");
            assert!(!message.is_empty());
        }
        other => panic!("expected Io error, got {other:?}"),
    }
}

#[test]
fn copying_into_a_missing_directory_is_an_io_error() {
    let temp = TempRoot::new("copy-no-dir");
    fs::write(temp.path().join("source.txt"), "x").unwrap();
    let mut executor = temp.executor();
    assert!(matches!(
        executor.copy_file(Path::new("source.txt"), Path::new("no-such-dir/copy.txt")),
        Err(ExecError::Io { .. })
    ));
}

#[test]
fn creating_a_directory_where_a_file_exists_is_an_io_error() {
    let temp = TempRoot::new("mkdir-over-file");
    fs::write(temp.path().join("dist"), "i am a file").unwrap();
    let mut executor = temp.executor();
    assert!(matches!(
        executor.create_dir(Path::new("dist")),
        Err(ExecError::Io { .. })
    ));
}
