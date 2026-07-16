//! Local user state: favorites and play history, per account, in
//! `~/.config/televisor/state.toml`. Purely local — panels have no concept
//! of either. Writes are atomic (same pattern as the config file) and every
//! mutation is best-effort from the caller's perspective: losing a history
//! entry must never fail a playback command.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::{ConfigError, config_dir};

/// Most recent plays kept per account.
const HISTORY_CAP: usize = 50;

type Result<T> = std::result::Result<T, ConfigError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Live,
    Movie,
    /// One playable episode (`id` is the episode id).
    Episode,
}

impl fmt::Display for MediaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MediaKind::Live => "live",
            MediaKind::Movie => "movie",
            MediaKind::Episode => "episode",
        })
    }
}

impl FromStr for MediaKind {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "live" | "channel" => Ok(MediaKind::Live),
            "movie" | "vod" => Ok(MediaKind::Movie),
            "episode" | "series" => Ok(MediaKind::Episode),
            other => Err(format!(
                "unknown kind '{other}' — use live, movie, or episode"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Favorite {
    pub kind: MediaKind,
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayRecord {
    pub kind: MediaKind,
    pub id: u64,
    pub name: String,
    /// Container extension needed to rebuild episode/movie URLs on replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext: Option<String>,
    pub played_at: u64,
}

impl PlayRecord {
    pub fn new(kind: MediaKind, id: u64, name: impl Into<String>, ext: Option<String>) -> Self {
        Self {
            kind,
            id,
            name: name.into(),
            ext,
            played_at: now_epoch(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub favorites: BTreeMap<String, Vec<Favorite>>,
    /// Most-recent-first per account, capped at [`HISTORY_CAP`].
    #[serde(default)]
    pub history: BTreeMap<String, Vec<PlayRecord>>,
}

/// Read-modify-write handle on the state file.
pub struct StateFile {
    path: PathBuf,
    pub state: State,
}

impl StateFile {
    pub fn open() -> Result<Self> {
        Self::open_at(config_dir()?.join("state.toml"))
    }

    pub fn open_at(path: PathBuf) -> Result<Self> {
        let state = match std::fs::read_to_string(&path) {
            Ok(raw) => toml::from_str(&raw).unwrap_or_default(), // corrupt state = fresh state
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(source) => return Err(ConfigError::Io { path, source }),
        };
        Ok(Self { path, state })
    }

    /// Idempotent: re-adding an existing favorite is a no-op. Returns
    /// whether it was newly added.
    pub fn add_favorite(&mut self, account: &str, favorite: Favorite) -> bool {
        let list = self.state.favorites.entry(account.to_string()).or_default();
        if list.contains(&favorite) {
            return false;
        }
        list.push(favorite);
        true
    }

    /// Remove by kind+id. Returns whether something was removed.
    pub fn remove_favorite(&mut self, account: &str, kind: MediaKind, id: u64) -> bool {
        let Some(list) = self.state.favorites.get_mut(account) else {
            return false;
        };
        let before = list.len();
        list.retain(|f| !(f.kind == kind && f.id == id));
        before != list.len()
    }

    pub fn favorites(&self, account: &str) -> &[Favorite] {
        self.state
            .favorites
            .get(account)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Prepend a play record, dropping any older entry for the same item.
    pub fn record_play(&mut self, account: &str, record: PlayRecord) {
        let list = self.state.history.entry(account.to_string()).or_default();
        list.retain(|r| !(r.kind == record.kind && r.id == record.id));
        list.insert(0, record);
        list.truncate(HISTORY_CAP);
    }

    pub fn history(&self, account: &str) -> &[PlayRecord] {
        self.state
            .history
            .get(account)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Atomic save, 0600 — names of what someone watches are private.
    pub fn save(&self) -> Result<()> {
        let dir = self.path.parent().ok_or(ConfigError::NoConfigDir)?;
        std::fs::create_dir_all(dir).map_err(|source| ConfigError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let raw = toml::to_string_pretty(&self.state)
            .map_err(|err| ConfigError::Parse(err.to_string()))?;
        let tmp = self.path.with_extension("toml.tmp");
        let io_err = |source| ConfigError::Io {
            path: tmp.clone(),
            source,
        };
        let mut file = std::fs::File::create(&tmp).map_err(io_err)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
                .map_err(io_err)?;
        }
        file.write_all(raw.as_bytes()).map_err(io_err)?;
        file.sync_all().map_err(io_err)?;
        drop(file);
        std::fs::rename(&tmp, &self.path).map_err(|source| ConfigError::Io {
            path: self.path.clone(),
            source,
        })
    }
}

/// Record a play without ever failing the playback path.
pub fn record_play_best_effort(account: &str, record: PlayRecord) {
    let result = StateFile::open().and_then(|mut file| {
        file.record_play(account, record);
        file.save()
    });
    if let Err(err) = result {
        tracing::warn!(error = %err, "failed to record play history");
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_file() -> (tempfile::TempDir, StateFile) {
        let dir = tempfile::tempdir().unwrap();
        let file = StateFile::open_at(dir.path().join("state.toml")).unwrap();
        (dir, file)
    }

    #[test]
    fn favorites_roundtrip_dedupe_and_remove() {
        let (dir, mut file) = state_file();
        let fav = Favorite {
            kind: MediaKind::Live,
            id: 429586,
            name: "US: FOX SPORTS 1".to_string(),
        };
        assert!(file.add_favorite("trial", fav.clone()));
        assert!(!file.add_favorite("trial", fav.clone())); // idempotent
        file.save().unwrap();

        let reloaded = StateFile::open_at(dir.path().join("state.toml")).unwrap();
        assert_eq!(reloaded.favorites("trial").len(), 1);
        assert!(reloaded.favorites("other").is_empty());

        let mut file = reloaded;
        assert!(file.remove_favorite("trial", MediaKind::Live, 429586));
        assert!(!file.remove_favorite("trial", MediaKind::Live, 429586));
    }

    #[test]
    fn history_is_most_recent_first_deduped_and_capped() {
        let (_dir, mut file) = state_file();
        for id in 0..60u64 {
            file.record_play(
                "trial",
                PlayRecord::new(MediaKind::Live, id, format!("CH {id}"), None),
            );
        }
        // Replaying an old item moves it to the front instead of duplicating.
        file.record_play("trial", PlayRecord::new(MediaKind::Live, 55, "CH 55", None));

        let history = file.history("trial");
        assert_eq!(history.len(), HISTORY_CAP);
        assert_eq!(history[0].id, 55);
        assert_eq!(history.iter().filter(|r| r.id == 55).count(), 1);
    }

    #[test]
    fn corrupt_state_file_resets_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");
        std::fs::write(&path, "{ not toml at all").unwrap();
        let file = StateFile::open_at(path).unwrap();
        assert!(file.state.favorites.is_empty());
    }
}
