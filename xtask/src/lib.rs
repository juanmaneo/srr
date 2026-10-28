//! Build automation for Shattered Realm: Rampage (`cargo xtask <task>`).
//!
//! The work is split into pure parts — [`parse`] turns arguments into a [`Task`], [`plan`]
//! turns a task into ordered [`Step`]s — and an [`Executor`] that performs those steps. The
//! whole workflow is therefore testable without starting a single process.
//!
//! ```
//! use std::path::Path;
//! use xtask::{parse, plan, Task};
//!
//! let task = parse(["test"]).unwrap();
//! assert_eq!(task, Task::Test);
//!
//! // Planning is pure: nothing runs until the steps are handed to an `Executor`.
//! let steps = plan(&task, Path::new("/project"));
//! assert_eq!(steps.len(), 1);
//! ```

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Tag of the Docker image holding the build toolchain.
pub const DOCKER_IMAGE: &str = "srr-builder";
/// The stage of the Dockerfile with the toolchain only. The Dockerfile's default target also
/// compiles the game, for machines without Rust: the tasks must stop before that.
pub const DOCKER_TOOLCHAIN_STAGE: &str = "toolchain";

const WASM_TARGET: &str = "wasm32-unknown-unknown";
const WASM_ARTIFACT: &str = "target/wasm32-unknown-unknown/release/srr.wasm";
const BINDGEN_OUT_DIR: &str = "target/web";
const DIST_DIR: &str = "dist";
/// Port of the LAN server started by `serve`, also published by `docker serve`.
pub const SERVE_PORT: &str = "8080";
/// The engine backends, the platform glue (sockets, sound card) and the thin binaries are
/// checked by playing, not by the coverage gate.
const COVERAGE_IGNORE: &str = r"(^|/)src/(main\.rs|engine/|platform/)";

const EXIT_SUCCESS: i32 = 0;
const EXIT_FAILURE: i32 = 1;
const EXIT_USAGE: i32 = 2;

// ---------------------------------------------------------------------------------------------
// Tasks and parsing
// ---------------------------------------------------------------------------------------------

/// A job `cargo xtask` can perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    /// Compile the game to WASM and run `wasm-bindgen`.
    Build,
    /// Run every test of the workspace.
    Test,
    /// Run the tests and fail below 100 % coverage of the lines, the regions or the functions.
    Coverage,
    /// Build and assemble the playable web build in `dist/`.
    Dist,
    /// Build `dist/` and serve it on the local network, with the relay of online matches
    /// (`srr-server`).
    Serve,
    /// Play a scripted game headlessly and save PNG screenshots in `target/screenshots`.
    Screenshots,
    /// Everything a push must pass: formatting, lints, tests, then the coverage ratchet.
    DevCheck,
    /// Build the Docker image holding the toolchain.
    DockerImage,
    /// Runs the inner task inside the Docker image.
    Docker(Box<Task>),
    /// Print the usage text.
    Help,
}

impl Task {
    /// The command line name of the task.
    ///
    /// ```
    /// use xtask::Task;
    ///
    /// assert_eq!(Task::DockerImage.name(), "docker-image");
    /// assert_eq!(Task::Docker(Box::new(Task::Test)).name(), "docker");
    /// ```
    pub fn name(&self) -> &'static str {
        match self {
            Task::Build => "build",
            Task::Test => "test",
            Task::Coverage => "coverage",
            Task::Dist => "dist",
            Task::Serve => "serve",
            Task::Screenshots => "screenshots",
            Task::DevCheck => "dev-check",
            Task::DockerImage => "docker-image",
            Task::Docker(_) => "docker",
            Task::Help => "help",
        }
    }

    /// Tasks that take no argument.
    fn from_name(name: &str) -> Option<Task> {
        match name {
            "build" => Some(Task::Build),
            "test" => Some(Task::Test),
            "coverage" => Some(Task::Coverage),
            "dist" => Some(Task::Dist),
            "serve" => Some(Task::Serve),
            "screenshots" => Some(Task::Screenshots),
            "dev-check" => Some(Task::DevCheck),
            "docker-image" => Some(Task::DockerImage),
            "help" => Some(Task::Help),
            _ => None,
        }
    }

    fn runs_in_docker(&self) -> bool {
        matches!(
            self,
            Task::Build | Task::Test | Task::Coverage | Task::DevCheck | Task::Dist | Task::Serve
        )
    }
}

