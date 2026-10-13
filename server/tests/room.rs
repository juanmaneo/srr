//! The room logic alone: seats, readiness, sessions and the relay, event by event.

use srr::game::{Input, PlayerId};
use srr::net::{ClientMessage, MAX_PLAYERS, PlayerSet, ServerMessage};
use srr_server::room::{Action, ClientId, Room};

const LEFT: Input = Input {
    left: true,
    ..Input::NONE
};
const JUMP_KICK: Input = Input {
    jump: true,
    kick_high: true,
    ..Input::NONE
};

/// The frame of a [`ClientMessage::Ready`].
fn ready() -> Vec<u8> {
    ClientMessage::Ready.encode()
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

/// The relay of an input of `player` to each of `clients`.
fn relayed(clients: &[ClientId], session: u8, player: u8, tick: u32, input: Input) -> Vec<Action> {
    to_all(
        clients,
        ServerMessage::Input {
            session,
            player: PlayerId(player),
            tick,
            input,
        },
    )
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

#[test]
fn empty_room() {
    let room = Room::new();
    assert_eq!(room, Room::default());
    assert_eq!(room.session(), 0);
    assert_eq!(room.seated(), PlayerSet::EMPTY);
    assert_eq!(room.ready(), PlayerSet::EMPTY);
    assert_eq!(room.playing(), PlayerSet::EMPTY);
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
fn fifth_client_is_turned_away() {
    let mut room = room_of(4);
    let before = room.clone();
    assert_eq!(
        room.join(5),
        vec![Action::Send(5, ServerMessage::RoomFull), Action::Close(5)]
    );
    assert_eq!(room, before);

    // Even during a match.
    all_ready(&mut room, &[1, 2, 3, 4]);
    let before = room.clone();
    assert_eq!(
        room.join(6),
        vec![Action::Send(6, ServerMessage::RoomFull), Action::Close(6)]
    );
    assert_eq!(room, before);
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
                players: PlayerSet::first(2)
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
                players: PlayerSet::first(2)
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
                players: PlayerSet::first(3)
            }
        )
    );
    assert_eq!(room.session(), 1);
    assert_eq!(room.playing(), PlayerSet::first(3));
    assert_eq!(room.ready(), PlayerSet::EMPTY);
}

#[test]
fn inputs_are_relayed_to_every_other_player() {
    let mut room = room_of(4);
    all_ready(&mut room, &[1, 2, 3, 4]);
    assert_eq!(room.playing(), PlayerSet::first(4));

    assert_eq!(
        room.receive(2, &input(1, 10, LEFT)),
        relayed(&[1, 3, 4], 1, 1, 10, LEFT)
    );
    assert_eq!(
        room.receive(4, &input(1, 11, JUMP_KICK)),
        relayed(&[1, 2, 3], 1, 3, 11, JUMP_KICK)
    );
    assert_eq!(
        room.receive(1, &input(1, u32::MAX, Input::NONE)),
        relayed(&[2, 3, 4], 1, 0, u32::MAX, Input::NONE)
    );
}

#[test]
fn inputs_of_another_session_are_dropped() {
    let mut room = room_of(2);
    // No session yet: nobody plays.
    assert_eq!(room.receive(1, &input(0, 10, LEFT)), vec![]);

    all_ready(&mut room, &[1, 2]);
    assert_eq!(room.receive(1, &input(0, 10, LEFT)), vec![]);
    assert_eq!(room.receive(1, &input(2, 10, LEFT)), vec![]);
    assert_eq!(
        room.receive(1, &input(1, 10, LEFT)),
        relayed(&[2], 1, 0, 10, LEFT)
    );

    // Inputs of the previous session arriving after the next one started are stale.
    all_ready(&mut room, &[1, 2]);
    assert_eq!(room.session(), 2);
    assert_eq!(room.receive(2, &input(1, 99, LEFT)), vec![]);
    assert_eq!(
        room.receive(2, &input(2, 0, LEFT)),
        relayed(&[1], 2, 1, 0, LEFT)
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

    // The duel goes on without them: they get no inputs and theirs go nowhere.
    assert_eq!(
        room.receive(1, &input(1, 5, LEFT)),
        relayed(&[2], 1, 0, 5, LEFT)
    );
    assert_eq!(room.receive(3, &input(1, 5, LEFT)), vec![]);

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
                players: PlayerSet::first(3)
            }
        )
    );
    assert_eq!(room.playing(), PlayerSet::first(3));
    assert_eq!(
        room.receive(3, &input(2, 0, JUMP_KICK)),
        relayed(&[1, 2], 2, 2, 0, JUMP_KICK)
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
        &[7, 0],
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
fn leaving_tells_the_others_and_frees_the_seat() {
    let mut room = room_of(3);
    all_ready(&mut room, &[1, 2, 3]);

    let mut expected = to_all(
        &[1, 3],
        ServerMessage::PlayerLeft {
            player: PlayerId(1),
        },
    );
    expected.extend(lobby(&[1, 3], players(&[0, 2]), PlayerSet::EMPTY));
    assert_eq!(room.leave(2), expected);
    assert_eq!(room.seat(PlayerId(1)), None);
    assert_eq!(room.seated(), players(&[0, 2]));
    assert_eq!(room.playing(), players(&[0, 2]));

    // The others' inputs still reach each other (their front ends decide what to do).
    assert_eq!(
        room.receive(3, &input(1, 7, LEFT)),
        relayed(&[1], 1, 2, 7, LEFT)
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

    // Down to one player, then nobody: the last one leaving tells nobody.
    room.leave(4);
    let mut expected = to_all(
        &[3],
        ServerMessage::PlayerLeft {
            player: PlayerId(0),
        },
    );
    expected.extend(lobby(&[3], players(&[2]), PlayerSet::EMPTY));
    assert_eq!(room.leave(1), expected);
    assert_eq!(room.playing(), players(&[2]));
    assert_eq!(room.leave(3), vec![]);
    assert_eq!(room.seated(), PlayerSet::EMPTY);
    assert_eq!(room.playing(), PlayerSet::EMPTY);
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
}

#[test]
fn session_numbers_wrap_around() {
    let mut room = room_of(2);
    let mut session: u8 = 0;
    for _ in 0..256 {
        session = session.wrapping_add(1);
        assert_eq!(
            all_ready(&mut room, &[1, 2]),
            to_all(
                &[1, 2],
                ServerMessage::Start {
                    session,
                    players: PlayerSet::first(2)
                }
            )
        );
        assert_eq!(room.session(), session);
        if session == u8::MAX {
            assert_eq!(
                room.receive(1, &input(u8::MAX, 0, LEFT)),
                relayed(&[2], u8::MAX, 0, 0, LEFT)
            );
        }
    }
    // Session 255 was followed by session 0.
    assert_eq!(room.session(), 0);
    assert_eq!(room.receive(1, &input(u8::MAX, 1, LEFT)), vec![]);
    assert_eq!(
        room.receive(2, &input(0, 1, LEFT)),
        relayed(&[1], 0, 1, 1, LEFT)
    );
}
