//! The room logic alone: seats, readiness, sessions and the frames, event by event.

use srr::game::{Character, Input, PlayerId};
use srr::net::{ClientMessage, MAX_PLAYERS, PlayerSet, ServerMessage};
use srr_server::room::{Action, ClientId, GRACE, MAX_AHEAD, Room, TIMING_PERIOD};

const LEFT: Input = Input {
    left: true,
    ..Input::NONE
};
const JUMP_KICK: Input = Input {
    jump: true,
    kick_high: true,
    ..Input::NONE
};

/// The frame of a [`ClientMessage::Ready { character: Character::Chalk }`].
fn ready() -> Vec<u8> {
    ClientMessage::Ready {
        character: Character::Chalk,
    }
    .encode()
}

/// The frame of a [`ClientMessage::Input`].
fn input(session: u8, tick: u32, input: Input) -> Vec<u8> {
    ClientMessage::Input {
        session,
        tick,
        input,
    }
    .encode()
}

/// The set of the players with the given ids.
fn players(ids: &[u8]) -> PlayerSet {
    ids.iter().map(|&id| PlayerId(id)).collect()
}

/// `message` sent to each of `clients`, in that order.
fn to_all(clients: &[ClientId], message: ServerMessage) -> Vec<Action> {
    clients
        .iter()
        .map(|&client| Action::Send(client, message))
        .collect()
}

/// A [`ServerMessage::Lobby`] sent to each of `clients`, in that order.
fn lobby(clients: &[ClientId], seated: PlayerSet, ready: PlayerSet) -> Vec<Action> {
    to_all(clients, ServerMessage::Lobby { seated, ready })
}

/// The frame of `tick` between `fighters`: `held` lists the inputs that are not
/// [`Input::NONE`].
fn frame(
    session: u8,
    tick: u32,
    fighters: PlayerSet,
    late: PlayerSet,
    held: &[(u8, Input)],
) -> ServerMessage {
    let mut inputs = [Input::NONE; PlayerSet::CAPACITY];
    for &(player, input) in held {
        inputs[usize::from(player)] = input;
    }
    ServerMessage::Frame {
        session,
        tick,
        players: fighters,
        late,
        inputs,
    }
}

/// A room where clients `1..=count` sit in seats `0..count`, nobody ready yet.
fn room_of(count: ClientId) -> Room {
    let mut room = Room::new();
    for client in 1..=count {
        room.join(client);
    }
    room
}

/// Every one of `clients` gets ready, in that order; returns what the last one triggered.
fn all_ready(room: &mut Room, clients: &[ClientId]) -> Vec<Action> {
    clients
        .iter()
        .map(|&client| room.receive(client, &ready()))
        .last()
        .unwrap_or_default()
}

/// `ticks` ticks of the room's clock, and everything they triggered.
fn run(room: &mut Room, ticks: u32) -> Vec<Action> {
    (0..ticks).flat_map(|_| room.tick()).collect()
}

/// The timing reports among `actions`.
fn timings(actions: Vec<Action>) -> Vec<Action> {
    actions
        .into_iter()
        .filter(|action| matches!(action, Action::Send(_, ServerMessage::Timing { .. })))
        .collect()
}

#[test]
fn empty_room() {
    let room = Room::new();
    assert_eq!(room, Room::default());
    assert_eq!(room.session(), 0);
    assert_eq!(room.seated(), PlayerSet::EMPTY);
    assert_eq!(room.ready(), PlayerSet::EMPTY);
    assert_eq!(room.playing(), PlayerSet::EMPTY);
    assert_eq!((room.clock(), room.next_frame()), (0, 0));
    for player in 0..=MAX_PLAYERS as u8 {
        assert_eq!(room.seat(PlayerId(player)), None);
    }
    // Far out of range players have no seat either.
    assert_eq!(room.seat(PlayerId(200)), None);
}

