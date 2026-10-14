//! `cargo xtask` command line parsing.
//!
//! Grammar:
//!
//! ```text
//! cargo xtask                      -> help
//! cargo xtask help | --help | -h   -> help
//! cargo xtask build                -> compile to WASM and run wasm-bindgen
//! cargo xtask test                 -> run every test of the workspace
//! cargo xtask coverage             -> run the tests and enforce 100 % coverage
//! cargo xtask dist                 -> build and assemble the distributable `dist/` folder
//! cargo xtask serve                -> dist, then serve it on the local network for online play
//! cargo xtask docker-image         -> build the Docker image
//! cargo xtask docker <task>        -> run build | test | coverage | dist | serve inside the image
//! ```

use xtask::{CliError, Task, parse, usage};

fn parse_args(args: &[&str]) -> Result<Task, CliError> {
    parse(args.iter().copied())
}

#[test]
fn the_plain_tasks_are_recognised() {
    assert_eq!(parse_args(&["build"]), Ok(Task::Build));
    assert_eq!(parse_args(&["test"]), Ok(Task::Test));
    assert_eq!(parse_args(&["coverage"]), Ok(Task::Coverage));
    assert_eq!(parse_args(&["dist"]), Ok(Task::Dist));
    assert_eq!(parse_args(&["serve"]), Ok(Task::Serve));
    assert_eq!(parse_args(&["screenshots"]), Ok(Task::Screenshots));
    assert_eq!(parse_args(&["docker-image"]), Ok(Task::DockerImage));
}

#[test]
fn no_arguments_and_the_help_spellings_all_mean_help() {
    assert_eq!(parse_args(&[]), Ok(Task::Help));
    assert_eq!(parse_args(&["help"]), Ok(Task::Help));
    assert_eq!(parse_args(&["--help"]), Ok(Task::Help));
    assert_eq!(parse_args(&["-h"]), Ok(Task::Help));
}

#[test]
fn docker_wraps_any_of_the_five_main_tasks() {
    for (name, task) in [
        ("build", Task::Build),
        ("test", Task::Test),
        ("coverage", Task::Coverage),
        ("dist", Task::Dist),
        ("serve", Task::Serve),
    ] {
        assert_eq!(
            parse_args(&["docker", name]),
            Ok(Task::Docker(Box::new(task)))
        );
    }
}

#[test]
fn an_unknown_task_is_rejected_with_its_name() {
    assert_eq!(
        parse_args(&["frobnicate"]),
        Err(CliError::UnknownTask("frobnicate".to_string()))
    );
}

#[test]
fn task_names_are_case_sensitive() {
    assert_eq!(
        parse_args(&["BUILD"]),
        Err(CliError::UnknownTask("BUILD".to_string()))
    );
}

#[test]
fn extra_arguments_are_rejected() {
    assert_eq!(
        parse_args(&["build", "extra"]),
        Err(CliError::UnexpectedArgument("extra".to_string()))
    );
    assert_eq!(
        parse_args(&["help", "me"]),
        Err(CliError::UnexpectedArgument("me".to_string()))
    );
    assert_eq!(
        parse_args(&["docker-image", "x"]),
        Err(CliError::UnexpectedArgument("x".to_string()))
    );
    assert_eq!(
        parse_args(&["docker", "build", "extra"]),
        Err(CliError::UnexpectedArgument("extra".to_string()))
    );
}

#[test]
fn docker_needs_a_task() {
    assert_eq!(parse_args(&["docker"]), Err(CliError::MissingDockerTask));
}

#[test]
fn docker_only_wraps_the_five_main_tasks() {
    for name in ["docker", "docker-image", "screenshots", "help", "nope"] {
        assert_eq!(
            parse_args(&["docker", name]),
            Err(CliError::InvalidDockerTask(name.to_string())),
            "docker {name}"
        );
    }
}

#[test]
fn task_names_round_trip_through_the_parser() {
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
        assert_eq!(parse_args(&[task.name()]), Ok(task.clone()), "{task:?}");
    }
}

#[test]
fn tasks_have_stable_names() {
    assert_eq!(Task::Build.name(), "build");
    assert_eq!(Task::Test.name(), "test");
    assert_eq!(Task::Coverage.name(), "coverage");
    assert_eq!(Task::Dist.name(), "dist");
    assert_eq!(Task::Serve.name(), "serve");
    assert_eq!(Task::Screenshots.name(), "screenshots");
    assert_eq!(Task::DockerImage.name(), "docker-image");
    assert_eq!(Task::Docker(Box::new(Task::Test)).name(), "docker");
    assert_eq!(Task::Help.name(), "help");
}

#[test]
fn owned_strings_work_as_arguments_too() {
    let args = vec!["docker".to_string(), "dist".to_string()];
    assert_eq!(parse(args), Ok(Task::Docker(Box::new(Task::Dist))));
}

#[test]
fn usage_lists_every_task() {
    let text = usage();
    for name in [
        "build",
        "test",
        "coverage",
        "dist",
        "serve",
        "screenshots",
        "docker",
        "docker-image",
        "help",
    ] {
        assert!(text.contains(name), "usage does not mention `{name}`");
    }
    assert!(text.contains("cargo xtask"));
}

#[test]
fn cli_errors_name_the_offending_argument() {
    assert!(
        CliError::UnknownTask("frobnicate".into())
            .to_string()
            .contains("frobnicate")
    );
    assert!(
        CliError::UnexpectedArgument("extra".into())
            .to_string()
            .contains("extra")
    );
    assert!(
        CliError::InvalidDockerTask("nope".into())
            .to_string()
            .contains("nope")
    );
    assert!(CliError::MissingDockerTask.to_string().contains("serve"));
    assert!(
        CliError::InvalidDockerTask("nope".into())
            .to_string()
            .contains("serve")
    );
}

#[test]
fn cli_error_is_a_std_error() {
    fn assert_error<E: std::error::Error>() {}
    assert_error::<CliError>();
}
