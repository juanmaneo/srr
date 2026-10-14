//! Integration tests of the online layer (`srr::net`): wire protocol, sets of players, rollback
//! sessions and the client state machine. Everything here is pure, so the "machines" are simply
//! values exchanging messages in memory. (The whole chain through the real server room, over
//! simulated bad networks, is tested in `server/tests/network.rs`.)

use std::collections::BTreeMap;
use std::ops::Range;

use srr::game::{
    Attack, ConfigError, Input, Match, MatchConfig, Outcome, Phase, PlayerId, PlayerSlot, Rival,
    Roster, RosterError, SetupError, Strength, TeamId,
};
use srr::net::{
    Client, ClientError, ClientMessage, DecodeError, FrameInputs, INPUT_DELAY, LAG_MEMORY,
    MAX_PLAYERS, MAX_ROLLBACK, PlayerSet, ServerMessage, Session, SessionError, Status,
    TARGET_LEAD,
};

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

/// No countdown and a 5-tick round: idle fighters reach a time-up draw almost at once.
fn quick_draw_config() -> MatchConfig {
    MatchConfig {
        countdown_ticks: 0,
        round_time_ticks: 5,
        ..MatchConfig::default()
    }
}

/// No countdown, a narrow arena and little health: a high punch of player 1 knocks player 2
/// out quickly.
fn one_punch_config() -> MatchConfig {
    MatchConfig {
        countdown_ticks: 0,
        arena_width: 200,
        max_health: 10,
        ..MatchConfig::default()
    }
}

fn no_countdown() -> MatchConfig {
    MatchConfig {
        countdown_ticks: 0,
        ..MatchConfig::default()
    }
}

fn idle() -> Input {
    Input::NONE
}

fn walk_right() -> Input {
    Input {
        right: true,
        ..Input::NONE
    }
}

fn walk_left() -> Input {
    Input {
        left: true,
        ..Input::NONE
    }
}

fn high_punch() -> Input {
    Input::pressing(Attack::punch(Strength::High))
}

fn p(player: u8) -> PlayerId {
    PlayerId(player)
}

fn set(players: &[u8]) -> PlayerSet {
    players.iter().map(|&player| PlayerId(player)).collect()
}

/// The inputs of a frame: `pairs` of player and input, everybody else idle.
fn inputs(pairs: &[(u8, Input)]) -> FrameInputs {
    let mut inputs = [Input::NONE; PlayerSet::CAPACITY];
    for &(player, input) in pairs {
        inputs[usize::from(player)] = input;
    }
    inputs
}

/// The server's frame for `tick`.
fn frame(
    session: u8,
    tick: u32,
    players: PlayerSet,
    late: PlayerSet,
    pairs: &[(u8, Input)],
) -> ServerMessage {
    ServerMessage::Frame {
        session,
        tick,
        players,
        late,
        inputs: inputs(pairs),
    }
}

/// One session per player of `players`.
fn machines(players: PlayerSet, config: &MatchConfig, id: u8, delay: u32) -> Vec<Session> {
    players
        .iter()
        .map(|player| Session::new(player, players, id, config.clone(), delay).unwrap())
        .collect()
}

/// The server's job on a perfect network: gather the inputs of every player and hand out each
/// tick's frame as soon as it is complete.
struct PerfectServer {
    players: PlayerSet,
    received: BTreeMap<u32, FrameInputs>,
    count: BTreeMap<u32, usize>,
    next: u32,
}

impl PerfectServer {
    fn new(players: PlayerSet) -> Self {
        PerfectServer {
            players,
            received: BTreeMap::new(),
            count: BTreeMap::new(),
            next: 0,
        }
    }

    fn submit(&mut self, player: PlayerId, ticks: Range<u32>, input: Input) {
        for tick in ticks {
            self.received
                .entry(tick)
                .or_insert([Input::NONE; PlayerSet::CAPACITY])[usize::from(player.0)] = input;
            *self.count.entry(tick).or_default() += 1;
        }
    }

    /// What a client sent: its inputs, decoded.
    fn submit_bytes(&mut self, player: PlayerId, bytes: &[u8]) {
        match ClientMessage::decode(bytes) {
            Ok(ClientMessage::Input { tick, input, .. }) => {
                self.submit(player, tick..tick + 1, input);
            }
            other => panic!("not an input: {other:?}"),
        }
    }

    /// The complete frames not handed out yet, in tick order.
    fn frames(&mut self) -> Vec<(u32, FrameInputs)> {
        let mut frames = Vec::new();
        while self.count.get(&self.next) == Some(&self.players.len()) {
            frames.push((self.next, self.received[&self.next]));
            self.next += 1;
        }
        frames
    }
}

/// One tick on a perfect network: every session submits `inputs[i]`, receives every complete
/// frame, and advances; all of them must agree, and none has to guess.
fn exchange(sessions: &mut [Session], server: &mut PerfectServer, inputs: &[Input]) {
    for (session, &input) in sessions.iter_mut().zip(inputs) {
        let ticks = session.submit_local(input);
        server.submit(session.local_player(), ticks, input);
    }
    for (tick, inputs) in server.frames() {
        for session in sessions.iter_mut() {
            assert!(session.receive_frame(session.id(), tick, &inputs, PlayerSet::EMPTY));
        }
    }
    for session in sessions.iter_mut() {
        assert!(session.advance());
    }
    for session in &sessions[1..] {
        assert_eq!(session.tick(), sessions[0].tick());
        assert_eq!(session.current(), sessions[0].current());
    }
}

/// Ticks every client once with `inputs[i]`, then hands everybody the complete frames.
fn serve_tick(clients: &mut [Client], server: &mut PerfectServer, inputs: &[Input]) {
    for (client, &input) in clients.iter_mut().zip(inputs) {
        let player = client.player().unwrap();
        for bytes in client.tick(input) {
            server.submit_bytes(player, &bytes);
        }
    }
    let players = server.players;
    for (tick, inputs) in server.frames() {
        for client in clients.iter_mut() {
            let id = client.session().unwrap().id();
            client.handle(ServerMessage::Frame {
                session: id,
                tick,
                players,
                late: PlayerSet::EMPTY,
                inputs,
            });
        }
    }
}

fn x_of(game: &Match, player: u8) -> i32 {
    game.fighter(PlayerId(player)).unwrap().x()
}

fn health_of(game: &Match, player: u8) -> i32 {
    game.fighter(PlayerId(player)).unwrap().health()
}

/// The decoded inputs a client sent.
fn decode_sent(frames: &[Vec<u8>]) -> Vec<(u8, u32, Input)> {
    frames
        .iter()
        .map(|frame| match ClientMessage::decode(frame) {
            Ok(ClientMessage::Input {
                session,
                tick,
                input,
            }) => (session, tick, input),
            other => panic!("not an input: {other:?}"),
        })
        .collect()
}

fn start(session: u8, players: PlayerSet) -> Vec<u8> {
    ServerMessage::Start { session, players }.encode()
}

/// An online client welcomed as `player` that started session `session` between `players`.
fn playing_client(
    config: MatchConfig,
    delay: u32,
    player: u8,
    session: u8,
    players: PlayerSet,
) -> Client {
    let mut client = Client::online(config, delay);
    client.receive(
        &ServerMessage::Welcome {
            player: PlayerId(player),
        }
        .encode(),
    );
    client.receive(&start(session, players));
    assert_eq!(client.status(), Status::Playing);
    client
}

// ---------------------------------------------------------------------------------------------
// Protocol
// ---------------------------------------------------------------------------------------------

#[test]
fn every_server_message_round_trips_through_its_bytes() {
    let messages = [
        ServerMessage::Welcome { player: p(0) },
        ServerMessage::Welcome { player: p(9) },
        ServerMessage::Start {
            session: 0,
            players: PlayerSet::first(2),
        },
        ServerMessage::Start {
            session: 255,
            players: set(&[0, 2, 9]),
        },
        frame(
            9,
            u32::MAX,
            set(&[0, 2, 9]),
            set(&[9]),
            &[
                (0, Input::pressing(Attack::kick(Strength::Medium))),
                (9, walk_left()),
            ],
        ),
        frame(1, 0, PlayerSet::EMPTY, PlayerSet::EMPTY, &[]),
        ServerMessage::PlayerLeft { player: p(1) },
        ServerMessage::RoomFull,
        ServerMessage::Lobby {
            seated: PlayerSet::first(MAX_PLAYERS),
            ready: set(&[1, 3, 8]),
        },
        ServerMessage::Lobby {
            seated: PlayerSet::EMPTY,
            ready: PlayerSet::EMPTY,
        },
        ServerMessage::Timing {
            tick: 0,
            lead: i8::MIN,
        },
        ServerMessage::Timing { tick: 7, lead: 0 },
        ServerMessage::Timing {
            tick: u32::MAX,
            lead: i8::MAX,
        },
    ];
    for message in messages {
        assert_eq!(ServerMessage::decode(&message.encode()), Ok(message));
    }
}

