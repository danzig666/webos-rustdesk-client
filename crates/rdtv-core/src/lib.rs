//! RemoteDesk TV session core: an outbound-only RustDesk-compatible client built on the
//! upstream `hbb_common` and `base` crates (RustDesk 1.5.0).

pub mod account;
pub mod http;
pub mod input;
pub mod profile;
pub mod reconnect;
pub mod session;
pub mod store;
pub mod transport;

pub use base::message_proto;
pub use hbb_common;
