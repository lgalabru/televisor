use rmcp::model::CallToolResult;
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    #[schemars(description = "Account name from list_accounts. Omit to use the default account.")]
    #[serde(default)]
    pub account: Option<String>,
}

pub async fn run(params: Params) -> Result<CallToolResult, rmcp::ErrorData> {
    let (name, source) = match super::connect(params.account.as_deref()) {
        Ok(connected) => connected,
        Err(message) => return Ok(super::tool_error(message)),
    };
    let client = match source.xtream() {
        Ok(client) => client,
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };
    match client.account_info().await {
        // UserInfo/ServerInfo intentionally never carry credentials, so the
        // whole summary is safe to serialize.
        Ok(summary) => Ok(super::tool_json(&serde_json::json!({
            "account": name,
            "user_info": summary.user_info,
            "server_info": summary.server_info,
            "note": "exp_date and created_at are unix epoch seconds; exp_date null means unlimited",
        }))),
        Err(err) => Ok(super::tool_error(err.to_string())),
    }
}