#[test]
fn clients_are_seated_welcomed_and_shown_the_lobby() {
    let mut room = Room::new();
    let mut clients = Vec::new();
    for client in 1..=MAX_PLAYERS as ClientId {
        let player = PlayerId(clients.len() as u8);
        clients.push(client);
        let mut expected = vec![Action::Send(client, ServerMessage::Welcome { player })];
        expected.extend(lobby(
            &clients,
            PlayerSet::first(clients.len()),
            PlayerSet::EMPTY,
        ));
        assert_eq!(room.join(client), expected);
        assert_eq!(room.seat(player), Some(client));
        assert_eq!(room.seated(), PlayerSet::first(clients.len()));
    }
    // Joining starts nothing: everybody has to be ready first.
    assert_eq!(room.session(), 0);
    assert_eq!(room.ready(), PlayerSet::EMPTY);
    assert_eq!(room.playing(), PlayerSet::EMPTY);
}

#[test]
fn ten_players_brawl_and_the_eleventh_is_turned_away() {
    let full = MAX_PLAYERS as ClientId;
    let mut room = room_of(full);
    let before = room.clone();
    assert_eq!(
        room.join(full + 1),
        vec![
            Action::Send(full + 1, ServerMessage::RoomFull),
            Action::Close(full + 1)
        ]
    );
    assert_eq!(room, before);

    let clients: Vec<ClientId> = (1..=full).collect();
    assert_eq!(
        all_ready(&mut room, &clients),
        to_all(
            &clients,
            ServerMessage::Start {
                session: 1,
                players: PlayerSet::first(MAX_PLAYERS),
                characters: [Character::Chalk; 10],
            }
        )
    );
    // Even during a match.
    let before = room.clone();
    assert_eq!(
        room.join(full + 2),
        vec![
            Action::Send(full + 2, ServerMessage::RoomFull),
            Action::Close(full + 2)
        ]
    );
    assert_eq!(room, before);

    // The frame of a tick goes out once all ten inputs are in.
    for &client in &clients[..MAX_PLAYERS - 1] {
        assert_eq!(room.receive(client, &input(1, 0, LEFT)), vec![]);
    }
    let held: Vec<(u8, Input)> = (0..9).map(|player| (player, LEFT)).collect();
    assert_eq!(
        room.receive(full, &input(1, 0, JUMP_KICK)),
        to_all(
            &clients,
            frame(
                1,
                0,
                PlayerSet::first(MAX_PLAYERS),
                PlayerSet::EMPTY,
                &[held, vec![(9, JUMP_KICK)]].concat()
            )
        )
    );
}

#[test]
fn a_single_ready_player_does_not_start_alone() {
    let mut room = room_of(1);
    assert_eq!(
        room.receive(1, &ready()),
        lobby(&[1], players(&[0]), players(&[0]))
    );
    // Getting ready twice changes nothing but is still answered.
    assert_eq!(
        room.receive(1, &ready()),
        lobby(&[1], players(&[0]), players(&[0]))
    );
    assert_eq!(room.session(), 0);
    assert_eq!(room.ready(), players(&[0]));
    assert_eq!(room.playing(), PlayerSet::EMPTY);

    // A newcomer sees who is ready; once they are ready too, the duel starts.
    assert_eq!(
        room.join(2),
        vec![
            Action::Send(
                2,
                ServerMessage::Welcome {
                    player: PlayerId(1)
                }
            ),
            Action::Send(
                1,
                ServerMessage::Lobby {
                    seated: PlayerSet::first(2),
                    ready: players(&[0])
                }
            ),
            Action::Send(
                2,
                ServerMessage::Lobby {
                    seated: PlayerSet::first(2),
                    ready: players(&[0])
                }
            ),
        ]
    );
    assert_eq!(
        room.receive(2, &ready()),
        to_all(
            &[1, 2],
            ServerMessage::Start {
                session: 1,
                players: PlayerSet::first(2),
                characters: [Character::Chalk; 10],
            }
        )
    );
}

#[test]
fn two_ready_players_start_a_duel() {
    let mut room = room_of(2);
    assert_eq!(
        room.receive(2, &ready()),
        lobby(&[1, 2], PlayerSet::first(2), players(&[1]))
    );
    assert_eq!(room.session(), 0);
    assert_eq!(
        room.receive(1, &ready()),
        to_all(
            &[1, 2],
            ServerMessage::Start {
                session: 1,
                players: PlayerSet::first(2),
                characters: [Character::Chalk; 10],
            }
        )
    );
    assert_eq!(room.session(), 1);
    assert_eq!(room.playing(), PlayerSet::first(2));
    // Readiness is used up: the next match needs everybody ready again.
    assert_eq!(room.ready(), PlayerSet::EMPTY);
    assert_eq!(
        room.receive(1, &ready()),
        lobby(&[1, 2], PlayerSet::first(2), players(&[0]))
    );
    assert_eq!(room.session(), 1);
    assert_eq!(room.playing(), PlayerSet::first(2));
}