#[test]
fn server_messages_have_the_documented_layout() {
    assert_eq!(ServerMessage::Welcome { player: p(1) }.encode(), [1, 1]);
    assert_eq!(start(42, PlayerSet::first(3)), [2, 42, 0b111, 0]);
    assert_eq!(start(1, set(&[0, 9])), [2, 1, 0b1, 0b10]);
    assert_eq!(ServerMessage::PlayerLeft { player: p(2) }.encode(), [4, 2]);
    assert_eq!(ServerMessage::RoomFull.encode(), [5]);
    assert_eq!(
        ServerMessage::Lobby {
            seated: PlayerSet::first(10),
            ready: set(&[1]),
        }
        .encode(),
        [6, 0xff, 0b11, 0b10, 0]
    );
    assert_eq!(
        ServerMessage::Timing {
            tick: 0x0403_0201,
            lead: -2
        }
        .encode(),
        [9, 1, 2, 3, 4, 0xfe]
    );

    // A frame: the header, then the inputs of the players only, by increasing id.
    let a = Input::from_bits(0x0105);
    let b = Input::from_bits(0x0203);
    let bytes = frame(7, 0x0102_0304, set(&[1, 3]), set(&[3]), &[(3, b), (1, a)]).encode();
    assert_eq!(
        bytes,
        [
            8, 7, 0x04, 0x03, 0x02, 0x01, 0b1010, 0, 0b1000, 0, 0x05, 0x01, 0x03, 0x02
        ]
    );
}

#[test]
fn a_frame_only_carries_the_inputs_of_its_players() {
    let sent = frame(1, 5, set(&[0, 1]), PlayerSet::EMPTY, &[(4, walk_right())]);
    let Ok(ServerMessage::Frame { inputs, .. }) = ServerMessage::decode(&sent.encode()) else {
        panic!("a frame");
    };
    assert_eq!(
        inputs,
        [Input::NONE; PlayerSet::CAPACITY],
        "player 5 is not sent"
    );
}

#[test]
fn client_messages_round_trip_with_the_documented_layout() {
    let message = ClientMessage::Input {
        session: 3,
        tick: 0x0102_0304,
        input: high_punch(),
    };
    let bytes = message.encode();
    let bits = high_punch().to_bits().to_le_bytes();
    assert_eq!(bytes, [3, 3, 0x04, 0x03, 0x02, 0x01, bits[0], bits[1]]);
    assert_eq!(ClientMessage::decode(&bytes), Ok(message));
    assert_eq!(ClientMessage::Ready.encode(), [7]);
    assert_eq!(ClientMessage::decode(&[7]), Ok(ClientMessage::Ready));

    // The server never sends inputs one by one: only whole frames.
    assert_eq!(
        ServerMessage::decode(&bytes),
        Err(DecodeError::UnknownTag(3))
    );
}

#[test]
fn bits_beyond_the_input_are_ignored_on_the_wire() {
    assert_eq!(
        ClientMessage::decode(&[3, 1, 2, 0, 0, 0, 0xff, 0xff]),
        Ok(ClientMessage::Input {
            session: 1,
            tick: 2,
            input: Input::from_bits((1 << Input::BITS) - 1),
        })
    );
}

#[test]
fn client_messages_reject_empty_unknown_and_malformed_frames() {
    assert_eq!(ClientMessage::decode(&[]), Err(DecodeError::Empty));
    // Only inputs and readiness come from a browser.
    for tag in [0, 1, 2, 4, 5, 6, 8, 9, 255] {
        assert_eq!(
            ClientMessage::decode(&[tag, 0, 0, 0, 0, 0, 0, 0]),
            Err(DecodeError::UnknownTag(tag))
        );
    }
    let wrong = |tag, expected, actual| DecodeError::WrongLength {
        tag,
        expected,
        actual,
    };
    assert_eq!(
        ClientMessage::decode(&[3, 1, 0, 0, 0, 0, 0]),
        Err(wrong(3, 8, 7))
    );
    assert_eq!(ClientMessage::decode(&[3; 9]), Err(wrong(3, 8, 9)));
    assert_eq!(ClientMessage::decode(&[7, 0]), Err(wrong(7, 1, 2)));
}

#[test]
fn server_messages_reject_empty_unknown_and_malformed_frames() {
    assert_eq!(ServerMessage::decode(&[]), Err(DecodeError::Empty));
    for tag in [0, 3, 7, 200] {
        assert_eq!(
            ServerMessage::decode(&[tag]),
            Err(DecodeError::UnknownTag(tag))
        );
    }
    let wrong = |tag, expected, actual| DecodeError::WrongLength {
        tag,
        expected,
        actual,
    };
    assert_eq!(ServerMessage::decode(&[1]), Err(wrong(1, 2, 1)));
    assert_eq!(ServerMessage::decode(&[1, 0, 0]), Err(wrong(1, 2, 3)));
    assert_eq!(ServerMessage::decode(&[2, 0, 0]), Err(wrong(2, 4, 3)));
    assert_eq!(ServerMessage::decode(&[2; 5]), Err(wrong(2, 4, 5)));
    assert_eq!(ServerMessage::decode(&[4]), Err(wrong(4, 2, 1)));
    assert_eq!(ServerMessage::decode(&[4, 0, 0]), Err(wrong(4, 2, 3)));
    assert_eq!(ServerMessage::decode(&[5, 5]), Err(wrong(5, 1, 2)));
    assert_eq!(ServerMessage::decode(&[6, 0]), Err(wrong(6, 5, 2)));
    assert_eq!(ServerMessage::decode(&[9]), Err(wrong(9, 6, 1)));
    assert_eq!(
        ServerMessage::decode(&[9, 0, 0, 0, 0, 0, 0]),
        Err(wrong(9, 6, 7))
    );

    // A frame needs its whole header, then exactly one input per player.
    assert_eq!(ServerMessage::decode(&[8; 9]), Err(wrong(8, 10, 9)));
    let two_players = frame(1, 0, PlayerSet::first(2), PlayerSet::EMPTY, &[]).encode();
    assert_eq!(two_players.len(), 14);
    assert_eq!(
        ServerMessage::decode(&two_players[..12]),
        Err(wrong(8, 14, 12))
    );
    let mut longer = two_players.clone();
    longer.extend_from_slice(&[0, 0]);
    assert_eq!(ServerMessage::decode(&longer), Err(wrong(8, 14, 16)));
}

#[test]
fn decode_errors_explain_themselves() {
    assert_eq!(DecodeError::Empty.to_string(), "empty message");
    assert_eq!(
        DecodeError::UnknownTag(9).to_string(),
        "unknown message tag 9"
    );
    assert_eq!(
        DecodeError::WrongLength {
            tag: 3,
            expected: 8,
            actual: 2
        }
        .to_string(),
        "message with tag 3 must be 8 bytes long, got 2"
    );
    let boxed: Box<dyn std::error::Error> = Box::new(DecodeError::Empty);
    assert_eq!(boxed.to_string(), "empty message");
}

// ---------------------------------------------------------------------------------------------
// PlayerSet
// ---------------------------------------------------------------------------------------------

#[test]
fn player_sets_hold_a_full_room_of_ten() {
    assert_eq!(MAX_PLAYERS, 10);
    assert_eq!(PlayerSet::CAPACITY, 16);
    assert_eq!(PlayerSet::default(), PlayerSet::EMPTY);
    assert!(PlayerSet::EMPTY.is_empty());
    assert_eq!(PlayerSet::first(0), PlayerSet::EMPTY);
    assert_eq!(PlayerSet::first(MAX_PLAYERS).bits(), 0b11_1111_1111);
    assert_eq!(PlayerSet::first(100).bits(), 0xffff);
    assert_eq!(PlayerSet::first(100).len(), PlayerSet::CAPACITY);

    let trio = set(&[3, 0, 9]);
    assert_eq!(trio.len(), 3);
    assert!(!trio.is_empty());
    assert_eq!(trio.iter().collect::<Vec<_>>(), [p(0), p(3), p(9)]);
    assert!(trio.contains(p(9)) && !trio.contains(p(1)));
    assert_eq!(trio.with(p(1)).len(), 4);
    assert_eq!(trio.with(p(3)), trio, "already a member");
    assert_eq!(trio.without(p(3)), set(&[0, 9]));
    assert_eq!(trio.without(p(1)), trio, "not a member");
    assert_eq!(PlayerSet::from_bits(trio.bits()), trio);

    // Ids that do not fit in the two bytes are never members.
    assert_eq!(trio.with(p(16)), trio);
    assert_eq!(trio.with(p(255)), trio);
    assert!(!PlayerSet::first(16).contains(p(16)));
    assert_eq!(PlayerSet::first(16).without(p(200)), PlayerSet::first(16));
}

