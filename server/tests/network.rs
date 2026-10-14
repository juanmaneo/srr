//! Ten browsers ([`Client`]) playing through the [`Room`] over simulated bad networks: latency,
//! jitter, connections freezing for a while, clocks running a little fast or slow.
//! Deterministic: the network, the clocks and the jitter are simulated.

use std::collections::BTreeMap;

use srr::game::{Input, Match, MatchConfig, TICKS_PER_SECOND};
use srr::net::{Client, INPUT_DELAY, MAX_PLAYERS, ServerMessage, Status};
use srr_server::room::{Action, ClientId, Room};

/// Microseconds of simulated time.
type Time = u64;

const MS: Time = 1_000;
const SECOND: Time = 1_000_000;
/// The period of the server's clock.
const TICK: Time = SECOND / TICKS_PER_SECOND as Time;

/// One direction of a connection. Like TCP, it delivers in order: each message `latency` plus
/// up to `jitter` after it was sent, but never before the previous one. Every `freeze_every`
/// it stops delivering for `freeze_for` (a Wi-Fi hiccup): the messages pile up and arrive
/// together.
#[derive(Debug, Clone, Copy)]
struct Link {
    latency: Time,
    jitter: Time,
    freeze_every: Time,
    freeze_for: Time,
}

impl Link {
    const fn steady(latency: Time, jitter: Time) -> Link {
        Link {
            latency,
            jitter,
            freeze_every: 0,
            freeze_for: 0,
        }
    }

    const fn freezing(self, every: Time, length: Time) -> Link {
        Link {
            freeze_every: every,
            freeze_for: length,
            ..self
        }
    }

    /// When a message sent at `sent` arrives, the previous one having arrived at `previous`.
    fn arrival(&self, sent: Time, previous: Time, rng: &mut Rng) -> Time {
        let mut at = sent + self.latency + rng.below(self.jitter + 1);
        if self.freeze_every > 0 {
            let phase = at % self.freeze_every;
            if phase < self.freeze_for {
                at += self.freeze_for - phase;
            }
        }
        at.max(previous)
    }
}

/// One browser: its connection (the same both ways) and the period of its clock.
#[derive(Debug, Clone, Copy)]
struct Browser {
    link: Link,
    period: Time,
}

/// A tiny deterministic generator (xorshift).
struct Rng(u64);

impl Rng {
    fn below(&mut self, bound: Time) -> Time {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % bound
    }
}

#[derive(Debug)]
enum Event {
    ServerTick,
    ClientTick(usize),
    ToServer(usize, Vec<u8>),
    ToClient(usize, Vec<u8>),
}

/// What happened during a run, from `measure_from` on.
#[derive(Debug, Default)]
struct Report {
    /// For each browser, the most consecutive ticks its match could not move on.
    worst_stall: Vec<u32>,
    /// For each player, the frames (as seen by browser 0) where their input came too late.
    late: Vec<u32>,
    /// The frames seen by browser 0.
    frames: u32,
    /// For each browser, its confirmed match at the checkpoint.
    checkpoints: Vec<Option<Match>>,
    /// For each browser, how far its round shown ran ahead of the confirmed one, at worst.
    worst_ahead: Vec<u32>,
    /// For each browser, the delay of its local inputs at the end.
    delay: Vec<u32>,
}

struct Simulation {
    now: Time,
    events: BTreeMap<(Time, u64), Event>,
    sequence: u64,
    rng: Rng,
    room: Room,
    browsers: Vec<Browser>,
    clients: Vec<Client>,
    /// The arrival of the last message on each link, up and down.
    last_up: Vec<Time>,
    last_down: Vec<Time>,
    ready_sent: Vec<bool>,
    ticks: Vec<u32>,
    measure_from: Time,
    checkpoint: u32,
    report: Report,
}

impl Simulation {
    fn new(browsers: &[Browser], config: &MatchConfig) -> Simulation {
        let count = browsers.len();
        let mut simulation = Simulation {
            now: 0,
            events: BTreeMap::new(),
            sequence: 0,
            rng: Rng(0x5EED_1234_ABCD_0042),
            room: Room::new(),
            browsers: browsers.to_vec(),
            clients: vec![Client::online(config.clone(), INPUT_DELAY); count],
            last_up: vec![0; count],
            last_down: vec![0; count],
            ready_sent: vec![false; count],
            ticks: vec![0; count],
            measure_from: 0,
            checkpoint: 0,
            report: Report {
                worst_stall: vec![0; count],
                late: vec![0; MAX_PLAYERS],
                frames: 0,
                checkpoints: vec![None; count],
                worst_ahead: vec![0; count],
                delay: vec![0; count],
            },
        };
        for index in 0..count {
            let actions = simulation.room.join(client_id(index));
            simulation.apply(actions);
            // The browsers' clocks start out of phase.
            simulation.schedule(index as Time * 1_700, Event::ClientTick(index));
        }
        simulation.schedule(0, Event::ServerTick);
        simulation
    }

