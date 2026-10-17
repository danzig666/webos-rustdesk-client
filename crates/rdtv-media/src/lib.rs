//! Media for RemoteDesk TV: webOS NDL hardware video decoding, libvpx software fallback,
//! Opus audio decoding and the bounded queues between network, decoder and presentation.

pub mod audio;
pub mod caps;
pub mod frame;
pub mod ndl;
pub mod opus;
pub mod pipeline;
pub mod vpx;

/// Package/app id, used for `NDL_DirectMediaInit` when `APPID` is not set by the launcher.
pub const APP_ID: &str = "io.github.danzig666.remotedesktv";
