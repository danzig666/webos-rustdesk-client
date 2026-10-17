//! libopus decoder for RustDesk audio frames (48 kHz Opus packets).

use anyhow::{bail, Result};
use std::ffi::c_int;

#[allow(non_camel_case_types)]
type OpusDecoderPtr = *mut std::ffi::c_void;

extern "C" {
    fn opus_decoder_create(fs: i32, channels: c_int, error: *mut c_int) -> OpusDecoderPtr;
    fn opus_decode_float(
        st: OpusDecoderPtr,
        data: *const u8,
        len: i32,
        pcm: *mut f32,
        frame_size: c_int,
        decode_fec: c_int,
    ) -> c_int;
    fn opus_decoder_destroy(st: OpusDecoderPtr);
}

pub struct OpusDecoder {
    st: OpusDecoderPtr,
    pub sample_rate: u32,
    pub channels: u32,
    buf: Vec<f32>,
}

unsafe impl Send for OpusDecoder {}

impl OpusDecoder {
    pub fn new(sample_rate: u32, channels: u32) -> Result<Self> {
        if ![8000, 12000, 16000, 24000, 48000].contains(&sample_rate) || !(1..=2).contains(&channels)
        {
            bail!("unsupported audio format {sample_rate} Hz x{channels}");
        }
        let mut err = 0;
        let st = unsafe { opus_decoder_create(sample_rate as i32, channels as c_int, &mut err) };
        if st.is_null() || err != 0 {
            bail!("opus_decoder_create failed: {err}");
        }
        // Max 120 ms per packet.
        let buf = vec![0f32; (sample_rate as usize / 1000) * 120 * channels as usize];
        Ok(OpusDecoder {
            st,
            sample_rate,
            channels,
            buf,
        })
    }

    /// Decode one packet into interleaved f32 samples.
    pub fn decode(&mut self, packet: &[u8]) -> Result<&[f32]> {
        let frame_size = (self.buf.len() / self.channels as usize) as c_int;
        let n = unsafe {
            opus_decode_float(
                self.st,
                packet.as_ptr(),
                packet.len() as i32,
                self.buf.as_mut_ptr(),
                frame_size,
                0,
            )
        };
        if n < 0 {
            bail!("opus decode error {n}");
        }
        Ok(&self.buf[..n as usize * self.channels as usize])
    }
}

impl Drop for OpusDecoder {
    fn drop(&mut self) {
        unsafe { opus_decoder_destroy(self.st) };
    }
}
