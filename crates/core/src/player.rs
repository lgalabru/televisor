//! Launching an external media player.
//!
//! VLC is the default, but it is not required: a [`Player`] can also be mpv,
//! IINA, ffplay, an arbitrary command template, or the OS default handler
//! (`system`). Presets know how to pass the window/playlist title so a
//! credentialed URL is never shown on screen, and buffering/fullscreen flags.
//! The `system` mode forfeits those (and the PID) — it just hands the URL to
//! `open`/`xdg-open`/`start`, which for an `http` stream usually opens a
//! browser, so it's opt-in only.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use thiserror::Error;
use tracing::{debug, info};

use crate::Result;
use crate::redact::redact_url_str;

/// Env var that overrides VLC binary discovery.
pub const VLC_PATH_ENV: &str = "VLC_PATH";

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error(
        "Player '{player}' not found. Install it (VLC: https://www.videolan.org), put it on PATH, or pick another with --player / the `player` config."
    )]
    NotFound { player: String },

    #[error("Failed to launch player: {0}")]
    Launch(#[from] std::io::Error),

    #[error("Invalid player command template: {0}")]
    BadTemplate(String),
}

/// Playback preferences shared across players. Individual players map these
/// onto their own flags; unsupported ones are silently skipped.
#[derive(Debug, Default, Clone)]
pub struct OpenOptions {
    pub fullscreen: bool,
    /// Network caching in milliseconds.
    pub caching_ms: Option<u32>,
    /// Quit the player when the stream ends.
    pub exit_on_end: bool,
    /// Window/playlist title — set this for credentialed URLs so the player
    /// shows the channel name instead of a URL embedding username/password.
    pub title: Option<String>,
}

/// Which player to launch. Parse from a user string with [`Player::parse`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Player {
    #[default]
    Vlc,
    Mpv,
    Iina,
    Ffplay,
    /// OS default handler (`open`/`xdg-open`/`start`). No title/PID.
    System,
    /// A command template; `{url}` and `{title}` are substituted per-token
    /// (so titles/URLs with spaces stay a single argument). A template
    /// without `{url}` gets the URL appended as the final argument.
    Custom(String),
}

impl Player {
    /// Parse a `--player` / config value: a preset name, `system`, or a
    /// custom command (anything else — a binary name/path or a template).
    pub fn parse(spec: &str) -> Self {
        match spec.trim().to_ascii_lowercase().as_str() {
            "vlc" => Player::Vlc,
            "mpv" => Player::Mpv,
            "iina" => Player::Iina,
            "ffplay" => Player::Ffplay,
            "system" | "open" | "default" => Player::System,
            _ => Player::Custom(spec.trim().to_string()),
        }
    }

    /// Human name for messages.
    pub fn name(&self) -> &str {
        match self {
            Player::Vlc => "vlc",
            Player::Mpv => "mpv",
            Player::Iina => "iina",
            Player::Ffplay => "ffplay",
            Player::System => "system",
            Player::Custom(_) => "custom",
        }
    }
}