/// A command line that could not be understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// The first argument is not the name of a task.
    UnknownTask(String),
    /// An argument followed a complete command.
    UnexpectedArgument(String),
    /// `docker` was given without a task to run.
    MissingDockerTask,
    /// `docker` was given a task that cannot run in the container.
    InvalidDockerTask(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::UnknownTask(name) => write!(f, "unknown task `{name}`"),
            CliError::UnexpectedArgument(argument) => {
                write!(f, "unexpected argument `{argument}`")
            }
            CliError::MissingDockerTask => {
                write!(
                    f,
                    "`docker` needs a task: build, test, coverage, dev-check, dist or serve"
                )
            }
            CliError::InvalidDockerTask(name) => write!(
                f,
                "`{name}` cannot run in Docker; use build, test, coverage, dev-check, dist or serve"
            ),
        }
    }
}

impl std::error::Error for CliError {}

/// Parses the arguments that follow `cargo xtask`.
///
/// No arguments, `help`, `--help` and `-h` all mean [`Task::Help`]. Task names are
/// case-sensitive, and `docker` must be followed by one of `build`, `test`, `coverage`,
/// `dist` or `serve`.
///
/// # Errors
///
/// Returns a [`CliError`] for unknown tasks, extra arguments, and a missing or invalid task
/// after `docker`.
///
/// # Examples
///
/// ```
/// use xtask::{parse, CliError, Task};
///
/// assert_eq!(parse(["dist"]), Ok(Task::Dist));
/// assert_eq!(
///     parse(["docker", "coverage"]),
///     Ok(Task::Docker(Box::new(Task::Coverage)))
/// );
/// assert_eq!(parse(Vec::<String>::new()), Ok(Task::Help));
/// assert_eq!(
///     parse(["frobnicate"]),
///     Err(CliError::UnknownTask("frobnicate".to_string()))
/// );
/// ```
pub fn parse(args: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Task, CliError> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Task::Help);
    };
    let task = match first.as_ref() {
        "docker" => {
            let inner = args.next().ok_or(CliError::MissingDockerTask)?;
            let inner = inner.as_ref();
            match Task::from_name(inner) {
                Some(task) if task.runs_in_docker() => Task::Docker(Box::new(task)),
                _ => return Err(CliError::InvalidDockerTask(inner.to_string())),
            }
        }
        "--help" | "-h" => Task::Help,
        name => Task::from_name(name).ok_or_else(|| CliError::UnknownTask(name.to_string()))?,
    };
    match args.next() {
        Some(extra) => Err(CliError::UnexpectedArgument(extra.as_ref().to_string())),
        None => Ok(task),
    }
}

/// The help text listing every task.
pub fn usage() -> String {
    "\
Usage: cargo xtask <task>

Tasks:
  build            compile the game to WASM and run wasm-bindgen (output in target/web)
  test             run every test of the workspace
  coverage         run the tests and fail below 100 % coverage (lines, regions, functions)
  dev-check        everything a push must pass: fmt, clippy, tests, then coverage
  dist             build and assemble the playable web build in dist/
  serve            dist, then serve it on the local network (port 8080) for online play
  screenshots      play a scripted game headlessly (no window) and save PNGs in target/screenshots
  docker-image     build the Docker image holding the toolchain
  docker <task>    run build, test, coverage, dev-check, dist or serve inside the Docker image
  help             show this message
"
    .to_string()
}

// ---------------------------------------------------------------------------------------------
// Plans
// ---------------------------------------------------------------------------------------------

/// A process to run from the project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// The program to start, looked up in `PATH`.
    pub program: String,
    /// Its arguments, one entry per argument (no shell quoting involved).
    pub args: Vec<String>,
}

impl CommandSpec {
    /// A command running `program` with `args`.
    ///
    /// ```
    /// use xtask::CommandSpec;
    ///
    /// let command = CommandSpec::new("cargo", &["test", "--workspace"]);
    /// assert_eq!(command.display(), "cargo test --workspace");
    /// ```
    pub fn new(program: &str, args: &[&str]) -> Self {
        CommandSpec {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
        }
    }

    /// The command as it would be typed in a shell (without quoting).
    pub fn display(&self) -> String {
        let mut text = self.program.clone();
        for arg in &self.args {
            text.push(' ');
            text.push_str(arg);
        }
        text
    }
}

/// One unit of work. Relative paths are relative to the project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Run a process and require it to succeed.
    Run(CommandSpec),
    /// Remove a directory tree; a missing directory is fine.
    RemoveDir(PathBuf),
    /// Create a directory and its parents.
    CreateDir(PathBuf),
    /// Copy one file to another path (the target directory must exist).
    Copy {
        /// The file to read.
        from: PathBuf,
        /// The file to create or overwrite.
        to: PathBuf,
    },
}