    fn schedule(&mut self, at: Time, event: Event) {
        self.sequence += 1;
        self.events.insert((at, self.sequence), event);
    }

    fn send_up(&mut self, index: usize, bytes: Vec<u8>) {
        let link = self.browsers[index].link;
        let at = link.arrival(self.now, self.last_up[index], &mut self.rng);
        self.last_up[index] = at;
        self.schedule(at, Event::ToServer(index, bytes));
    }

    fn apply(&mut self, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Send(client, message) => {
                    let index = (client - 1) as usize;
                    let link = self.browsers[index].link;
                    let at = link.arrival(self.now, self.last_down[index], &mut self.rng);
                    self.last_down[index] = at;
                    self.schedule(at, Event::ToClient(index, message.encode()));
                }
                Action::Close(client) => panic!("client {client} was closed"),
            }
        }
    }

    /// Runs until `end`, measuring from `measure_from`, and keeps every browser's confirmed
    /// match after `checkpoint` ticks.
    fn run(mut self, end: Time, measure_from: Time, checkpoint: u32) -> Report {
        self.measure_from = measure_from;
        self.checkpoint = checkpoint;
        while let Some(((at, _), event)) = self.events.pop_first() {
            if at > end {
                break;
            }
            self.now = at;
            match event {
                Event::ServerTick => {
                    let actions = self.room.tick();
                    self.apply(actions);
                    self.schedule(at + TICK, Event::ServerTick);
                }
                Event::ClientTick(index) => self.client_tick(index),
                Event::ToServer(index, bytes) => {
                    let actions = self.room.receive(client_id(index), &bytes);
                    self.apply(actions);
                }
                Event::ToClient(index, bytes) => self.deliver(index, &bytes),
            }
        }
        self.report
    }

    fn client_tick(&mut self, index: usize) {
        let client = &mut self.clients[index];
        let mut outgoing = Vec::new();
        if client.status() == Status::Lobby && !self.ready_sent[index] {
            outgoing.extend(client.ready());
            self.ready_sent[index] = true;
        }
        let input = input_of(index, self.ticks[index]);
        self.ticks[index] += 1;
        outgoing.extend(client.tick(input));
        if self.now >= self.measure_from {
            let stall = &mut self.report.worst_stall[index];
            *stall = (*stall).max(client.stalled_ticks());
            if let Some(session) = client.session() {
                let ahead = session.tick().saturating_sub(session.confirmed_tick());
                let worst = &mut self.report.worst_ahead[index];
                *worst = (*worst).max(ahead);
                self.report.delay[index] = session.delay();
            }
        }
        for bytes in outgoing {
            self.send_up(index, bytes);
        }
        let next = self.now + self.browsers[index].period;
        self.schedule(next, Event::ClientTick(index));
    }

    fn deliver(&mut self, index: usize, bytes: &[u8]) {
        self.clients[index].receive(bytes);
        if index == 0
            && self.now >= self.measure_from
            && let Ok(ServerMessage::Frame { late, .. }) = ServerMessage::decode(bytes)
        {
            self.report.frames += 1;
            for player in late.iter() {
                self.report.late[usize::from(player.0)] += 1;
            }
        }
        if let Some(session) = self.clients[index].session()
            && session.confirmed_tick() == self.checkpoint
            && self.report.checkpoints[index].is_none()
        {
            self.report.checkpoints[index] = Some(session.confirmed().clone());
        }
    }
}

fn client_id(index: usize) -> ClientId {
    index as ClientId + 1
}

/// What player `index` holds on their `tick`-th tick: a pattern that keeps changing, so the
/// guesses of the others are often wrong.
fn input_of(index: usize, tick: u32) -> Input {
    let player = index as u32;
    match (tick / (7 + 3 * player) + player) % 6 {
        0 => Input {
            right: true,
            ..Input::NONE
        },
        1 => Input {
            left: true,
            ..Input::NONE
        },
        2 => Input {
            punch_high: true,
            ..Input::NONE
        },
        3 => Input {
            sidestep_in: true,
            ..Input::NONE
        },
        4 => Input {
            kick_low: true,
            ..Input::NONE
        },
        _ => Input::NONE,
    }
}