#[test]
fn three_ready_players_start_a_free_for_all() {
    let mut room = room_of(3);
    assert_eq!(
        room.receive(1, &ready()),
        lobby(&[1, 2, 3], PlayerSet::first(3), players(&[0]))
    );
    assert_eq!(
        room.receive(3, &ready()),
        lobby(&[1, 2, 3], PlayerSet::first(3), players(&[0, 2]))
    );
    assert_eq!(room.session(), 0);
    assert_eq!(
        room.receive(2, &ready()),
        to_all(
            &[1, 2, 3],
            ServerMessage::Start {
                session: 1,
                players: PlayerSet::first(3),
                characters: [Character::Chalk; 10],
            }
        )
    );
    assert_eq!(room.session(), 1);
    assert_eq!(room.playing(), PlayerSet::first(3));
    assert_eq!(room.ready(), PlayerSet::EMPTY);
}

#[test]
fn a_frame_goes_out_in_order_as_soon_as_every_input_is_there() {
    let mut room = room_of(3);
    all_ready(&mut room, &[1, 2, 3]);
    let all = PlayerSet::first(3);

    assert_eq!(room.receive(1, &input(1, 0, LEFT)), vec![]);
    assert_eq!(room.receive(3, &input(1, 0, JUMP_KICK)), vec![]);
    assert_eq!(
        room.receive(2, &input(1, 0, Input::NONE)),
        to_all(
            &[1, 2, 3],
            frame(1, 0, all, PlayerSet::EMPTY, &[(0, LEFT), (2, JUMP_KICK)])
        )
    );
    assert_eq!(room.next_frame(), 1);

    // Tick 2 is complete before tick 1: it waits for it.
    for client in 1..=3 {
        assert_eq!(room.receive(client, &input(1, 2, LEFT)), vec![]);
    }
    room.receive(1, &input(1, 1, Input::NONE));
    room.receive(2, &input(1, 1, Input::NONE));
    let mut expected = to_all(&[1, 2, 3], frame(1, 1, all, PlayerSet::EMPTY, &[]));
    expected.extend(to_all(
        &[1, 2, 3],
        frame(
            1,
            2,
            all,
            PlayerSet::EMPTY,
            &[(0, LEFT), (1, LEFT), (2, LEFT)],
        ),
    ));
    assert_eq!(room.receive(3, &input(1, 1, Input::NONE)), expected);
    // No clock needed on a fast network.
    assert_eq!((room.clock(), room.next_frame()), (0, 3));
}

#[test]
fn a_late_input_is_replaced_with_the_previous_one() {
    let mut room = room_of(2);
    all_ready(&mut room, &[1, 2]);
    let both = PlayerSet::first(2);
    room.receive(1, &input(1, 0, LEFT));
    room.receive(2, &input(1, 0, LEFT));
    room.receive(1, &input(1, 1, JUMP_KICK));
    room.receive(1, &input(1, 2, Input::NONE));
    room.receive(2, &input(1, 2, LEFT));

    // The frame of tick 1 waits GRACE ticks of the clock for player 2...
    for _ in 0..=GRACE {
        assert_eq!(room.tick(), vec![]);
    }
    assert_eq!(room.clock(), GRACE + 1);
    // ...then goes out with their previous input, followed by the complete frame of tick 2.
    let mut expected = to_all(
        &[1, 2],
        frame(1, 1, both, players(&[1]), &[(0, JUMP_KICK), (1, LEFT)]),
    );
    expected.extend(to_all(
        &[1, 2],
        frame(1, 2, both, PlayerSet::EMPTY, &[(1, LEFT)]),
    ));
    assert_eq!(room.tick(), expected);
    // The input, when it comes, is too late to count.
    assert_eq!(room.receive(2, &input(1, 1, Input::NONE)), vec![]);

    // Nobody sent anything for tick 3: everybody repeats, everybody is late.
    assert_eq!(room.tick(), vec![]);
    assert_eq!(
        room.tick(),
        to_all(&[1, 2], frame(1, 3, both, both, &[(1, LEFT)]))
    );
    assert_eq!(room.next_frame(), 4);
}

