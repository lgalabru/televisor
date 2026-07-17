//! Local catalog cache and channel resolution.
//!
//! The full live-stream list is ~37k entries / 10–20 MB of JSON on real
//! panels — far too heavy to refetch on every `play`. It's cached per
//! account under `~/.cache/televisor/<account>/` with an embedded timestamp
//! (file mtimes lie across backups/sync) and a version for clean format
//! migrations. A corrupt or outdated cache is treated as a miss, never as an
//! error.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use tracing::{debug, warn};

use crate::source::MediaSource;
use crate::xtream::{Category, CategoryId, LiveStream, Series, VodStream, XtreamError};

/// Anything findable by id / name / category — live channels, movies, and
/// series all share one resolution and ranking implementation.
pub trait MediaItem: Clone {
    fn media_id(&self) -> u64;
    fn media_name(&self) -> &str;
    fn media_category(&self) -> Option<&CategoryId>;
    fn media_is_adult(&self) -> bool {
        false
    }
}

impl MediaItem for LiveStream {
    fn media_id(&self) -> u64 {
        self.stream_id.0
    }
    fn media_name(&self) -> &str {
        &self.name
    }
    fn media_category(&self) -> Option<&CategoryId> {
        self.category_id.as_ref()
    }
    fn media_is_adult(&self) -> bool {
        self.is_adult
    }
}

impl MediaItem for VodStream {
    fn media_id(&self) -> u64 {
        self.stream_id.0
    }
    fn media_name(&self) -> &str {
        &self.name
    }
    fn media_category(&self) -> Option<&CategoryId> {
        self.category_id.as_ref()
    }
    fn media_is_adult(&self) -> bool {
        self.is_adult
    }
}

impl MediaItem for Series {
    fn media_id(&self) -> u64 {
        self.series_id
    }
    fn media_name(&self) -> &str {
        &self.name
    }
    fn media_category(&self) -> Option<&CategoryId> {
        self.category_id.as_ref()
    }
}

// v2: LiveStream gained tv_archive fields; v3: VodStream/Series gained
// rating/plot/genre; v4: LiveStream.direct_source + Series.last_modified.
// Old caches would read as metadata-less — force refetch.
const CACHE_VERSION: u32 = 4;
pub const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("Could not determine the cache directory (HOME is not set)")]
    NoCacheDir,

    #[error("Cache I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Category '{0}' not found — run `televisor categories` to list them")]
    CategoryNotFound(String),

    #[error(transparent)]
    Xtream(#[from] XtreamError),
}

impl crate::payment::PaymentChallengeExt for CatalogError {
    fn payment_challenge(&self) -> Option<&crate::payment::PaymentChallenge> {
        match self {
            CatalogError::Xtream(e) => e.payment_challenge(),
            _ => None,
        }
    }
}

type Result<T> = std::result::Result<T, CatalogError>;

#[derive(Serialize, Deserialize)]
struct CacheFile<T> {
    cache_version: u32,
    fetched_at: u64,
    data: T,
}

/// Per-account catalog with transparent caching.
pub struct Catalog<'a> {
    source: &'a MediaSource,
    dir: PathBuf,
    ttl: Duration,
}

impl<'a> Catalog<'a> {
    pub fn new(source: &'a MediaSource, account_name: &str) -> Result<Self> {
        Ok(Self {
            source,
            dir: cache_dir()?.join(account_name),
            ttl: DEFAULT_TTL,
        })
    }

    pub async fn categories(&self, refresh: bool) -> Result<Vec<Category>> {
        self.cached("categories.json", refresh, || async {
            Ok(self.source.live_categories().await?)
        })
        .await
        .map(|(data, _)| data)
    }

    pub async fn vod_categories(&self, refresh: bool) -> Result<Vec<Category>> {
        self.cached("vod_categories.json", refresh, || async {
            Ok(self.source.xtream()?.vod_categories().await?)
        })
        .await
        .map(|(data, _)| data)
    }

