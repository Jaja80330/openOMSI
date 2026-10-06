//! The tokens of the servers' own logins (`omsi_net::login`), kept per server address in
//! `logins.json` of the data folder: the launcher gets one when a server asks for its login,
//! the game sends it with its hello (`token_for`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Saved {
    pub token: String,
    /// The name the server calls the player by.
    pub name: String,
    /// Unix seconds.
    pub expires: u64,
}

fn path() -> std::path::PathBuf {
    crate::data_dir().join("logins.json")
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn load() -> HashMap<String, Saved> {
    std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn key(target: &str) -> String {
    target.trim().trim_end_matches('/').to_ascii_lowercase()
}

/// The token kept for the server at `target`, while it is good for another hour at least.
pub fn saved(target: &str) -> Option<Saved> {
    load().remove(&key(target)).filter(|s| s.expires > now() + 3600)
}

/// The token to send to the server at `target` (empty: none).
pub fn token_for(target: &str) -> String {
    saved(target).map(|s| s.token).unwrap_or_default()
}

pub fn save(target: &str, saved: Saved) {
    let mut all = load();
    let t = now();
    all.retain(|_, s| s.expires > t);
    all.insert(key(target), saved);
    let _ = std::fs::create_dir_all(crate::data_dir());
    if let Ok(text) = serde_json::to_string_pretty(&all) {
        let _ = std::fs::write(path(), text);
    }
}

/// Forget the token of `target` (the server turned it away).
pub fn forget(target: &str) {
    let mut all = load();
    if all.remove(&key(target)).is_some() {
        if let Ok(text) = serde_json::to_string_pretty(&all) {
            let _ = std::fs::write(path(), text);
        }
    }
}