#[test]
fn inputs_too_far_ahead_or_sent_twice_are_dropped() {
    let mut room = room_of(2);
    all_ready(&mut room, &[1, 2]);
    assert_eq!(room.receive(1, &input(1, MAX_AHEAD + 1, LEFT)), vec![]);
    for tick in 0..MAX_AHEAD {
        room.receive(1, &input(1, tick, Input::NONE));
        room.receive(2, &input(1, tick, Input::NONE));
    }
    assert_eq!(room.next_frame(), MAX_AHEAD);

    assert_eq!(room.receive(1, &input(1, MAX_AHEAD, LEFT)), vec![]);
    assert_eq!(
        room.receive(1, &input(1, MAX_AHEAD, JUMP_KICK)),
        vec![],
        "the first input of a tick counts"
    );
    assert_eq!(
        room.receive(2, &input(1, MAX_AHEAD, Input::NONE)),
        to_all(
            &[1, 2],
            frame(
                1,
                MAX_AHEAD,
                PlayerSet::first(2),
                PlayerSet::EMPTY,
                &[(0, LEFT)]
            )
        )
    );
    // The input too far ahead was not kept: the next frame still waits for player 1.
    assert_eq!(room.receive(2, &input(1, MAX_AHEAD + 1, LEFT)), vec![]);
    // Neither are inputs of frames already sent.
    assert_eq!(room.receive(2, &input(1, 3, LEFT)), vec![]);
    assert_eq!(room.next_frame(), MAX_AHEAD + 1);
}

#[test]
fn every_player_hears_how_early_its_inputs_come() {
    let mut room = room_of(3);
    all_ready(&mut room, &[1, 2, 3]);
    // At clock 0, player 1 is 10 ticks early at worst (tick 2), player 2 is 8 ticks early.
    room.receive(1, &input(1, 4, LEFT));
    room.receive(1, &input(1, 2, LEFT));
    room.receive(2, &input(1, 0, LEFT));
    assert_eq!(timings(run(&mut room, TIMING_PERIOD - 1)), vec![]);
    // Then player 2's input for tick 3 comes at clock 14: three ticks too late.
    assert_eq!(room.receive(2, &input(1, 3, LEFT)), vec![]);
    // Player 3 sent nothing: no report.
    assert_eq!(
        timings(room.tick()),
        vec![
            Action::Send(1, ServerMessage::Timing { tick: 2, lead: 10 }),
            Action::Send(2, ServerMessage::Timing { tick: 3, lead: -3 }),
        ]
    );
    // Each report covers the inputs since the previous one.
    assert_eq!(timings(run(&mut room, TIMING_PERIOD)), vec![]);
    assert_eq!(room.clock(), 2 * TIMING_PERIOD);
}

#[test]
fn timing_reports_are_clamped_to_a_byte() {
    let mut room = room_of(2);
    all_ready(&mut room, &[1, 2]);
    run(&mut room, 10 * TIMING_PERIOD);
    room.receive(1, &input(1, 0, LEFT));
    room.receive(2, &input(1, u32::MAX, LEFT));
    assert_eq!(
        timings(run(&mut room, TIMING_PERIOD)),
        vec![
            Action::Send(
                1,
                ServerMessage::Timing {
                    tick: 0,
                    lead: i8::MIN
                }
            ),
            Action::Send(
                2,
                ServerMessage::Timing {
                    tick: u32::MAX,
                    lead: i8::MAX
                }
            ),
        ]
    );
}

#[test]
fn the_clock_runs_during_sessions_only_and_restarts_with_each() {
    let mut room = room_of(2);
    assert_eq!(room.tick(), vec![], "nothing to do before a session");
    assert_eq!(room.clock(), 0);

    all_ready(&mut room, &[1, 2]);
    // Nobody sends anything: the frames go out on time anyway, everybody late.
    let actions = run(&mut room, GRACE + 1);
    assert_eq!(
        actions,
        to_all(
            &[1, 2],
            frame(1, 0, PlayerSet::first(2), PlayerSet::first(2), &[])
        )
    );
    run(&mut room, 11);
    assert_eq!((room.clock(), room.next_frame()), (20, 20 - GRACE));

    all_ready(&mut room, &[1, 2]);
    assert_eq!(room.session(), 2);
    assert_eq!((room.clock(), room.next_frame()), (0, 0));
}

