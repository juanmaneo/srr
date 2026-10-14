//! Local network server for Shattered Realm: Rampage.
//!
//! One process does everything 2 to [`MAX_PLAYERS`] players on the same network need:
//!
//! * serves the web build (`dist/`) over HTTP, so every browser loads the game from it;
//! * accepts WebSocket connections on `/ws` and seats the first [`MAX_PLAYERS`] in the
//!   [`room`], which starts a free-for-all once everybody seated is ready;
//! * gathers the inputs of the players into one frame per tick for everybody, without waiting
//!   long for a late one (the browsers run the match with rollback, see `srr::net`).
//!
//! Only the standard library is used: one thread reading each connection and one writing to
//! it, a clock thread, and a mutex around the room. Nothing ever writes to a socket while
//! holding the mutex, so a connection that stops reading cannot hold up the others: once its
//! queue of [`QUEUE_LENGTH`] messages is full, it is closed.

pub mod http;
pub mod room;
pub mod websocket;

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use room::{Action, ClientId, Room};
use srr::game::TICKS_PER_SECOND;
use srr::net::MAX_PLAYERS;
use websocket::{OP_BINARY, OP_CLOSE, OP_PING, OP_PONG, OP_TEXT};

/// The port used when none is given.
pub const DEFAULT_PORT: u16 = 8080;
/// The directory served when none is given (the output of `cargo xtask dist`).
pub const DEFAULT_ROOT: &str = "dist";
/// The path of the WebSocket endpoint.
pub const WEBSOCKET_PATH: &str = "/ws";
/// The period of the room's clock: the game's fixed tick.
pub const TICK: Duration = Duration::from_nanos(1_000_000_000 / TICKS_PER_SECOND as u64);
/// Messages waiting to be written to one client (about four seconds of frames) before the
/// server gives up on it and closes the connection.
pub const QUEUE_LENGTH: usize = 256;

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

Serves the web build of Shattered Realm: Rampage on the local network and runs the online
matches of up to {MAX_PLAYERS} players, everybody for themselves.

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

/// Accepts connections forever, one thread each, serving files from `root`; the room's clock
/// ticks every [`TICK`].
pub fn serve(listener: TcpListener, root: PathBuf) {
    serve_with_tick(listener, root, TICK);
}

/// [`serve`] with the room's clock ticking every `tick` (tests slow it down to control when
/// the room gives up on late inputs).
pub fn serve_with_tick(listener: TcpListener, root: PathBuf, tick: Duration) {
    let hub = Arc::new(Mutex::new(Hub::default()));
    let clock = Arc::clone(&hub);
    thread::spawn(move || run_clock(&clock, tick));
    for stream in listener.incoming().flatten() {
        let hub = Arc::clone(&hub);
        let root = root.clone();
        thread::spawn(move || {
            // A failing connection only concerns its own client.
            let _ = handle_connection(stream, &root, &hub);
        });
    }
}

/// The ticks of a clock of period `period` that are due at `now`, the next one having been
/// due at `next`: how many to run now, and when the next one is due. After a hiccup (a
/// suspended machine) the clock catches up, but by a second of ticks at most; then it starts
/// afresh from `now`.
///
/// ```
/// use std::time::{Duration, Instant};
/// use srr_server::ticks_due;
///
/// let start = Instant::now();
/// let period = Duration::from_millis(10);
/// assert_eq!(ticks_due(start + period, start, period), (0, start + period));
/// // Due at `start`, two periods ago: that tick and the two since.
/// assert_eq!(ticks_due(start, start + period * 2, period), (3, start + period * 3));
/// ```
pub fn ticks_due(next: Instant, now: Instant, period: Duration) -> (u32, Instant) {
    let Some(late) = now.checked_duration_since(next) else {
        return (0, next);
    };
    let due = late.as_nanos() / period.as_nanos().max(1) + 1;
    match u32::try_from(due) {
        Ok(due) if due <= TICKS_PER_SECOND => (due, next + period * due),
        _ => (TICKS_PER_SECOND, now + period),
    }
}

/// Ticks the room every `period`, on a fixed schedule (see [`ticks_due`]).
fn run_clock(hub: &Mutex<Hub>, period: Duration) {
    let mut next = Instant::now() + period;
    loop {
        thread::sleep(next.saturating_duration_since(Instant::now()));
        let (ticks, after) = ticks_due(next, Instant::now(), period);
        next = after;
        let mut hub = lock(hub);
        for _ in 0..ticks {
            let actions = hub.room.tick();
            hub.apply(actions);
        }
    }
}

/// What the writing thread of a connection has to do.
#[derive(Debug)]
enum Outgoing {
    /// Write one WebSocket frame.
    Frame(u8, Vec<u8>),
    /// Write a close frame, then end the connection.
    Close,
}

/// The way to one client: the queue of its writing thread, and the socket, to cut it off.
#[derive(Debug)]
struct Writer {
    queue: SyncSender<Outgoing>,
    stream: TcpStream,
}

/// The room plus a way to write to every seated (or just connected) client.
#[derive(Debug, Default)]
struct Hub {
    room: Room,
    writers: HashMap<ClientId, Writer>,
    next_id: ClientId,
}

impl Hub {
    fn apply(&mut self, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Send(client, message) => self.send(client, OP_BINARY, &message.encode()),
                Action::Close(client) => self.queue(client, Outgoing::Close),
            }
        }
    }

    fn send(&mut self, client: ClientId, opcode: u8, payload: &[u8]) {
        self.queue(client, Outgoing::Frame(opcode, payload.to_vec()));
    }

    /// Best effort: a broken connection is noticed, and cleaned up, by its reading thread. A
    /// client that does not keep up (its queue is full) is cut off, which ends its reading
    /// thread too.
    fn queue(&mut self, client: ClientId, outgoing: Outgoing) {
        if let Some(writer) = self.writers.get(&client)
            && let Err(TrySendError::Full(_)) = writer.queue.try_send(outgoing)
        {
            let _ = writer.stream.shutdown(Shutdown::Both);
        }
    }
}

/// Writes the queued frames of one client until the queue closes (the client left) or the
/// connection breaks.
fn write_queue(mut stream: TcpStream, queue: Receiver<Outgoing>) {
    for outgoing in queue {
        let (opcode, payload, last) = match outgoing {
            Outgoing::Frame(opcode, payload) => (opcode, payload, false),
            Outgoing::Close => (OP_CLOSE, Vec::new(), true),
        };
        if websocket::write_frame(&mut stream, opcode, &payload).is_err() || last {
            let _ = stream.shutdown(Shutdown::Both);
            return;
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
    let (queue, queued) = mpsc::sync_channel(QUEUE_LENGTH);
    let stream = writer.try_clone()?;
    thread::spawn(move || write_queue(writer, queued));
    let client = {
        let mut hub = lock(hub);
        let client = hub.next_id;
        hub.next_id += 1;
        hub.writers.insert(client, Writer { queue, stream });
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
