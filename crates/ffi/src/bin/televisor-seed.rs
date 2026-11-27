//! Write the default CLI account as JSON, for the Apple TV build to embed.
//!
//! `televisor-seed <output.json>` resolves the account the CLI would use
//! (`~/.config/televisor/config.toml`, or the `TELEVISOR_*` env triple) and
//! writes it to the output file, mode 0600. Xcode runs this at build time and
//! copies the file into the app bundle, so the TV app signs in on first
//! launch without the credentials ever entering the repository.

use std::io::Write;
use std::process::ExitCode;

use televisor_core::config::TelevisorConfig;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let (Some(output), None) = (arguments.next(), arguments.next()) else {
        eprintln!("usage: televisor-seed <output.json>");
        return ExitCode::from(2);
    };
    match seed(&output) {
        Ok(name) => {
            println!("televisor-seed: wrote account '{name}'");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("televisor-seed: {message}");
            ExitCode::FAILURE
        }
    }
}

fn seed(output: &str) -> Result<String, String> {
    let config = TelevisorConfig::load().map_err(|err| err.to_string())?;
    let (name, account) = config
        .resolve_account(None)
        .map_err(|err| err.to_string())?;
    if account.m3u_url.is_some() {
        return Err(format!(
            "account '{name}' is a plain M3U playlist; the TV app needs an Xtream login"
        ));
    }
    let seed = serde_json::json!({
        "account_name": name,
        "host": account.host,
        "username": account.username,
        "password": account.password.expose(),
    });
    write_private(output, seed.to_string().as_bytes())
        .map_err(|err| format!("could not write {output}: {err}"))?;
    Ok(name)
}

/// Create the file 0600 from the start: it holds a password.
fn write_private(path: &str, contents: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()
}
