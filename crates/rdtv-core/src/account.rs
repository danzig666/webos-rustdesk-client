//! Account sign-in and address book against a RustDesk API server (implemented in a later
//! step; see `docs/FEATURE_MATRIX.md`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AbPeer {
    pub id: String,
    pub alias: String,
    pub hostname: String,
    pub platform: String,
    pub tags: Vec<String>,
}
