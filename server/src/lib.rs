//! Local network server for Shattered Realm: Rampage.
//!
//! One process does everything 2 to [`MAX_PLAYERS`] players on the same network need:
//!
//! * serves the web build (`dist/`) over HTTP, so every browser loads the game from it;
//! * accepts WebSocket connections on `/ws` and seats the first [`MAX_PLAYERS`] in the
//!   [`room`], which starts a free-for-all once everybody seated is ready, and on `/watch`
//!   the spectators (see [`room::Room::watch`]), who watch the same matches without a seat;
//! * gathers the inputs of the players into one frame per tick for everybody, without waiting
//!   long for a late one (the browsers run the match with rollback, see `srr::net`).
//!
//! Only the standard library is used: one thread reading each connection and one writing to
//! it, a clock thread, and a mutex around the room. Nothing ever writes to a socket while
//! holding the mutex, so a connection that stops reading cannot hold up the others: once its
//! queue of [`QUEUE_LENGTH`] messages is full, it is closed.
//!
//! Every connection costs threads, so their number is capped, and one that does not finish
//! its HTTP request in time is dropped (see [`Limits`]). Once upgraded to a WebSocket, a
//! connection may stay silent for as long as it likes: a player sits in the lobby without
//! sending anything until they are ready. A [`Stopper`] handle stops the whole server,
//! telling every browser first (the binary wires it to Ctrl+C).

pub mod http;
pub mod room;
pub mod websocket;

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader, Write};
use std::iter;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use room::{Action, ClientId, Room};
use srr::game::TICKS_PER_SECOND;
use srr::game::{Style, StyleSet};
use srr::net::MAX_PLAYERS;
use websocket::{OP_BINARY, OP_CLOSE, OP_PING, OP_PONG, OP_TEXT};

/// The port used when none is given.
pub const DEFAULT_PORT: u16 = 8080;
/// The directory served when none is given (the output of `cargo xtask dist`).
pub const DEFAULT_ROOT: &str = "dist";
/// The path of the WebSocket endpoint.
pub const WEBSOCKET_PATH: &str = "/ws";
/// The WebSocket endpoint of the spectators (see [`Room::watch`]): the same page with
/// `?spectate` in its URL opens this one instead.
pub const WATCH_PATH: &str = "/watch";
/// The period of the room's clock: the game's fixed tick.
pub const TICK: Duration = Duration::from_nanos(1_000_000_000 / TICKS_PER_SECOND as u64);
/// Messages waiting to be written to one client (about four seconds of frames) before the
/// server gives up on it and closes the connection.
pub const QUEUE_LENGTH: usize = 256;
/// The most connections open at once by default: the players plus browsers reloading the page
/// or fetching files in parallel.
pub const MAX_CONNECTIONS: usize = MAX_PLAYERS * 8;
/// How long a client has, by default, to send the head of its HTTP request.
pub const HEAD_TIMEOUT: Duration = Duration::from_secs(30);

/// What the server tolerates from its clients.
///
/// ```
/// use srr_server::{HEAD_TIMEOUT, Limits, MAX_CONNECTIONS};
///
/// let limits = Limits::default();
/// assert_eq!(limits.max_connections, MAX_CONNECTIONS);
/// assert_eq!(limits.head_timeout, HEAD_TIMEOUT);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Connections open at once; those beyond are closed straight away.
    pub max_connections: usize,
    /// How long a client has to send the head of its request before it is dropped.
    pub head_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_connections: MAX_CONNECTIONS,
            head_timeout: HEAD_TIMEOUT,
        }
    }
}

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Serve `root` on `port`, on every network interface, allowing `styles`.
    Serve {
        /// The TCP port.
        port: u16,
        /// The directory holding the web build.
        root: PathBuf,
        /// The styles the players may wear (see [`ServerMessage::Styles`]); the chalk is
        /// always allowed, whatever the set says.
        styles: StyleSet,
    },
    /// Print [`usage`].
    Help,
}

