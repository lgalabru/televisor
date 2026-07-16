use rmcp::model::CallToolResult;
use serde::Serialize;
use televisor_core::config::TelevisorConfig;

#[derive(Serialize)]
struct AccountEntry {
    name: String,
    host: String,
    is_default: bool,
    /// "xtream" (full API) or "m3u" (playlist — live TV only).
    kind: &'static str,
}

pub async fn run() -> Result<CallToolResult, rmcp::ErrorData> {
    let config = match TelevisorConfig::load() {
        Ok(config) => config,
        Err(err) => return Ok(super::tool_error(format!("Failed to load config: {err}"))),
    };
    if config.accounts.is_empty() {
        return Ok(super::tool_error(
            "No accounts configured. Ask the user to run `televisor account add <name> --host <url>` in a terminal — account credentials are deliberately not manageable through MCP.",
        ));
    }
    // Names and hosts only — usernames and passwords stay out of the
    // model conversation.
    let accounts: Vec<AccountEntry> = config
        .accounts
        .iter()
        .map(|(name, account)| AccountEntry {
            name: name.clone(),
            host: account.host.clone(),
            is_default: config.default_account.as_deref() == Some(name.as_str()),
            kind: if account.m3u_url.is_some() {
                "m3u"
            } else {
                "xtream"
            },
        })
        .collect();
    Ok(super::tool_json(&accounts))
}
