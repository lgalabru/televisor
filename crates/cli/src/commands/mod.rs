pub mod account;
pub mod categories;
pub mod channels;
pub mod open;
pub mod play;
pub mod setup;
pub mod status;

use televisor_core::catalog::{self, CatalogError};
use televisor_core::config::TelevisorConfig;
use televisor_core::xtream::{CategoryId, XtreamClient};

/// Resolve the account and build a client — the shared entry point of every
/// panel-touching command.
pub(crate) fn client_for(account: Option<&str>) -> televisor_core::Result<(String, XtreamClient)> {
    let config = TelevisorConfig::load()?;
    let (name, account) = config.resolve_account(account)?;
    let client = XtreamClient::new(&name, account)?;
    Ok((name, client))
}

/// Resolve a `--category` flag value (id or name) against the cached
/// category list.
pub(crate) async fn category_for(
    catalog: &catalog::Catalog<'_>,
    input: Option<&str>,
    refresh: bool,
) -> televisor_core::Result<Option<CategoryId>> {
    match input {
        None => Ok(None),
        Some(input) => {
            let categories = catalog.categories(refresh).await?;
            catalog::resolve_category(&categories, input)
                .map(Some)
                .ok_or_else(|| CatalogError::CategoryNotFound(input.to_string()).into())
        }
    }
}