// ---------------------------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------------------------

#[test]
fn a_session_needs_the_local_player_among_its_players() {
    for player in [2, 3, 255] {
        let error = Session::new(
            p(player),
            PlayerSet::first(2),
            1,
            MatchConfig::default(),
            INPUT_DELAY,
        )
        .unwrap_err();
        assert_eq!(error, SessionError::NotInSession(p(player)));
        assert_eq!(
            error.to_string(),
            format!("player {player} does not take part in the match")
        );
    }
    // Checked first, before the players and the configuration.
    let broken = MatchConfig {
        gravity: 0,
        ..MatchConfig::default()
    };
    assert_eq!(
        Session::new(p(1), set(&[0]), 1, broken, 0).unwrap_err(),
        SessionError::NotInSession(p(1))
    );
}

#[test]
fn a_session_needs_at_least_two_players() {
    let error = Session::new(p(2), set(&[2]), 1, MatchConfig::default(), 0).unwrap_err();
    let roster = RosterError::TooFewPlayers { count: 1 };
    assert_eq!(error, SessionError::InvalidRoster(roster));
    assert_eq!(error.to_string(), roster.to_string());
}

#[test]
fn a_session_needs_a_valid_configuration() {
    let broken = MatchConfig {
        gravity: 0,
        ..MatchConfig::default()
    };
    let error = Session::new(p(0), PlayerSet::first(2), 1, broken, INPUT_DELAY).unwrap_err();
    let setup = SetupError::InvalidConfig(ConfigError::NonPositiveGravity);
    assert_eq!(error, SessionError::InvalidSetup(setup));
    assert_eq!(error.to_string(), setup.to_string());
    let boxed: Box<dyn std::error::Error> = Box::new(error);
    assert_eq!(boxed.to_string(), setup.to_string());
}

#[test]
fn a_new_session_describes_itself() {
    let config = MatchConfig::default();
    let players = PlayerSet::first(3);
    let slots = (0..3)
        .map(|i| PlayerSlot {
            player: p(i),
            team: TeamId(i),
        })
        .collect();
    let expected = Match::new(Roster::new(slots).unwrap(), config.clone()).unwrap();
    for local in players.iter() {
        let session = Session::new(local, players, 17, config.clone(), INPUT_DELAY).unwrap();
        assert_eq!(session.players(), players);
        assert_eq!(session.local_player(), local);
        assert_eq!(session.id(), 17);
        assert_eq!((session.tick(), session.confirmed_tick()), (0, 0));
        assert_eq!(session.current(), &expected);
        assert_eq!(session.confirmed(), &expected);
        assert!(!session.is_finished());
        assert_eq!(session.lagging(), PlayerSet::EMPTY);
        for player in 0..4 {
            assert_eq!(session.wins(p(player)), 0);
        }
    }
}

#[test]
fn everybody_fights_for_themselves_in_any_seats() {
    let players = set(&[0, 2, 9]);
    let session = Session::new(p(2), players, 1, MatchConfig::default(), 0).unwrap();
    let roster = session.current().roster();
    assert_eq!(roster.players(), [p(0), p(2), p(9)]);
    assert_eq!(roster.team_of(p(9)), Some(TeamId(9)));
    assert!(roster.are_enemies(p(0), p(2)) && roster.are_enemies(p(2), p(9)));
    assert!(session.current().fighter(p(1)).is_none());
    // Spread over the arena in seat order.
    assert!(x_of(session.current(), 0) < x_of(session.current(), 2));
    assert!(x_of(session.current(), 2) < x_of(session.current(), 9));
}

#[test]
fn a_session_of_ten_spreads_everybody_over_the_arena() {
    let session = Session::new(
        p(4),
        PlayerSet::first(MAX_PLAYERS),
        1,
        MatchConfig::default(),
        INPUT_DELAY,
    )
    .unwrap();
    let xs: Vec<i32> = (0..10)
        .map(|player| x_of(session.current(), player))
        .collect();
    assert!(xs.windows(2).all(|pair| pair[0] < pair[1]), "{xs:?}");
}

#[test]
fn local_inputs_are_scheduled_delay_ticks_ahead_and_never_further() {
    let mut session = Session::new(
        p(1),
        PlayerSet::first(2),
        5,
        MatchConfig::default(),
        INPUT_DELAY,
    )
    .unwrap();
    // The first call fills every tick up to the delay.
    assert_eq!(session.submit_local(walk_right()), 0..INPUT_DELAY + 1);
    // Already `delay` ticks ahead of tick 0: further inputs are dropped.
    assert_eq!(session.submit_local(walk_right()), 3..3);
    assert!(session.submit_local(idle()).is_empty());

    // One tick on, one more tick to schedule.
    assert!(session.advance());
    assert_eq!(
        session.submit_local(idle()),
        INPUT_DELAY + 1..INPUT_DELAY + 2
    );
    assert!(session.submit_local(idle()).is_empty());
}

#[test]
fn a_session_plays_ahead_by_guessing_up_to_the_rollback_window() {
    let mut session =
        Session::new(p(0), PlayerSet::first(3), 1, no_countdown(), INPUT_DELAY).unwrap();
    for tick in 1..=MAX_ROLLBACK {
        session.submit_local(idle());
        assert!(session.advance(), "tick {tick} is guessed");
        assert_eq!(session.tick(), tick);
    }
    assert_eq!(session.confirmed_tick(), 0, "nothing came from the server");

    // The window is full: the session waits, and drops the inputs scheduled beyond it.
    assert_eq!(
        session.submit_local(idle()).len(),
        1,
        "the last tick of the delay"
    );
    assert!(!session.advance());
    assert!(session.submit_local(idle()).is_empty());
    assert!(!session.advance());
    assert_eq!(session.tick(), MAX_ROLLBACK);

    // One frame: one more tick.
    assert!(session.receive_frame(1, 0, &inputs(&[]), PlayerSet::EMPTY));
    assert!(session.advance());
    assert_eq!(session.tick(), MAX_ROLLBACK + 1);
    assert!(!session.advance());
}

#[test]
fn the_others_are_guessed_to_hold_what_they_held_in_the_last_frame() {
    let config = no_countdown();
    let start = Match::new(Roster::free_for_all(3).unwrap(), config.clone()).unwrap();
    let players = PlayerSet::first(3);
    let mut session = Session::new(p(0), players, 1, config.clone(), 0).unwrap();

    // The server says player 3 walks left on tick 0; nothing more for now.
    assert!(session.receive_frame(1, 0, &inputs(&[(2, walk_left())]), PlayerSet::EMPTY));
    assert!(session.advance(), "caught up with the frame");
    assert_eq!(session.tick(), 1);
    for _ in 0..5 {
        session.submit_local(idle());
        session.advance();
    }
    // Six ticks of walking: one confirmed, five guessed.
    assert_eq!(
        x_of(session.current(), 2),
        x_of(&start, 2) - 6 * config.walk_speed
    );
    assert_eq!(
        x_of(session.confirmed(), 2),
        x_of(&start, 2) - config.walk_speed
    );
    assert_eq!(
        x_of(session.current(), 1),
        x_of(&start, 1),
        "player 2 stays"
    );
}

