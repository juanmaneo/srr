//! The single room: who sits in which seat, who is ready, who plays, and what to tell whom.
//!
//! Pure logic: every event returns the [`Action`]s the network layer must perform, and time
//! only passes when the network layer calls [`Room::tick`].

use std::collections::BTreeMap;

use srr::game::{Character, Input, PlayerId, Skin, StyleSet};
use srr::net::{Characters, ClientMessage, MAX_PLAYERS, PlayerSet, ServerMessage};

/// Identifies one WebSocket connection.
pub type ClientId = u64;

/// Ticks of the room's clock the frame of a tick waits for late inputs: the frame of tick `t`
/// goes out, with the missing inputs replaced, once the clock reaches `t + GRACE`. The clock
/// starts with the session, so a client whose round trip to the server is shorter than about
/// `GRACE` plus its input delay is in time from the start (the others catch up with
/// [`ServerMessage::Timing`]).
pub const GRACE: u32 = 8;
/// Ticks of the room's clock between two [`ServerMessage::Timing`] reports to each player.
pub const TIMING_PERIOD: u32 = 15;
/// How many ticks ahead of the clock an input may be; inputs further ahead are dropped, which
/// bounds what the room keeps for a client whose clock runs too fast.
pub const MAX_AHEAD: u32 = 120;

/// What the network layer has to do after an event.
#[allow(
    clippy::large_enum_variant,
    reason = "as big as a `ServerMessage`, which stays unboxed on purpose; actions are handled at once"
)]
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
    /// The character picked with the last [`ClientMessage::Ready`].
    character: Character,
    /// The skin the player wears (the chalk until they pick one).
    skin: Skin,
}

/// The inputs received for one tick, by seat.
type Received = [Option<Input>; MAX_PLAYERS];

/// The [`MAX_PLAYERS`] seats of the room.
///
/// A new connection takes the lowest free seat (seat `i` plays fighter `i`) and is welcomed;
/// one more is turned away. Every change of who is seated or ready is broadcast as a
/// [`ServerMessage::Lobby`]. Once every seated player is ready, and at least two are seated, a
/// new session starts between all of them, everybody for themselves, each with the character
/// they picked when they got ready; readiness is then used up, so the next match needs
/// everybody ready again (which lets newcomers join it, and everybody change character).
///
/// During a session the room gathers the inputs of its players and sends everybody one
/// [`ServerMessage::Frame`] per tick, in tick order: as soon as every input of the tick is
/// there, or once the clock says the tick waited [`GRACE`] ticks. Then a missing input is
/// replaced with the player's previous one (and the input, when it comes, is dropped): a slow
/// connection only slows its own fighter. Every [`TIMING_PERIOD`] ticks each player is told
/// how early its inputs came, at worst ([`ServerMessage::Timing`]).
///
/// When a player leaves, the others are told and the match goes on with the leaver's fighter
/// standing still, as long as two players remain.
///
/// ```
/// use srr::game::{Character, PlayerId};
/// use srr::net::{ClientMessage, PlayerSet, ServerMessage};
/// use srr_server::room::{Action, Room};
///
/// let mut room = Room::new();
/// for client in 1..=3 {
///     room.join(client);
/// }
/// assert_eq!(room.seated(), PlayerSet::first(3));
/// let ready = ClientMessage::Ready { character: Character::Chalk }.encode();
/// room.receive(1, &ready);
/// room.receive(2, &ClientMessage::Ready { character: Character::Torch }.encode());
/// let actions = room.receive(3, &ready);
/// let mut characters = [Character::Chalk; 10];
/// characters[1] = Character::Torch;
/// let start = ServerMessage::Start { session: 1, players: PlayerSet::first(3), characters };
/// assert!(actions.contains(&Action::Send(3, start)));
/// assert_eq!(room.playing(), PlayerSet::first(3));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    seats: [Option<Seat>; MAX_PLAYERS],
    session: u8,
    /// Who fights in the current session, connected or not: whose inputs the frames carry.
    fighters: PlayerSet,
    /// The fighters of the current session who are still connected.
    playing: PlayerSet,
    /// Ticks since the current session started.
    clock: u32,
    /// The tick of the next frame to send.
    next_frame: u32,
    /// The inputs received for the ticks from `next_frame` on.
    pending: BTreeMap<u32, Received>,
    /// The input each player had in the last frame: what a late input is replaced with.
    last: [Input; MAX_PLAYERS],
    /// The worst margin of each player's inputs since the last timing report, and the tick of
    /// that input.
    worst_lead: [Option<(i64, u32)>; MAX_PLAYERS],
    /// The styles this server allows its players to wear (the chalk always among them);
    /// told to every newcomer with [`ServerMessage::Styles`].
    styles: StyleSet,
}