#[test]
fn inputs_of_another_session_are_dropped() {
    let mut room = room_of(2);
    // No session yet: nobody plays.
    assert_eq!(room.receive(1, &input(0, 0, LEFT)), vec![]);

    all_ready(&mut room, &[1, 2]);
    assert_eq!(room.receive(1, &input(0, 0, LEFT)), vec![]);
    assert_eq!(room.receive(1, &input(2, 0, LEFT)), vec![]);
    assert_eq!(room.receive(1, &input(1, 0, LEFT)), vec![]);
    assert_eq!(
        room.receive(2, &input(1, 0, Input::NONE)),
        to_all(
            &[1, 2],
            frame(1, 0, PlayerSet::first(2), PlayerSet::EMPTY, &[(0, LEFT)])
        )
    );

    // Inputs of the previous session arriving after the next one started are stale.
    all_ready(&mut room, &[1, 2]);
    assert_eq!(room.session(), 2);
    assert_eq!(room.receive(2, &input(1, 0, LEFT)), vec![]);
    assert_eq!(room.receive(2, &input(2, 0, LEFT)), vec![]);
    assert_eq!(
        room.receive(1, &input(2, 0, Input::NONE)),
        to_all(
            &[1, 2],
            frame(2, 0, PlayerSet::first(2), PlayerSet::EMPTY, &[(1, LEFT)])
        )
    );
}

#[test]
fn a_late_joiner_sits_out_the_running_match() {
    let mut room = room_of(2);
    all_ready(&mut room, &[1, 2]);

    assert_eq!(
        room.join(3),
        vec![
            Action::Send(
                3,
                ServerMessage::Welcome {
                    player: PlayerId(2)
                }
            ),
            Action::Send(
                1,
                ServerMessage::Lobby {
                    seated: PlayerSet::first(3),
                    ready: PlayerSet::EMPTY
                }
            ),
            Action::Send(
                2,
                ServerMessage::Lobby {
                    seated: PlayerSet::first(3),
                    ready: PlayerSet::EMPTY
                }
            ),
            Action::Send(
                3,
                ServerMessage::Lobby {
                    seated: PlayerSet::first(3),
                    ready: PlayerSet::EMPTY
                }
            ),
        ]
    );
    assert_eq!(room.seated(), PlayerSet::first(3));
    assert_eq!(room.playing(), PlayerSet::first(2));

    // The duel goes on without them: they get no frames and their inputs go nowhere.
    room.receive(1, &input(1, 0, LEFT));
    assert_eq!(room.receive(3, &input(1, 0, LEFT)), vec![]);
    assert_eq!(
        room.receive(2, &input(1, 0, Input::NONE)),
        to_all(
            &[1, 2],
            frame(1, 0, PlayerSet::first(2), PlayerSet::EMPTY, &[(0, LEFT)])
        )
    );
    let lead = GRACE as i8;
    assert_eq!(
        timings(run(&mut room, TIMING_PERIOD)),
        to_all(&[1, 2], ServerMessage::Timing { tick: 0, lead })
    );

    // The next match waits for them too.
    assert_eq!(
        room.receive(1, &ready()),
        lobby(&[1, 2, 3], PlayerSet::first(3), players(&[0]))
    );
    assert_eq!(
        room.receive(2, &ready()),
        lobby(&[1, 2, 3], PlayerSet::first(3), players(&[0, 1]))
    );
    assert_eq!(
        room.receive(3, &ready()),
        to_all(
            &[1, 2, 3],
            ServerMessage::Start {
                session: 2,
                players: PlayerSet::first(3),
                characters: [Character::Chalk; 10],
            }
        )
    );
    assert_eq!(room.playing(), PlayerSet::first(3));
    room.receive(1, &input(2, 0, Input::NONE));
    room.receive(2, &input(2, 0, Input::NONE));
    assert_eq!(
        room.receive(3, &input(2, 0, JUMP_KICK)),
        to_all(
            &[1, 2, 3],
            frame(
                2,
                0,
                PlayerSet::first(3),
                PlayerSet::EMPTY,
                &[(2, JUMP_KICK)]
            )
        )
    );
}

