//! Integration tests of the online layer (`srr::net`): wire protocol, sets of players, lock-step
//! sessions and the client state machine. Everything here is pure, so the "machines" are simply
//! values exchanging messages in memory.

use srr::game::{
    Attack, ConfigError, Input, Match, MatchConfig, Outcome, Phase, PlayerId, PlayerSlot, Roster,
    RosterError, SetupError, Strength, TeamId,
};
use srr::net::{
    Client, ClientError, ClientMessage, DecodeError, INPUT_DELAY, MAX_PLAYERS, PlayerSet, Rival,
    ServerMessage, Session, SessionError, Status,
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

/// No countdown, two fighters spawned within punch range (200 / 3 = 66 and 133) and so little
/// health that a single high punch knocks out.
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

fn high_punch() -> Input {
    Input::pressing(Attack::punch(Strength::High))
}

fn p(player: u8) -> PlayerId {
    PlayerId(player)
}

fn set(players: &[u8]) -> PlayerSet {
    players.iter().map(|&player| PlayerId(player)).collect()
}

/// One session per player of `players`, all numbered `id`.
fn machines(players: PlayerSet, config: &MatchConfig, id: u8, delay: u32) -> Vec<Session> {
    players
        .iter()
        .map(|player| Session::new(player, players, id, config.clone(), delay).unwrap())
        .collect()
}

/// Delivers `message`, sent by `from`, to the session `to`.
fn deliver(to: &mut Session, from: PlayerId, message: ClientMessage) -> bool {
    let ClientMessage::Input {
        session,
        tick,
        input,
    } = message
    else {
        panic!("sessions only send inputs");
    };
    to.receive_remote(session, from, tick, input)
}

/// One perfect-network tick: everybody submits `inputs[i]`, everybody receives everything,
/// everybody advances; all machines must agree.
fn exchange(sessions: &mut [Session], inputs: &[Input]) {
    let sent: Vec<(PlayerId, ClientMessage)> = sessions
        .iter_mut()
        .zip(inputs)
        .map(|(session, &input)| {
            let message = session.submit_local(input).expect("nobody is ahead");
            (session.local_player(), message)
        })
        .collect();
    for (from, message) in sent {
        for session in sessions.iter_mut().filter(|s| s.local_player() != from) {
            assert!(deliver(session, from, message));
        }
    }
    let ran: Vec<u32> = sessions.iter_mut().map(Session::advance).collect();
    assert!(ran.iter().all(|&count| count == ran[0]));
    for session in &sessions[1..] {
        assert_eq!(session.current(), sessions[0].current());
    }
}

fn x_of(game: &Match, player: u8) -> i32 {
    game.fighter(PlayerId(player)).unwrap().x()
}

fn health_of(game: &Match, player: u8) -> i32 {
    game.fighter(PlayerId(player)).unwrap().health()
}

/// The frame the relay sends for an input of `player`.
fn input_frame(session: u8, player: u8, tick: u32, input: Input) -> Vec<u8> {
    ServerMessage::Input {
        session,
        player: PlayerId(player),
        tick,
        input,
    }
    .encode()
}

/// What the relay forwards to the others when `from` sends `frame`.
fn relay(from: PlayerId, frame: &[u8]) -> Vec<u8> {
    let Ok(ClientMessage::Input {
        session,
        tick,
        input,
    }) = ClientMessage::decode(frame)
    else {
        panic!("not an input: {frame:?}");
    };
    ServerMessage::Input {
        session,
        player: from,
        tick,
        input,
    }
    .encode()
}

fn decode_sent(frame: &[u8]) -> (u8, u32, Input) {
    match ClientMessage::decode(frame) {
        Ok(ClientMessage::Input {
            session,
            tick,
            input,
        }) => (session, tick, input),
        other => panic!("not an input: {other:?}"),
    }
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

/// Ticks every client once with `inputs[i]` and relays what each one sent to all the others.
fn relay_tick(clients: &mut [Client], inputs: &[Input]) {
    let sent: Vec<(PlayerId, Option<Vec<u8>>)> = clients
        .iter_mut()
        .zip(inputs)
        .map(|(client, &input)| (client.player().unwrap(), client.tick(input)))
        .collect();
    for (from, frame) in sent {
        let Some(frame) = frame else { continue };
        let relayed = relay(from, &frame);
        for client in clients.iter_mut().filter(|c| c.player() != Some(from)) {
            client.receive(&relayed);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Protocol
// ---------------------------------------------------------------------------------------------

#[test]
fn every_server_message_round_trips_through_its_bytes() {
    let messages = [
        ServerMessage::Welcome { player: p(0) },
        ServerMessage::Welcome { player: p(3) },
        ServerMessage::Start {
            session: 0,
            players: PlayerSet::first(2),
        },
        ServerMessage::Start {
            session: 255,
            players: set(&[0, 2, 3]),
        },
        ServerMessage::Input {
            session: 9,
            player: p(2),
            tick: u32::MAX,
            input: Input::pressing(Attack::kick(Strength::Medium)),
        },
        ServerMessage::PlayerLeft { player: p(1) },
        ServerMessage::RoomFull,
        ServerMessage::Lobby {
            seated: PlayerSet::first(4),
            ready: set(&[1, 3]),
        },
        ServerMessage::Lobby {
            seated: PlayerSet::EMPTY,
            ready: PlayerSet::EMPTY,
        },
    ];
    for message in messages {
        assert_eq!(ServerMessage::decode(&message.encode()), Ok(message));
    }
}

#[test]
fn server_messages_have_the_documented_layout() {
    assert_eq!(ServerMessage::Welcome { player: p(1) }.encode(), [1, 1]);
    assert_eq!(start(42, PlayerSet::first(3)), [2, 42, 0b111]);
    assert_eq!(ServerMessage::PlayerLeft { player: p(2) }.encode(), [4, 2]);
    assert_eq!(ServerMessage::RoomFull.encode(), [5]);
    assert_eq!(
        ServerMessage::Lobby {
            seated: PlayerSet::first(3),
            ready: set(&[1]),
        }
        .encode(),
        [6, 0b111, 0b10]
    );
    let input = Input::from_bits(0x0105);
    assert_eq!(
        input_frame(7, 2, 0x0102_0304, input),
        [3, 7, 2, 0x04, 0x03, 0x02, 0x01, 0x05, 0x01]
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

    // The relay adds the sender: a client's frame is not a valid server input.
    assert_eq!(
        ServerMessage::decode(&bytes),
        Err(DecodeError::WrongLength {
            tag: 3,
            expected: 9,
            actual: 8
        })
    );
    assert_eq!(
        relay(p(1), &bytes),
        input_frame(3, 1, 0x0102_0304, high_punch())
    );
}

#[test]
fn bits_beyond_the_input_are_ignored_on_the_wire() {
    let frame = [3, 1, 2, 0, 0, 0, 0, 0xff, 0xff];
    assert_eq!(
        ServerMessage::decode(&frame),
        Ok(ServerMessage::Input {
            session: 1,
            player: p(2),
            tick: 0,
            input: Input::from_bits((1 << Input::BITS) - 1),
        })
    );
}

#[test]
fn client_messages_reject_empty_unknown_and_malformed_frames() {
    assert_eq!(ClientMessage::decode(&[]), Err(DecodeError::Empty));
    // Only inputs and readiness come from a browser.
    for tag in [0, 1, 2, 4, 5, 6, 255] {
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
    for tag in [0, 7, 200] {
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
    assert_eq!(ServerMessage::decode(&[2, 0]), Err(wrong(2, 3, 2)));
    assert_eq!(ServerMessage::decode(&[2, 0, 0, 0]), Err(wrong(2, 3, 4)));
    assert_eq!(ServerMessage::decode(&[3, 0]), Err(wrong(3, 9, 2)));
    assert_eq!(ServerMessage::decode(&[3; 10]), Err(wrong(3, 9, 10)));
    assert_eq!(ServerMessage::decode(&[4]), Err(wrong(4, 2, 1)));
    assert_eq!(ServerMessage::decode(&[4, 0, 0]), Err(wrong(4, 2, 3)));
    assert_eq!(ServerMessage::decode(&[5, 5]), Err(wrong(5, 1, 2)));
    assert_eq!(ServerMessage::decode(&[6, 0]), Err(wrong(6, 3, 2)));
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
fn player_sets_hold_up_to_eight_players() {
    assert_eq!(MAX_PLAYERS, 4);
    assert_eq!(PlayerSet::default(), PlayerSet::EMPTY);
    assert!(PlayerSet::EMPTY.is_empty());
    assert_eq!(PlayerSet::first(0), PlayerSet::EMPTY);
    assert_eq!(PlayerSet::first(4).bits(), 0b1111);
    assert_eq!(PlayerSet::first(100).bits(), 0xff);
    assert_eq!(PlayerSet::first(100).len(), PlayerSet::CAPACITY);

    let trio = set(&[3, 0, 5]);
    assert_eq!(trio.len(), 3);
    assert!(!trio.is_empty());
    assert_eq!(trio.iter().collect::<Vec<_>>(), [p(0), p(3), p(5)]);
    assert!(trio.contains(p(5)) && !trio.contains(p(1)));
    assert_eq!(trio.with(p(1)).len(), 4);
    assert_eq!(trio.with(p(3)), trio, "already a member");
    assert_eq!(trio.without(p(3)), set(&[0, 5]));
    assert_eq!(trio.without(p(1)), trio, "not a member");
    assert_eq!(PlayerSet::from_bits(trio.bits()), trio);

    // Ids that do not fit in the byte are never members.
    assert_eq!(trio.with(p(8)), trio);
    assert_eq!(trio.with(p(255)), trio);
    assert!(!PlayerSet::first(8).contains(p(8)));
    assert_eq!(PlayerSet::first(8).without(p(200)), PlayerSet::first(8));
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
        assert_eq!(session.tick(), 0);
        assert_eq!(session.current(), &expected);
        for player in 0..4 {
            assert_eq!(session.wins(p(player)), 0);
        }
        // The first `delay` ticks are implicit, so nothing is awaited yet.
        assert_eq!(session.waiting_for(), PlayerSet::EMPTY);
        assert!(!session.awaiting_remote());
    }
}

#[test]
fn everybody_fights_for_themselves_in_any_seats() {
    let players = set(&[0, 2, 3]);
    let session = Session::new(p(2), players, 1, MatchConfig::default(), 0).unwrap();
    let roster = session.current().roster();
    assert_eq!(roster.players(), [p(0), p(2), p(3)]);
    assert_eq!(roster.team_of(p(3)), Some(TeamId(3)));
    assert!(roster.are_enemies(p(0), p(2)) && roster.are_enemies(p(2), p(3)));
    assert!(session.current().fighter(p(1)).is_none());
    // Spread over the arena in seat order.
    assert!(x_of(session.current(), 0) < x_of(session.current(), 2));
    assert!(x_of(session.current(), 2) < x_of(session.current(), 3));
}

#[test]
fn the_first_delay_ticks_run_without_any_message() {
    let mut session = Session::new(
        p(0),
        PlayerSet::first(3),
        1,
        MatchConfig::default(),
        INPUT_DELAY,
    )
    .unwrap();
    assert_eq!(session.advance(), INPUT_DELAY);
    assert_eq!(session.tick(), INPUT_DELAY);
    assert!(session.awaiting_remote());
    assert_eq!(session.waiting_for(), set(&[1, 2]));
    assert_eq!(session.advance(), 0, "nothing more is known");
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
    assert_eq!(
        session.submit_local(walk_right()),
        Some(ClientMessage::Input {
            session: 5,
            tick: INPUT_DELAY,
            input: walk_right(),
        })
    );
    // Already `delay` ticks ahead of tick 0: further inputs are dropped.
    assert_eq!(session.submit_local(walk_right()), None);
    assert_eq!(session.submit_local(idle()), None);

    // The implicit ticks run; the window moves on to tick `2 * delay`.
    assert_eq!(session.advance(), INPUT_DELAY);
    for tick in INPUT_DELAY + 1..=2 * INPUT_DELAY {
        let Some(ClientMessage::Input { tick: sent, .. }) = session.submit_local(idle()) else {
            panic!("tick {tick} is within the window");
        };
        assert_eq!(sent, tick);
    }
    assert_eq!(session.submit_local(idle()), None);

    // Without the others nothing more is simulated.
    assert!(session.awaiting_remote());
    assert_eq!(session.advance(), 0);
    assert_eq!(session.tick(), INPUT_DELAY);
}

#[test]
fn a_session_stalls_until_every_remote_input_arrives() {
    let mut all = machines(PlayerSet::first(3), &MatchConfig::default(), 1, INPUT_DELAY);
    let sent: Vec<ClientMessage> = all
        .iter_mut()
        .map(|s| s.submit_local(idle()).unwrap())
        .collect();
    for session in all.iter_mut() {
        assert_eq!(session.advance(), INPUT_DELAY);
    }
    let [host, second, third] = &mut all[..] else {
        unreachable!()
    };

    // Player 1's input reaches the host; player 2's is late.
    assert!(deliver(host, p(1), sent[1]));
    assert_eq!(host.waiting_for(), set(&[2]));
    assert_eq!(host.advance(), 0);

    assert!(deliver(host, p(2), sent[2]));
    assert!(!host.awaiting_remote());
    assert_eq!(host.advance(), 1);

    // The others catch up from the same inputs.
    for (from, message) in [(0, sent[0]), (2, sent[2])] {
        deliver(second, p(from), message);
    }
    for (from, message) in [(0, sent[0]), (1, sent[1])] {
        deliver(third, p(from), message);
    }
    assert_eq!(second.advance(), 1);
    assert_eq!(third.advance(), 1);
    assert_eq!(host.current(), second.current());
    assert_eq!(host.current(), third.current());
}

#[test]
fn a_session_does_not_simulate_without_its_own_input() {
    let mut session = Session::new(
        p(0),
        PlayerSet::first(2),
        1,
        MatchConfig::default(),
        INPUT_DELAY,
    )
    .unwrap();
    session.advance();
    assert!(session.receive_remote(1, p(1), INPUT_DELAY, idle()));
    assert!(!session.awaiting_remote());
    assert_eq!(
        session.advance(),
        0,
        "the local input for the tick is missing"
    );
    session.submit_local(idle()).unwrap();
    assert_eq!(session.advance(), 1);
}

#[test]
fn remote_inputs_that_do_not_belong_are_ignored() {
    let players = set(&[0, 1, 3]);
    let mut session = Session::new(p(0), players, 4, MatchConfig::default(), INPUT_DELAY).unwrap();

    assert!(
        !session.receive_remote(5, p(1), INPUT_DELAY, walk_right()),
        "other session"
    );
    assert!(
        !session.receive_remote(4, p(0), INPUT_DELAY, walk_right()),
        "the local player"
    );
    assert!(
        !session.receive_remote(4, p(2), INPUT_DELAY, walk_right()),
        "not in the session"
    );
    for implicit in 0..INPUT_DELAY {
        assert!(
            !session.receive_remote(4, p(1), implicit, walk_right()),
            "implicit tick"
        );
    }
    assert!(session.receive_remote(4, p(1), INPUT_DELAY, idle()));
    assert!(
        !session.receive_remote(4, p(1), INPUT_DELAY, walk_right()),
        "duplicate"
    );
    assert!(session.receive_remote(4, p(3), INPUT_DELAY, idle()));

    session.submit_local(idle()).unwrap();
    assert_eq!(session.advance(), INPUT_DELAY + 1);
    assert!(
        !session.receive_remote(4, p(3), INPUT_DELAY, idle()),
        "already simulated"
    );
    // Far-future inputs are fine.
    assert!(session.receive_remote(4, p(3), 1000, idle()));
}

#[test]
fn the_remote_duplicate_does_not_replace_the_first_input() {
    let config = no_countdown();
    let mut all = machines(PlayerSet::first(2), &config, 1, 0);
    let from_guest = all[1].submit_local(walk_right()).unwrap();
    let host = &mut all[0];
    assert!(deliver(host, p(1), from_guest));
    assert!(!host.receive_remote(1, p(1), 0, idle()));
    host.submit_local(idle()).unwrap();
    assert_eq!(host.advance(), 1);
    // The guest (player 2, facing left) walked right, i.e. away from the host.
    let start = Match::new(Roster::duel(), config).unwrap();
    assert_eq!(
        x_of(host.current(), 1),
        x_of(&start, 1) + start.config().walk_speed
    );
}

#[test]
fn three_sessions_in_lock_step_play_the_same_match() {
    let config = no_countdown();
    let players = PlayerSet::first(3);
    let mut all = machines(players, &config, 2, INPUT_DELAY);
    let start = all[0].current().clone();

    for i in 0..60 {
        let punch = if i % 7 == 0 { high_punch() } else { idle() };
        exchange(&mut all, &[walk_right(), punch, idle()]);
    }
    assert_eq!(
        all[0].tick(),
        60 + INPUT_DELAY,
        "the first call also runs the implicit ticks"
    );
    // The inputs drove the right fighters, delayed by `INPUT_DELAY` ticks.
    let walked = all[0].tick() - INPUT_DELAY;
    let current = all[2].current();
    assert!(x_of(current, 0) > x_of(&start, 0));
    assert!(x_of(current, 0) <= x_of(&start, 0) + config.walk_speed * walked as i32);
    assert!(
        x_of(current, 2) >= x_of(&start, 2),
        "player 3 never walks left"
    );
}

#[test]
fn four_sessions_in_lock_step_play_the_same_match() {
    let mut all = machines(PlayerSet::first(4), &no_countdown(), 9, 1);
    for i in 0..40 {
        let inputs = [
            walk_right(),
            if i % 3 == 0 { high_punch() } else { idle() },
            Input {
                left: true,
                ..Input::NONE
            },
            Input {
                jump: i % 10 == 0,
                ..Input::NONE
            },
        ];
        exchange(&mut all, &inputs);
    }
    assert_eq!(all[3].tick(), 41);
    assert_eq!(all[0].current().fighters().len(), 4);
}

#[test]
fn a_finished_round_ignores_inputs_and_counts_no_win_for_a_draw() {
    let config = quick_draw_config();
    let mut all = machines(PlayerSet::first(2), &config, 1, 0);
    while all[0].current().outcome().is_none() {
        exchange(&mut all, &[idle(), idle()]);
    }
    assert_eq!(all[0].current().outcome(), Some(Outcome::Draw));
    let finished = all[0].current().clone();
    let tick = all[0].tick();
    exchange(&mut all, &[walk_right(), high_punch()]);
    assert_eq!(all[0].tick(), tick + 1, "the ticks still run");
    assert_eq!(all[0].current(), &finished, "the round does not change");
    assert_eq!(all[0].current().phase(), Phase::Finished(Outcome::Draw));
    assert_eq!(all[0].wins(p(0)) + all[0].wins(p(1)), 0);
}

#[test]
fn wins_are_counted_once_and_carried_to_the_next_round() {
    let config = one_punch_config();
    let mut all = machines(PlayerSet::first(2), &config, 3, 0);
    let start = all[0].current().clone();
    assert!(x_of(&start, 1) - x_of(&start, 0) < config.punches[2].range);

    for round in 1..=2u8 {
        let mut ticks = 0;
        while all[0].current().outcome().is_none() {
            exchange(&mut all, &[high_punch(), idle()]);
            ticks += 1;
            assert!(ticks < 100, "the punch must land");
        }
        assert_eq!(all[0].current().outcome(), Some(Outcome::Winner(TeamId(0))));
        assert!(health_of(all[0].current(), 1) <= 0);
        // Not counted again while the round stays finished.
        exchange(&mut all, &[idle(), idle()]);
        for session in &all {
            assert_eq!(session.wins(p(0)), u32::from(round));
            assert_eq!(session.wins(p(1)), 0);
        }
        for session in all.iter_mut() {
            *session = session.next(3 + round);
            assert_eq!(session.current(), &start);
            assert_eq!(session.id(), 3 + round);
            assert_eq!(session.tick(), 0);
        }
    }
    assert_eq!(all[1].wins(p(0)), 2);
}

#[test]
fn the_next_session_forgets_pending_inputs() {
    let mut session = Session::new(
        p(0),
        PlayerSet::first(3),
        1,
        MatchConfig::default(),
        INPUT_DELAY,
    )
    .unwrap();
    session.submit_local(walk_right()).unwrap();
    assert!(session.receive_remote(1, p(1), 5, idle()));
    let next = session.next(2);
    assert_eq!(next.players(), PlayerSet::first(3));
    assert_eq!(next.local_player(), p(0));
    assert!(!next.awaiting_remote());
    let mut next = next;
    assert_eq!(next.advance(), INPUT_DELAY);
    assert_eq!(next.waiting_for(), set(&[1, 2]));
    assert!(
        next.receive_remote(2, p(1), 5, idle()),
        "the old input is gone"
    );
    assert_eq!(
        next.submit_local(idle())
            .map(|m| decode_sent(&m.encode()).1),
        Some(INPUT_DELAY),
        "the local schedule restarts"
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
    assert_eq!(client.stalled_ticks(), 0);
    assert_eq!(client.tick(walk_right()), None);
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
    assert_eq!(client.tick(idle()), None, "nothing to do in the lobby");

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

    // An input before any session is dropped silently.
    client.receive(&input_frame(1, 0, INPUT_DELAY, idle()));
    assert!(client.session().is_none());

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
fn a_playing_client_sends_its_delayed_input() {
    let mut client = playing_client(
        MatchConfig::default(),
        INPUT_DELAY,
        0,
        8,
        PlayerSet::first(2),
    );
    let frame = client
        .tick(high_punch())
        .expect("the input goes to the others");
    assert_eq!(decode_sent(&frame), (8, INPUT_DELAY, high_punch()));
    // The implicit ticks were simulated right away.
    assert_eq!(client.session().unwrap().tick(), INPUT_DELAY);
    assert_eq!(client.stalled_ticks(), 0);

    let frame = client.tick(idle()).unwrap();
    assert_eq!(decode_sent(&frame), (8, INPUT_DELAY + 1, idle()));
}

#[test]
fn a_client_counts_stalled_ticks_and_resets_when_the_others_catch_up() {
    let mut client = playing_client(
        MatchConfig::default(),
        INPUT_DELAY,
        0,
        1,
        PlayerSet::first(3),
    );
    assert!(client.tick(idle()).is_some());
    assert_eq!(client.stalled_ticks(), 0);

    for stalled in 1..=INPUT_DELAY {
        assert!(client.tick(idle()).is_some());
        assert_eq!(client.stalled_ticks(), stalled);
    }
    // The input window is full: inputs are dropped and nothing is sent.
    assert_eq!(client.tick(idle()), None);
    assert_eq!(client.stalled_ticks(), INPUT_DELAY + 1);
    assert_eq!(client.session().unwrap().waiting_for(), set(&[1, 2]));

    // Only player 2's inputs arrive: still stalled.
    for tick in INPUT_DELAY..=2 * INPUT_DELAY {
        client.receive(&input_frame(1, 1, tick, walk_right()));
    }
    assert_eq!(client.session().unwrap().waiting_for(), set(&[2]));
    // Then player 3's.
    for tick in INPUT_DELAY..=2 * INPUT_DELAY {
        client.receive(&input_frame(1, 2, tick, idle()));
    }
    assert!(!client.session().unwrap().awaiting_remote());
    assert_eq!(client.tick(idle()), None, "the window was still full");
    assert_eq!(client.stalled_ticks(), 0);
    assert_eq!(client.session().unwrap().tick(), 2 * INPUT_DELAY + 1);
    assert!(client.tick(idle()).is_some());
}

#[test]
fn inputs_from_the_server_reach_the_session() {
    let mut client = playing_client(
        MatchConfig::default(),
        INPUT_DELAY,
        1,
        2,
        PlayerSet::first(2),
    );
    client.tick(idle()).unwrap();
    assert!(client.session().unwrap().awaiting_remote());

    // Another session's input, or our own echoed back, is ignored by the session.
    client.handle(ServerMessage::Input {
        session: 3,
        player: p(0),
        tick: INPUT_DELAY,
        input: idle(),
    });
    client.handle(ServerMessage::Input {
        session: 2,
        player: p(1),
        tick: INPUT_DELAY,
        input: idle(),
    });
    assert!(client.session().unwrap().awaiting_remote());

    client.handle(ServerMessage::Input {
        session: 2,
        player: p(0),
        tick: INPUT_DELAY,
        input: idle(),
    });
    assert!(!client.session().unwrap().awaiting_remote());
    assert_eq!(client.status(), Status::Playing);
}

#[test]
fn three_clients_through_a_relay_play_the_same_match() {
    let players = PlayerSet::first(3);
    let mut clients: Vec<Client> = (0..3)
        .map(|player| playing_client(no_countdown(), INPUT_DELAY, player, 1, players))
        .collect();
    for i in 0..30 {
        let punch = if i % 5 == 0 { high_punch() } else { idle() };
        relay_tick(&mut clients, &[walk_right(), punch, idle()]);
    }
    let sessions: Vec<&Session> = clients.iter().map(|c| c.session().unwrap()).collect();
    // Symmetric traffic: everybody simulated the same ticks from the same inputs.
    assert_eq!(sessions[0].tick(), 30 + INPUT_DELAY - 1);
    for session in &sessions[1..] {
        assert_eq!(session.tick(), sessions[0].tick());
        assert_eq!(session.current(), sessions[0].current());
    }
}

#[test]
fn ready_is_sent_in_the_lobby_or_once_the_round_is_over() {
    let players = PlayerSet::first(2);
    let mut clients: Vec<Client> = (0..2)
        .map(|player| playing_client(quick_draw_config(), 0, player, 1, players))
        .collect();
    assert_eq!(clients[0].ready(), None, "not during a round");
    while clients[0].session().unwrap().current().outcome().is_none() {
        relay_tick(&mut clients, &[idle(), idle()]);
    }
    assert_eq!(clients[0].ready(), Some(ClientMessage::Ready.encode()));
    assert_eq!(
        clients[0].status(),
        Status::Playing,
        "the round stays shown"
    );
}

#[test]
fn a_new_start_with_the_same_players_keeps_the_wins() {
    let players = PlayerSet::first(2);
    let mut clients: Vec<Client> = (0..2)
        .map(|player| playing_client(one_punch_config(), 0, player, 1, players))
        .collect();
    while clients[1].session().unwrap().current().outcome().is_none() {
        relay_tick(&mut clients, &[high_punch(), idle()]);
    }
    for client in clients.iter_mut() {
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
fn a_player_leaving_ends_the_match_they_played_in() {
    let mut client = playing_client(MatchConfig::default(), INPUT_DELAY, 0, 1, set(&[0, 1, 2]));
    client.tick(idle()).unwrap();
    client.tick(idle()).unwrap();
    assert_eq!(client.stalled_ticks(), 1);

    // Somebody who sits out the match (a late joiner) leaving changes nothing.
    client.receive(&ServerMessage::PlayerLeft { player: p(3) }.encode());
    assert_eq!(client.status(), Status::Playing);
    assert_eq!(client.left_player(), None);

    client.receive(&ServerMessage::PlayerLeft { player: p(2) }.encode());
    assert_eq!(client.status(), Status::Lobby);
    assert_eq!(client.left_player(), Some(p(2)));
    assert_eq!(
        client.session().unwrap().tick(),
        INPUT_DELAY,
        "the last round stays visible"
    );
    assert_eq!(client.tick(idle()), None);
    assert_eq!(
        client.stalled_ticks(),
        1,
        "not playing: not stalled further"
    );
    assert_eq!(client.ready(), Some(vec![7]), "ready for the next match");

    // Further departures in the lobby keep the first one shown.
    client.receive(&ServerMessage::PlayerLeft { player: p(1) }.encode());
    assert_eq!(client.left_player(), Some(p(2)));

    // The next match starts afresh.
    client.receive(&start(2, set(&[0, 1])));
    assert_eq!(client.status(), Status::Playing);
    assert_eq!(client.left_player(), None);
    assert_eq!(client.session().unwrap().id(), 2);
    assert_eq!(client.session().unwrap().tick(), 0);
    assert_eq!(client.stalled_ticks(), 0);
    assert_eq!(client.player(), Some(p(0)));
}

#[test]
fn a_full_room_is_final_even_when_the_connection_closes() {
    let mut client = Client::online(MatchConfig::default(), INPUT_DELAY);
    client.receive(&ServerMessage::RoomFull.encode());
    assert_eq!(client.status(), Status::RoomFull);
    client.disconnected();
    assert_eq!(client.status(), Status::RoomFull);
    assert_eq!(client.tick(idle()), None);
    assert_eq!(client.ready(), None);
}

#[test]
fn a_lost_connection_ends_everything() {
    let mut client = playing_client(
        MatchConfig::default(),
        INPUT_DELAY,
        0,
        1,
        PlayerSet::first(2),
    );
    client.tick(idle()).unwrap();
    client.disconnected();
    assert_eq!(client.status(), Status::Disconnected);
    assert_eq!(client.tick(idle()), None);
    assert_eq!(client.ready(), None);

    // Late messages change nothing.
    client.receive(&start(9, PlayerSet::first(2)));
    client.handle(ServerMessage::Welcome { player: p(1) });
    client.receive(&ServerMessage::PlayerLeft { player: p(1) }.encode());
    client.receive(
        &ServerMessage::Lobby {
            seated: PlayerSet::first(4),
            ready: PlayerSet::first(4),
        }
        .encode(),
    );
    client.receive(&input_frame(1, 1, INPUT_DELAY, idle()));
    client.receive(&[]);
    assert_eq!(
        client.status(),
        Status::Disconnected,
        "even garbage is ignored"
    );
    assert!(client.session().unwrap().awaiting_remote());
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
    assert_eq!(client.tick(idle()), None);

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
fn a_practice_client_never_sends_and_advances_every_tick() {
    let config = no_countdown();
    let start = Match::new(Roster::duel(), config.clone()).unwrap();
    let mut client = Client::practice(config.clone());
    for tick in 1..=20 {
        assert_eq!(client.tick(walk_right()), None);
        assert_eq!(client.session().unwrap().tick(), tick);
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
        assert_eq!(client.tick(high_punch()), None);
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
    let mut client = Client::versus_bot(config.clone());
    assert!(client.is_local());
    assert_eq!(client.rival(), Some(Rival::Bot));
    assert_eq!(client.bot_player(), Some(p(1)));
    assert_eq!(client.player(), Some(p(0)));
    // 30 ticks: walking in, not yet in reach.
    for _ in 0..30 {
        assert_eq!(client.tick(idle()), None);
    }
    let current = client.session().unwrap().current();
    assert_eq!(x_of(current, 0), x_of(&start, 0), "the player stood still");
    assert!(
        x_of(current, 1) < x_of(&start, 1),
        "the bot walked in from the right"
    );

    // Deterministic: the same inputs play the same match.
    let mut twin = Client::versus_bot(config);
    for _ in 0..30 {
        twin.tick(idle());
    }
    assert_eq!(twin.session().unwrap().current(), current);
}

#[test]
fn the_bot_comes_back_for_the_next_round() {
    let mut client = Client::versus_bot(quick_draw_config());
    while client.session().unwrap().current().outcome().is_none() {
        client.tick(idle());
    }
    client.ready();
    assert_eq!(client.session().unwrap().id(), 1);
    assert_eq!(client.rival(), Some(Rival::Bot));
    assert_eq!(client.session().unwrap().current().outcome(), None);
}

#[test]
fn a_local_client_still_obeys_the_connection_state() {
    let mut client = Client::practice(MatchConfig::default());
    client.disconnected();
    assert_eq!(client.status(), Status::Disconnected);
    assert_eq!(client.tick(idle()), None);
    assert_eq!(client.session().unwrap().tick(), 0);
}