impl Default for Room {
    /// An empty room allowing every style.
    fn default() -> Self {
        Room::with_styles(StyleSet::all())
    }
}

impl Room {
    /// An empty room, allowing every style.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty room allowing only `styles` (what the server's command line asked for).
    /// The chalk is always allowed, whatever the set says.
    pub fn with_styles(styles: StyleSet) -> Self {
        Room {
            seats: [None; MAX_PLAYERS],
            session: 0,
            fighters: PlayerSet::EMPTY,
            playing: PlayerSet::EMPTY,
            clock: 0,
            next_frame: 0,
            pending: BTreeMap::new(),
            last: [Input::NONE; MAX_PLAYERS],
            worst_lead: [None; MAX_PLAYERS],
            styles,
        }
    }

    /// The styles this room allows (see [`ServerMessage::Styles`]).
    pub fn styles(&self) -> StyleSet {
        self.styles
    }

    /// The client in the seat of `player`, if any.
    pub fn seat(&self, player: PlayerId) -> Option<ClientId> {
        self.seats
            .get(usize::from(player.0))
            .copied()
            .flatten()
            .map(|seat| seat.client)
    }

    /// The skin `player`'s fighter wears in this room (the chalk of an empty seat).
    pub fn skin(&self, player: PlayerId) -> Skin {
        self.seat_skin(player)
    }