#[test]
fn malformed_frames_close_the_sender() {
    let mut room = room_of(2);
    all_ready(&mut room, &[1, 2]);
    let before = room.clone();
    let frames: [&[u8]; 6] = [
        &[],
        &[0xFF, 0, 0],
        // An input one byte short, and a ready one byte long.
        &[3, 1, 0, 0, 0, 0, 0],
        &[7, 0, 0],
        // Server messages are not client messages.
        &[5],
        &[1, 0],
    ];
    for frame in frames {
        assert_eq!(room.receive(1, frame), vec![Action::Close(1)], "{frame:?}");
        assert_eq!(room.receive(2, frame), vec![Action::Close(2)], "{frame:?}");
    }
    // Closing is up to the network layer, which then reports the leave.
    assert_eq!(room, before);
}

#[test]
fn unknown_clients_are_ignored() {
    let mut room = room_of(2);
    all_ready(&mut room, &[1, 2]);
    let before = room.clone();
    assert_eq!(room.receive(42, &ready()), vec![]);
    assert_eq!(room.receive(42, &input(1, 10, LEFT)), vec![]);
    // Even malformed frames: the client is not ours to close.
    assert_eq!(room.receive(42, &[0xFF]), vec![]);
    assert_eq!(room.leave(42), vec![]);
    assert_eq!(room, before);
}

#[test]
fn the_match_goes_on_without_a_leaver_while_two_remain() {
    let mut room = room_of(3);
    all_ready(&mut room, &[1, 2, 3]);
    let fighters = PlayerSet::first(3);
    room.receive(1, &input(1, 0, LEFT));
    room.receive(3, &input(1, 0, JUMP_KICK));

    // The others are told, and the frame that only waited for the leaver goes out.
    let mut expected = to_all(
        &[1, 3],
        ServerMessage::PlayerLeft {
            player: PlayerId(1),
        },
    );
    expected.extend(to_all(
        &[1, 3],
        frame(
            1,
            0,
            fighters,
            PlayerSet::EMPTY,
            &[(0, LEFT), (2, JUMP_KICK)],
        ),
    ));
    expected.extend(lobby(&[1, 3], players(&[0, 2]), PlayerSet::EMPTY));
    assert_eq!(room.leave(2), expected);
    assert_eq!(room.seat(PlayerId(1)), None);
    assert_eq!(room.seated(), players(&[0, 2]));
    assert_eq!(room.playing(), players(&[0, 2]));

    // Nobody waits for the leaver, whose fighter stands still (and is not late).
    room.receive(1, &input(1, 1, LEFT));
    assert_eq!(
        room.receive(3, &input(1, 1, LEFT)),
        to_all(
            &[1, 3],
            frame(1, 1, fighters, PlayerSet::EMPTY, &[(0, LEFT), (2, LEFT)])
        )
    );

    // A newcomer takes the lowest free seat and is not part of the running session.
    let mut expected = vec![Action::Send(
        4,
        ServerMessage::Welcome {
            player: PlayerId(1),
        },
    )];
    expected.extend(lobby(&[1, 4, 3], PlayerSet::first(3), PlayerSet::EMPTY));
    assert_eq!(room.join(4), expected);
    assert_eq!(room.playing(), players(&[0, 2]));
    assert_eq!(room.receive(4, &input(1, 2, LEFT)), vec![]);
    room.receive(1, &input(1, 2, Input::NONE));
    assert_eq!(
        room.receive(3, &input(1, 2, Input::NONE)),
        to_all(&[1, 3], frame(1, 2, fighters, PlayerSet::EMPTY, &[]))
    );

    // Down to one player: the match is over, the clock stops.
    let mut expected = to_all(
        &[4, 3],
        ServerMessage::PlayerLeft {
            player: PlayerId(0),
        },
    );
    expected.extend(lobby(&[4, 3], players(&[1, 2]), PlayerSet::EMPTY));
    assert_eq!(room.leave(1), expected);
    assert_eq!(room.playing(), PlayerSet::EMPTY);
    assert_eq!(room.receive(3, &input(1, 3, LEFT)), vec![]);
    assert_eq!(run(&mut room, 2 * GRACE + TIMING_PERIOD), vec![]);

    // The last one leaving tells nobody.
    room.leave(4);
    assert_eq!(room.leave(3), vec![]);
    assert_eq!(room.seated(), PlayerSet::EMPTY);
}

