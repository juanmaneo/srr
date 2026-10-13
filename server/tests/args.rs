use std::path::PathBuf;

use srr_server::{Command, DEFAULT_PORT, DEFAULT_ROOT, WEBSOCKET_PATH, parse_args, usage};

/// Every case goes through the same instantiation of the generic `parse_args`, so that one
/// instantiation alone covers all of its lines (coverage summaries do not merge instantiations).
fn parse(args: &[&str]) -> Result<Command, String> {
    parse_args(args)
}

fn serve(port: u16, root: &str) -> Result<Command, String> {
    Ok(Command::Serve {
        port,
        root: PathBuf::from(root),
    })
}

fn error(message: &str) -> Result<Command, String> {
    Err(message.to_string())
}

#[test]
fn defaults() {
    assert_eq!(DEFAULT_PORT, 8080);
    assert_eq!(DEFAULT_ROOT, "dist");
    assert_eq!(WEBSOCKET_PATH, "/ws");
    assert_eq!(parse(&[]), serve(8080, "dist"));
}

#[test]
fn port_and_dir() {
    assert_eq!(parse(&["--port", "9000"]), serve(9000, "dist"));
    assert_eq!(parse(&["--dir", "web"]), serve(8080, "web"));
    assert_eq!(
        parse(&["--dir", "a", "--port", "1", "--dir", "b"]),
        serve(1, "b")
    );
    // Other argument types work too.
    assert_eq!(
        parse_args(vec![String::from("--port"), String::from("0")]),
        serve(0, "dist")
    );
}

#[test]
fn help() {
    assert_eq!(parse(&["-h"]), Ok(Command::Help));
    assert_eq!(parse(&["--help"]), Ok(Command::Help));
    // Help wins even after other options, and before invalid ones.
    assert_eq!(
        parse(&["--port", "1", "--help", "--bogus"]),
        Ok(Command::Help)
    );
}

#[test]
fn errors() {
    assert_eq!(parse(&["--port"]), error("`--port` needs a value"));
    assert_eq!(parse(&["--dir"]), error("`--dir` needs a value"));
    assert_eq!(parse(&["--port", "x"]), error("invalid port `x`"));
    assert_eq!(parse(&["--port", "65536"]), error("invalid port `65536`"));
    assert_eq!(parse(&["--port", "-1"]), error("invalid port `-1`"));
    assert_eq!(parse(&["serve"]), error("unexpected argument `serve`"));
    assert_eq!(
        parse(&["--port", "1", "-x"]),
        error("unexpected argument `-x`")
    );
}

#[test]
fn usage_mentions_every_option_and_default() {
    let text = usage();
    assert!(text.starts_with("Usage: srr-server [--port PORT] [--dir DIR]\n"));
    for needle in [
        "--port PORT",
        "--dir DIR",
        "-h, --help",
        "(default 8080)",
        "(default dist)",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
    assert!(text.ends_with('\n'));
}

#[test]
fn commands_compare_and_clone() {
    let command = Command::Serve {
        port: 1,
        root: PathBuf::from("x"),
    };
    assert_eq!(command.clone(), command);
    assert_ne!(command, Command::Help);
    assert!(format!("{command:?}").contains("Serve"));
}