/// The ordered steps implementing `task`. `root` is only used to mount the project in Docker.
///
/// The function is pure: it performs no I/O, and the same arguments always give the same
/// steps. Help plans nothing; `Dist` is the `Build` steps followed by the packaging steps;
/// `Serve` is the `Dist` steps followed by the LAN server (which runs until interrupted);
/// `Docker` builds the image and then runs the inner task in a throw-away container with the
/// project mounted at `/work` (and the server port published for `serve`).
///
/// ```
/// use std::path::Path;
/// use xtask::{plan, Step, Task};
///
/// let steps = plan(&Task::Docker(Box::new(Task::Test)), Path::new("/project"));
/// assert_eq!(steps.len(), 2); // build the image, then run the task in it
/// assert!(matches!(steps[0], Step::Run(_)));
///
/// assert!(plan(&Task::Help, Path::new("/project")).is_empty());
/// ```
pub fn plan(task: &Task, root: &Path) -> Vec<Step> {
    match task {
        Task::Build => build_steps(),
        Task::Test => vec![Step::Run(CommandSpec::new(
            "cargo",
            &["test", "--workspace"],
        ))],
        Task::Coverage => vec![Step::Run(CommandSpec::new(
            "cargo",
            &[
                "llvm-cov",
                "--workspace",
                "--fail-under-lines",
                "100",
                "--fail-under-regions",
                "100",
                "--fail-under-functions",
                "100",
                "--ignore-filename-regex",
                COVERAGE_IGNORE,
            ],
        ))],
        Task::Dist => {
            let mut steps = build_steps();
            steps.extend(dist_steps());
            steps
        }
        Task::Serve => {
            let mut steps = build_steps();
            steps.extend(dist_steps());
            steps.push(serve_step());
            steps
        }
        Task::Screenshots => vec![Step::Run(CommandSpec::new(
            "cargo",
            &[
                "run",
                "--package",
                "srr",
                "--no-default-features",
                "--example",
                "screenshots",
            ],
        ))],
        Task::DevCheck => dev_check_steps(),
        Task::DockerImage => vec![docker_image_step()],
        Task::Docker(inner) => vec![docker_image_step(), docker_run_step(inner, root)],
        Task::Help => Vec::new(),
    }
}

/// The steps of [`Task::DevCheck`]: everything a push must pass, in the order that fails
/// fastest first — formatting, lints (warnings are errors), tests, then the coverage
/// ratchet. `dev-check` is the whole gate in one command.
fn dev_check_steps() -> Vec<Step> {
    vec![
        Step::Run(CommandSpec::new("cargo", &["fmt", "--check"])),
        Step::Run(CommandSpec::new(
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        )),
        Step::Run(CommandSpec::new("cargo", &["test", "--workspace"])),
        Step::Run(CommandSpec::new(
            "cargo",
            &[
                "llvm-cov",
                "--workspace",
                "--fail-under-lines",
                "100",
                "--fail-under-regions",
                "100",
                "--fail-under-functions",
                "100",
                "--ignore-filename-regex",
                COVERAGE_IGNORE,
            ],
        )),
    ]
}

fn build_steps() -> Vec<Step> {
    vec![
        Step::Run(CommandSpec::new(
            "cargo",
            &[
                "build",
                "--release",
                "--package",
                "srr",
                "--target",
                WASM_TARGET,
            ],
        )),
        Step::Run(CommandSpec::new(
            "wasm-bindgen",
            &[
                "--target",
                "web",
                "--no-typescript",
                "--out-dir",
                BINDGEN_OUT_DIR,
                WASM_ARTIFACT,
            ],
        )),
    ]
}

fn dist_steps() -> Vec<Step> {
    let dist = Path::new(DIST_DIR);
    let bindgen_out = Path::new(BINDGEN_OUT_DIR);
    let mut steps = vec![
        Step::RemoveDir(dist.to_path_buf()),
        Step::CreateDir(dist.to_path_buf()),
        Step::Copy {
            from: PathBuf::from("web/index.html"),
            to: dist.join("index.html"),
        },
    ];
    steps.extend(["srr.js", "srr_bg.wasm"].map(|file| Step::Copy {
        from: bindgen_out.join(file),
        to: dist.join(file),
    }));
    steps
}

fn serve_step() -> Step {
    Step::Run(CommandSpec::new(
        "cargo",
        &[
            "run",
            "--release",
            "--package",
            "srr-server",
            "--",
            "--dir",
            DIST_DIR,
            "--port",
            SERVE_PORT,
        ],
    ))
}

