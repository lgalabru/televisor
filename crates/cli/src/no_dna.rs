//! NO_DNA support — machine-readable, non-interactive behavior for agents.
//!
//! Follows the NO_DNA standard (<https://no-dna.org>): when the `NO_DNA`
//! environment variable is set and non-empty, the caller is a non-human
//! operator (AI agent, automation). In that mode televisor emits JSON on
//! stdout, JSON errors on stderr, and never blocks on an interactive prompt.
//! Matches pay's implementation so the team's CLIs behave identically for
//! agents.

use serde::Serialize;

/// True when the caller is an agent/automation (`NO_DNA` set and non-empty).
pub fn is_agent() -> bool {
    std::env::var("NO_DNA").is_ok_and(|v| !v.is_empty())
}

/// Force NO_DNA mode for this process (the `--no-dna` startup path).
///
/// # Safety
/// Call during CLI startup, before any worker threads are spawned.
pub fn enable_for_process() {
    unsafe { std::env::set_var("NO_DNA", "1") };
}

/// Whether a command should emit JSON. An explicit `--json` flag wins;
/// otherwise agent mode forces JSON. Human terminals keep the text tables,
/// and plain pipes stay grep-friendly text unless `--json`/`NO_DNA` is set.
pub fn should_json(explicit_json: bool) -> bool {
    explicit_json || is_agent()
}

/// Print a value as JSON to stdout — compact for agents, pretty for humans.
pub fn emit_json<T: Serialize>(value: &T) {
    let rendered = if is_agent() {
        serde_json::to_string(value)
    } else {
        serde_json::to_string_pretty(value)
    };
    match rendered {
        Ok(s) => println!("{s}"),
        Err(e) => error_json(&format!("failed to serialize output: {e}")),
    }
}

/// Write a structured error to stderr as `{"error":{"message":…}}`.
pub fn error_json(message: &str) {
    let json = serde_json::json!({ "error": { "message": message } });
    match serde_json::to_string(&json) {
        Ok(s) => eprintln!("{s}"),
        Err(_) => eprintln!("{{\"error\":{{\"message\":\"internal error\"}}}}"),
    }
}

/// Guard an interactive prompt: in agent mode, fail with actionable guidance
/// instead of blocking on stdin.
pub fn require_interactive(what: &str) -> Result<(), String> {
    if is_agent() {
        Err(format!(
            "{what} requires interactive input, which is unavailable in NO_DNA mode — pass it as a flag instead"
        ))
    } else {
        Ok(())
    }
}