/// The help text.
pub fn usage() -> String {
    format!(
        "\
Usage: srr-server [--port PORT] [--dir DIR] [--styles STYLES]

Serves the web build of Shattered Realm: Rampage on the local network and runs the online
matches of up to {MAX_PLAYERS} players, everybody for themselves.

Options:
  --port PORT   TCP port to listen on (default {DEFAULT_PORT})
  --dir DIR     directory with the web build (default {DEFAULT_ROOT})
  --styles LIST the skins the players may wear, comma-separated among: stick,
                manga (default: all of them; the chalk stick figure is always allowed)
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
/// use srr::game::{Style, StyleSet};
/// use srr_server::{DEFAULT_PORT, parse_args, Command};
///
/// assert_eq!(
///     parse_args(["--port", "9000"]),
///     Ok(Command::Serve { port: 9000, root: "dist".into(), styles: StyleSet::all() })
/// );
/// assert_eq!(
///     parse_args(["--styles", "manga"]),
///     Ok(Command::Serve {
///         port: DEFAULT_PORT,
///         root: "dist".into(),
///         styles: StyleSet::none().with(Style::Manga),
///     })
/// );
/// assert!(parse_args(["--port", "x"]).is_err());
/// assert!(parse_args(["--styles", "nope"]).is_err());
/// ```
pub fn parse_args(args: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Command, String> {
    let mut port = DEFAULT_PORT;
    let mut root = PathBuf::from(DEFAULT_ROOT);
    let mut styles = StyleSet::all();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg.as_ref();
        match arg {
            "-h" | "--help" => return Ok(Command::Help),
            "--styles" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("`{arg}` needs a value"))?;
                styles = parse_styles(value.as_ref())?;
            }
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
    Ok(Command::Serve { port, root, styles })
}

/// The styles a `--styles` value names: comma-separated `stick`, `manga`.
///
/// # Errors
///
/// A message for a name no style has.
///
/// ```
/// use srr::game::{Style, StyleSet};
/// use srr_server::parse_styles;
///
/// assert_eq!(parse_styles("all"), Ok(StyleSet::all()));
/// assert_eq!(parse_styles("manga"), Ok(StyleSet::all().without(Style::Stick)));
/// assert!(parse_styles("nope").is_err());
/// ```
pub fn parse_styles(list: &str) -> Result<StyleSet, String> {
    if list.trim().eq_ignore_ascii_case("all") {
        return Ok(StyleSet::all());
    }
    let mut set = StyleSet::none();
    for name in list.split(',') {
        let style = match name.trim().to_ascii_lowercase().as_str() {
            "stick" | "chalk" => Style::Stick,
            "manga" => Style::Manga,
            other => return Err(format!("unknown style `{other}` (stick, manga)")),
        };
        set = set.with(style);
    }
    Ok(set)
}

/// Accepts connections until the server is stopped (see [`serve_with`]); blocks until then.
/// The room's clock ticks every [`TICK`].
pub fn serve(listener: TcpListener, root: PathBuf) {
    serve_with_tick(listener, root, TICK).join();
}

/// [`serve`] with the room's clock ticking every `tick` (tests slow it down to control when
/// the room gives up on late inputs).
pub fn serve_with_tick(listener: TcpListener, root: PathBuf, tick: Duration) -> ServerHandle {
    serve_with_styles(listener, root, tick, Limits::default(), StyleSet::all())
}

/// [`serve_with_tick`] within `limits` instead of the default ones. The server runs on its own
/// thread: stop it through [`ServerHandle::stopper`] and wait for it with [`ServerHandle::join`].
///
/// ```no_run
/// use std::net::TcpListener;
/// use srr_server::{Limits, serve_with, TICK};
///
/// let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
/// let server = serve_with(listener, ".".into(), TICK, Limits::default());
/// server.stopper().stop();
/// server.join();
/// ```
pub fn serve_with(
    listener: TcpListener,
    root: PathBuf,
    tick: Duration,
    limits: Limits,
) -> ServerHandle {
    serve_with_styles(listener, root, tick, limits, StyleSet::all())
}

/// [`serve_with`] allowing only `styles` (what `--styles` asked for; see
/// [`Command::Serve`]). The room tells every browser with [`ServerMessage::Styles`].
pub fn serve_with_styles(
    listener: TcpListener,
    root: PathBuf,
    tick: Duration,
    limits: Limits,
    styles: StyleSet,
) -> ServerHandle {
    let hub = Arc::new(Mutex::new(Hub {
        room: Room::with_styles(styles),
        ..Hub::default()
    }));
    let stopped = Arc::new(AtomicBool::new(false));
    {
        let clock = Arc::clone(&hub);
        let clock_stopped = Arc::clone(&stopped);
        thread::spawn(move || run_clock(&clock, tick, &clock_stopped));
    }
    // A stop while nobody connects must still be noticed: the listener is polled.
    let _ = listener.set_nonblocking(true);
    let open = Arc::new(AtomicUsize::new(0));
    let loop_stopped = Arc::clone(&stopped);
    let thread = thread::spawn(move || {
        loop {
            if loop_stopped.load(Ordering::SeqCst) {
                break;
            }
            let Ok((stream, _)) = listener.accept() else {
                thread::sleep(ACCEPT_POLL);
                continue;
            };
            // BSD platforms let an accepted socket inherit O_NONBLOCK from its listener.
            let _ = stream.set_nonblocking(false);
            // Too many connections: dropping this one closes it.
            let Some(slot) = Slot::take(&open, limits.max_connections) else {
                continue;
            };
            let hub = Arc::clone(&hub);
            let root = root.clone();
            thread::spawn(move || {
                let _slot = slot;
                // A failing connection only concerns its own client.
                let _ = handle_connection(stream, &root, &hub, limits.head_timeout);
            });
        }
        // Tell every client with a close frame (the writer threads flush it and hang up,
        // which ends the reading threads).
        let mut hub = lock(&hub);
        let clients: Vec<ClientId> = hub.writers.keys().copied().collect();
        for client in clients {
            hub.queue(client, Outgoing::Close);
        }
    });
    ServerHandle {
        shutdown: Stopper { stopped },
        thread,
    }
}

/// How long the accept loop waits between two polls of the nonblocking listener: short enough
/// to notice a stop within moments, long enough to keep the thread idle.
const ACCEPT_POLL: Duration = Duration::from_millis(5);

/// How to stop a running server from anywhere: a signal handler, another thread, [`ServerHandle`]
/// itself. Cloning it hands the same power to somebody else.
///
/// Stopping is graceful: no connection is accepted anymore, every browser is told with a close
/// frame (so its match ends), and the room's clock stops ticking.
#[derive(Clone)]
pub struct Stopper {
    stopped: Arc<AtomicBool>,
}

impl Stopper {
    /// Asks the server to stop; it does within a few [`ACCEPT_POLL`]s.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

/// A server started with [`serve_with`], running on its own thread.
pub struct ServerHandle {
    shutdown: Stopper,
    thread: thread::JoinHandle<()>,
}

impl ServerHandle {
    /// A [`Stopper`] that stops this server: keep it (or give it to a signal handler), the
    /// server runs on.
    pub fn stopper(&self) -> Stopper {
        self.shutdown.clone()
    }

    /// Waits until the server has stopped ([`Stopper::stop`]) and every client has been told.
    /// Without a stop this blocks forever, like the server does.
    pub fn join(self) {
        let _ = self.thread.join();
    }
}

/// One of the connections counted by `open`, given back when dropped (even by a panic).
struct Slot(Arc<AtomicUsize>);

impl Slot {
    /// A slot, unless `max` are taken already.
    fn take(open: &Arc<AtomicUsize>, max: usize) -> Option<Slot> {
        open.try_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
            (count < max).then_some(count + 1)
        })
        .ok()
        .map(|_| Slot(Arc::clone(open)))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
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

/// Ticks the room every `period`, on a fixed schedule (see [`ticks_due`]), until `stopped`.
fn run_clock(hub: &Mutex<Hub>, period: Duration, stopped: &AtomicBool) {
    let mut next = Instant::now() + period;
    while !stopped.load(Ordering::SeqCst) {
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
/// connection breaks. Whatever was queued meanwhile goes out in one write, so a client that
/// reads is not cut off just because the clock queued a burst of frames (after a hiccup).
fn write_queue(mut stream: TcpStream, queue: Receiver<Outgoing>) {
    let mut batch = Vec::new();
    while let Ok(first) = queue.recv() {
        let mut last = false;
        for outgoing in iter::once(first).chain(queue.try_iter().take(QUEUE_LENGTH)) {
            let (opcode, payload) = match outgoing {
                Outgoing::Frame(opcode, payload) => (opcode, payload),
                Outgoing::Close => {
                    last = true;
                    (OP_CLOSE, Vec::new())
                }
            };
            // Writing to a vector cannot fail.
            let _ = websocket::write_frame(&mut batch, opcode, &payload);
            if last {
                break;
            }
        }
        if stream.write_all(&batch).is_err() || last {
            let _ = stream.shutdown(Shutdown::Both);
            return;
        }
        batch.clear();
    }
}

fn lock(hub: &Mutex<Hub>) -> MutexGuard<'_, Hub> {
    // Every update of the hub is complete before anything can panic, so a poisoned hub is
    // still consistent.
    hub.lock().unwrap_or_else(PoisonError::into_inner)
}

fn handle_connection(
    stream: TcpStream,
    root: &Path,
    hub: &Mutex<Hub>,
    head_timeout: Duration,
) -> io::Result<()> {
    // Inputs are tiny and latency matters more than throughput, and a client that never
    // finishes its request would hold its thread forever. (std refuses a zero timeout.)
    let mut writer = stream
        .set_nodelay(true)
        .and_then(|()| stream.set_read_timeout(Some(head_timeout)))
        .and_then(|()| stream.try_clone())?;
    let mut reader = BufReader::new(stream);
    let head = http::read_head(&mut reader)?;
    let Some(request) = http::parse_request(&head) else {
        return writer.write_all(&http::error_response("400 Bad Request"));
    };
    if request.method != "GET" {
        return writer.write_all(&http::error_response("405 Method Not Allowed"));
    }
    if request.path() != WEBSOCKET_PATH && request.path() != WATCH_PATH {
        return serve_file(&mut writer, root, request.path());
    }
    let spectating = request.path() == WATCH_PATH;
    match request.header("sec-websocket-key") {
        Some(key) if request.is_websocket_upgrade() => {
            // Seated players stay silent in the lobby; a timeout in the middle of a frame
            // would also lose track of where the next one starts.
            reader
                .get_ref()
                .set_read_timeout(None)
                .and_then(|()| writer.write_all(&http::websocket_handshake(key)))
                .and_then(|()| writer.try_clone())
                .and_then(|stream| run_websocket(reader, writer, stream, hub, spectating))
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

/// Seats the client (or seats it among the spectators, when it connected to [`WATCH_PATH`]):
/// `writer` goes to its writing thread, `stream` (another handle on the same socket) stays in
/// the hub to cut the connection off.
fn run_websocket(
    mut reader: BufReader<TcpStream>,
    writer: TcpStream,
    stream: TcpStream,
    hub: &Mutex<Hub>,
    spectating: bool,
) -> io::Result<()> {
    let (queue, queued) = mpsc::sync_channel(QUEUE_LENGTH);
    thread::spawn(move || write_queue(writer, queued));
    let client = {
        let mut hub = lock(hub);
        let client = hub.next_id;
        hub.next_id += 1;
        hub.writers.insert(client, Writer { queue, stream });
        let actions = if spectating {
            hub.room.watch(client)
        } else {
            hub.room.join(client)
        };
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
                // A spectator is not seated, so this is ignored (a room only listens to the
                // connections it seated).
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