    /// The skin of a player, whoever sits in their seat.
    fn seat_skin(&self, player: PlayerId) -> Skin {
        self.seats
            .get(usize::from(player.0))
            .copied()
            .flatten()
            .map_or(Skin::STICK, |seat| seat.skin)
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

    /// The players of the current session who are still connected (nobody once fewer than two
    /// remain: the match is over).
    pub fn playing(&self) -> PlayerSet {
        self.playing
    }

    /// Ticks of the clock since the current session started.
    pub fn clock(&self) -> u32 {
        self.clock
    }

    /// The tick of the next frame to send.
    pub fn next_frame(&self) -> u32 {
        self.next_frame
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
            character: Character::Chalk,
            skin: Skin::STICK,
        });
        let mut actions = vec![Action::Send(
            client,
            ServerMessage::Welcome {
                player: player_at(index),
            },
        )];
        // What this server allows, then the skins already worn in the room: a newcomer
        // sees the room dressed as it is.
        actions.push(Action::Send(
            client,
            ServerMessage::Styles {
                allowed: self.styles,
            },
        ));
        for (player, _) in self.clients() {
            let skin = self.seat_skin(player);
            actions.push(Action::Send(client, ServerMessage::SkinOf { player, skin }));
        }
        actions.extend(self.lobby());
        actions
    }

    /// A connection closed. Leaving a room one is not seated in changes nothing. The match
    /// goes on without the leaver (whose fighter stands still) while two players remain; the
    /// frames waiting only for the leaver go out at once.
    pub fn leave(&mut self, client: ClientId) -> Vec<Action> {
        let Some(index) = self.index_of(client) else {
            return Vec::new();
        };
        let player = player_at(index);
        self.seats[index] = None;
        self.worst_lead[index] = None;
        self.playing = self.playing.without(player);
        if self.playing.len() < 2 {
            self.playing = PlayerSet::EMPTY;
        }
        let mut actions: Vec<Action> = self
            .clients()
            .map(|(_, other)| Action::Send(other, ServerMessage::PlayerLeft { player }))
            .collect();
        actions.extend(self.flush());
        actions.extend(self.start_or_lobby());
        actions
    }

    /// A binary frame from `client`. [`ClientMessage::Ready`] marks the sender ready with its
    /// character (and may start the next session); sent again, it changes the character. An input of the current session is kept for its frame, unless
    /// that frame already went out or the tick is more than [`MAX_AHEAD`] ticks ahead of the
    /// clock; the first input for a tick wins. Inputs of an older session or of a player
    /// outside it are dropped. A malformed frame closes the sender's connection.
    pub fn receive(&mut self, client: ClientId, bytes: &[u8]) -> Vec<Action> {
        let Some(index) = self.index_of(client) else {
            return Vec::new();
        };
        match ClientMessage::decode(bytes) {
            Err(_) => vec![Action::Close(client)],
            Ok(ClientMessage::Ready { character }) => {
                // `index_of` found the seat, so it is occupied.
                self.seats[index].iter_mut().for_each(|seat| {
                    seat.ready = true;
                    seat.character = character;
                });
                self.start_or_lobby()
            }
            Ok(ClientMessage::Skin { skin }) => {
                // A style the room does not allow is ignored; an allowed one is worn at once
                // and told to everybody, so every browser redraws the fighter in the same
                // tick. The sender hears it too: their own display changes when the server
                // confirms it, like everybody else's.
                if !self.styles.contains(skin.style) {
                    return Vec::new();
                }
                let player = player_at(index);
                if let Some(seat) = self.seats[index].as_mut() {
                    seat.skin = skin;
                }
                self.clients()
                    .map(|(_, other)| Action::Send(other, ServerMessage::SkinOf { player, skin }))
                    .collect()
            }
            Ok(ClientMessage::Input {
                session,
                tick,
                input,
            }) => {
                if session != self.session || !self.playing.contains(player_at(index)) {
                    return Vec::new();
                }
                let lead = i64::from(tick) + i64::from(GRACE) - i64::from(self.clock);
                let worst = &mut self.worst_lead[index];
                if worst.is_none_or(|(worst, _)| lead < worst) {
                    *worst = Some((lead, tick));
                }
                if tick < self.next_frame || tick > self.clock.saturating_add(MAX_AHEAD) {
                    return Vec::new();
                }
                self.pending.entry(tick).or_default()[index].get_or_insert(input);
                self.flush()
            }
        }
    }

    /// One tick of the clock (the network layer calls it [`srr::game::TICKS_PER_SECOND`]
    /// times per second): sends the frames that waited long enough, then, every
    /// [`TIMING_PERIOD`] ticks, the timing reports. Nothing happens between sessions.
    pub fn tick(&mut self) -> Vec<Action> {
        if self.playing.is_empty() {
            return Vec::new();
        }
        let mut actions = Vec::new();
        while self.next_frame + GRACE <= self.clock {
            actions.extend(self.send_frame());
        }
        actions.extend(self.flush());
        self.clock += 1;
        if self.clock.is_multiple_of(TIMING_PERIOD) {
            for (player, client) in self.clients().collect::<Vec<_>>() {
                let index = usize::from(player.0);
                if let Some((lead, tick)) = self.worst_lead[index].take() {
                    let lead = lead.clamp(i8::MIN.into(), i8::MAX.into()) as i8;
                    actions.push(Action::Send(client, ServerMessage::Timing { tick, lead }));
                }
            }
        }
        actions
    }

    /// Sends every frame whose inputs all arrived, in order.
    fn flush(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        while !self.playing.is_empty()
            && self.pending.get(&self.next_frame).is_some_and(|received| {
                self.playing
                    .iter()
                    .all(|player| received[usize::from(player.0)].is_some())
            })
        {
            actions.extend(self.send_frame());
        }
        actions
    }

    /// Sends the frame of `next_frame` to every player still in the session: the inputs that
    /// arrived, the previous input of the connected players whose input is missing (they are
    /// late), and no input at all for the players who left.
    fn send_frame(&mut self) -> Vec<Action> {
        let received = self.pending.remove(&self.next_frame).unwrap_or_default();
        let mut inputs = [Input::NONE; PlayerSet::CAPACITY];
        let mut late = PlayerSet::EMPTY;
        for player in self.fighters.iter() {
            let index = usize::from(player.0);
            let input = match received[index] {
                _ if !self.playing.contains(player) => Input::NONE,
                Some(input) => input,
                None => {
                    late = late.with(player);
                    self.last[index]
                }
            };
            self.last[index] = input;
            inputs[index] = input;
        }
        let frame = ServerMessage::Frame {
            session: self.session,
            tick: self.next_frame,
            players: self.fighters,
            late,
            inputs,
        };
        self.next_frame += 1;
        self.clients()
            .filter(|&(player, _)| self.playing.contains(player))
            .map(|(_, client)| Action::Send(client, frame))
            .collect()
    }

    /// Starts the next session if everybody seated (at least two) is ready; otherwise tells
    /// everybody who is seated and ready.
    fn start_or_lobby(&mut self) -> Vec<Action> {
        let seated = self.seated();
        if seated.len() < 2 || self.ready() != seated {
            return self.lobby();
        }
        self.session = self.session.wrapping_add(1);
        self.fighters = seated;
        self.playing = seated;
        self.clock = 0;
        self.next_frame = 0;
        self.pending.clear();
        self.last = [Input::NONE; MAX_PLAYERS];
        self.worst_lead = [None; MAX_PLAYERS];
        for seat in self.seats.iter_mut().flatten() {
            seat.ready = false;
        }
        let mut characters: Characters = [Character::Chalk; MAX_PLAYERS];
        for (slot, seat) in characters.iter_mut().zip(&self.seats) {
            *slot = seat.map_or(Character::Chalk, |seat| seat.character);
        }
        let start = ServerMessage::Start {
            session: self.session,
            players: seated,
            characters,
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