    pub async fn series_categories(&self, refresh: bool) -> Result<Vec<Category>> {
        self.cached("series_categories.json", refresh, || async {
            Ok(self.source.xtream()?.series_categories().await?)
        })
        .await
        .map(|(data, _)| data)
    }

    pub async fn streams(&self, refresh: bool) -> Result<Vec<LiveStream>> {
        self.cached("streams.json", refresh, || async {
            Ok(self.source.live_streams().await?)
        })
        .await
        .map(|(data, _)| data)
    }

    pub async fn movies(&self, refresh: bool) -> Result<Vec<VodStream>> {
        self.cached("vod_streams.json", refresh, || async {
            Ok(self.source.xtream()?.vod_streams(None).await?)
        })
        .await
        .map(|(data, _)| data)
    }

    pub async fn series_list(&self, refresh: bool) -> Result<Vec<Series>> {
        self.cached("series.json", refresh, || async {
            Ok(self.source.xtream()?.series_list(None).await?)
        })
        .await
        .map(|(data, _)| data)
    }

    /// Resolve a user query to a single live channel. See [`Self::resolve_media`].
    pub async fn resolve(
        &self,
        query: &str,
        opts: &ResolveOptions,
    ) -> Result<Resolution<LiveStream>> {
        self.resolve_media("streams.json", query, opts, || async {
            Ok(self.source.live_streams().await?)
        })
        .await
    }

    /// Resolve a user query to a single movie.
    pub async fn resolve_movie(
        &self,
        query: &str,
        opts: &ResolveOptions,
    ) -> Result<Resolution<VodStream>> {
        self.resolve_media("vod_streams.json", query, opts, || async {
            Ok(self.source.xtream()?.vod_streams(None).await?)
        })
        .await
    }

    /// Resolve a user query to a single series (show).
    pub async fn resolve_series(
        &self,
        query: &str,
        opts: &ResolveOptions,
    ) -> Result<Resolution<Series>> {
        self.resolve_media("series.json", query, opts, || async {
            Ok(self.source.xtream()?.series_list(None).await?)
        })
        .await
    }

    /// Shared resolution flow: when a disk-cached catalog yields nothing,
    /// refresh once and retry — panels renumber and rename entries routinely.
    async fn resolve_media<T, F, Fut>(
        &self,
        file: &str,
        query: &str,
        opts: &ResolveOptions,
        fetch: F,
    ) -> Result<Resolution<T>>
    where
        T: MediaItem + Serialize + DeserializeOwned,
        F: Fn() -> Fut,
        Fut: Future<Output = Result<Vec<T>>>,
    {
        let (items, from_cache) = self.cached(file, false, &fetch).await?;
        let resolution = resolve_in(&items, query, opts);
        if matches!(resolution, Resolution::NotFound) && from_cache {
            debug!(query, file, "no match in cached catalog — refreshing once");
            let (fresh, _) = self.cached(file, true, &fetch).await?;
            return Ok(resolve_in(&fresh, query, opts));
        }
        Ok(resolution)
    }

    /// Returns `(data, came_from_cache)`.
    async fn cached<T, F, Fut>(&self, file: &str, refresh: bool, fetch: F) -> Result<(T, bool)>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let path = self.dir.join(file);
        if !refresh && let Some(data) = read_cache(&path, self.ttl) {
            return Ok((data, true));
        }
        let data = fetch().await?;
        if let Err(err) = write_cache(&path, &data) {
            // A broken cache write must not fail the command.
            warn!(path = %path.display(), error = %err, "failed to write catalog cache");
        }
        Ok((data, false))
    }
}

