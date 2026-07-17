use rmcp::model::CallToolResult;
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;
use televisor_core::catalog::Catalog;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    #[schemars(description = "Account name from list_accounts. Omit to use the default account.")]
    #[serde(default)]
    pub account: Option<String>,
    #[schemars(description = "Set true to refetch from the panel instead of the local cache.")]
    #[serde(default)]
    pub refresh: bool,
}

pub async fn run(params: Params) -> Result<CallToolResult, rmcp::ErrorData> {
    let (name, source) = match super::connect(params.account.as_deref()) {
        Ok(connected) => connected,
        Err(message) => return Ok(super::tool_error(message)),
    };
    let catalog = match Catalog::new(&source, &name) {
        Ok(catalog) => catalog,
        Err(err) => return Ok(super::tool_err(err)),
    };
    match catalog.categories(params.refresh).await {
        Ok(categories) => Ok(super::tool_json(&categories)),
        Err(err) => Ok(super::tool_err(err)),
    }
}