#[test]
fn a_wrong_guess_is_rolled_back_to_what_the_server_says() {
    let config = no_countdown();
    let players = PlayerSet::first(2);
    let mut guessing = Session::new(p(0), players, 1, config.clone(), 0).unwrap();
    // The reference knows every frame before playing.
    let mut knowing = Session::new(p(0), players, 1, config.clone(), 0).unwrap();

    // Ten ticks played ahead: the opponent is guessed to stand still.
    for _ in 0..10 {
        guessing.submit_local(walk_right());
        guessing.advance();
    }
    // But they walked left from tick 3 on: the frames rewrite the guess.
    let frames: Vec<FrameInputs> = (0..10)
        .map(|tick| {
            let theirs = if tick >= 3 { walk_left() } else { idle() };
            inputs(&[(0, walk_right()), (1, theirs)])
        })
        .collect();
    for (tick, frame) in (0..).zip(&frames) {
        assert!(guessing.receive_frame(1, tick, frame, PlayerSet::EMPTY));
        assert!(knowing.receive_frame(1, tick, frame, PlayerSet::EMPTY));
    }
    // The reference plays the ten frames; the guesses are still on screen until the next tick.
    for _ in 0..10 {
        assert!(knowing.advance());
    }
    assert_eq!(knowing.current(), knowing.confirmed());
    assert_ne!(guessing.current(), knowing.current());
    // Both play tick 10, the guessing one after correcting ticks 3 to 9.
    assert!(guessing.advance());
    assert!(knowing.advance());
    assert_eq!(guessing.tick(), 11);
    assert_eq!(guessing.current(), knowing.current());
}

#[test]
fn a_partial_correction_keeps_guessing_with_the_newest_inputs() {
    let config = no_countdown();
    let start = Match::new(Roster::duel(), config.clone()).unwrap();
    let mut session = Session::new(p(0), PlayerSet::first(2), 1, config.clone(), 0).unwrap();
    for _ in 0..8 {
        session.submit_local(idle());
        session.advance();
    }
    // Only the first two frames arrive: player 2 started walking left on tick 1.
    session.receive_frame(1, 0, &inputs(&[]), PlayerSet::EMPTY);
    session.receive_frame(1, 1, &inputs(&[(1, walk_left())]), PlayerSet::EMPTY);
    session.submit_local(idle());
    session.advance();
    // Tick 1 confirmed, ticks 2 to 8 guessed with the new input: 8 ticks of walking.
    assert_eq!(session.tick(), 9);
    assert_eq!(
        x_of(session.current(), 1),
        x_of(&start, 1) - 8 * config.walk_speed
    );
}

#[test]
fn the_server_overrides_the_local_input_it_did_not_get_in_time() {
    let config = no_countdown();
    let start = Match::new(Roster::duel(), config.clone()).unwrap();
    let mut session = Session::new(p(0), PlayerSet::first(2), 1, config.clone(), 0).unwrap();
    session.submit_local(walk_right());
    session.advance();
    assert_eq!(
        x_of(session.current(), 0),
        x_of(&start, 0) + config.walk_speed
    );

    // Our input was late: the server repeated our previous one (idle).
    assert!(session.receive_frame(1, 0, &inputs(&[]), set(&[0])));
    session.submit_local(idle());
    session.advance();
    assert_eq!(x_of(session.confirmed(), 0), x_of(&start, 0));
    assert_eq!(
        session.lagging(),
        set(&[0]),
        "we lag, as far as the others see"
    );
}

#[test]
fn frames_that_do_not_belong_are_ignored() {
    let players = set(&[0, 1, 3]);
    let mut session = Session::new(p(0), players, 4, MatchConfig::default(), INPUT_DELAY).unwrap();
    let none = inputs(&[]);
    assert!(
        !session.receive_frame(5, 0, &none, PlayerSet::EMPTY),
        "other session"
    );
    assert!(
        !session.receive_frame(4, 1, &none, PlayerSet::EMPTY),
        "out of order"
    );
    assert!(session.receive_frame(4, 0, &none, PlayerSet::EMPTY));
    assert!(
        !session.receive_frame(4, 0, &none, PlayerSet::EMPTY),
        "duplicate"
    );
    assert!(session.receive_frame(4, 1, &none, PlayerSet::EMPTY));
    assert_eq!(session.confirmed_tick(), 2);
}

#[test]
fn inputs_of_players_outside_the_session_are_ignored() {
    let config = no_countdown();
    let players = set(&[0, 1]);
    let mut with_noise = Session::new(p(0), players, 1, config.clone(), 0).unwrap();
    let mut clean = Session::new(p(0), players, 1, config, 0).unwrap();
    let noisy = inputs(&[(1, walk_left()), (5, high_punch())]);
    assert!(with_noise.receive_frame(1, 0, &noisy, set(&[5])));
    assert!(clean.receive_frame(1, 0, &inputs(&[(1, walk_left())]), PlayerSet::EMPTY));
    for _ in 0..3 {
        with_noise.advance();
        clean.advance();
    }
    assert_eq!(with_noise.current(), clean.current());
    assert_eq!(with_noise.lagging(), PlayerSet::EMPTY);
}

#[test]
fn a_session_left_behind_jumps_to_the_frames() {
    let mut session = Session::new(
        p(1),
        PlayerSet::first(2),
        1,
        MatchConfig::default(),
        INPUT_DELAY,
    )
    .unwrap();
    // The tab was in the background: 100 frames arrived, nothing was played.
    for tick in 0..100 {
        assert!(session.receive_frame(1, tick, &inputs(&[]), PlayerSet::EMPTY));
    }
    assert!(session.advance(), "one call catches up");
    assert_eq!((session.tick(), session.confirmed_tick()), (100, 100));
    assert_eq!(session.current(), session.confirmed());
    // The inputs for the ticks the server already decided are not sent.
    assert_eq!(session.submit_local(idle()), 100..100 + INPUT_DELAY + 1);
}

#[test]
fn a_late_input_marks_its_player_as_lagging_for_a_while() {
    let players = PlayerSet::first(3);
    let mut session = Session::new(p(0), players, 1, MatchConfig::default(), 0).unwrap();
    assert!(session.receive_frame(1, 0, &inputs(&[]), set(&[2])));
    assert_eq!(session.lagging(), set(&[2]));
    for tick in 1..LAG_MEMORY {
        session.receive_frame(1, tick, &inputs(&[]), PlayerSet::EMPTY);
    }
    assert_eq!(session.lagging(), set(&[2]), "still within the memory");
    session.receive_frame(1, LAG_MEMORY, &inputs(&[]), PlayerSet::EMPTY);
    assert_eq!(session.lagging(), PlayerSet::EMPTY);
}

#[test]
fn three_sessions_play_the_same_match() {
    let config = no_countdown();
    let players = PlayerSet::first(3);
    let mut all = machines(players, &config, 2, INPUT_DELAY);
    let mut server = PerfectServer::new(players);
    let start = all[0].current().clone();

    for i in 0..60 {
        let punch = if i % 7 == 0 { high_punch() } else { idle() };
        exchange(&mut all, &mut server, &[walk_right(), punch, idle()]);
    }
    // On a perfect network the frames are ahead of the round shown, by the input delay: the
    // round is played from them, nothing is guessed.
    assert_eq!(all[0].tick(), 60);
    assert_eq!(all[0].confirmed_tick(), 60 + INPUT_DELAY);
    assert_eq!(all[0].wins(p(0)), 0);
    let current = all[2].current();
    assert!(x_of(current, 0) > x_of(&start, 0));
    assert!(x_of(current, 0) <= x_of(&start, 0) + config.walk_speed * 62);
    assert!(
        x_of(current, 2) >= x_of(&start, 2),
        "player 3 never walks left"
    );
}

#[test]
fn ten_sessions_play_the_same_match() {
    let players = PlayerSet::first(MAX_PLAYERS);
    let mut all = machines(players, &no_countdown(), 9, 1);
    let mut server = PerfectServer::new(players);
    for i in 0..40u32 {
        let inputs: Vec<Input> = (0..10u32)
            .map(|player| match (i + player) % 4 {
                0 => walk_right(),
                1 => walk_left(),
                2 if i % 5 == 0 => high_punch(),
                _ => idle(),
            })
            .collect();
        exchange(&mut all, &mut server, &inputs);
    }
    assert_eq!((all[9].tick(), all[9].confirmed_tick()), (40, 41));
    assert_eq!(all[0].current().fighters().len(), 10);
}

