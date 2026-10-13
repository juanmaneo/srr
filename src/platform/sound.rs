//! The audio output of the generated music: the thin `cpal` glue around
//! [`Music`], natively (ALSA, CoreAudio, WASAPI...) and in the browser
//! (WebAudio).
//!
//! Excluded from the coverage gate because it needs a sound card: everything musical lives in
//! [`crate::audio`] and is tested there. This file only opens the default output device and
//! copies rendered samples into the device's buffers.

use std::fmt;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, OutputCallbackInfo, SampleFormat, SizedSample, Stream};
use cpal::{StreamConfig, default_host};

use crate::audio::{DEFAULT_SEED, Music, MusicControl};

/// Frames rendered at a time inside the audio callback (the scratch buffer is allocated once,
/// when the stream is built, so the callback never allocates).
const CHUNK_FRAMES: usize = 512;

/// The music playing on the default output device. Dropping it stops the music.
///
/// Holds the `cpal` stream, which may be neither `Send` nor `Sync` (in the browser it lives on
/// the main thread): keep it where it was created.
pub struct Sound {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    stream: Stream,
}

impl fmt::Debug for Sound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sound").finish_non_exhaustive()
    }
}

impl Sound {
    /// Opens the default output device and plays `Music::new(rate, DEFAULT_SEED)` steered by
    /// `control`, whose mood and intensity are read at the start of every buffer.
    ///
    /// Returns `None` (with a message on stderr, or in the browser console) when no device or
    /// stream is available. In the browser, call it from inside a user gesture handler (a DOM
    /// `keydown`, `pointerdown`...): the autoplay policy keeps the `AudioContext` suspended
    /// otherwise.
    pub fn start(control: MusicControl) -> Option<Sound> {
        match open(control) {
            Ok(stream) => Some(Sound { stream }),
            Err(message) => {
                report(&format!("srr: no music: {message}"));
                None
            }
        }
    }

    /// Whether the browser lets the music play (its `AudioContext` is running).
    #[cfg(target_arch = "wasm32")]
    pub fn is_running(&self) -> bool {
        self.audio_context().state() == web_sys::AudioContextState::Running
    }

    /// Asks the browser to resume a suspended or interrupted `AudioContext` (autoplay policy,
    /// audio device change, phone call...). Call it from a user gesture handler.
    #[cfg(target_arch = "wasm32")]
    pub fn resume(&self) {
        if !self.is_running() && self.audio_context().resume().is_err() {
            report("srr: the browser refused to resume the audio");
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn audio_context(&self) -> &web_sys::AudioContext {
        let cpal::platform::StreamInner::WebAudio(stream) = self.stream.as_inner();
        stream.audio_context()
    }
}

/// Prints an audio problem where the player can find it: stderr natively, the browser console
/// on the web (stderr goes nowhere there).
pub(crate) fn report(message: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::error_1(&message.into());
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{message}");
}

/// Builds and starts the output stream in the device's preferred sample format.
fn open(control: MusicControl) -> Result<Stream, String> {
    let device = default_host()
        .default_output_device()
        .ok_or_else(|| "no audio output device".to_owned())?;
    let supported = device
        .default_output_config()
        .map_err(|error| format!("no output configuration: {error}"))?;
    let config = supported.config();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, control),
        SampleFormat::F64 => build::<f64>(&device, config, control),
        SampleFormat::I16 => build::<i16>(&device, config, control),
        SampleFormat::U16 => build::<u16>(&device, config, control),
        SampleFormat::I32 => build::<i32>(&device, config, control),
        SampleFormat::U32 => build::<u32>(&device, config, control),
        SampleFormat::I8 => build::<i8>(&device, config, control),
        SampleFormat::U8 => build::<u8>(&device, config, control),
        other => Err(format!("unsupported sample format {other}")),
    }?;
    stream
        .play()
        .map_err(|error| format!("cannot start the output stream: {error}"))?;
    Ok(stream)
}

/// Builds a stream of `T` samples whose callback renders the music.
fn build<T>(device: &Device, config: StreamConfig, control: MusicControl) -> Result<Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels).max(1);
    let mut music = Music::new(config.sample_rate, DEFAULT_SEED);
    let mut scratch = vec![0.0_f32; CHUNK_FRAMES * channels];
    let render = move |data: &mut [T], _: &OutputCallbackInfo| {
        music.set_mood(control.mood());
        music.set_intensity(control.intensity());
        // Chunks hold whole frames, since the scratch length is a multiple of `channels`.
        for chunk in data.chunks_mut(scratch.len()) {
            let rendered = &mut scratch[..chunk.len()];
            music.render(rendered, channels);
            for (out, &sample) in chunk.iter_mut().zip(rendered.iter()) {
                *out = T::from_sample(sample);
            }
        }
    };
    device
        .build_output_stream(
            &config,
            render,
            |error| report(&format!("srr: audio stream error: {error}")),
            None,
        )
        .map_err(|error| format!("cannot open the output stream: {error}"))
}