#[derive(Debug, Default, Clone)]
pub struct ResolveOptions {
    /// Treat the query strictly as a stream id (skip name matching).
    pub id_only: bool,
    /// Include adult-flagged channels in name matching. An explicit id match
    /// always works — passing an exact id is explicit intent.
    pub include_adult: bool,
    /// Restrict name matching to one category.
    pub category: Option<CategoryId>,
    /// Cap for the candidate list on ambiguous matches.
    pub max_candidates: usize,
}

impl ResolveOptions {
    fn cap(&self) -> usize {
        if self.max_candidates == 0 {
            15
        } else {
            self.max_candidates
        }
    }
}

#[derive(Debug)]
pub enum Resolution<T> {
    One(T),
    /// Ranked best-first, capped at `max_candidates`.
    Ambiguous(Vec<T>),
    NotFound,
}

/// Pure resolution over an in-memory item list:
/// exact id → exact normalized name → ranked substring match.
pub fn resolve_in<T: MediaItem>(items: &[T], query: &str, opts: &ResolveOptions) -> Resolution<T> {
    // Numeric queries try the id first but fall through to name search — a
    // channel literally named "24" must stay findable by name.
    if let Ok(id) = query.trim().parse::<u64>()
        && let Some(hit) = items.iter().find(|s| s.media_id() == id)
    {
        return Resolution::One(hit.clone());
    }
    if opts.id_only {
        return Resolution::NotFound;
    }

    let in_scope = |item: &&T| {
        (opts.include_adult || !item.media_is_adult())
            && opts
                .category
                .as_ref()
                .is_none_or(|wanted| item.media_category() == Some(wanted))
    };
    let needle = normalize(query);
    if needle.is_empty() {
        return Resolution::NotFound;
    }

    let exact: Vec<&T> = items
        .iter()
        .filter(in_scope)
        .filter(|s| normalize(s.media_name()) == needle)
        .collect();
    if let [only] = exact.as_slice() {
        return Resolution::One((*only).clone());
    }

    let mut matches: Vec<&T> = items
        .iter()
        .filter(in_scope)
        .filter(|s| normalize(s.media_name()).contains(&needle))
        .collect();
    match matches.len() {
        0 => Resolution::NotFound,
        1 => Resolution::One(matches[0].clone()),
        _ => {
            matches.sort_by_key(|s| {
                let name = normalize(s.media_name());
                (!name.starts_with(&needle), name.len(), s.media_id())
            });
            matches.truncate(opts.cap());
            Resolution::Ambiguous(matches.into_iter().cloned().collect())
        }
    }
}

/// Substring search used by the CLI listing commands and the MCP search
/// tools: same normalization and ranking as [`resolve_in`], but returns all
/// matches (up to `limit`).
pub fn search<T: MediaItem>(
    items: &[T],
    query: &str,
    opts: &ResolveOptions,
    limit: usize,
) -> Vec<T> {
    let needle = normalize(query);
    let mut matches: Vec<&T> = items
        .iter()
        .filter(|s| opts.include_adult || !s.media_is_adult())
        .filter(|s| {
            opts.category
                .as_ref()
                .is_none_or(|wanted| s.media_category() == Some(wanted))
        })
        .filter(|s| needle.is_empty() || normalize(s.media_name()).contains(&needle))
        .collect();
    matches.sort_by_key(|s| {
        let name = normalize(s.media_name());
        (!name.starts_with(&needle), name.len(), s.media_id())
    });
    matches.truncate(limit);
    matches.into_iter().cloned().collect()
}

/// Resolve a category by exact id, then case-insensitive name substring.
pub fn resolve_category(categories: &[Category], input: &str) -> Option<CategoryId> {
    if let Some(hit) = categories.iter().find(|c| c.category_id.0 == input.trim()) {
        return Some(hit.category_id.clone());
    }
    let needle = normalize(input);
    let matches: Vec<&Category> = categories
        .iter()
        .filter(|c| normalize(&c.category_name).contains(&needle))
        .collect();
    match matches.as_slice() {
        [only] => Some(only.category_id.clone()),
        _ => matches
            .iter()
            .find(|c| normalize(&c.category_name) == needle)
            .map(|c| c.category_id.clone()),
    }
}