#[test]
fn a_ready_player_leaving_is_no_longer_ready() {
    let mut room = room_of(3);
    room.receive(1, &ready());
    room.receive(2, &ready());
    let mut expected = to_all(
        &[2, 3],
        ServerMessage::PlayerLeft {
            player: PlayerId(0),
        },
    );
    expected.extend(lobby(&[2, 3], players(&[1, 2]), players(&[1])));
    assert_eq!(room.leave(1), expected);
    assert_eq!(room.ready(), players(&[1]));
    assert_eq!(room.session(), 0);
}

#[test]
fn leaving_starts_the_match_when_the_others_are_all_ready() {
    let mut room = room_of(3);
    room.receive(1, &ready());
    room.receive(3, &ready());
    let mut expected = to_all(
        &[1, 3],
        ServerMessage::PlayerLeft {
            player: PlayerId(1),
        },
    );
    expected.extend(to_all(
        &[1, 3],
        ServerMessage::Start {
            session: 1,
            players: players(&[0, 2]),
            characters: [Character::Chalk; 10],
        },
    ));
    assert_eq!(room.leave(2), expected);
    assert_eq!(room.session(), 1);
    assert_eq!(room.playing(), players(&[0, 2]));
    assert_eq!(room.ready(), PlayerSet::EMPTY);

    // A lone ready player left behind does not start anything.
    room.receive(1, &ready());
    let mut expected = to_all(
        &[1],
        ServerMessage::PlayerLeft {
            player: PlayerId(2),
        },
    );
    expected.extend(lobby(&[1], players(&[0]), players(&[0])));
    assert_eq!(room.leave(3), expected);
    assert_eq!(room.session(), 1);
    assert_eq!(room.playing(), PlayerSet::EMPTY);
}

#[test]
fn session_numbers_wrap_around() {
    let mut room = room_of(2);
    let both = PlayerSet::first(2);
    let mut session: u8 = 0;
    for _ in 0..256 {
        session = session.wrapping_add(1);
        assert_eq!(
            all_ready(&mut room, &[1, 2]),
            to_all(
                &[1, 2],
                ServerMessage::Start {
                    session,
                    players: both,
                    characters: [Character::Chalk; 10],
                }
            )
        );
        assert_eq!(room.session(), session);
        if session == u8::MAX {
            room.receive(1, &input(u8::MAX, 0, LEFT));
            assert_eq!(
                room.receive(2, &input(u8::MAX, 0, Input::NONE)),
                to_all(
                    &[1, 2],
                    frame(u8::MAX, 0, both, PlayerSet::EMPTY, &[(0, LEFT)])
                )
            );
        }
    }
    // Session 255 was followed by session 0.
    assert_eq!(room.session(), 0);
    assert_eq!(room.receive(1, &input(u8::MAX, 0, LEFT)), vec![]);
    room.receive(1, &input(0, 0, Input::NONE));
    assert_eq!(
        room.receive(2, &input(0, 0, LEFT)),
        to_all(&[1, 2], frame(0, 0, both, PlayerSet::EMPTY, &[(1, LEFT)]))
    );
}

#[test]
fn the_match_starts_with_the_characters_picked_when_getting_ready() {
    let mut room = room_of(3);
    let ready_as = |character| ClientMessage::Ready { character }.encode();
    room.receive(1, &ready_as(Character::Torch));
    // Changing one's mind while waiting: the last pick counts.
    room.receive(2, &ready_as(Character::Zip));
    room.receive(2, &ready_as(Character::Brick));
    let actions = room.receive(3, &ready());
    let mut characters = [Character::Chalk; MAX_PLAYERS];
    characters[0] = Character::Torch;
    characters[1] = Character::Brick;
    let start = ServerMessage::Start {
        session: 1,
        players: PlayerSet::first(3),
        characters,
    };
    assert!(actions.contains(&Action::Send(1, start)), "{actions:?}");

    // The next match keeps the picks unless they change.
    let mut room = room_of(10);
    for client in 1..=10 {
        room.receive(client, &ready_as(Character::Zip));
    }
    assert_eq!(room.playing(), PlayerSet::first(10));
}