fn docker_image_step() -> Step {
    Step::Run(CommandSpec::new(
        "docker",
        &[
            "build",
            "--target",
            DOCKER_TOOLCHAIN_STAGE,
            "-t",
            DOCKER_IMAGE,
            ".",
        ],
    ))
}

fn docker_run_step(inner: &Task, root: &Path) -> Step {
    let mount = format!("{}:/work", root.display());
    let publish = format!("{SERVE_PORT}:{SERVE_PORT}");
    let mut args = vec!["run", "--rm"];
    if *inner == Task::Serve {
        // `--init` forwards Ctrl+C to the server; `-p` lets the LAN reach it.
        args.extend(["--init", "-p", publish.as_str()]);
    }
    args.extend([
        "-v",
        mount.as_str(),
        // Keeps downloaded crates between throw-away containers.
        "-v",
        "srr-cargo-registry:/usr/local/cargo/registry",
        "-w",
        "/work",
        DOCKER_IMAGE,
        "cargo",
        "xtask",
        inner.name(),
    ]);
    Step::Run(CommandSpec::new("docker", &args))
}

// ---------------------------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------------------------

/// Why a step failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecError {
    /// The command ran and failed; `code` is `None` when it was killed by a signal.
    CommandFailed {
        /// The command as displayed by [`CommandSpec::display`].
        command: String,
        /// The exit code, if the process exited normally.
        code: Option<i32>,
    },
    /// The command could not be started at all (for instance, the program is not installed).
    SpawnFailed {
        /// The command as displayed by [`CommandSpec::display`].
        command: String,
        /// The operating system's explanation.
        message: String,
    },
    /// A file system operation failed.
    Io {
        /// The path the operation failed on.
        path: PathBuf,
        /// The operating system's explanation.
        message: String,
    },
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecError::CommandFailed {
                command,
                code: Some(code),
            } => write!(f, "`{command}` failed with exit code {code}"),
            ExecError::CommandFailed {
                command,
                code: None,
            } => write!(f, "`{command}` was terminated by a signal"),
            ExecError::SpawnFailed { command, message } => {
                write!(f, "could not start `{command}`: {message}")
            }
            ExecError::Io { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for ExecError {}

/// Performs steps; implemented by [`SystemExecutor`] and by test doubles.
pub trait Executor {
    /// Runs `command` to completion; a non-zero exit status is an error.
    fn run_command(&mut self, command: &CommandSpec) -> Result<(), ExecError>;
    /// Removes a directory tree; a missing directory is not an error.
    fn remove_dir(&mut self, path: &Path) -> Result<(), ExecError>;
    /// Creates a directory and its parents; an existing directory is not an error.
    fn create_dir(&mut self, path: &Path) -> Result<(), ExecError>;
    /// Copies the file `from` to `to`, overwriting `to`. The directory of `to` must exist.
    fn copy_file(&mut self, from: &Path, to: &Path) -> Result<(), ExecError>;
}

fn execute_step(step: &Step, executor: &mut impl Executor) -> Result<(), ExecError> {
    match step {
        Step::Run(command) => executor.run_command(command),
        Step::RemoveDir(path) => executor.remove_dir(path),
        Step::CreateDir(path) => executor.create_dir(path),
        Step::Copy { from, to } => executor.copy_file(from, to),
    }
}

/// Runs the steps in order and stops at the first failure.
///
/// # Errors
///
/// Returns the error of the first failing step; later steps are not executed.
///
/// Complexity: O(s) executor calls for `s` steps.
pub fn execute(steps: &[Step], executor: &mut impl Executor) -> Result<(), ExecError> {
    steps
        .iter()
        .try_for_each(|step| execute_step(step, executor))
}

fn describe(step: &Step) -> String {
    match step {
        Step::Run(command) => command.display(),
        Step::RemoveDir(path) => format!("remove {}", path.display()),
        Step::CreateDir(path) => format!("create {}", path.display()),
        Step::Copy { from, to } => format!("copy {} -> {}", from.display(), to.display()),
    }
}

/// The whole command line tool. Returns the exit code: 0 on success (or help), 1 when a step
/// failed, 2 on a usage error.
///
/// Each step is announced on `out` (as `==> <step>`) before it runs; usage errors and step
/// failures are reported on `err`.
///
/// Writing progress to `out` / `err` is best effort: a closed terminal must not change the
/// result of the build.
///
/// ```
/// use std::path::Path;
/// use xtask::{run_cli, SystemExecutor};
///
/// let mut executor = SystemExecutor::new(".".into());
/// let (mut out, mut err) = (Vec::new(), Vec::new());
/// let code = run_cli(["help"], Path::new("."), &mut executor, &mut out, &mut err);
/// assert_eq!(code, 0);
/// assert!(String::from_utf8(out).unwrap().contains("Usage: cargo xtask"));
/// ```
pub fn run_cli(
    args: impl IntoIterator<Item = impl AsRef<str>>,
    root: &Path,
    executor: &mut impl Executor,
    out: &mut impl Write,
    err: &mut impl Write,
) -> i32 {
    let task = match parse(args) {
        Ok(task) => task,
        Err(error) => {
            let _ = writeln!(err, "error: {error}\n\n{}", usage());
            return EXIT_USAGE;
        }
    };
    if task == Task::Help {
        let _ = write!(out, "{}", usage());
        return EXIT_SUCCESS;
    }
    for step in plan(&task, root) {
        let _ = writeln!(out, "==> {}", describe(&step));
        if let Err(error) = execute_step(&step, executor) {
            let _ = writeln!(err, "error: {error}");
            return EXIT_FAILURE;
        }
    }
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------------------------
// The real executor
// ---------------------------------------------------------------------------------------------

/// Where the standard output and error of the commands go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// To the terminal of `cargo xtask`, so its user sees the build.
    Inherit,
    /// Nowhere: tests run commands without printing anything.
    Discard,
}

impl Output {
    fn stdio(self) -> Stdio {
        match self {
            Output::Inherit => Stdio::inherit(),
            Output::Discard => Stdio::null(),
        }
    }
}

/// Runs commands in `root` and resolves relative paths against it.
#[derive(Debug, Clone)]
pub struct SystemExecutor {
    root: PathBuf,
    output: Output,
}

impl SystemExecutor {
    /// An executor working in the directory `root` (normally the project root), showing the
    /// output of its commands.
    pub fn new(root: PathBuf) -> Self {
        SystemExecutor {
            root,
            output: Output::Inherit,
        }
    }

    /// An executor working in `root` that discards the output of its commands (exit codes are
    /// still reported).
    pub fn quiet(root: PathBuf) -> Self {
        SystemExecutor {
            root,
            output: Output::Discard,
        }
    }

    /// Where the output of the commands goes.
    pub fn output(&self) -> Output {
        self.output
    }

    fn resolve(&self, path: &Path) -> PathBuf {
        self.root.join(path)
    }
}

fn io_error(path: &Path, error: io::Error) -> ExecError {
    ExecError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

impl Executor for SystemExecutor {
    fn run_command(&mut self, command: &CommandSpec) -> Result<(), ExecError> {
        let status = Command::new(&command.program)
            .args(&command.args)
            .current_dir(&self.root)
            .stdout(self.output.stdio())
            .stderr(self.output.stdio())
            .status()
            .map_err(|error| ExecError::SpawnFailed {
                command: command.display(),
                message: error.to_string(),
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(ExecError::CommandFailed {
                command: command.display(),
                code: status.code(),
            })
        }
    }

    fn remove_dir(&mut self, path: &Path) -> Result<(), ExecError> {
        let path = self.resolve(path);
        match fs::remove_dir_all(&path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(io_error(&path, error)),
            _ => Ok(()),
        }
    }

    fn create_dir(&mut self, path: &Path) -> Result<(), ExecError> {
        let path = self.resolve(path);
        fs::create_dir_all(&path).map_err(|error| io_error(&path, error))
    }

    fn copy_file(&mut self, from: &Path, to: &Path) -> Result<(), ExecError> {
        let from = self.resolve(from);
        let to = self.resolve(to);
        // Opening each side separately tells which file is at fault.
        let mut source = File::open(&from).map_err(|error| io_error(&from, error))?;
        File::create(&to)
            .and_then(|mut target| io::copy(&mut source, &mut target))
            .map(|_| ())
            .map_err(|error| io_error(&to, error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_outputs_make_a_stdio() {
        for output in [Output::Inherit, Output::Discard] {
            assert!(!format!("{:?}", output.stdio()).is_empty());
        }
    }

    #[test]
    fn removing_a_file_as_if_it_were_a_directory_is_an_io_error() {
        let root = std::env::temp_dir().join(format!("srr-xtask-unit-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("not-a-dir"), "x").unwrap();
        let mut executor = SystemExecutor::new(root.clone());
        let result = executor.remove_dir(Path::new("not-a-dir"));
        fs::create_dir_all(root.join("a-dir")).unwrap();
        assert_eq!(executor.remove_dir(Path::new("a-dir")), Ok(()));
        assert!(!root.join("a-dir").exists());
        let _ = fs::remove_dir_all(&root);
        let message = result.unwrap_err().to_string();
        assert!(message.contains("not-a-dir"), "{message}");
    }
}