fn normalize(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_cache<T: DeserializeOwned>(path: &Path, ttl: Duration) -> Option<T> {
    let raw = std::fs::read_to_string(path).ok()?;
    let cached: CacheFile<T> = serde_json::from_str(&raw).ok()?;
    if cached.cache_version != CACHE_VERSION {
        return None;
    }
    if now_epoch().saturating_sub(cached.fetched_at) >= ttl.as_secs() {
        return None;
    }
    Some(cached.data)
}

fn write_cache<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let dir = path.parent().ok_or(CatalogError::NoCacheDir)?;
    std::fs::create_dir_all(dir).map_err(|source| CatalogError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let file = CacheFile {
        cache_version: CACHE_VERSION,
        fetched_at: now_epoch(),
        data,
    };
    let raw = serde_json::to_string(&file).expect("cache types always serialize");
    let tmp = path.with_extension("json.tmp");
    let io_err = |source| CatalogError::Io {
        path: tmp.clone(),
        source,
    };
    std::fs::write(&tmp, raw).map_err(io_err)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The channel lineup reveals the subscription — cheap to protect.
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).map_err(io_err)?;
    }
    std::fs::rename(&tmp, path).map_err(|source| CatalogError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// `$XDG_CACHE_HOME/televisor`, else `~/.cache/televisor` (platform dir on Windows).
pub fn cache_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        return dirs::cache_dir()
            .map(|dir| dir.join("televisor"))
            .ok_or(CatalogError::NoCacheDir);
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .ok_or(CatalogError::NoCacheDir)?;
        Ok(base.join("televisor"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xtream::StreamId;

    fn stream(id: u64, name: &str) -> LiveStream {
        LiveStream {
            stream_id: StreamId(id),
            name: name.to_string(),
            category_id: Some(CategoryId("1".into())),
            epg_channel_id: None,
            is_adult: false,
            added: None,
            tv_archive: false,
            tv_archive_duration: None,
            direct_source: None,
        }
    }

    fn adult(id: u64, name: &str) -> LiveStream {
        LiveStream {
            is_adult: true,
            ..stream(id, name)
        }
    }

    fn catalog() -> Vec<LiveStream> {
        vec![
            stream(1, "US: FOX SPORTS 1"),
            stream(2, "US: FOX SPORTS 2"),
            stream(3, "FR| TF1 HD"),
            stream(24, "24 NEWS"),
            adult(9, "XX: LATE FOX"),
        ]
    }

    #[test]
    fn numeric_query_matches_id_first() {
        let streams = catalog();
        match resolve_in(&streams, "24", &ResolveOptions::default()) {
            Resolution::One(s) => assert_eq!(s.stream_id, StreamId(24)),
            other => panic!("expected One, got {other:?}"),
        }
    }

    #[test]
    fn numeric_query_falls_through_to_name_search() {
        let streams = vec![stream(1, "CHANNEL 99 HD")];
        match resolve_in(&streams, "99", &ResolveOptions::default()) {
            Resolution::One(s) => assert_eq!(s.stream_id, StreamId(1)),
            other => panic!("expected One, got {other:?}"),
        }
    }

    #[test]
    fn id_only_never_name_matches() {
        let streams = vec![stream(1, "CHANNEL 99 HD")];
        let opts = ResolveOptions {
            id_only: true,
            ..Default::default()
        };
        assert!(matches!(
            resolve_in(&streams, "99", &opts),
            Resolution::NotFound
        ));
    }

    #[test]
    fn exact_name_beats_substring_ambiguity() {
        let streams = vec![stream(1, "TF1"), stream(2, "TF1 HD"), stream(3, "TF1 4K")];
        match resolve_in(&streams, "tf1", &ResolveOptions::default()) {
            Resolution::One(s) => assert_eq!(s.stream_id, StreamId(1)),
            other => panic!("expected One, got {other:?}"),
        }
    }

    #[test]
    fn ambiguous_is_ranked_starts_with_first_then_shortest() {
        let streams = catalog();
        match resolve_in(&streams, "fox", &ResolveOptions::default()) {
            Resolution::Ambiguous(candidates) => {
                // Adult entry filtered out by default; none start with "fox",
                // so shortest names win: both FOX SPORTS before nothing else.
                assert_eq!(candidates.len(), 2);
                assert_eq!(candidates[0].stream_id, StreamId(1));
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn adult_streams_hidden_unless_opted_in() {
        let streams = catalog();
        let opts = ResolveOptions {
            include_adult: true,
            ..Default::default()
        };
        match resolve_in(&streams, "late fox", &opts) {
            Resolution::One(s) => assert_eq!(s.stream_id, StreamId(9)),
            other => panic!("expected One, got {other:?}"),
        }
        assert!(matches!(
            resolve_in(&streams, "late fox", &ResolveOptions::default()),
            Resolution::NotFound
        ));
        // …but an explicit id is explicit intent.
        assert!(matches!(
            resolve_in(&streams, "9", &ResolveOptions::default()),
            Resolution::One(_)
        ));
    }

    #[test]
    fn category_scoping_filters_matches() {
        let mut streams = catalog();
        streams.push(LiveStream {
            category_id: Some(CategoryId("2".into())),
            ..stream(50, "UK: FOX NEWS")
        });
        let opts = ResolveOptions {
            category: Some(CategoryId("2".into())),
            ..Default::default()
        };
        match resolve_in(&streams, "fox", &opts) {
            Resolution::One(s) => assert_eq!(s.stream_id, StreamId(50)),
            other => panic!("expected One, got {other:?}"),
        }
    }

    #[test]
    fn search_empty_query_lists_everything_ranked() {
        let streams = catalog();
        let results = search(&streams, "", &ResolveOptions::default(), 100);
        assert_eq!(results.len(), 4); // adult hidden
        let capped = search(&streams, "", &ResolveOptions::default(), 2);
        assert_eq!(capped.len(), 2);
    }

    #[test]
    fn resolve_category_by_id_and_name() {
        let categories: Vec<Category> =
            serde_json::from_str(include_str!("xtream/fixtures/live_categories.json")).unwrap();
        assert_eq!(
            resolve_category(&categories, "2343").unwrap(),
            CategoryId("2343".into())
        );
        assert_eq!(
            resolve_category(&categories, "documentaries").unwrap(),
            CategoryId("17".into())
        );
        assert!(resolve_category(&categories, "nope").is_none());
    }

    #[test]
    fn cache_roundtrip_ttl_version_and_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("streams.json");
        let data = vec![stream(1, "A")];

        write_cache(&path, &data).unwrap();
        let hit: Vec<LiveStream> = read_cache(&path, DEFAULT_TTL).unwrap();
        assert_eq!(hit.len(), 1);

        // Expired TTL → miss.
        assert!(read_cache::<Vec<LiveStream>>(&path, Duration::ZERO).is_none());

        // Version bump → miss.
        let raw = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            raw.replace(
                &format!("\"cache_version\":{CACHE_VERSION}"),
                "\"cache_version\":0",
            ),
        )
        .unwrap();
        assert!(read_cache::<Vec<LiveStream>>(&path, DEFAULT_TTL).is_none());

        // Corrupt JSON → miss, not error.
        std::fs::write(&path, "{ not json").unwrap();
        assert!(read_cache::<Vec<LiveStream>>(&path, DEFAULT_TTL).is_none());

        // Missing file → miss.
        assert!(
            read_cache::<Vec<LiveStream>>(&dir.path().join("nope.json"), DEFAULT_TTL).is_none()
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            write_cache(&path, &data).unwrap();
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