#[test]
fn machines_hearing_from_the_server_at_different_times_agree() {
    let config = no_countdown();
    let players = PlayerSet::first(3);
    let mut all = machines(players, &config, 1, INPUT_DELAY);
    // Everybody walks and punches in a pattern the guesses keep getting wrong.
    let input_of = |player: u32, tick: u32| match (tick / (3 + player) + player) % 3 {
        0 => walk_right(),
        1 => high_punch(),
        _ => walk_left(),
    };
    let mut frames = Vec::new();
    for tick in 0..=120u32 {
        frames.push(inputs(&[
            (0, input_of(0, tick)),
            (1, input_of(1, tick)),
            (2, input_of(2, tick)),
        ]));
    }
    // Machine 1 hears every frame at once, machine 2 ten ticks late, machine 3 in bursts.
    for now in 0..120u32 {
        for (index, session) in all.iter_mut().enumerate() {
            let heard = match index {
                0 => now + 1,
                1 => now.saturating_sub(10),
                _ => now / 20 * 20,
            };
            while session.confirmed_tick() < heard {
                let tick = session.confirmed_tick();
                session.receive_frame(1, tick, &frames[tick as usize], PlayerSet::EMPTY);
            }
            let local = session.local_player().0;
            session.submit_local(input_of(u32::from(local), now));
            session.advance();
        }
    }
    // Then everybody hears one frame past the round shown (tick 120) and plays it.
    for session in all.iter_mut() {
        assert_eq!(session.tick(), 120);
        while session.confirmed_tick() <= 120 {
            let tick = session.confirmed_tick();
            session.receive_frame(1, tick, &frames[tick as usize], PlayerSet::EMPTY);
        }
        assert!(session.advance());
        assert_eq!(session.tick(), 121);
        assert_eq!(session.current(), session.confirmed());
    }
    for session in &all[1..] {
        assert_eq!(session.confirmed(), all[0].confirmed());
    }
}

#[test]
fn a_finished_round_stays_finished_and_counts_no_win_for_a_draw() {
    let config = quick_draw_config();
    let players = PlayerSet::first(2);
    let mut all = machines(players, &config, 1, 0);
    let mut server = PerfectServer::new(players);
    while !all[0].is_finished() {
        exchange(&mut all, &mut server, &[idle(), idle()]);
    }
    assert_eq!(all[0].current().outcome(), Some(Outcome::Draw));
    let finished = all[0].current().clone();
    let tick = all[0].tick();
    exchange(&mut all, &mut server, &[walk_right(), high_punch()]);
    assert_eq!(all[0].tick(), tick + 1, "the ticks still run");
    assert_eq!(all[0].current(), &finished, "the round does not change");
    assert_eq!(all[0].current().phase(), Phase::Finished(Outcome::Draw));
    assert_eq!(all[0].wins(p(0)) + all[0].wins(p(1)), 0);
}

#[test]
fn wins_are_counted_once_and_carried_to_the_next_round() {
    let config = one_punch_config();
    let players = PlayerSet::first(2);
    let mut all = machines(players, &config, 3, 0);
    let start = all[0].current().clone();
    assert!(x_of(&start, 1) - x_of(&start, 0) < config.punches[2].range);

    for round in 1..=2u8 {
        let mut server = PerfectServer::new(players);
        let mut ticks = 0;
        while !all[0].is_finished() {
            exchange(&mut all, &mut server, &[high_punch(), idle()]);
            ticks += 1;
            assert!(ticks < 100, "the punch must land");
        }
        assert_eq!(all[0].current().outcome(), Some(Outcome::Winner(TeamId(0))));
        assert!(health_of(all[0].current(), 1) <= 0);
        // Not counted again while the round stays finished.
        exchange(&mut all, &mut server, &[idle(), idle()]);
        for session in &all {
            assert_eq!(session.wins(p(0)), u32::from(round));
            assert_eq!(session.wins(p(1)), 0);
        }
        for session in all.iter_mut() {
            *session = session.next(3 + round);
            assert_eq!(session.current(), &start);
            assert_eq!(session.id(), 3 + round);
            assert_eq!((session.tick(), session.confirmed_tick()), (0, 0));
        }
    }
    assert_eq!(all[1].wins(p(0)), 2);
}

#[test]
fn a_guessed_win_is_shown_but_only_a_confirmed_one_is_carried() {
    let config = one_punch_config();
    let mut session = Session::new(p(0), PlayerSet::first(2), 1, config, 0).unwrap();
    let mut ticks = 0;
    while session.current().outcome().is_none() {
        session.submit_local(high_punch());
        assert!(session.advance());
        ticks += 1;
        assert!(
            ticks < MAX_ROLLBACK,
            "the punch must land within the guesses"
        );
    }
    assert_eq!(session.wins(p(0)), 1, "the round shown is won");
    assert!(!session.is_finished(), "but the server has not said so");
    assert_eq!(session.next(2).wins(p(0)), 0);

    // The server's frames agree with the guess: now it counts.
    for tick in 0..session.tick() {
        session.receive_frame(1, tick, &inputs(&[(0, high_punch())]), PlayerSet::EMPTY);
    }
    assert!(session.is_finished());
    assert_eq!(session.next(2).wins(p(0)), 1);
}

#[test]
fn the_next_session_forgets_everything_but_the_wins() {
    let mut session = Session::new(
        p(0),
        PlayerSet::first(3),
        1,
        MatchConfig::default(),
        INPUT_DELAY,
    )
    .unwrap();
    session.submit_local(walk_right());
    session.advance();
    session.receive_frame(1, 0, &inputs(&[]), set(&[1]));
    let mut next = session.next(2);
    assert_eq!(next.players(), PlayerSet::first(3));
    assert_eq!(next.local_player(), p(0));
    assert_eq!((next.tick(), next.confirmed_tick()), (0, 0));
    assert_eq!(next.lagging(), PlayerSet::EMPTY);
    assert_eq!(
        next.submit_local(idle()),
        0..INPUT_DELAY + 1,
        "the local schedule restarts"
    );
    assert!(
        !next.receive_frame(1, 0, &inputs(&[]), PlayerSet::EMPTY),
        "old session"
    );
}

#[test]
fn players_outside_the_session_have_no_wins() {
    let session = Session::new(p(1), set(&[1, 2]), 1, quick_draw_config(), 0).unwrap();
    for outsider in [0, 3, 9, 255] {
        assert_eq!(session.wins(p(outsider)), 0);
    }
}

// ---------------------------------------------------------------------------------------------
// Online client
// ---------------------------------------------------------------------------------------------

#[test]
fn an_online_client_starts_connecting_and_idle() {
    let mut client = Client::online(MatchConfig::default(), INPUT_DELAY);
    assert_eq!(client.status(), Status::Connecting);
    assert!(!client.is_local());
    assert_eq!(client.rival(), None);
    assert_eq!(client.bot_player(), None);
    assert_eq!(client.config(), &MatchConfig::default());
    assert_eq!(client.player(), None);
    assert!(client.session().is_none());
    assert_eq!(client.seated(), PlayerSet::EMPTY);
    assert_eq!(client.ready_players(), PlayerSet::EMPTY);
    assert_eq!(client.left_player(), None);
    assert_eq!(client.departed(), PlayerSet::EMPTY);
    assert_eq!(client.stalled_ticks(), 0);
    assert_eq!(client.pace(), 0);
    assert!(client.tick(walk_right()).is_empty());
    assert_eq!(client.ready(), None, "no seat yet");
    assert_eq!(client.stalled_ticks(), 0);
}

#[test]
fn an_online_client_goes_from_the_lobby_to_playing() {
    let mut client = Client::online(MatchConfig::default(), INPUT_DELAY);
    client.receive(&ServerMessage::Welcome { player: p(1) }.encode());
    assert_eq!(client.status(), Status::Lobby);
    assert_eq!(client.player(), Some(p(1)));
    assert!(client.session().is_none());
    assert!(client.tick(idle()).is_empty(), "nothing to do in the lobby");

    client.receive(
        &ServerMessage::Lobby {
            seated: PlayerSet::first(3),
            ready: set(&[0]),
        }
        .encode(),
    );
    assert_eq!(client.seated(), PlayerSet::first(3));
    assert_eq!(client.ready_players(), set(&[0]));
    assert_eq!(client.ready(), Some(vec![7]), "Enter: ready");
    assert_eq!(client.status(), Status::Lobby, "until the server starts");

    // A frame or a timing report before any session is dropped silently.
    client.receive(&frame(1, 0, PlayerSet::first(3), PlayerSet::EMPTY, &[]).encode());
    client.receive(&ServerMessage::Timing { tick: 0, lead: -20 }.encode());
    assert!(client.session().is_none());
    assert_eq!(client.pace(), 0);

    client.receive(&start(6, PlayerSet::first(3)));
    assert_eq!(client.status(), Status::Playing);
    assert_eq!(client.ready_players(), PlayerSet::EMPTY, "used up");
    assert_eq!(client.seated(), PlayerSet::first(3));
    let session = client.session().unwrap();
    assert_eq!(session.id(), 6);
    assert_eq!(session.players(), PlayerSet::first(3));
    assert_eq!(session.local_player(), p(1));
    assert_eq!(session.tick(), 0);
}

