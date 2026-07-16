use std::path::PathBuf;
use std::process::{Command, Stdio};

use thiserror::Error;
use tracing::{debug, info};

use crate::Result;

/// Env var that overrides VLC binary discovery.
pub const VLC_PATH_ENV: &str = "VLC_PATH";

#[derive(Debug, Error)]
pub enum VlcError {
    #[error(
        "VLC not found — install it from https://www.videolan.org or set {VLC_PATH_ENV} to the binary path"
    )]
    NotFound,

    #[error("Failed to launch VLC: {0}")]
    Launch(#[from] std::io::Error),
}

#[derive(Debug, Default, Clone)]
pub struct OpenOptions {
    pub fullscreen: bool,
    /// Network caching in milliseconds; VLC's default applies when `None`.
    pub caching_ms: Option<u32>,
    /// Quit VLC when the stream ends.
    pub exit_on_end: bool,
    /// Window/playlist title. Set this for credentialed URLs so VLC displays
    /// the channel name instead of a URL embedding username/password.
    pub title: Option<String>,
}

/// Locate the VLC binary: `VLC_PATH` override first, then `PATH`, then
/// well-known install locations per platform.
pub fn locate_vlc() -> Result<PathBuf> {
    if let Ok(path) = std::env::var(VLC_PATH_ENV) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(VlcError::NotFound.into());
    }

    if let Some(path) = find_on_path() {
        return Ok(path);
    }

    let known_locations: &[&str] = &[
        "/Applications/VLC.app/Contents/MacOS/VLC",
        "/usr/bin/vlc",
        "/usr/local/bin/vlc",
        "/snap/bin/vlc",
        "C:\\Program Files\\VideoLAN\\VLC\\vlc.exe",
    ];
    known_locations
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .ok_or_else(|| VlcError::NotFound.into())
}

fn find_on_path() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(vlc_binary_name()))
        .find(|candidate| candidate.is_file())
}

fn vlc_binary_name() -> &'static str {
    if cfg!(windows) { "vlc.exe" } else { "vlc" }
}

/// Translate open options into VLC command-line arguments.
pub fn build_args(url: &str, opts: &OpenOptions) -> Vec<String> {
    let mut args = Vec::new();
    if opts.fullscreen {
        args.push("--fullscreen".to_string());
    }
    if let Some(ms) = opts.caching_ms {
        args.push(format!("--network-caching={ms}"));
    }
    if opts.exit_on_end {
        args.push("--play-and-exit".to_string());
    }
    if let Some(title) = &opts.title {
        args.push(format!("--meta-title={title}"));
    }
    args.push(url.to_string());
    args
}

/// Launch VLC detached with the given stream URL. Returns the child PID.
pub fn open_stream(url: &str, opts: &OpenOptions) -> Result<u32> {
    let vlc = locate_vlc()?;
    let args = build_args(url, opts);
    // The URL may embed panel credentials — never log the raw args.
    debug!(vlc = %vlc.display(), url = %crate::redact::redact_url_str(url), "Launching VLC");

    let child = Command::new(&vlc)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(VlcError::Launch)?;

    let pid = child.id();
    info!(url = %crate::redact::redact_url_str(url), pid, "Stream opened in VLC");
    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_args_url_only_by_default() {
        let args = build_args("https://example.com/live.m3u8", &OpenOptions::default());
        assert_eq!(args, vec!["https://example.com/live.m3u8"]);
    }

    #[test]
    fn build_args_includes_all_options() {
        let opts = OpenOptions {
            fullscreen: true,
            caching_ms: Some(3000),
            exit_on_end: true,
            title: None,
        };
        let args = build_args("rtsp://cam.local/feed", &opts);
        assert_eq!(
            args,
            vec![
                "--fullscreen",
                "--network-caching=3000",
                "--play-and-exit",
                "rtsp://cam.local/feed",
            ]
        );
    }

    #[test]
    fn build_args_sets_meta_title() {
        let opts = OpenOptions {
            title: Some("FOX Sports 1".to_string()),
            ..Default::default()
        };
        let args = build_args("http://host/live/u/p/1.m3u8", &opts);
        assert_eq!(
            args,
            vec!["--meta-title=FOX Sports 1", "http://host/live/u/p/1.m3u8"]
        );
    }

    #[test]
    fn url_is_always_last() {
        let opts = OpenOptions {
            fullscreen: true,
            ..Default::default()
        };
        let args = build_args("udp://@239.0.0.1:1234", &opts);
        assert_eq!(
            args.last().map(String::as_str),
            Some("udp://@239.0.0.1:1234")
        );
    }
}
