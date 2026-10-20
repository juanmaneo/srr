//! The server over real localhost sockets: static files, the WebSocket handshake and the room.

use std::fs;
use std::io::{BufReader, ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use srr::game::{Input, PlayerId, TICKS_PER_SECOND};
use srr::net::{ClientMessage, MAX_PLAYERS, PlayerSet, ServerMessage};
use srr_server::http::read_head;
use srr_server::websocket::{OP_BINARY, OP_CLOSE, OP_CONTINUATION, OP_PING, OP_PONG, OP_TEXT};
use srr_server::{Limits, ServerHandle, Stopper, serve, serve_with, serve_with_tick, ticks_due};

const TIMEOUT: Duration = Duration::from_secs(10);
const KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
const ACCEPT: &str = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";
const FIN: u8 = 0x80;
const WASM: &[u8] = b"\0asm\x01\0\0\0";
const LEFT: Input = Input {
    left: true,
    ..Input::NONE
};
const JUMP_KICK: Input = Input {
    jump: true,
    kick_high: true,
    ..Input::NONE
};

/// A server on an ephemeral port, serving a fresh temporary directory.
struct Server {
    addr: SocketAddr,
    dir: PathBuf,
    /// How to stop and wait for this server; `None` for one serving forever on its own thread
    /// (only [`serve`] blocks, so only it needs the thread).
    handle: Option<ServerHandle>,
}

impl Server {
    /// A server whose clock does not tick during a test: frames go out only once complete.
    fn start() -> Server {
        Server::with_clock(Some(Duration::from_secs(3600)))
    }

    /// A server ticking every `tick`, or at the game's pace when `None`.
    fn with_clock(tick: Option<Duration>) -> Server {
        Server::new(tick, None)
    }

    /// A server whose clock does not tick during a test, within `limits`.
    fn with_limits(limits: Limits) -> Server {
        Server::new(Some(Duration::from_secs(3600)), Some(limits))
    }

    fn new(tick: Option<Duration>, limits: Option<Limits>) -> Server {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "srr-server-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = dir.join("dist");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("index.html"), "<h1>srr</h1>").unwrap();
        fs::write(root.join("srr_bg.wasm"), WASM).unwrap();
        fs::write(root.join("sub").join("index.html"), "sub").unwrap();
        fs::write(root.join("sub").join("data.json"), "{}").unwrap();
        fs::write(dir.join("secret.txt"), "secret").unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = match (tick, limits) {
            (Some(tick), limits) => Some(match limits {
                Some(limits) => serve_with(listener, root, tick, limits),
                None => serve_with_tick(listener, root, tick),
            }),
            (None, _) => {
                thread::spawn(move || serve(listener, root));
                None
            }
        };
        Server { addr, dir, handle }
    }

    /// The shutdown handle of a server started with a clock.
    fn stopper(&self) -> Stopper {
        self.handle
            .as_ref()
            .expect("this server has no handle")
            .stopper()
    }

    /// Waits for the server to stop (call [`Server::stopper`] first).
    fn join(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.join();
        }
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect(self.addr).unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        stream.set_write_timeout(Some(TIMEOUT)).unwrap();
        stream
    }

    /// Sends `request` and returns the status line, the head and the body of the response.
    fn http(&self, request: &str) -> (String, String, Vec<u8>) {
        let mut stream = self.connect();
        stream.write_all(request.as_bytes()).unwrap();
        let mut reader = BufReader::new(stream);
        let head = read_head(&mut reader).unwrap();
        let mut body = Vec::new();
        reader.read_to_end(&mut body).unwrap();
        let status = head.lines().next().unwrap().to_string();
        (status, head, body)
    }

    fn get(&self, path: &str) -> (String, String, Vec<u8>) {
        self.http(&format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"))
    }

    fn websocket(&self) -> Ws {
        let mut stream = self.connect();
        write!(
            stream,
            "GET /ws?v=1 HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\n\
             Connection: keep-alive, Upgrade\r\nSec-WebSocket-Key: {KEY}\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n"
        )
        .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let head = read_head(&mut reader).unwrap();
        assert!(
            head.starts_with("HTTP/1.1 101 Switching Protocols\r\n"),
            "{head}"
        );
        assert!(
            head.contains(&format!("Sec-WebSocket-Accept: {ACCEPT}\r\n")),
            "{head}"
        );
        Ws { stream, reader }
    }

    /// A WebSocket client that has been seated as `player`: welcomed, then shown the lobby.
    fn player(&self, player: u8) -> Ws {
        let mut ws = self.websocket();
        assert_eq!(
            ws.message(),
            ServerMessage::Welcome {
                player: PlayerId(player)
            }
        );
        let lobby = ws.message();
        assert!(
            matches!(lobby, ServerMessage::Lobby { seated, .. } if seated.contains(PlayerId(player))),
            "{lobby:?}"
        );
        ws
    }

    /// `N` clients seated as players `0..N`, all ready and told that session `1` started.
    fn match_of<const N: usize>(&self) -> [Ws; N] {
        let mut clients = std::array::from_fn(|player| self.player(player as u8));
        start(&mut clients.each_mut(), 1, PlayerSet::first(N));
        clients
    }
}

/// Every one of `clients` gets ready, then each expects session `session` between `players`.
fn start(clients: &mut [&mut Ws], session: u8, players: PlayerSet) {
    for ws in clients.iter_mut() {
        ws.send_ready();
    }
    for ws in clients.iter_mut() {
        ws.expect(ServerMessage::Start { session, players });
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A minimal WebSocket client: masks what it sends, reads unmasked server frames.
struct Ws {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Ws {
    fn send_frame(&mut self, first: u8, payload: &[u8]) {
        let mask = [0x12, 0x34, 0x56, 0x78];
        assert!(payload.len() < 126);
        let mut frame = vec![first, 0x80 | payload.len() as u8];
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        self.stream.write_all(&frame).unwrap();
    }

    fn send_message(&mut self, message: ClientMessage) {
        self.send_frame(FIN | OP_BINARY, &message.encode());
    }

    fn send_ready(&mut self) {
        self.send_message(ClientMessage::Ready);
    }

    fn send_input(&mut self, session: u8, tick: u32, input: Input) {
        self.send_message(ClientMessage::Input {
            session,
            tick,
            input,
        });
    }

    /// The next server frame: its first byte and its payload.
    fn frame(&mut self) -> (u8, Vec<u8>) {
        let mut header = [0; 2];
        self.reader
            .read_exact(&mut header)
            .expect("no frame from the server in time");
        assert_eq!(header[1] & 0x80, 0, "server frames are not masked");
        let length = match header[1] {
            126 => {
                let mut bytes = [0; 2];
                self.reader.read_exact(&mut bytes).unwrap();
                usize::from(u16::from_be_bytes(bytes))
            }
            127 => panic!("unexpectedly large frame"),
            short => usize::from(short),
        };
        let mut payload = vec![0; length];
        self.reader.read_exact(&mut payload).unwrap();
        (header[0], payload)
    }

    /// The very next server message.
    fn message(&mut self) -> ServerMessage {
        let (first, payload) = self.frame();
        assert_eq!(first, FIN | OP_BINARY);
        ServerMessage::decode(&payload).unwrap()
    }

    /// Reads messages until one is `accept`ed, skipping the [`ServerMessage::Lobby`] updates
    /// sent on the way. Any other message, or nothing acceptable within [`TIMEOUT`], fails.
    fn wait_for(&mut self, what: &str, accept: impl Fn(&ServerMessage) -> bool) -> ServerMessage {
        let deadline = Instant::now() + TIMEOUT;
        let found = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "timed out waiting for {what}");
            self.stream.set_read_timeout(Some(remaining)).unwrap();
            let message = self.message();
            if accept(&message) {
                break message;
            }
            assert!(
                matches!(message, ServerMessage::Lobby { .. }),
                "expected {what}, got {message:?}"
            );
        };
        self.stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        found
    }

    /// Waits for exactly `expected`, skipping lobby updates.
    fn expect(&mut self, expected: ServerMessage) {
        self.wait_for(&format!("{expected:?}"), |message| *message == expected);
    }

    /// Expects the frame of `tick` of session `session` between `players`, nobody late, as the
    /// very next message; `held` lists the inputs that are not [`Input::NONE`].
    fn expect_frame(&mut self, session: u8, tick: u32, players: PlayerSet, held: &[(u8, Input)]) {
        let mut inputs = [Input::NONE; PlayerSet::CAPACITY];
        for &(player, input) in held {
            inputs[usize::from(player)] = input;
        }
        assert_eq!(
            self.message(),
            ServerMessage::Frame {
                session,
                tick,
                players,
                late: PlayerSet::EMPTY,
                inputs,
            }
        );
    }

    /// Reads messages until `expected`, skipping frames and timing reports.
    fn expect_after_frames(&mut self, expected: ServerMessage) {
        loop {
            match self.message() {
                ServerMessage::Frame { .. } | ServerMessage::Timing { .. } => {}
                message => {
                    assert_eq!(message, expected);
                    return;
                }
            }
        }
    }

    /// Expects `player` to have left, followed by the lobby without them.
    fn expect_left(&mut self, player: u8, seated: PlayerSet) {
        assert_eq!(
            self.message(),
            ServerMessage::PlayerLeft {
                player: PlayerId(player)
            }
        );
        assert_eq!(
            self.message(),
            ServerMessage::Lobby {
                seated,
                ready: PlayerSet::EMPTY
            }
        );
    }

    /// Expects a close frame, then the end of the connection.
    fn expect_closed(&mut self) {
        assert_eq!(self.frame(), (FIN | OP_CLOSE, Vec::new()));
        self.expect_eof();
    }

    fn expect_eof(&mut self) {
        let mut rest = Vec::new();
        match self.reader.read_to_end(&mut rest) {
            Ok(_) => assert_eq!(rest, b""),
            Err(error) => assert_eq!(error.kind(), ErrorKind::ConnectionReset),
        }
    }
}

#[test]
fn serves_files_with_their_content_type() {
    // At the game's pace: the clock does nothing without a match.
    let server = Server::with_clock(None);

    let (status, head, body) = server.get("/");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(head.contains("Content-Type: text/html; charset=utf-8\r\n"));
    assert!(head.contains("Content-Length: 12\r\n"));
    assert!(head.contains("Cache-Control: no-store\r\n"));
    assert_eq!(body, b"<h1>srr</h1>");

    let (status, head, body) = server.get("/srr_bg.wasm");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(head.contains("Content-Type: application/wasm\r\n"));
    assert_eq!(body, WASM);

    let (status, _, body) = server.get("/sub/");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert_eq!(body, b"sub");

    let (status, head, body) = server.get("/sub/data.json?v=3");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(head.contains("Content-Type: application/json\r\n"));
    assert_eq!(body, b"{}");

    // Bare newlines are accepted as well.
    let (status, _, body) = server.http("GET /index.html HTTP/1.0\n\n");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert_eq!(body, b"<h1>srr</h1>");
}

#[test]
fn missing_files_are_not_found() {
    let server = Server::start();
    for path in ["/missing.js", "/sub", "/empty/", "/sub/missing/"] {
        let (status, _, body) = server.get(path);
        assert_eq!(status, "HTTP/1.1 404 Not Found", "{path}");
        assert_eq!(body, b"404 Not Found");
    }
}

#[test]
fn paths_leaving_the_root_are_refused() {
    let server = Server::start();
    for path in [
        "/../secret.txt",
        "/sub/../../secret.txt",
        "/..\\secret.txt",
        "relative",
        "*",
    ] {
        let (status, _, body) = server.get(path);
        assert_eq!(status, "HTTP/1.1 404 Not Found", "{path}");
        assert!(!body.windows(6).any(|window| window == b"secret"));
    }
}

#[test]
fn only_get_is_allowed() {
    let server = Server::start();
    for method in ["POST", "HEAD", "PUT"] {
        let (status, _, body) = server.http(&format!("{method} / HTTP/1.1\r\n\r\n"));
        assert_eq!(status, "HTTP/1.1 405 Method Not Allowed", "{method}");
        assert_eq!(body, b"405 Method Not Allowed");
    }
    // Even on the WebSocket endpoint.
    let (status, _, _) = server.http("POST /ws HTTP/1.1\r\n\r\n");
    assert_eq!(status, "HTTP/1.1 405 Method Not Allowed");
}

#[test]
fn garbage_is_a_bad_request() {
    let server = Server::start();
    for request in ["garbage\r\n\r\n", "GET / SPDY/3\r\n\r\n", "\r\n\r\n"] {
        let (status, _, body) = server.http(request);
        assert_eq!(status, "HTTP/1.1 400 Bad Request", "{request:?}");
        assert_eq!(body, b"400 Bad Request");
    }
}

#[test]
fn incomplete_or_huge_heads_get_no_answer() {
    let server = Server::start();

    // The client gives up before the end of the head.
    let mut stream = server.connect();
    stream.write_all(b"GET / HTTP/1.1\r\n").unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).unwrap();
    assert_eq!(answer, b"");

    // A head above the limit: the server hangs up.
    let mut stream = server.connect();
    let filler = format!("X-Filler: {}\r\n", "a".repeat(1000));
    let _ = stream.write_all(b"GET / HTTP/1.1\r\n");
    for _ in 0..20 {
        let _ = stream.write_all(filler.as_bytes());
    }
    let mut answer = Vec::new();
    match stream.read_to_end(&mut answer) {
        Ok(_) => assert_eq!(answer, b""),
        Err(error) => assert_eq!(error.kind(), ErrorKind::ConnectionReset),
    }
}

#[test]
fn websocket_endpoint_requires_an_upgrade() {
    let server = Server::start();
    let requests = [
        "GET /ws HTTP/1.1\r\n\r\n".to_string(),
        // A key, but no upgrade.
        format!("GET /ws HTTP/1.1\r\nSec-WebSocket-Key: {KEY}\r\n\r\n"),
        // An upgrade, but no key.
        "GET /ws HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n".to_string(),
    ];
    for request in requests {
        let (status, _, _) = server.http(&request);
        assert_eq!(status, "HTTP/1.1 400 Bad Request", "{request:?}");
    }
}

#[test]
fn readiness_is_shown_until_everybody_is_ready() {
    let server = Server::start();
    let mut first = server.player(0);

    // Ready alone: told so, but nothing starts.
    first.send_ready();
    assert_eq!(
        first.message(),
        ServerMessage::Lobby {
            seated: PlayerSet::first(1),
            ready: PlayerSet::first(1)
        }
    );

    // A newcomer sees who is ready.
    let mut second = server.websocket();
    assert_eq!(
        second.message(),
        ServerMessage::Welcome {
            player: PlayerId(1)
        }
    );
    let lobby = ServerMessage::Lobby {
        seated: PlayerSet::first(2),
        ready: PlayerSet::first(1),
    };
    assert_eq!(second.message(), lobby);
    assert_eq!(first.message(), lobby);

    second.send_ready();
    let start = ServerMessage::Start {
        session: 1,
        players: PlayerSet::first(2),
    };
    assert_eq!(first.message(), start);
    assert_eq!(second.message(), start);
}

#[test]
fn two_players_get_one_frame_per_tick() {
    let server = Server::start();
    let [mut first, mut second] = server.match_of();
    let both = PlayerSet::first(2);

    first.send_input(1, 0, LEFT);
    // Inputs of another session are dropped, and only the first input of a tick counts.
    second.send_input(7, 0, JUMP_KICK);
    first.send_input(1, 0, JUMP_KICK);
    second.send_input(1, 0, Input::NONE);
    first.expect_frame(1, 0, both, &[(0, LEFT)]);
    second.expect_frame(1, 0, both, &[(0, LEFT)]);

    second.send_input(1, 1, JUMP_KICK);
    first.send_input(1, 1, Input::NONE);
    first.expect_frame(1, 1, both, &[(1, JUMP_KICK)]);
    second.expect_frame(1, 1, both, &[(1, JUMP_KICK)]);
}

#[test]
fn three_players_play_a_free_for_all() {
    let server = Server::start();
    let mut clients: [Ws; 3] = server.match_of();
    let held = [(0, Input::NONE), (1, LEFT), (2, JUMP_KICK)];
    for (ws, &(_, input)) in clients.iter_mut().zip(&held) {
        ws.send_input(1, 0, input);
    }
    // Everybody gets the frame, the senders included.
    for ws in &mut clients {
        ws.expect_frame(1, 0, PlayerSet::first(3), &held);
    }
}

#[test]
fn a_late_joiner_waits_for_the_next_match() {
    let server = Server::start();
    let [mut first, mut second] = server.match_of();

    let mut third = server.player(2);
    let lobby = ServerMessage::Lobby {
        seated: PlayerSet::first(3),
        ready: PlayerSet::EMPTY,
    };
    assert_eq!(first.message(), lobby);
    assert_eq!(second.message(), lobby);

    // The duel goes on without them: their input is dropped, they get no frame.
    third.send_input(1, 0, LEFT);
    first.send_input(1, 0, LEFT);
    second.send_input(1, 0, Input::NONE);
    first.expect_frame(1, 0, PlayerSet::first(2), &[(0, LEFT)]);
    second.expect_frame(1, 0, PlayerSet::first(2), &[(0, LEFT)]);

    // Once everybody is ready, they play; `start` fails on any frame they got meanwhile.
    start(
        &mut [&mut first, &mut second, &mut third],
        2,
        PlayerSet::first(3),
    );
    first.send_input(2, 0, Input::NONE);
    second.send_input(2, 0, Input::NONE);
    third.send_input(2, 0, JUMP_KICK);
    for ws in [&mut first, &mut second, &mut third] {
        ws.expect_frame(2, 0, PlayerSet::first(3), &[(2, JUMP_KICK)]);
    }
}

#[test]
fn ten_players_brawl_and_the_eleventh_is_turned_away() {
    let server = Server::start();
    let mut clients: [Ws; MAX_PLAYERS] = server.match_of();

    let mut eleventh = server.websocket();
    assert_eq!(eleventh.message(), ServerMessage::RoomFull);
    eleventh.expect_closed();

    // The match goes on, nobody noticed.
    for ws in &mut clients {
        ws.send_input(1, 0, LEFT);
    }
    let held: Vec<(u8, Input)> = (0..MAX_PLAYERS as u8)
        .map(|player| (player, LEFT))
        .collect();
    for ws in &mut clients {
        ws.expect_frame(1, 0, PlayerSet::first(MAX_PLAYERS), &held);
    }
}

#[test]
fn the_match_goes_on_when_a_player_leaves() {
    let server = Server::start();
    let [mut first, second, mut third] = server.match_of();

    drop(second);
    let seated = PlayerSet::first(3).without(PlayerId(1));
    first.expect_left(1, seated);
    third.expect_left(1, seated);

    // Nobody waits for the leaver, whose fighter stands still.
    first.send_input(1, 0, LEFT);
    third.send_input(1, 0, Input::NONE);
    first.expect_frame(1, 0, PlayerSet::first(3), &[(0, LEFT)]);
    third.expect_frame(1, 0, PlayerSet::first(3), &[(0, LEFT)]);

    // The two others can play the next match.
    start(&mut [&mut first, &mut third], 2, seated);
    first.send_input(2, 0, LEFT);
    third.send_input(2, 0, LEFT);
    third.expect_frame(2, 0, seated, &[(0, LEFT), (2, LEFT)]);
}

#[test]
fn the_clock_sends_the_frames_and_cuts_off_a_client_that_stops_reading() {
    // A clock far faster than the game's: the frames pile up quickly.
    let server = Server::with_clock(Some(Duration::from_micros(20)));
    let [_deaf, mut listening] = server.match_of();
    // Nobody sends inputs, yet the frames come; the client that never reads is cut off once
    // its queue is full, and the match is over.
    listening.expect_after_frames(ServerMessage::PlayerLeft {
        player: PlayerId(0),
    });
    assert_eq!(
        listening.message(),
        ServerMessage::Lobby {
            seated: PlayerSet::first(2).without(PlayerId(0)),
            ready: PlayerSet::EMPTY
        }
    );
}

#[test]
fn the_clock_catches_up_a_second_at_most() {
    let start = Instant::now();
    let period = Duration::from_millis(10);
    assert_eq!(
        ticks_due(start + period, start, period),
        (0, start + period),
        "woken up early: nothing is due yet"
    );
    assert_eq!(
        ticks_due(start, start + period / 2, period),
        (1, start + period)
    );
    let late = start + period * TICKS_PER_SECOND;
    assert_eq!(
        ticks_due(start, late - period, period),
        (TICKS_PER_SECOND, late)
    );
    assert_eq!(
        ticks_due(start, late, period),
        (TICKS_PER_SECOND, late + period)
    );
    // Hours late, the clock starts afresh.
    let much_later = start + Duration::from_secs(3600);
    assert_eq!(
        ticks_due(start, much_later, period),
        (TICKS_PER_SECOND, much_later + period)
    );
}

#[test]
fn pings_are_answered_and_text_and_pongs_ignored() {
    let server = Server::start();
    let mut ws = server.player(0);

    // Nothing else arrives in between: the first frame after the lobby is the pong.
    ws.send_frame(FIN | OP_TEXT, b"hello");
    ws.send_frame(FIN | OP_PONG, b"unsolicited");
    ws.send_frame(FIN | OP_PING, b"ping!");
    assert_eq!(ws.frame(), (FIN | OP_PONG, b"ping!".to_vec()));

    // A non-final ping (invalid, but harmless) is answered too.
    ws.send_frame(OP_PING, b"");
    assert_eq!(ws.frame(), (FIN | OP_PONG, Vec::new()));
}

#[test]
fn close_is_echoed_and_the_others_told() {
    let server = Server::start();
    let [mut first, mut second] = server.match_of();

    first.send_frame(FIN | OP_CLOSE, &1000u16.to_be_bytes());
    first.expect_closed();
    second.expect_left(0, PlayerSet::first(2).without(PlayerId(0)));

    // A newcomer takes the free seat; a new session starts once both are ready.
    let mut third = server.player(0);
    start(&mut [&mut third, &mut second], 2, PlayerSet::first(2));
}

#[test]
fn fragmented_and_unknown_frames_close_the_connection() {
    let frames = [
        OP_TEXT,
        OP_BINARY,
        OP_PONG,
        FIN | OP_CONTINUATION,
        OP_CONTINUATION,
        FIN | 0x3,
        FIN | 0xB,
    ];
    let server = Server::start();
    let mut second = server.player(0);
    for (first, session) in frames.into_iter().zip(1..) {
        let mut ws = server.player(1);
        start(&mut [&mut ws, &mut second], session, PlayerSet::first(2));

        ws.send_frame(first, b"frag");
        ws.expect_closed();
        second.expect_left(1, PlayerSet::first(1));
    }
}

#[test]
fn malformed_messages_close_the_connection() {
    let server = Server::start();
    let [mut first, mut second] = server.match_of();

    first.send_frame(FIN | OP_BINARY, &[0xFF, 1, 2]);
    first.expect_closed();
    second.expect_left(0, PlayerSet::first(2).without(PlayerId(0)));
}

#[test]
fn unmasked_frames_end_the_connection() {
    let server = Server::start();
    let [mut first, mut second] = server.match_of();

    first.stream.write_all(&[FIN | OP_BINARY, 1, 0]).unwrap();
    first.expect_eof();
    second.expect_left(0, PlayerSet::first(2).without(PlayerId(0)));
}

#[test]
fn abrupt_disconnects_free_the_seat() {
    let server = Server::start();
    let [first, mut second] = server.match_of();
    let alone = PlayerSet::first(2).without(PlayerId(0));

    drop(first);
    second.expect_left(0, alone);

    let mut third = server.player(0);
    start(&mut [&mut third, &mut second], 2, PlayerSet::first(2));

    // Then everybody leaves; a fresh pair can play again.
    third.stream.shutdown(Shutdown::Both).unwrap();
    second.expect_left(0, alone);
    // The connection only ends once the server has freed the seat.
    second.send_frame(FIN | OP_CLOSE, b"");
    second.expect_closed();

    let mut ws = server.player(0);
    ws.send_frame(FIN | OP_PING, b"");
    assert_eq!(ws.frame(), (FIN | OP_PONG, Vec::new()));
}

#[test]
fn idle_connections_do_not_block_others() {
    let server = Server::start();
    // Connected but silent.
    let _idle = server.connect();
    let (status, _, _) = server.get("/");
    assert_eq!(status, "HTTP/1.1 200 OK");
}

/// Reads until the server hangs up, expecting no answer.
fn expect_hang_up(mut stream: TcpStream) {
    let mut answer = Vec::new();
    match stream.read_to_end(&mut answer) {
        Ok(_) => assert_eq!(answer, b""),
        Err(error) => assert_eq!(error.kind(), ErrorKind::ConnectionReset),
    }
}

#[test]
fn connections_beyond_the_limit_are_closed() {
    let server = Server::with_limits(Limits {
        max_connections: 2,
        ..Limits::default()
    });
    let first = server.connect();
    let _second = server.player(0);
    expect_hang_up(server.connect());

    // Once a connection ends, its slot is free again (as soon as the server notices).
    drop(first);
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let mut stream = server.connect();
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer);
        if answer.starts_with(b"HTTP/1.1 200 OK\r\n") {
            break;
        }
        assert!(Instant::now() < deadline, "the slot was never freed");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn stopping_closes_the_websockets_and_shuts_the_server_down() {
    // A fast clock, so the room's clock thread notices the stop within the test.
    let mut server = Server::with_clock(Some(Duration::from_millis(20)));
    let mut ws = server.player(0);

    server.stopper().stop();
    ws.expect_closed();
    server.join();
    thread::sleep(Duration::from_millis(50));

    // The listener is gone: nobody is served anymore.
    assert!(
        TcpStream::connect(server.addr).is_err(),
        "the server is down"
    );
}

#[test]
fn a_connection_whose_setup_fails_is_dropped_at_once() {
    // The operating system refuses a read timeout of zero, so the setup of every connection
    // fails: the server hangs up without a word and goes on.
    let server = Server::with_limits(Limits {
        head_timeout: Duration::ZERO,
        ..Limits::default()
    });
    expect_hang_up(server.connect());
    expect_hang_up(server.connect());
}

#[test]
fn requests_must_arrive_in_time_but_websockets_may_stay_silent() {
    let head_timeout = Duration::from_millis(100);
    let server = Server::with_limits(Limits {
        head_timeout,
        ..Limits::default()
    });

    // Half a request, then nothing: the server hangs up.
    let mut stream = server.connect();
    stream.write_all(b"GET / HTTP/1.1\r\n").unwrap();
    let start = Instant::now();
    expect_hang_up(stream);
    assert!(start.elapsed() < TIMEOUT);

    // A player waiting in the lobby sends nothing for longer, and is still there.
    let mut ws = server.player(0);
    thread::sleep(head_timeout * 3);
    ws.send_frame(FIN | OP_PING, b"still here");
    assert_eq!(ws.frame(), (FIN | OP_PONG, b"still here".to_vec()));
}
