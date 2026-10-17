//! Bounded PCM ring between the network thread (Opus decode) and the audio callback.
//! Latency is capped: when the buffer exceeds `max_ms`, the oldest samples are dropped.

use crate::opus::OpusDecoder;
use std::{collections::VecDeque, sync::Mutex};

pub struct AudioRing {
    inner: Mutex<Inner>,
}

struct Inner {
    buf: VecDeque<f32>,
    decoder: Option<OpusDecoder>,
    sample_rate: u32,
    channels: u32,
    max_samples: usize,
    muted: bool,
    underruns: u64,
}

pub const OUTPUT_RATE: u32 = 48_000;
pub const OUTPUT_CHANNELS: u32 = 2;

impl AudioRing {
    pub fn new(max_ms: u32) -> AudioRing {
        AudioRing {
            inner: Mutex::new(Inner {
                buf: VecDeque::new(),
                decoder: None,
                sample_rate: OUTPUT_RATE,
                channels: OUTPUT_CHANNELS,
                max_samples: (OUTPUT_RATE * OUTPUT_CHANNELS * max_ms / 1000) as usize,
                muted: false,
                underruns: 0,
            }),
        }
    }

    pub fn set_format(&self, sample_rate: u32, channels: u32) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match OpusDecoder::new(sample_rate, channels) {
            Ok(d) => {
                g.decoder = Some(d);
                g.sample_rate = sample_rate;
                g.channels = channels;
                g.buf.clear();
            }
            Err(e) => {
                log::warn!("audio format rejected: {e}");
                g.decoder = None;
            }
        }
    }

    pub fn set_muted(&self, muted: bool) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.muted = muted;
        if muted {
            g.buf.clear();
        }
    }

    pub fn clear(&self) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.buf.clear();
        g.decoder = None;
    }

    /// Decode one Opus packet and queue it as 48 kHz stereo.
    pub fn push_packet(&self, packet: &[u8]) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.muted {
            return;
        }
        let (rate, ch) = (g.sample_rate, g.channels);
        let Some(dec) = g.decoder.as_mut() else { return };
        let pcm: Vec<f32> = match dec.decode(packet) {
            Ok(p) => p.to_vec(),
            Err(e) => {
                log::trace!("opus: {e}");
                return;
            }
        };
        let out = convert(&pcm, rate, ch);
        g.buf.extend(out);
        let max = g.max_samples;
        if g.buf.len() > max {
            let excess = g.buf.len() - max;
            g.buf.drain(..excess);
        }
    }

    /// Fill `out` (interleaved stereo f32); silence on underrun.
    pub fn fill(&self, out: &mut [f32]) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let n = out.len().min(g.buf.len());
        for (o, s) in out.iter_mut().zip(g.buf.drain(..n)) {
            *o = s;
        }
        if n < out.len() {
            if n > 0 || !g.buf.is_empty() {
                g.underruns += 1;
            }
            out[n..].fill(0.0);
        }
    }

    pub fn queued_ms(&self) -> u32 {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        (g.buf.len() as u64 * 1000 / (OUTPUT_RATE * OUTPUT_CHANNELS) as u64) as u32
    }
}

/// Mono->stereo and simple linear resampling to 48 kHz.
fn convert(pcm: &[f32], rate: u32, ch: u32) -> Vec<f32> {
    let frames = pcm.len() / ch as usize;
    let stereo: Vec<[f32; 2]> = (0..frames)
        .map(|i| {
            if ch == 1 {
                [pcm[i], pcm[i]]
            } else {
                [pcm[i * 2], pcm[i * 2 + 1]]
            }
        })
        .collect();
    if rate == OUTPUT_RATE || stereo.is_empty() {
        return stereo.into_iter().flatten().collect();
    }
    let out_frames = (frames as u64 * OUTPUT_RATE as u64 / rate as u64) as usize;
    let mut out = Vec::with_capacity(out_frames * 2);
    for i in 0..out_frames {
        let pos = i as f64 * rate as f64 / OUTPUT_RATE as f64;
        let i0 = pos.floor() as usize;
        let i1 = (i0 + 1).min(frames - 1);
        let t = (pos - i0 as f64) as f32;
        for c in 0..2 {
            out.push(stereo[i0][c] * (1.0 - t) + stereo[i1][c] * t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_mono_and_rate() {
        let out = convert(&[1.0, 0.5], 48_000, 1);
        assert_eq!(out, vec![1.0, 1.0, 0.5, 0.5]);
        let out = convert(&vec![0.0; 24_000 * 2], 24_000, 2);
        assert_eq!(out.len(), 48_000 * 2);
    }

    #[test]
    fn underrun_fills_silence_and_latency_is_capped() {
        let r = AudioRing::new(100);
        let mut out = vec![1.0f32; 64];
        r.fill(&mut out);
        assert!(out.iter().all(|s| *s == 0.0));
        {
            let mut g = r.inner.lock().unwrap();
            g.buf.extend(std::iter::repeat_n(0.1, 100_000));
            let max = g.max_samples;
            let excess = g.buf.len() - max;
            g.buf.drain(..excess);
        }
        assert!(r.queued_ms() <= 100);
    }
}
