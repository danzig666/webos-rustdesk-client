//! Remote audio playback through SDL audio (PulseAudio on webOS), fed from the bounded
//! `AudioRing`. Failure to open audio never blocks video or input.

use rdtv_media::audio::{AudioRing, OUTPUT_CHANNELS, OUTPUT_RATE};
use sdl2::audio::{AudioCallback, AudioDevice, AudioSpecDesired};
use std::sync::Arc;

pub struct Feed(Arc<AudioRing>);

impl AudioCallback for Feed {
    type Channel = f32;
    fn callback(&mut self, out: &mut [f32]) {
        self.0.fill(out);
    }
}

pub struct AudioOut {
    _dev: AudioDevice<Feed>,
}

impl AudioOut {
    pub fn open(sdl: &sdl2::Sdl, ring: Arc<AudioRing>) -> Result<AudioOut, String> {
        let audio = sdl.audio()?;
        let spec = AudioSpecDesired {
            freq: Some(OUTPUT_RATE as i32),
            channels: Some(OUTPUT_CHANNELS as u8),
            samples: Some(1024),
        };
        let dev = audio.open_playback(None, &spec, |got| {
            log::info!(
                "audio device: {} Hz x{} ({} samples)",
                got.freq,
                got.channels,
                got.samples
            );
            Feed(ring)
        })?;
        dev.resume();
        Ok(AudioOut { _dev: dev })
    }
}