#[test]
fn a_playing_client_sends_its_inputs_ahead_and_plays_on() {
    let mut client = playing_client(
        MatchConfig::default(),
        INPUT_DELAY,
        0,
        8,
        PlayerSet::first(2),
    );
    let sent = decode_sent(&client.tick(high_punch()));
    let expected: Vec<(u8, u32, Input)> = (0..=INPUT_DELAY)
        .map(|tick| (8, tick, high_punch()))
        .collect();
    assert_eq!(sent, expected, "every tick up to the delay");
    assert_eq!(client.session().unwrap().tick(), 1, "without waiting");
    assert_eq!(client.stalled_ticks(), 0);

    let sent = decode_sent(&client.tick(idle()));
    assert_eq!(sent, [(8, INPUT_DELAY + 1, idle())]);
    assert_eq!(client.session().unwrap().tick(), 2);
}

#[test]
fn frames_from_the_server_reach_the_session() {
    let players = PlayerSet::first(2);
    let mut client = playing_client(MatchConfig::default(), INPUT_DELAY, 1, 2, players);
    client.tick(idle());

    // Another session's frame, or a frame of other players, is ignored.
    client.handle(frame(3, 0, players, PlayerSet::EMPTY, &[]));
    client.handle(frame(2, 0, PlayerSet::first(3), PlayerSet::EMPTY, &[]));
    assert_eq!(client.session().unwrap().confirmed_tick(), 0);

    client.handle(frame(2, 0, players, set(&[0]), &[(0, walk_right())]));
    let session = client.session().unwrap();
    assert_eq!(session.confirmed_tick(), 1);
    assert_eq!(session.lagging(), set(&[0]));
    assert_eq!(client.status(), Status::Playing);
}

#[test]
fn a_client_waits_for_a_silent_server_and_goes_on_when_it_speaks() {
    let players = PlayerSet::first(3);
    let mut client = playing_client(MatchConfig::default(), INPUT_DELAY, 0, 1, players);
    for _ in 0..MAX_ROLLBACK {
        assert!(!client.tick(idle()).is_empty());
        assert_eq!(client.stalled_ticks(), 0);
    }
    assert_eq!(client.session().unwrap().tick(), MAX_ROLLBACK);

    // The last input of the delay still goes out; then the inputs are dropped.
    assert_eq!(client.tick(idle()).len(), 1);
    assert_eq!(client.stalled_ticks(), 1);
    assert!(client.tick(idle()).is_empty());
    assert_eq!(client.stalled_ticks(), 2);
    assert_eq!(client.session().unwrap().tick(), MAX_ROLLBACK);

    client.handle(frame(1, 0, players, PlayerSet::EMPTY, &[]));
    client.tick(idle());
    assert_eq!(client.stalled_ticks(), 0);
    assert_eq!(client.session().unwrap().tick(), MAX_ROLLBACK + 1);
}

#[test]
fn a_client_keeps_in_time_with_the_server() {
    let mut client = playing_client(
        MatchConfig::default(),
        INPUT_DELAY,
        0,
        1,
        PlayerSet::first(2),
    );
    client.tick(idle());

    // Inputs a tick short of the target margin: run one tick twice.
    let short = ServerMessage::Timing {
        tick: 2,
        lead: (TARGET_LEAD - 1) as i8,
    };
    client.handle(short);
    assert_eq!(client.pace(), 1);
    let sent = decode_sent(&client.tick(walk_right()));
    assert_eq!(sent.len(), 2, "two ticks, two inputs");
    assert!(sent.iter().all(|&(_, _, input)| input == walk_right()));
    assert_eq!(client.session().unwrap().tick(), 3);
    assert_eq!(client.pace(), 0);

    // A report about an input sent before that correction already counts it: no more.
    client.handle(short);
    assert_eq!(client.pace(), 0);

    // Too late: catch up half the gap.
    client.handle(ServerMessage::Timing { tick: 4, lead: -10 });
    assert_eq!(client.pace(), (TARGET_LEAD + 11) / 2);

    // Within the margin: nothing to change, even if a correction was planned.
    client.handle(ServerMessage::Timing {
        tick: 4,
        lead: (TARGET_LEAD + 4) as i8,
    });
    assert_eq!(client.pace(), 0);

    // Far too early: skip ticks (without counting them as stalls).
    client.handle(ServerMessage::Timing {
        tick: 4,
        lead: (TARGET_LEAD + 6) as i8,
    });
    assert_eq!(client.pace(), -3);
    assert!(client.tick(idle()).is_empty());
    assert_eq!(client.session().unwrap().tick(), 3);
    assert_eq!(client.stalled_ticks(), 0);
    assert_eq!(client.pace(), -2);

    // A new match starts in time.
    client.receive(&start(2, PlayerSet::first(2)));
    assert_eq!(client.pace(), 0);
}

#[test]
fn a_far_client_delays_its_inputs_instead_of_guessing_further_ahead() {
    let mut client = playing_client(
        MatchConfig::default(),
        INPUT_DELAY,
        0,
        1,
        PlayerSet::first(2),
    );
    for _ in 0..15 {
        client.tick(idle());
    }
    assert_eq!(client.session().unwrap().tick(), 15);

    // Late, but already 15 ticks ahead of the frames: each catch-up tick schedules the input
    // one tick further instead of running twice.
    client.handle(ServerMessage::Timing { tick: 0, lead: -4 });
    assert_eq!(client.pace(), 3);
    for (delay, first) in [(3, 17), (4, 19), (5, 21)] {
        let sent = decode_sent(&client.tick(idle()));
        assert_eq!(sent, [(1, first, idle()), (1, first + 1, idle())]);
        assert_eq!(client.session().unwrap().delay(), delay);
    }
    assert_eq!(client.session().unwrap().tick(), 18);
    assert_eq!(client.pace(), 0);

    // The connection got better: give the delay back first (the round shown still moves on),
    // then skip ticks.
    client.handle(ServerMessage::Timing { tick: 22, lead: 20 });
    assert_eq!(client.pace(), -9);
    for (delay, tick) in [(4, 19), (3, 20), (2, 21)] {
        assert!(client.tick(idle()).is_empty());
        let session = client.session().unwrap();
        assert_eq!((session.delay(), session.tick()), (delay, tick));
    }
    assert!(client.tick(idle()).is_empty());
    let session = client.session().unwrap();
    assert_eq!((session.delay(), session.tick()), (INPUT_DELAY, 21));
    assert_eq!(client.stalled_ticks(), 0);
}

#[test]
fn stalled_ticks_count_against_the_margin() {
    let mut client = playing_client(
        MatchConfig::default(),
        INPUT_DELAY,
        0,
        1,
        PlayerSet::first(2),
    );
    for _ in 0..MAX_ROLLBACK + 5 {
        client.tick(idle());
    }
    assert_eq!(client.stalled_ticks(), 5);

    // On time when the report was made, but five ticks were lost since.
    client.handle(ServerMessage::Timing {
        tick: 0,
        lead: TARGET_LEAD as i8,
    });
    assert_eq!(client.pace(), 3);
}

#[test]
fn three_clients_through_a_server_play_the_same_match() {
    let players = PlayerSet::first(3);
    let mut clients: Vec<Client> = (0..3)
        .map(|player| playing_client(no_countdown(), INPUT_DELAY, player, 1, players))
        .collect();
    let mut server = PerfectServer::new(players);
    for i in 0..30 {
        let punch = if i % 5 == 0 { high_punch() } else { idle() };
        serve_tick(&mut clients, &mut server, &[walk_right(), punch, idle()]);
    }
    for client in clients.iter_mut() {
        client.tick(idle());
    }
    let sessions: Vec<&Session> = clients.iter().map(|c| c.session().unwrap()).collect();
    assert_eq!(sessions[0].confirmed_tick(), 30 + INPUT_DELAY);
    for session in &sessions[1..] {
        assert_eq!(session.tick(), sessions[0].tick());
        assert_eq!(session.current(), sessions[0].current());
    }
}

