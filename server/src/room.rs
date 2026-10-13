//! The single room: who sits in which seat, who is ready, who plays, and what to tell whom.
//!
//! Pure logic: every event returns the [`Action`]s the network layer must perform.

use srr::game::PlayerId;
use srr::net::{ClientMessage, MAX_PLAYERS, PlayerSet, ServerMessage};

/// Identifies one WebSocket connection.
pub type ClientId = u64;

/// What the network layer has to do after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Send `message` to `client`.
    Send(ClientId, ServerMessage),
    /// Close the connection of `client`.
    Close(ClientId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seat {
    client: ClientId,
    ready: bool,
}

/// The [`MAX_PLAYERS`] seats of the room.
///
/// A new connection takes the lowest free seat (seat `i` plays fighter `i`) and is welcomed;
/// one more is turned away. Every change of who is seated or ready is broadcast as a
/// [`ServerMessage::Lobby`]. Once every seated player is ready, and at least two are seated, a
/// new session starts between all of them, everybody for themselves; readiness is then used
/// up, so the next match needs everybody ready again (which lets newcomers join it).
///
/// Inputs of the current session are relayed to the other players of the session. When a
/// player leaves, the others are told; a match they played in cannot go on.
///
/// ```
/// use srr::game::PlayerId;
/// use srr::net::{ClientMessage, PlayerSet, ServerMessage};
/// use srr_server::room::{Action, Room};
///
/// let mut room = Room::new();
/// for client in 1..=3 {
///     room.join(client);
/// }
/// assert_eq!(room.seated(), PlayerSet::first(3));
/// let ready = ClientMessage::Ready.encode();
/// room.receive(1, &ready);
/// room.receive(2, &ready);
/// let actions = room.receive(3, &ready);
/// let start = ServerMessage::Start { session: 1, players: PlayerSet::first(3) };
/// assert!(actions.contains(&Action::Send(3, start)));
/// assert_eq!(room.playing(), PlayerSet::first(3));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Room {
    seats: [Option<Seat>; MAX_PLAYERS],
    session: u8,
    playing: PlayerSet,
}

impl Room {
    /// An empty room.
    pub fn new() -> Self {
        Room::default()
    }

    /// The client in the seat of `player`, if any.
    pub fn seat(&self, player: PlayerId) -> Option<ClientId> {
        self.seats
            .get(usize::from(player.0))
            .copied()
            .flatten()
            .map(|seat| seat.client)
    }

    /// The number of the current (or last) session; 0 before the first one.
    pub fn session(&self) -> u8 {
        self.session
    }

    /// The occupied seats.
    pub fn seated(&self) -> PlayerSet {
        self.players_where(|_| true)
    }

    /// The seated players waiting for the next match.
    pub fn ready(&self) -> PlayerSet {
        self.players_where(|seat| seat.ready)
    }

    /// The players of the current session who are still connected.
    pub fn playing(&self) -> PlayerSet {
        self.playing
    }

    /// A new connection: seat it, or turn it away when the room is full.
    pub fn join(&mut self, client: ClientId) -> Vec<Action> {
        let Some(index) = self.seats.iter().position(Option::is_none) else {
            return vec![
                Action::Send(client, ServerMessage::RoomFull),
                Action::Close(client),
            ];
        };
        self.seats[index] = Some(Seat {
            client,
            ready: false,
        });
        let mut actions = vec![Action::Send(
            client,
            ServerMessage::Welcome {
                player: player_at(index),
            },
        )];
        actions.extend(self.lobby());
        actions
    }

    /// A connection closed. Leaving a room one is not seated in changes nothing.
    pub fn leave(&mut self, client: ClientId) -> Vec<Action> {
        let Some(index) = self.index_of(client) else {
            return Vec::new();
        };
        let player = player_at(index);
        self.seats[index] = None;
        self.playing = self.playing.without(player);
        let mut actions: Vec<Action> = self
            .clients()
            .map(|(_, other)| Action::Send(other, ServerMessage::PlayerLeft { player }))
            .collect();
        actions.extend(self.start_or_lobby());
        actions
    }

    /// A binary frame from `client`. [`ClientMessage::Ready`] marks the sender ready (and may
    /// start the next session); inputs of the current session go to its other players, inputs
    /// of an older session or of a player outside it are dropped. A malformed frame closes the
    /// sender's connection.
    pub fn receive(&mut self, client: ClientId, bytes: &[u8]) -> Vec<Action> {
        let Some(index) = self.index_of(client) else {
            return Vec::new();
        };
        let player = player_at(index);
        match ClientMessage::decode(bytes) {
            Err(_) => vec![Action::Close(client)],
            Ok(ClientMessage::Ready) => {
                if let Some(seat) = self.seats[index].as_mut() {
                    seat.ready = true;
                }
                self.start_or_lobby()
            }
            Ok(ClientMessage::Input {
                session,
                tick,
                input,
            }) => {
                if session != self.session || !self.playing.contains(player) {
                    return Vec::new();
                }
                let relayed = ServerMessage::Input {
                    session,
                    player,
                    tick,
                    input,
                };
                self.clients()
                    .filter(|&(other, _)| other != player && self.playing.contains(other))
                    .map(|(_, other)| Action::Send(other, relayed))
                    .collect()
            }
        }
    }

    /// Starts the next session if everybody seated (at least two) is ready; otherwise tells
    /// everybody who is seated and ready.
    fn start_or_lobby(&mut self) -> Vec<Action> {
        let seated = self.seated();
        if seated.len() < 2 || self.ready() != seated {
            return self.lobby();
        }
        self.session = self.session.wrapping_add(1);
        self.playing = seated;
        for seat in self.seats.iter_mut().flatten() {
            seat.ready = false;
        }
        let start = ServerMessage::Start {
            session: self.session,
            players: seated,
        };
        self.clients()
            .map(|(_, client)| Action::Send(client, start))
            .collect()
    }

    fn lobby(&self) -> Vec<Action> {
        let lobby = ServerMessage::Lobby {
            seated: self.seated(),
            ready: self.ready(),
        };
        self.clients()
            .map(|(_, client)| Action::Send(client, lobby))
            .collect()
    }

    /// Every seated player with their client, by seat.
    fn clients(&self) -> impl Iterator<Item = (PlayerId, ClientId)> + '_ {
        self.seats
            .iter()
            .enumerate()
            .filter_map(|(index, seat)| seat.map(|seat| (player_at(index), seat.client)))
    }

    fn players_where(&self, keep: impl Fn(&Seat) -> bool) -> PlayerSet {
        self.seats
            .iter()
            .enumerate()
            .filter(|(_, seat)| seat.as_ref().is_some_and(&keep))
            .map(|(index, _)| player_at(index))
            .collect()
    }

    fn index_of(&self, client: ClientId) -> Option<usize> {
        self.seats
            .iter()
            .position(|seat| seat.is_some_and(|seat| seat.client == client))
    }
}

/// The player of seat `index` (`index` is below [`MAX_PLAYERS`]).
fn player_at(index: usize) -> PlayerId {
    PlayerId(index as u8)
}
