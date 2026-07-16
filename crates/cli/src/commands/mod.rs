pub mod account;
pub mod categories;
pub mod channels;
pub mod episodes;
pub mod guide;
pub mod info;
pub mod movies;
pub mod open;
pub mod play;
pub mod series;
pub mod setup;
pub mod status;

use televisor_core::catalog::{self, CatalogError};
use televisor_core::config::TelevisorConfig;
use televisor_core::source::MediaSource;
use televisor_core::xtream::CategoryId;

/// Resolve the account and build its media source (Xtream client or M3U
/// playlist) — the shared entry point of every panel-touching command.
pub(crate) fn client_for(account: Option<&str>) -> televisor_core::Result<(String, MediaSource)> {
    let config = TelevisorConfig::load()?;
    let (name, account) = config.resolve_account(account)?;
    let source = MediaSource::new(&name, account)?;
    Ok((name, source))
}

/// Which category list a `--category` flag resolves against.
#[derive(Clone, Copy)]
pub(crate) enum CategoryKind {
    Live,
    Vod,
    Series,
}

/// Resolve a `--category` flag value (id or name) against the cached
/// category list of the given kind.
pub(crate) async fn category_for(
    catalog: &catalog::Catalog<'_>,
    kind: CategoryKind,
    input: Option<&str>,
    refresh: bool,
) -> televisor_core::Result<Option<CategoryId>> {
    match input {
        None => Ok(None),
        Some(input) => {
            let categories = match kind {
                CategoryKind::Live => catalog.categories(refresh).await?,
                CategoryKind::Vod => catalog.vod_categories(refresh).await?,
                CategoryKind::Series => catalog.series_categories(refresh).await?,
            };
            catalog::resolve_category(&categories, input)
                .map(Some)
                .ok_or_else(|| CatalogError::CategoryNotFound(input.to_string()).into())
        }
    }
}
