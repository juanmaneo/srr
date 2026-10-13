//! Local network server for Shattered Realm: Rampage.
//!
//! One process does everything 2 to 4 players on the same network need:
//!
//! * serves the web build (`dist/`) over HTTP, so every browser loads the game from it;
//! * accepts WebSocket connections on `/ws` and seats the first four in the [`room`], which
//!   starts a free-for-all (1v1, 1v1v1 or 1v1v1v1) once everybody seated is ready;
//! * relays every input of a player to the others (the browsers run the match in lock-step,
//!   see `srr::net`).
//!
//! Only the standard library is used: one thread per connection, a mutex around the room.

pub mod http;
pub mod room;
pub mod websocket;

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use room::{Action, ClientId, Room};
use websocket::{OP_BINARY, OP_CLOSE, OP_PING, OP_PONG, OP_TEXT};

/// The port used when none is given.
pub const DEFAULT_PORT: u16 = 8080;
/// The directory served when none is given (the output of `cargo xtask dist`).
pub const DEFAULT_ROOT: &str = "dist";
/// The path of the WebSocket endpoint.
pub const WEBSOCKET_PATH: &str = "/ws";

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Serve `root` on `port`, on every network interface.
    Serve {
        /// The TCP port.
        port: u16,
        /// The directory holding the web build.
        root: PathBuf,
    },
    /// Print [`usage`].
    Help,
}

/// The help text.
pub fn usage() -> String {
    format!(
        "\
Usage: srr-server [--port PORT] [--dir DIR]

Serves the web build of Shattered Realm: Rampage on the local network and relays the inputs
of up to 4 players (1v1, 1v1v1 or 1v1v1v1).

Options:
  --port PORT   TCP port to listen on (default {DEFAULT_PORT})
  --dir DIR     directory with the web build (default {DEFAULT_ROOT})
  -h, --help    show this message
"
    )
}

/// Parses the arguments that follow the program name.
///
/// # Errors
///
/// A message for unknown options, missing values and invalid ports.
///
/// ```
/// use srr_server::{parse_args, Command};
///
/// assert_eq!(
///     parse_args(["--port", "9000"]),
///     Ok(Command::Serve { port: 9000, root: "dist".into() })
/// );
/// assert!(parse_args(["--port", "x"]).is_err());
/// ```
pub fn parse_args(args: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Command, String> {
    let mut port = DEFAULT_PORT;
    let mut root = PathBuf::from(DEFAULT_ROOT);
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg.as_ref();
        match arg {
            "-h" | "--help" => return Ok(Command::Help),
            "--port" | "--dir" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("`{arg}` needs a value"))?;
                let value = value.as_ref();
                if arg == "--port" {
                    port = value
                        .parse()
                        .map_err(|_| format!("invalid port `{value}`"))?;
                } else {
                    root = PathBuf::from(value);
                }
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }
    Ok(Command::Serve { port, root })
}

/// Accepts connections forever, one thread each, serving files from `root`.
pub fn serve(listener: TcpListener, root: PathBuf) {
    let hub = Arc::new(Mutex::new(Hub::default()));
    for stream in listener.incoming().flatten() {
        let hub = Arc::clone(&hub);
        let root = root.clone();
        thread::spawn(move || {
            // A failing connection only concerns its own client.
            let _ = handle_connection(stream, &root, &hub);
        });
    }
}

/// The room plus a way to write to every seated (or just connected) client.
#[derive(Debug, Default)]
struct Hub {
    room: Room,
    writers: HashMap<ClientId, TcpStream>,
    next_id: ClientId,
}

impl Hub {
    fn apply(&mut self, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Send(client, message) => self.send(client, OP_BINARY, &message.encode()),
                Action::Close(client) => {
                    self.send(client, OP_CLOSE, &[]);
                    if let Some(writer) = self.writers.get(&client) {
                        let _ = writer.shutdown(Shutdown::Both);
                    }
                }
            }
        }
    }

    /// Best effort: a broken connection is noticed, and cleaned up, by its reading thread.
    fn send(&mut self, client: ClientId, opcode: u8, payload: &[u8]) {
        if let Some(writer) = self.writers.get_mut(&client) {
            let _ = websocket::write_frame(writer, opcode, payload);
        }
    }
}

fn lock(hub: &Mutex<Hub>) -> MutexGuard<'_, Hub> {
    // Every update of the hub is complete before anything can panic, so a poisoned hub is
    // still consistent.
    hub.lock().unwrap_or_else(PoisonError::into_inner)
}

fn handle_connection(stream: TcpStream, root: &Path, hub: &Mutex<Hub>) -> io::Result<()> {
    // Inputs are tiny and latency matters more than throughput.
    stream.set_nodelay(true)?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let head = http::read_head(&mut reader)?;
    let Some(request) = http::parse_request(&head) else {
        return writer.write_all(&http::error_response("400 Bad Request"));
    };
    if request.method != "GET" {
        return writer.write_all(&http::error_response("405 Method Not Allowed"));
    }
    if request.path() != WEBSOCKET_PATH {
        return serve_file(&mut writer, root, request.path());
    }
    match request.header("sec-websocket-key") {
        Some(key) if request.is_websocket_upgrade() => {
            writer.write_all(&http::websocket_handshake(key))?;
            run_websocket(reader, writer, hub)
        }
        _ => writer.write_all(&http::error_response("400 Bad Request")),
    }
}

fn serve_file(writer: &mut TcpStream, root: &Path, path: &str) -> io::Result<()> {
    let found =
        http::resolve(root, path).and_then(|file| fs::read(&file).ok().map(|body| (file, body)));
    let response = match found {
        Some((file, body)) => http::response("200 OK", http::content_type(&file), &body),
        None => http::error_response("404 Not Found"),
    };
    writer.write_all(&response)
}

fn run_websocket(
    mut reader: BufReader<TcpStream>,
    writer: TcpStream,
    hub: &Mutex<Hub>,
) -> io::Result<()> {
    let client = {
        let mut hub = lock(hub);
        let client = hub.next_id;
        hub.next_id += 1;
        hub.writers.insert(client, writer);
        let actions = hub.room.join(client);
        hub.apply(actions);
        client
    };
    let result = pump(&mut reader, client, hub);
    let mut hub = lock(hub);
    let actions = hub.room.leave(client);
    hub.apply(actions);
    hub.writers.remove(&client);
    result
}

/// Handles the frames of one client until it closes the connection or breaks the protocol.
fn pump(reader: &mut BufReader<TcpStream>, client: ClientId, hub: &Mutex<Hub>) -> io::Result<()> {
    loop {
        let frame = websocket::read_frame(reader)?;
        let mut hub = lock(hub);
        match frame.opcode {
            OP_BINARY if frame.fin => {
                let actions = hub.room.receive(client, &frame.payload);
                hub.apply(actions);
            }
            OP_TEXT | OP_PONG if frame.fin => {}
            OP_PING => hub.send(client, OP_PONG, &frame.payload),
            OP_CLOSE => {
                hub.send(client, OP_CLOSE, &[]);
                return Ok(());
            }
            _ => {
                hub.apply(vec![Action::Close(client)]);
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "fragmented or unknown WebSocket frame",
                ));
            }
        }
    }
}