/// Launch `url` in the given player, detached. Returns the child PID when a
/// player was spawned directly, or `None` for `system` mode (the OS opener
/// exits immediately and doesn't expose the real player's PID).
pub fn open_stream(url: &str, opts: &OpenOptions, player: &Player) -> Result<Option<u32>> {
    let safe_url = redact_url_str(url);

    if matches!(player, Player::System) {
        let (program, args) = system_opener(url);
        debug!(program, url = %safe_url, "Opening via system handler");
        Command::new(program)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(PlayerError::Launch)?;
        info!(url = %safe_url, "Stream handed to system opener");
        return Ok(None);
    }

    let (program, args) = resolve_command(url, opts, player)?;
    // Never log the raw args — for Custom templates they may contain the URL.
    debug!(program = %program.display(), url = %safe_url, player = player.name(), "Launching player");

    let child = Command::new(&program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(PlayerError::Launch)?;

    let pid = child.id();
    info!(url = %safe_url, pid, player = player.name(), "Stream opened");
    Ok(Some(pid))
}

/// Resolve a player to `(program, args)`, locating the binary for presets.
fn resolve_command(
    url: &str,
    opts: &OpenOptions,
    player: &Player,
) -> Result<(PathBuf, Vec<String>)> {
    let out = match player {
        Player::Vlc => (locate_vlc()?, vlc_args(url, opts)),
        Player::Mpv => (locate("mpv", &[])?, mpv_args(url, opts)),
        Player::Iina => (
            locate(
                "iina-cli",
                &["/Applications/IINA.app/Contents/MacOS/iina-cli"],
            )?,
            iina_args(url, opts),
        ),
        Player::Ffplay => (locate("ffplay", &[])?, ffplay_args(url, opts)),
        Player::Custom(template) => render_custom(template, url, opts)?,
        Player::System => unreachable!("system handled before resolve_command"),
    };
    Ok(out)
}

fn vlc_args(url: &str, opts: &OpenOptions) -> Vec<String> {
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

fn mpv_args(url: &str, opts: &OpenOptions) -> Vec<String> {
    let mut args = Vec::new();
    if opts.fullscreen {
        args.push("--fs".to_string());
    }
    if let Some(ms) = opts.caching_ms {
        args.push(format!("--cache-secs={}", ms / 1000));
    }
    if let Some(title) = &opts.title {
        args.push(format!("--force-media-title={title}"));
    }
    // mpv exits at end by default, so `exit_on_end` needs nothing.
    args.push(url.to_string());
    args
}

fn iina_args(url: &str, opts: &OpenOptions) -> Vec<String> {
    // iina-cli forwards `--mpv-<opt>` flags to its embedded mpv.
    let mut args = Vec::new();
    if opts.fullscreen {
        args.push("--mpv-fullscreen".to_string());
    }
    if let Some(title) = &opts.title {
        args.push(format!("--mpv-force-media-title={title}"));
    }
    args.push(url.to_string());
    args
}

fn ffplay_args(url: &str, opts: &OpenOptions) -> Vec<String> {
    let mut args = Vec::new();
    if opts.fullscreen {
        args.push("-fs".to_string());
    }
    if let Some(title) = &opts.title {
        args.push("-window_title".to_string());
        args.push(title.clone());
    }
    args.push(url.to_string());
    args
}

/// Render a custom template into `(program, args)`. Tokenizes on whitespace,
/// then substitutes `{url}`/`{title}` inside each token — so a title with
/// spaces stays one argument, and the URL never touches a shell.
fn render_custom(template: &str, url: &str, opts: &OpenOptions) -> Result<(PathBuf, Vec<String>)> {
    let title = opts.title.clone().unwrap_or_default();
    let mut tokens = template.split_whitespace();
    let program = tokens
        .next()
        .ok_or_else(|| PlayerError::BadTemplate("empty command".to_string()))?;

    let mut had_url = template.contains("{url}");
    let subst = |tok: &str| tok.replace("{url}", url).replace("{title}", &title);

    let mut args: Vec<String> = tokens.map(subst).collect();
    // Bare program (no {url} placeholder) → append the URL as the last arg.
    if !had_url {
        args.push(url.to_string());
        had_url = true;
    }
    let _ = had_url;

    Ok((PathBuf::from(subst(program)), args))
}

/// `open`/`xdg-open`/`start` for the current platform.
fn system_opener(url: &str) -> (&'static str, Vec<String>) {
    #[cfg(target_os = "macos")]
    {
        ("open", vec![url.to_string()])
    }
    #[cfg(target_os = "windows")]
    {
        // `start` is a cmd builtin; the empty "" is the window title arg.
        (
            "cmd",
            vec!["/C".into(), "start".into(), String::new(), url.to_string()],
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        ("xdg-open", vec![url.to_string()])
    }
}

/// Locate VLC: `VLC_PATH` override, then `PATH`, then well-known locations.
pub fn locate_vlc() -> Result<PathBuf> {
    if let Ok(path) = std::env::var(VLC_PATH_ENV) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(PlayerError::NotFound {
            player: "vlc".to_string(),
        }
        .into());
    }
    locate(
        vlc_binary_name(),
        &[
            "/Applications/VLC.app/Contents/MacOS/VLC",
            "/usr/bin/vlc",
            "/usr/local/bin/vlc",
            "/snap/bin/vlc",
            "C:\\Program Files\\VideoLAN\\VLC\\vlc.exe",
        ],
    )
}

/// Locate a binary by name on `PATH`, then in `known` absolute locations.
fn locate(name: &str, known: &[&str]) -> Result<PathBuf> {
    if let Some(path) = find_on_path(name) {
        return Ok(path);
    }
    known
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .ok_or_else(|| {
            PlayerError::NotFound {
                player: name.to_string(),
            }
            .into()
        })
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn vlc_binary_name() -> &'static str {
    if cfg!(windows) { "vlc.exe" } else { "vlc" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> OpenOptions {
        OpenOptions {
            fullscreen: true,
            caching_ms: Some(3000),
            exit_on_end: true,
            title: Some("FOX Sports 1".to_string()),
        }
    }

    #[test]
    fn parse_presets_and_custom() {
        assert_eq!(Player::parse("vlc"), Player::Vlc);
        assert_eq!(Player::parse("MPV"), Player::Mpv);
        assert_eq!(Player::parse("system"), Player::System);
        assert_eq!(Player::parse("open"), Player::System);
        assert_eq!(
            Player::parse("mpv --fs {url}"),
            Player::Custom("mpv --fs {url}".to_string())
        );
        assert_eq!(Player::default(), Player::Vlc);
    }

    #[test]
    fn vlc_args_include_all_and_url_last() {
        let args = vlc_args("http://h/live/u/p/1.m3u8", &opts());
        assert_eq!(
            args,
            vec![
                "--fullscreen",
                "--network-caching=3000",
                "--play-and-exit",
                "--meta-title=FOX Sports 1",
                "http://h/live/u/p/1.m3u8",
            ]
        );
    }

    #[test]
    fn mpv_and_ffplay_titles() {
        let m = mpv_args("URL", &opts());
        assert!(m.contains(&"--force-media-title=FOX Sports 1".to_string()));
        assert!(m.contains(&"--fs".to_string()));
        assert_eq!(m.last().unwrap(), "URL");
        let f = ffplay_args("URL", &opts());
        assert!(f.windows(2).any(|w| w == ["-window_title", "FOX Sports 1"]));
        assert_eq!(f.last().unwrap(), "URL");
    }

    #[test]
    fn custom_template_substitutes_and_keeps_spaces() {
        let o = OpenOptions {
            title: Some("My Show".to_string()),
            ..Default::default()
        };
        let (prog, args) =
            render_custom("mpv --force-media-title={title} {url}", "http://h/s.ts", &o).unwrap();
        assert_eq!(prog, PathBuf::from("mpv"));
        // Title with a space stays a single argument.
        assert_eq!(args, vec!["--force-media-title=My Show", "http://h/s.ts"]);
    }

    #[test]
    fn custom_bare_program_appends_url() {
        let (prog, args) = render_custom("mpv", "http://h/s.ts", &OpenOptions::default()).unwrap();
        assert_eq!(prog, PathBuf::from("mpv"));
        assert_eq!(args, vec!["http://h/s.ts"]);
    }
}
