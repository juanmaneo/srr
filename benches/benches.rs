//! Criterion benchmarks of the three hot paths of the game: the simulation, the music and
//! the protocol.
//!
//! ```sh
//! cargo bench                       # everything, ~a minute a bench
//! cargo bench -- simulation         # one group
//! cargo bench -- --quick            # fast, rough numbers
//! ```
//!
//! Every bench is deterministic (seeded scripts and matches, like `tests/determinism.rs`),
//! so runs can be compared with each other.

use std::collections::HashMap;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use srr::audio::{Mood, Music};
use srr::game::{Attack, Input, Match, MatchConfig, PlayerId, Roster, Strength};
use srr::net::{ClientMessage, FrameInputs, PlayerSet, ServerMessage};

/// Ticks per bench iteration of the simulation: one second of play.
const TICKS: usize = 60;

/// The chunk the sound card asks for (frames of stereo `f32`s, see `src/platform/sound.rs`).
const AUDIO_CHUNK: usize = 2048;

/// Tiny linear congruential generator, the same as the one of `tests/determinism.rs`, so the
/// scripts are reproducible and the benches need no dependencies.
struct Lcg(u64);

impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    /// Random movement buttons and at most one attack button, held a quarter of the time, so
    /// the fighters walk, jump and grab as well.
    fn input(&mut self) -> Input {
        let bits = self.next_u64();
        let movement = Input::from_bits((bits & 0b111) as u16);
        if (bits >> 3) & 0b11 == 0 {
            movement.with(Attack::ALL[((bits >> 5) % 6) as usize])
        } else {
            movement
        }
    }
}

/// `steps` frames of inputs for `players` fighters.
fn scripted_inputs(players: usize, steps: usize, seed: u64) -> Vec<HashMap<PlayerId, Input>> {
    let mut rng = Lcg(seed);
    (0..steps)
        .map(|_| {
            (0..players)
                .map(|id| (PlayerId(id as u8), rng.input()))
                .collect()
        })
        .collect()
}

/// The default balance, without the countdown (the fight starts at once) and with a round
/// long enough that a bench never reaches the time-up.
fn bench_config() -> MatchConfig {
    MatchConfig {
        countdown_ticks: 0,
        ..MatchConfig::default()
    }
}

fn simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("simulation");
    // The numbers are ticks: the per-tick cost is what the 60 Hz frame budget buys.
    group.throughput(Throughput::Elements(TICKS as u64));

    for players in [2, 5, 10] {
        let roster = Roster::free_for_all(players).expect("within MAX_PLAYERS");
        let script = scripted_inputs(players, TICKS, 42);
        group.bench_function(format!("{players} players"), |b| {
            b.iter_batched(
                || Match::new(roster.clone(), bench_config()).expect("valid setup"),
                |mut brawl| {
                    for frame in &script {
                        brawl.step(frame);
                    }
                },
                BatchSize::LargeInput,
            )
        });
    }
    group.finish();
}

fn music(c: &mut Criterion) {
    let mut group = c.benchmark_group("music");
    group.throughput(Throughput::Elements(AUDIO_CHUNK as u64));

    group.bench_function("render 48 kHz stereo", |b| {
        b.iter_batched(
            || {
                let mut music = Music::new(48_000, 0);
                music.set_mood(Mood::Fight);
                music.set_intensity(0.8);
                music
            },
            |mut music| {
                let mut out = [0.0; 2 * AUDIO_CHUNK];
                music.render(&mut out, 2);
            },
            BatchSize::LargeInput,
        )
    });
    group.finish();
}

fn protocol(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol");

    // The message every browser sends 60 times a second.
    let input = ClientMessage::Input {
        session: 3,
        tick: 987_654,
        input: Input::pressing(Attack::kick(Strength::High)),
    };
    group.bench_function("client input round-trip", |b| {
        b.iter(|| {
            let bytes = input.encode();
            ClientMessage::decode(&bytes)
        })
    });

    // The frame of a full room: the message the server sends 60 times a second to everybody.
    let players: PlayerSet = (0..10).fold(PlayerSet::EMPTY, |set, id| set.with(PlayerId(id)));
    let mut inputs: FrameInputs = [Input::NONE; PlayerSet::CAPACITY];
    for (id, input) in inputs.iter_mut().enumerate() {
        *input = Input::from_bits((id as u16 + 1) * 0b101);
    }
    let frame = ServerMessage::Frame {
        session: 1,
        tick: 123_456,
        players,
        late: PlayerSet::EMPTY.with(PlayerId(7)),
        inputs,
    };
    group.bench_function("server frame round-trip (10 players)", |b| {
        b.iter(|| {
            let bytes = frame.encode();
            ServerMessage::decode(&bytes)
        })
    });
    group.finish();
}

criterion_group!(benches, simulation, music, protocol);
criterion_main!(benches);