/// A fair connection with an accurate clock.
const fn fair(latency_ms: Time) -> Browser {
    Browser {
        link: Link::steady(latency_ms * MS, 5 * MS),
        period: TICK,
    }
}

/// Ten browsers, from the LAN to the far side of the world.
fn the_ten() -> Vec<Browser> {
    vec![
        // On the LAN.
        Browser {
            link: Link::steady(MS, MS),
            period: TICK,
        },
        fair(20),
        // Far away: the round trip alone exceeds the server's patience.
        Browser {
            link: Link::steady(110 * MS, 20 * MS),
            period: TICK,
        },
        // A clock 2 % slow, then one 2 % fast.
        Browser {
            period: TICK * 102 / 100,
            ..fair(25)
        },
        Browser {
            period: TICK * 98 / 100,
            ..fair(25)
        },
        // Wi-Fi: lots of jitter, and a short freeze every three seconds.
        Browser {
            link: Link::steady(30 * MS, 40 * MS).freezing(3 * SECOND, 250 * MS),
            period: TICK,
        },
        fair(10),
        fair(40),
        fair(60),
        // Hopeless: half a second each way at times, and a second-long freeze every four.
        Browser {
            link: Link::steady(200 * MS, 150 * MS).freezing(4 * SECOND, SECOND),
            period: TICK,
        },
    ]
}

const WIFI: usize = 5;
const HOPELESS: usize = 9;

#[test]
fn ten_browsers_over_bad_networks_play_the_same_match() {
    // Thirty seconds of play, measured once settled (after ten).
    let report = Simulation::new(&the_ten(), &MatchConfig::default()).run(
        30 * SECOND,
        10 * SECOND,
        20 * TICKS_PER_SECOND,
    );

    // Everybody has the same match: the server's.
    let reference = report.checkpoints[0].as_ref().expect("checkpoint reached");
    assert_eq!(reference.fighters().len(), MAX_PLAYERS);
    for (index, checkpoint) in report.checkpoints.iter().enumerate() {
        assert_eq!(checkpoint.as_ref(), Some(reference), "browser {index}");
    }
    // One frame per tick, whatever the connections.
    assert!(
        (20 * TICKS_PER_SECOND - 2..=20 * TICKS_PER_SECOND + 2).contains(&report.frames),
        "{} frames",
        report.frames
    );

    // A bad connection only hurts its own player: nobody else ever waits...
    for (index, &stall) in report.worst_stall.iter().enumerate() {
        if index != HOPELESS {
            assert_eq!(stall, 0, "browser {index} waited");
        }
    }
    // ...and the hopeless one only through its freezes.
    assert!(report.worst_stall[HOPELESS] < 2 * TICKS_PER_SECOND);

    // The others keep in time with the server, the far one and the drifting clocks included:
    // their inputs are never late. The Wi-Fi ones are, rarely (when it freezes).
    for (player, &late) in report.late.iter().enumerate() {
        if player != WIFI && player != HOPELESS {
            assert_eq!(late, 0, "player {player} was late");
        }
    }
    assert!(report.late[WIFI] * 10 < report.frames);

    // On the LAN the round shown stays a few ticks from the server's frames.
    assert!(report.worst_ahead[0] <= 2 * INPUT_DELAY + 2);
}

#[test]
fn a_far_browser_delays_its_inputs_rather_than_guessing_for_long() {
    let lan = Browser {
        link: Link::steady(MS, MS),
        period: TICK,
    };
    // A round trip of more than half a second.
    let far = Browser {
        link: Link::steady(280 * MS, 10 * MS),
        period: TICK,
    };
    let report = Simulation::new(&[lan, far], &MatchConfig::default()).run(
        20 * SECOND,
        10 * SECOND,
        15 * TICKS_PER_SECOND,
    );
    assert_eq!(report.checkpoints[0], report.checkpoints[1]);
    assert!(report.checkpoints[0].is_some());
    assert_eq!(report.late, vec![0; MAX_PLAYERS], "settled in time");
    assert_eq!(report.worst_stall, [0, 0]);
    // The LAN keeps the usual delay; the far one delays its inputs, which keeps its guesses
    // shorter than the round trip.
    assert_eq!(report.delay[0], INPUT_DELAY);
    assert!(report.delay[1] > INPUT_DELAY);
    assert!(report.worst_ahead[1] < 25, "{report:?}");
}
