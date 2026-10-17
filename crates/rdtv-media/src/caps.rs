//! Decide which codecs to advertise to the host.
//!
//! Hardware (NDL API 2): H.264 and H.265 on every webOS 5+ TV; VP9 and AV1 where the SoC
//! has them. NDL offers no capability query, so VP9/AV1 are advertised by webOS release
//! (AV1 decoders arrived with the 2020/2021 SoCs) and are withdrawn at runtime if a load
//! or feed fails (the pipeline reports `CodecFailed`, the session re-advertises).
//! Software: libvpx VP8/VP9 is always available as the fallback.

use crate::ndl;
use rdtv_core::session::DecodeCaps;

#[derive(Debug, Clone, Default)]
pub struct Platform {
    /// `webos_release` from /var/run/nyx/os_info.json, e.g. "10.2.2" (owner-reported C3).
    pub webos_release: Option<String>,
    pub ndl_available: bool,
    pub ndl_error: Option<String>,
}

impl Platform {
    pub fn webos_major(&self) -> Option<u32> {
        self.webos_release
            .as_deref()
            .and_then(|v| v.split('.').next())
            .and_then(|m| m.trim().parse().ok())
    }
}

fn json_str_field(text: &str, key: &str) -> Option<String> {
    let rest = text.split_once(&format!("\"{key}\""))?.1;
    let rest = rest.split_once(':')?.1.trim_start().strip_prefix('"')?;
    Some(rest.split_once('"')?.0.to_string())
}

pub fn detect() -> Platform {
    let os_info = std::fs::read_to_string("/var/run/nyx/os_info.json").unwrap_or_default();
    let webos_release = json_str_field(&os_info, "webos_release");
    let (ndl_available, ndl_error) = match ndl::available() {
        Ok(()) => (true, None),
        Err(e) => (false, Some(e.to_string())),
    };
    Platform {
        webos_release,
        ndl_available,
        ndl_error,
    }
}

/// User override for hardware decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HwPolicy {
    pub hardware: bool,
}

pub fn decode_caps(p: &Platform, policy: HwPolicy, failed: &[ndl::HwCodec]) -> DecodeCaps {
    let hw = policy.hardware && p.ndl_available;
    let ok = |c: ndl::HwCodec| hw && !failed.contains(&c);
    let major = p.webos_major().unwrap_or(0);
    DecodeCaps {
        vp8: true,
        vp9: true,
        h264: ok(ndl::HwCodec::H264),
        h265: ok(ndl::HwCodec::H265),
        // AV1 has no software fallback here, so only advertise it in hardware.
        av1: ok(ndl::HwCodec::Av1) && major >= 6,
    }
}

/// Whether VP9 should be sent to the hardware decoder (else libvpx).
pub fn vp9_in_hardware(p: &Platform, policy: HwPolicy, failed: &[ndl::HwCodec]) -> bool {
    policy.hardware && p.ndl_available && !failed.contains(&ndl::HwCodec::Vp9)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_os_info() {
        let t = r#"{ "webos_release": "10.2.2", "core_os_release": "x" }"#;
        assert_eq!(json_str_field(t, "webos_release").as_deref(), Some("10.2.2"));
    }

    #[test]
    fn caps_without_ndl_are_software_only() {
        let p = Platform::default();
        let c = decode_caps(&p, HwPolicy { hardware: true }, &[]);
        assert!(c.vp9 && c.vp8 && !c.h264 && !c.h265 && !c.av1);
    }

    #[test]
    fn caps_with_ndl() {
        let p = Platform {
            webos_release: Some("10.2.2".into()),
            ndl_available: true,
            ndl_error: None,
        };
        let c = decode_caps(&p, HwPolicy { hardware: true }, &[]);
        assert!(c.h264 && c.h265 && c.av1);
        let c = decode_caps(&p, HwPolicy { hardware: true }, &[ndl::HwCodec::H265, ndl::HwCodec::Av1]);
        assert!(c.h264 && !c.h265 && !c.av1);
        let c = decode_caps(&p, HwPolicy { hardware: false }, &[]);
        assert!(!c.h264 && c.vp9);
    }
}