#[test]
fn ready_is_sent_in_the_lobby_or_once_the_server_says_the_round_is_over() {
    let players = PlayerSet::first(2);
    let mut client = playing_client(one_punch_config(), 0, 0, 1, players);
    assert_eq!(client.ready(), None, "not during a round");

    // Played ahead of the server: the round shown ends before the server confirms it.
    let mut ticks = 0;
    while client.session().unwrap().current().outcome().is_none() {
        client.tick(high_punch());
        let session = client.session().unwrap();
        if session.tick() - session.confirmed_tick() > 5 {
            let tick = session.confirmed_tick();
            client.handle(frame(
                1,
                tick,
                players,
                PlayerSet::EMPTY,
                &[(0, high_punch())],
            ));
        }
        ticks += 1;
        assert!(ticks < 100, "the punch must land");
    }
    assert_eq!(client.ready(), None, "not confirmed yet");

    while client.session().unwrap().confirmed_tick() < client.session().unwrap().tick() {
        let tick = client.session().unwrap().confirmed_tick();
        client.handle(frame(
            1,
            tick,
            players,
            PlayerSet::EMPTY,
            &[(0, high_punch())],
        ));
    }
    assert_eq!(client.ready(), Some(ClientMessage::Ready.encode()));
    assert_eq!(client.status(), Status::Playing, "the round stays shown");
}

#[test]
fn a_new_start_with_the_same_players_keeps_the_wins() {
    let players = PlayerSet::first(2);
    let mut clients: Vec<Client> = (0..2)
        .map(|player| playing_client(one_punch_config(), 0, player, 1, players))
        .collect();
    let mut server = PerfectServer::new(players);
    while !clients[1].session().unwrap().is_finished() {
        serve_tick(&mut clients, &mut server, &[high_punch(), idle()]);
    }
    for client in clients.iter_mut() {
        client.tick(idle());
        assert_eq!(client.session().unwrap().wins(p(0)), 1);
        client.receive(&start(2, players));
        let session = client.session().unwrap();
        assert_eq!((session.id(), session.tick()), (2, 0));
        assert_eq!(session.wins(p(0)), 1, "same players: the score goes on");
    }

    // A third player joins the next match: a new score.
    clients[0].receive(&start(3, PlayerSet::first(3)));
    let session = clients[0].session().unwrap();
    assert_eq!(session.players(), PlayerSet::first(3));
    assert_eq!(session.wins(p(0)), 0);
}

#[test]
fn a_player_leaving_a_brawl_does_not_end_it() {
    let players = PlayerSet::first(4);
    let mut client = playing_client(MatchConfig::default(), INPUT_DELAY, 0, 1, players);
    client.tick(idle());

    // Somebody who sits out the match (a late joiner) leaving changes nothing.
    client.receive(&ServerMessage::PlayerLeft { player: p(5) }.encode());
    assert_eq!(client.departed(), PlayerSet::EMPTY);

    client.receive(&ServerMessage::PlayerLeft { player: p(2) }.encode());
    assert_eq!(client.status(), Status::Playing, "three remain");
    assert_eq!(client.departed(), set(&[2]));
    assert_eq!(client.left_player(), None);
    client.receive(&ServerMessage::PlayerLeft { player: p(3) }.encode());
    assert_eq!(client.status(), Status::Playing, "two remain");
    assert!(!client.tick(idle()).is_empty(), "the match goes on");

    // The last opponent leaves: the match is over.
    client.receive(&ServerMessage::PlayerLeft { player: p(1) }.encode());
    assert_eq!(client.status(), Status::Lobby);
    assert_eq!(client.left_player(), Some(p(1)));
    assert_eq!(client.departed(), set(&[1, 2, 3]));
    assert_eq!(
        client.session().unwrap().tick(),
        2,
        "the last round stays visible"
    );
    assert!(client.tick(idle()).is_empty());
    assert_eq!(client.ready(), Some(vec![7]), "ready for the next match");

    // Further departures in the lobby keep the last match as it was.
    client.receive(&ServerMessage::PlayerLeft { player: p(2) }.encode());
    assert_eq!(client.left_player(), Some(p(1)));

    // The next match starts afresh.
    client.receive(&start(2, set(&[0, 4])));
    assert_eq!(client.status(), Status::Playing);
    assert_eq!(client.left_player(), None);
    assert_eq!(client.departed(), PlayerSet::EMPTY);
    assert_eq!(client.session().unwrap().id(), 2);
    assert_eq!(client.session().unwrap().tick(), 0);
    assert_eq!(client.player(), Some(p(0)));
}

#[test]
fn a_player_leaving_a_duel_ends_it() {
    let mut client = playing_client(MatchConfig::default(), 0, 1, 1, PlayerSet::first(2));
    client.receive(&ServerMessage::PlayerLeft { player: p(0) }.encode());
    assert_eq!(client.status(), Status::Lobby);
    assert_eq!(client.left_player(), Some(p(0)));

    // Without any match, a departure is only news for the lobby.
    let mut client = Client::online(MatchConfig::default(), 0);
    client.handle(ServerMessage::Welcome { player: p(0) });
    client.handle(ServerMessage::PlayerLeft { player: p(1) });
    assert_eq!(client.status(), Status::Lobby);
    assert_eq!(client.left_player(), None);
}

#[test]
fn a_full_room_is_final_even_when_the_connection_closes() {
    let mut client = Client::online(MatchConfig::default(), INPUT_DELAY);
    client.receive(&ServerMessage::RoomFull.encode());
    assert_eq!(client.status(), Status::RoomFull);
    client.disconnected();
    assert_eq!(client.status(), Status::RoomFull);
    assert!(client.tick(idle()).is_empty());
    assert_eq!(client.ready(), None);
}

#[test]
fn a_lost_connection_ends_everything() {
    let players = PlayerSet::first(2);
    let mut client = playing_client(MatchConfig::default(), INPUT_DELAY, 0, 1, players);
    client.tick(idle());
    client.disconnected();
    assert_eq!(client.status(), Status::Disconnected);
    assert!(client.tick(idle()).is_empty());
    assert_eq!(client.ready(), None);

    // Late messages change nothing.
    client.receive(&start(9, players));
    client.handle(ServerMessage::Welcome { player: p(1) });
    client.receive(&ServerMessage::PlayerLeft { player: p(1) }.encode());
    client.receive(
        &ServerMessage::Lobby {
            seated: PlayerSet::first(4),
            ready: PlayerSet::first(4),
        }
        .encode(),
    );
    client.receive(&frame(1, 0, players, PlayerSet::EMPTY, &[]).encode());
    client.receive(&[]);
    assert_eq!(
        client.status(),
        Status::Disconnected,
        "even garbage is ignored"
    );
    assert_eq!(client.session().unwrap().confirmed_tick(), 0);
    assert_eq!(client.player(), Some(p(0)));
    assert_eq!(client.session().unwrap().id(), 1);
    assert_eq!(client.seated(), PlayerSet::EMPTY);

    for welcomed in [false, true] {
        let mut client = Client::online(MatchConfig::default(), INPUT_DELAY);
        if welcomed {
            client.handle(ServerMessage::Welcome { player: p(0) });
        }
        client.disconnected();
        assert_eq!(client.status(), Status::Disconnected);
    }
}

#[test]
fn an_undecodable_frame_fails_the_client_for_good() {
    let mut client = playing_client(
        MatchConfig::default(),
        INPUT_DELAY,
        0,
        1,
        PlayerSet::first(2),
    );
    client.receive(&[]);
    let failed = Status::Failed(ClientError::Decode(DecodeError::Empty));
    assert_eq!(client.status(), failed);
    assert!(client.tick(idle()).is_empty());

    client.receive(&start(2, PlayerSet::first(2)));
    client.receive(&ServerMessage::RoomFull.encode());
    client.handle(ServerMessage::PlayerLeft { player: p(1) });
    client.receive(&[42]);
    client.disconnected();
    assert_eq!(client.status(), failed, "the first failure stays");
    assert_eq!(client.session().unwrap().id(), 1);

    let mut client = Client::online(MatchConfig::default(), INPUT_DELAY);
    client.receive(&[42, 0]);
    assert_eq!(
        client.status(),
        Status::Failed(ClientError::Decode(DecodeError::UnknownTag(42)))
    );
    let mut client = Client::online(MatchConfig::default(), INPUT_DELAY);
    client.receive(&[1]);
    assert_eq!(
        client.status(),
        Status::Failed(ClientError::Decode(DecodeError::WrongLength {
            tag: 1,
            expected: 2,
            actual: 1
        }))
    );
}

#[test]
fn a_start_before_the_welcome_fails() {
    let mut client = Client::online(MatchConfig::default(), INPUT_DELAY);
    client.receive(&start(1, PlayerSet::first(2)));
    assert_eq!(
        client.status(),
        Status::Failed(ClientError::StartBeforeWelcome)
    );
    assert!(client.session().is_none());
    client.receive(&ServerMessage::Welcome { player: p(0) }.encode());
    assert_eq!(client.player(), None, "ignored after failing");
}

#[test]
fn a_start_that_cannot_make_a_match_fails() {
    let mut client = Client::online(MatchConfig::default(), INPUT_DELAY);
    client.handle(ServerMessage::Welcome { player: p(3) });
    client.handle(ServerMessage::Start {
        session: 1,
        players: PlayerSet::first(3),
    });
    assert_eq!(
        client.status(),
        Status::Failed(ClientError::Session(SessionError::NotInSession(p(3))))
    );
    assert!(client.session().is_none());

    let mut client = Client::online(MatchConfig::default(), INPUT_DELAY);
    client.handle(ServerMessage::Welcome { player: p(0) });
    client.handle(ServerMessage::Start {
        session: 1,
        players: set(&[0]),
    });
    assert_eq!(
        client.status(),
        Status::Failed(ClientError::Session(SessionError::InvalidRoster(
            RosterError::TooFewPlayers { count: 1 }
        )))
    );

    let broken = MatchConfig {
        arena_width: 0,
        ..MatchConfig::default()
    };
    let mut client = Client::online(broken, INPUT_DELAY);
    client.handle(ServerMessage::Welcome { player: p(0) });
    client.handle(ServerMessage::Start {
        session: 1,
        players: PlayerSet::first(2),
    });
    assert_eq!(
        client.status(),
        Status::Failed(ClientError::Session(SessionError::InvalidSetup(
            SetupError::InvalidConfig(ConfigError::NonPositiveArenaWidth)
        )))
    );
}

#[test]
fn client_errors_explain_themselves() {
    assert_eq!(
        ClientError::Decode(DecodeError::UnknownTag(7)).to_string(),
        "bad message from the server: unknown message tag 7"
    );
    assert_eq!(
        ClientError::StartBeforeWelcome.to_string(),
        "the server started a match before assigning a seat"
    );
    assert_eq!(
        ClientError::Session(SessionError::NotInSession(p(4))).to_string(),
        "cannot start the match: player 4 does not take part in the match"
    );
    let boxed: Box<dyn std::error::Error> = Box::new(ClientError::StartBeforeWelcome);
    assert!(boxed.to_string().contains("before assigning"));
}

// ---------------------------------------------------------------------------------------------
// Playing alone: the dummy and the bot
// ---------------------------------------------------------------------------------------------

#[test]
fn a_practice_client_plays_at_once_against_the_dummy() {
    let client = Client::practice(MatchConfig::default());
    assert!(client.is_local());
    assert_eq!(client.rival(), Some(Rival::Dummy));
    assert_eq!(client.bot_player(), None);
    assert_eq!(client.status(), Status::Playing);
    assert_eq!(client.player(), Some(p(0)));
    let session = client.session().unwrap();
    assert_eq!(session.local_player(), p(0));
    assert_eq!(session.players(), PlayerSet::first(2));
    assert_eq!(session.id(), 0);
    assert_eq!(session.tick(), 0);
}

#[test]
fn a_practice_client_never_sends_nor_guesses_and_advances_every_tick() {
    let config = no_countdown();
    let start = Match::new(Roster::duel(), config.clone()).unwrap();
    let mut client = Client::practice(config.clone());
    for tick in 1..=20 {
        assert!(client.tick(walk_right()).is_empty());
        let session = client.session().unwrap();
        assert_eq!((session.tick(), session.confirmed_tick()), (tick, tick));
        assert_eq!(client.stalled_ticks(), 0);
    }
    let current = client.session().unwrap().current();
    // No delay alone: every walking tick moved the player; the dummy stood still.
    assert_eq!(x_of(current, 0), x_of(&start, 0) + 20 * config.walk_speed);
    assert_eq!(x_of(current, 1), x_of(&start, 1));
    assert_eq!(health_of(current, 1), config.max_health);
}

#[test]
fn enter_starts_the_next_local_round_once_the_round_is_over() {
    let config = one_punch_config();
    let fresh = Match::new(Roster::duel(), config.clone()).unwrap();
    let mut client = Client::practice(config);
    assert_eq!(client.ready(), None);
    assert_eq!(client.session().unwrap().id(), 0, "not during the round");

    let mut ticks = 0;
    while client.session().unwrap().current().outcome().is_none() {
        assert!(client.tick(high_punch()).is_empty());
        ticks += 1;
        assert!(ticks < 100, "the punch must land");
    }
    assert_eq!(client.session().unwrap().wins(p(0)), 1);

    assert_eq!(client.ready(), None, "nothing to send");
    let session = client.session().unwrap();
    assert_eq!(session.current(), &fresh);
    assert_eq!(session.id(), 1);
    assert_eq!(session.wins(p(0)), 1);
    assert_eq!(client.status(), Status::Playing);
}

#[test]
fn a_client_versus_the_bot_lets_the_computer_play_player_two() {
    let config = no_countdown();
    let start = Match::new(Roster::duel(), config.clone()).unwrap();
    let mut client = Client::versus(config.clone(), Rival::Brawler);
    assert!(client.is_local());
    assert_eq!(client.rival(), Some(Rival::Brawler));
    assert_eq!(client.bot_player(), Some(p(1)));
    assert_eq!(client.player(), Some(p(0)));
    // 30 ticks: walking in, not yet in reach.
    for _ in 0..30 {
        assert!(client.tick(idle()).is_empty());
    }
    let current = client.session().unwrap().current();
    assert_eq!(x_of(current, 0), x_of(&start, 0), "the player stood still");
    assert!(
        x_of(current, 1) < x_of(&start, 1),
        "the bot walked in from the right"
    );

    // Deterministic: the same inputs play the same match, however the computer came in.
    let mut twin = Client::practice(config);
    twin.set_rival(Rival::Brawler);
    for _ in 0..30 {
        twin.tick(idle());
    }
    assert_eq!(twin.session().unwrap().current(), current);
    let copy = twin.clone();
    assert_eq!(copy.session().unwrap().current(), current);
}

#[test]
fn the_rival_can_change_in_the_middle_of_a_round() {
    let config = no_countdown();
    let mut client = Client::practice(config.clone());
    for _ in 0..30 {
        client.tick(idle());
    }
    let before = client.session().unwrap().current().clone();
    assert_eq!(client.rival(), Some(Rival::Dummy));
    assert_eq!(client.bot_player(), None);

    client.set_rival(Rival::Brawler);
    assert_eq!(client.rival(), Some(Rival::Brawler));
    assert_eq!(client.bot_player(), Some(p(1)));
    assert_eq!(
        client.session().unwrap().current(),
        &before,
        "the round goes on from where it was"
    );
    assert_eq!(client.session().unwrap().tick(), 30);
    for _ in 0..30 {
        client.tick(idle());
    }
    let current = client.session().unwrap().current();
    assert!(x_of(current, 1) < x_of(&before, 1), "the computer walks in");

    client.set_rival(Rival::Dummy);
    assert_eq!(client.bot_player(), None);
    let x = x_of(client.session().unwrap().current(), 1);
    for _ in 0..30 {
        client.tick(idle());
    }
    assert_eq!(
        x_of(client.session().unwrap().current(), 1),
        x,
        "the dummy stops"
    );

    let mut online = Client::online(config, INPUT_DELAY);
    online.set_rival(Rival::Brawler);
    assert_eq!(online.rival(), None, "nothing to switch online");
}

#[test]
fn setting_the_same_rival_again_keeps_the_bot_as_it_is() {
    let config = no_countdown();
    let mut client = Client::versus(config.clone(), Rival::Brawler);
    let mut twin = client.clone();
    for _ in 0..200 {
        client.tick(idle());
        twin.tick(idle());
        // A fresh bot every tick would restart its dice, and play differently.
        twin.set_rival(Rival::Brawler);
    }
    assert_eq!(
        twin.session().unwrap().current(),
        client.session().unwrap().current()
    );
}

#[test]
fn the_bot_comes_back_for_the_next_round() {
    let mut client = Client::versus(quick_draw_config(), Rival::Brawler);
    while client.session().unwrap().current().outcome().is_none() {
        client.tick(idle());
    }
    client.ready();
    assert_eq!(client.session().unwrap().id(), 1);
    assert_eq!(client.rival(), Some(Rival::Brawler));
    assert_eq!(client.bot_player(), Some(p(1)));
    assert_eq!(client.session().unwrap().current().outcome(), None);
}

#[test]
fn a_local_client_still_obeys_the_connection_state() {
    let mut client = Client::practice(MatchConfig::default());
    client.disconnected();
    assert_eq!(client.status(), Status::Disconnected);
    assert!(client.tick(idle()).is_empty());
    assert_eq!(client.session().unwrap().tick(), 0);
}
