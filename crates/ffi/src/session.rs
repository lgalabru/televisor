//! One signed-in account and the handlers that serve requests against it.
//!
//! The full channel, movie and series lists are 10 to 20 MB of JSON each on
//! real panels. Core caches them on disk; the session also keeps each one in
//! memory after first use, so browsing a shelf is a filter over a vector
//! instead of a 100 ms parse. Each list loads at most once at a time: the
//! tvOS app fires a request per visible shelf, and they must not all fetch.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use televisor_core::catalog::{Catalog, MediaItem, ResolveOptions, search_page};
use televisor_core::config::{Account, normalize_host, validate_account_name};
use televisor_core::redact::Redacted;
use televisor_core::source::MediaSource;
use televisor_core::sports::{self, Event, EventState, League, Schedule, SportsError};
use televisor_core::state::{Favorite, MediaKind, PlayRecord, StateFile};
use televisor_core::xtream::{
    self, CategoryId, EpgListing, LiveStream, StreamFormat, StreamId, VodStream,
};
use tokio::sync::{Mutex, Semaphore};

use crate::error::{BridgeError, Result};
use crate::protocol::{
    AccountOverview, BrowseParams, Category, EPG_BATCH_MAX, EPG_LISTINGS_MAX, Entry, Episode,
    ErrorKind, GAME_CHANNELS_MAX, Game, Item, Library, MovieDetail, NowNext, OpenParams, Order,
    PAGE_ITEMS_MAX, Page, PlayParams, Playback, Programme, SearchResults, Season, SeriesDetail,
    SetFavoriteParams,
};

/// A game that started this long ago still shows: most run under 4 hours.
const GAME_PAST_SECS_MAX: i64 = 5 * 60 * 60;
/// Games starting within this window count as "today": an evening's
/// schedule, and the next morning's European kickoffs seen from the US.
const GAME_AHEAD_SECS_MAX: i64 = 20 * 60 * 60;

/// Concurrent requests to ESPN's public API: polite, and 36 small
/// requests still finish in about five round trips.
const SCHEDULE_CONCURRENCY_MAX: usize = 8;

/// Concurrent EPG requests to the panel. Panels throttle bursts per account;
/// 8 keeps a 50-channel batch near one round trip times seven.
const EPG_CONCURRENCY_MAX: usize = 8;

type Memo<T> = Mutex<Option<Arc<Vec<T>>>>;

pub struct Session {
    account_name: String,
    source: MediaSource,
    cache_dir: PathBuf,
    state_path: PathBuf,
    live: Memo<LiveStream>,
    movies: Memo<VodStream>,
    series: Memo<xtream::Series>,
    /// Serializes read-modify-write of the state file: two favorites saved
    /// from two threads must not overwrite each other.
    state_lock: std::sync::Mutex<()>,
}

impl Session {
    /// Build the account, check it against the panel, and return the session
    /// with the subscription summary. Nothing is kept on failure.
    pub async fn open(params: OpenParams) -> Result<(Self, AccountOverview)> {
        validate_account_name(&params.account_name)?;
        let account = account_from(&params)?;
        let source = MediaSource::new(&params.account_name, &account)?;
        let session = Self {
            cache_dir: PathBuf::from(&params.cache_dir).join(&params.account_name),
            state_path: PathBuf::from(&params.state_path),
            account_name: params.account_name,
            source,
            live: Mutex::new(None),
            movies: Mutex::new(None),
            series: Mutex::new(None),
            state_lock: std::sync::Mutex::new(()),
        };
        let overview = session.status().await?;
        Ok((session, overview))
    }

    pub async fn status(&self) -> Result<AccountOverview> {
        let summary = self.source.xtream()?.account_info().await?;
        let user = summary.user_info;
        Ok(AccountOverview {
            status: user.status,
            expires_at: user.exp_date,
            is_trial: user.is_trial,
            connections_active: user.active_cons,
            connections_max: user.max_connections,
            timezone: summary.server_info.and_then(|server| server.timezone),
        })
    }

    pub async fn refresh(&self) -> Result<()> {
        // Hold every memo while the files go, so no load re-reads a file
        // that is half deleted.
        let mut live = self.live.lock().await;
        let mut movies = self.movies.lock().await;
        let mut series = self.series.lock().await;
        match std::fs::remove_dir_all(&self.cache_dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(BridgeError::new(
                    ErrorKind::Storage,
                    format!("could not clear the catalog cache: {err}"),
                ));
            }
        }
        *live = None;
        *movies = None;
        *series = None;
        Ok(())
    }

    pub async fn categories(&self, library: Library) -> Result<Vec<Category>> {
        let catalog = self.catalog();
        let categories = match library {
            Library::Live => catalog.categories(false).await?,
            Library::Movies => catalog.vod_categories(false).await?,
            Library::Series => catalog.series_categories(false).await?,
        };
        Ok(categories
            .into_iter()
            .map(|category| Category {
                id: category.category_id.0,
                name: category.category_name,
            })
            .collect())
    }

    pub async fn browse(&self, params: &BrowseParams) -> Result<Page> {
        match params.library {
            Library::Live => Ok(page_of(&self.live().await?, params)),
            Library::Series => Ok(page_of(&self.series().await?, params)),
            Library::Movies => match &params.category_id {
                // One category is a small request of its own, which beats
                // downloading the whole movie list to show a single shelf.
                Some(category) => {
                    let category = CategoryId(category.clone());
                    let movies = self.catalog().movies_in_category(&category, false).await?;
                    Ok(page_of(&movies, params))
                }
                None => Ok(page_of(&self.movies().await?, params)),
            },
        }
    }

    pub async fn search(&self, query: &str, limit: u32) -> Result<SearchResults> {
        let limit = clamp_limit(limit);
        let options = ResolveOptions::default();
        let live = self.live().await?;
        let found_live = search_page(&live, query, &options, 0, limit).matches;
        // M3U accounts have live TV only; search what exists.
        let (found_movies, found_series) = match self.source.xtream() {
            Ok(_) => {
                let movies = self.movies().await?;
                let series = self.series().await?;
                (
                    search_page(&movies, query, &options, 0, limit).matches,
                    search_page(&series, query, &options, 0, limit).matches,
                )
            }
            Err(_) => (Vec::new(), Vec::new()),
        };
        Ok(SearchResults {
            live: found_live
                .iter()
                .filter(|stream| !is_separator(&stream.name))
                .map(Card::card)
                .collect(),
            movies: found_movies.iter().map(Card::card).collect(),
            series: found_series.iter().map(Card::card).collect(),
        })
    }

    pub async fn movie(&self, id: u64) -> Result<MovieDetail> {
        let info = self.source.xtream()?.vod_info(StreamId(id)).await?;
        Ok(MovieDetail {
            duration_secs: info.duration.as_deref().and_then(parse_clock),
            released: info.released().map(str::to_string),
            plot: info.plot,
            genre: info.genre,
            cast: info.cast,
            director: info.director,
            rating: info.rating,
            poster: info.movie_image,
            backdrop: info.backdrop_path,
            trailer: info.youtube_trailer,
        })
    }

    pub async fn series_detail(&self, id: u64) -> Result<SeriesDetail> {
        let info = self.source.xtream()?.series_info(id).await?;
        let seasons = group_seasons(info.episodes());
        let detail = info.info.unwrap_or_default();
        Ok(SeriesDetail {
            plot: detail.plot,
            genre: detail.genre,
            cast: detail.cast,
            rating: detail.rating,
            poster: detail.cover,
            backdrop: detail.backdrop_path,
            seasons,
        })
    }

    pub async fn epg(&self, stream_id: u64, limit: u32) -> Result<Vec<Programme>> {
        let limit = limit.clamp(1, EPG_LISTINGS_MAX);
        let listings = self
            .source
            .xtream()?
            .short_epg(StreamId(stream_id), limit)
            .await?;
        Ok(listings.iter().filter_map(programme).collect())
    }

    /// Now and next for each channel. A channel whose guide fails comes back
    /// empty rather than failing the batch: one dead EPG feed must not blank
    /// the whole guide.
    pub async fn epg_now(self: &Arc<Self>, stream_ids: &[u64]) -> Result<Vec<NowNext>> {
        if stream_ids.len() > EPG_BATCH_MAX {
            return Err(BridgeError::new(
                ErrorKind::InvalidRequest,
                format!("at most {EPG_BATCH_MAX} channels per EPG batch"),
            ));
        }
        self.source.xtream()?;
        let permits = Arc::new(Semaphore::new(EPG_CONCURRENCY_MAX));
        let mut tasks = tokio::task::JoinSet::new();
        for (index, &stream_id) in stream_ids.iter().enumerate() {
            let session = Arc::clone(self);
            let permits = Arc::clone(&permits);
            tasks.spawn(async move {
                let _permit = permits.acquire().await.expect("semaphore is never closed");
                (index, session.epg(stream_id, 2).await)
            });
        }
        let mut slots: Vec<Option<NowNext>> = stream_ids.iter().map(|_| None).collect();
        while let Some(joined) = tasks.join_next().await {
            let (index, result) = joined.expect("EPG task never panics or is cancelled");
            // Swallowed on purpose: see the doc comment.
            let programmes = result.unwrap_or_default();
            slots[index] = Some(now_next(stream_ids[index], &programmes, now_epoch()));
        }
        let batch: Vec<NowNext> = slots.into_iter().flatten().collect();
        assert_eq!(batch.len(), stream_ids.len(), "every channel gets one slot");
        Ok(batch)
    }

    pub async fn play(&self, mut params: PlayParams) -> Result<Playback> {
        // Favorites created before they carried container metadata (and old
        // history entries) ask to play a movie without an extension. Recover
        // it from the catalog before constructing the Xtream URL; defaulting
        // to .mp4 here can point at the wrong resource and also prevents the
        // Apple apps from selecting the Matroska remuxer.
        if params.kind == MediaKind::Movie
            && params
                .ext
                .as_deref()
                .is_none_or(|ext| ext.trim().trim_start_matches('.').is_empty())
        {
            params.ext = self
                .movies()
                .await?
                .iter()
                .find(|movie| movie.stream_id.0 == params.id)
                .and_then(|movie| movie.container_extension.clone());
        }
        let (url, is_live) = match params.kind {
            MediaKind::Live => {
                let live = self.live().await?;
                let stream = live
                    .iter()
                    .find(|stream| stream.stream_id.0 == params.id)
                    .ok_or_else(|| {
                        BridgeError::new(ErrorKind::NotFound, "this channel is gone from the panel")
                    })?;
                // AVPlayer streams HLS natively; MPEG-TS needs a demuxer.
                (
                    self.source.live_stream_url(stream, StreamFormat::M3u8)?,
                    true,
                )
            }
            MediaKind::Movie => (
                self.source
                    .xtream()?
                    .movie_url(StreamId(params.id), params.ext.as_deref())?,
                false,
            ),
            MediaKind::Episode => (
                self.source
                    .xtream()?
                    .episode_url(params.id, params.ext.as_deref())?,
                false,
            ),
            MediaKind::Series => {
                return Err(BridgeError::new(
                    ErrorKind::InvalidRequest,
                    "a show is not playable; play one of its episodes",
                ));
            }
        };
        let record = PlayRecord::new(params.kind, params.id, params.name, params.ext)
            .with_image(params.image);
        // History is a convenience; failing to save it must not stop playback.
        if let Err(err) = self.update_state(|state| state.record_play(&self.account_name, record)) {
            eprintln!("televisor: could not record play history: {}", err.message);
        }
        Ok(Playback {
            url: url.to_string(),
            is_live,
        })
    }

    /// Every followed league's games around now, each with the channels
    /// that carry it. A league whose schedule fails is skipped: one outage
    /// must not hide the rest.
    pub async fn sports_today(&self, region: Option<&str>) -> Result<Vec<Game>> {
        let schedule = Arc::new(Schedule::new().map_err(sports_error)?);
        let today = jiff::Zoned::now()
            .with_time_zone(jiff::tz::TimeZone::UTC)
            .date();
        // From Europe, tonight's US games sit on yesterday's or tomorrow's
        // ESPN day; three days cover every game in the window.
        let days = [
            today.yesterday().unwrap_or(today),
            today,
            today.tomorrow().unwrap_or(today),
        ];
        let permits = Arc::new(Semaphore::new(SCHEDULE_CONCURRENCY_MAX));
        let mut tasks = tokio::task::JoinSet::new();
        for league in League::ALL {
            for day in days {
                let schedule = Arc::clone(&schedule);
                let permits = Arc::clone(&permits);
                tasks.spawn(async move {
                    let _permit = permits.acquire().await.expect("semaphore is never closed");
                    schedule.league_events(league, day).await
                });
            }
        }
        let requests = tasks.len();
        let now = now_epoch();
        let mut events: Vec<Event> = Vec::new();
        let mut failures = 0_usize;
        while let Some(joined) = tasks.join_next().await {
            match joined.expect("schedule tasks never panic or are cancelled") {
                Ok(found) => events.extend(found.into_iter().filter(|event| {
                    event.state == EventState::Live
                        || (event.start >= now - GAME_PAST_SECS_MAX
                            && event.start <= now + GAME_AHEAD_SECS_MAX)
                })),
                Err(err) => {
                    failures += 1;
                    eprintln!("televisor: sports schedule: {err}");
                }
            }
        }
        // Adjacent days overlap around midnight.
        events.sort_by(|a, b| (a.league as u8, &a.id).cmp(&(b.league as u8, &b.id)));
        events.dedup_by(|a, b| a.league == b.league && a.id == b.id);
        if failures == requests {
            return Err(BridgeError::new(
                ErrorKind::Network,
                "the sports schedule could not be reached",
            ));
        }
        events.sort_by_key(|event| {
            let order = match event.state {
                EventState::Live => 0,
                EventState::Scheduled => 1,
                EventState::Finished => 2,
            };
            (order, event.start)
        });

        let live = self.live().await?;
        let names = sports::lowercase_names(&live);
        Ok(events
            .into_iter()
            .filter_map(|event| {
                let channels: Vec<Item> =
                    sports::channels_for(&event, &live, &names, region, GAME_CHANNELS_MAX)
                        .into_iter()
                        .map(Card::card)
                        .collect();
                if channels.is_empty() {
                    return None;
                }
                Some(Game {
                    id: event.id,
                    league: event.league,
                    league_name: event.league.name(),
                    start: event.start,
                    state: event.state,
                    status: event.status,
                    home: event.home,
                    away: event.away,
                    channels,
                })
            })
            .collect())
    }

    pub fn history(&self) -> Result<Vec<Entry>> {
        let state = StateFile::open_at(self.state_path.clone())?;
        Ok(state
            .history(&self.account_name)
            .iter()
            .map(|record| Entry {
                kind: record.kind,
                id: record.id,
                name: record.name.clone(),
                image: record.image.clone(),
                ext: record.ext.clone(),
                played_at: Some(record.played_at),
            })
            .collect())
    }

    pub fn favorites(&self) -> Result<Vec<Entry>> {
        let state = StateFile::open_at(self.state_path.clone())?;
        Ok(state
            .favorites(&self.account_name)
            .iter()
            .map(|favorite| Entry {
                kind: favorite.kind,
                id: favorite.id,
                name: favorite.name.clone(),
                image: favorite.image.clone(),
                ext: None,
                played_at: None,
            })
            .collect())
    }

    pub fn set_favorite(&self, params: SetFavoriteParams) -> Result<Vec<Entry>> {
        self.update_state(|state| {
            if params.is_favorite {
                state.add_favorite(
                    &self.account_name,
                    Favorite {
                        kind: params.kind,
                        id: params.id,
                        name: params.name,
                        image: params.image,
                    },
                );
            } else {
                state.remove_favorite(&self.account_name, params.kind, params.id);
            }
        })?;
        self.favorites()
    }

    fn update_state(&self, change: impl FnOnce(&mut StateFile)) -> Result<()> {
        let _guard = self
            .state_lock
            .lock()
            .expect("no thread panics while holding the state lock");
        let mut state = StateFile::open_at(self.state_path.clone())?;
        change(&mut state);
        Ok(state.save()?)
    }

    fn catalog(&self) -> Catalog<'_> {
        Catalog::at(&self.source, self.cache_dir.clone())
    }

    async fn live(&self) -> Result<Arc<Vec<LiveStream>>> {
        let mut slot = self.live.lock().await;
        if let Some(list) = slot.as_ref() {
            return Ok(Arc::clone(list));
        }
        let list = Arc::new(self.catalog().streams(false).await?);
        *slot = Some(Arc::clone(&list));
        Ok(list)
    }

    async fn movies(&self) -> Result<Arc<Vec<VodStream>>> {
        let mut slot = self.movies.lock().await;
        if let Some(list) = slot.as_ref() {
            return Ok(Arc::clone(list));
        }
        let list = Arc::new(self.catalog().movies(false).await?);
        *slot = Some(Arc::clone(&list));
        Ok(list)
    }

    async fn series(&self) -> Result<Arc<Vec<xtream::Series>>> {
        let mut slot = self.series.lock().await;
        if let Some(list) = slot.as_ref() {
            return Ok(Arc::clone(list));
        }
        let list = Arc::new(self.catalog().series_list(false).await?);
        *slot = Some(Arc::clone(&list));
        Ok(list)
    }
}

/// Fill the account from the sign-in form. People paste the playlist URL
/// their provider emailed them; its embedded credentials count as typed.
fn account_from(params: &OpenParams) -> Result<Account> {
    let input = normalize_host(&params.host)?;
    let username = non_empty(&params.username)
        .or(input.username.as_deref())
        .ok_or_else(|| BridgeError::new(ErrorKind::AuthFailed, "a username is required"))?
        .to_string();
    let password = match non_empty(&params.password) {
        Some(typed) => Redacted::new(typed),
        None => input
            .password
            .ok_or_else(|| BridgeError::new(ErrorKind::AuthFailed, "a password is required"))?,
    };
    Ok(Account {
        host: input.host,
        username,
        password,
        user_agent: None,
        m3u_url: None,
    })
}

fn non_empty(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// What every catalog row becomes on screen.
trait Card: MediaItem {
    fn card(&self) -> Item;
    fn added_at(&self) -> Option<i64>;
}

impl Card for LiveStream {
    fn card(&self) -> Item {
        Item {
            library: Library::Live,
            id: self.stream_id.0,
            name: self.name.clone(),
            image: self.stream_icon.clone(),
            backdrop: None,
            rating: None,
            genre: None,
            added_at: self.added,
            ext: None,
            has_archive: self.tv_archive,
            category_id: self.category_id.as_ref().map(|id| id.0.clone()),
        }
    }

    fn added_at(&self) -> Option<i64> {
        self.added
    }
}

impl Card for VodStream {
    fn card(&self) -> Item {
        Item {
            library: Library::Movies,
            id: self.stream_id.0,
            name: self.name.clone(),
            image: self.stream_icon.clone(),
            backdrop: None,
            rating: self.rating,
            genre: None,
            added_at: self.added,
            ext: self.container_extension.clone(),
            has_archive: false,
            category_id: self.category_id.as_ref().map(|id| id.0.clone()),
        }
    }

    fn added_at(&self) -> Option<i64> {
        self.added
    }
}

impl Card for xtream::Series {
    fn card(&self) -> Item {
        Item {
            library: Library::Series,
            id: self.series_id,
            name: self.name.clone(),
            image: self.cover.clone(),
            backdrop: self.backdrop_path.clone(),
            rating: self.rating,
            genre: self.genre.clone(),
            added_at: self.last_modified,
            ext: None,
            has_archive: false,
            category_id: self.category_id.as_ref().map(|id| id.0.clone()),
        }
    }

    fn added_at(&self) -> Option<i64> {
        self.last_modified
    }
}

/// One page of a catalog: ranked by relevance when there is a query,
/// otherwise in the requested order. Adult items are never listed.
fn page_of<T: Card>(items: &[T], params: &BrowseParams) -> Page {
    let limit = clamp_limit(params.limit);
    let offset = usize::try_from(params.offset).expect("u32 fits in usize on 64-bit targets");
    let category = params.category_id.clone().map(CategoryId);
    let query = params.query.trim();
    if !query.is_empty() {
        let options = ResolveOptions {
            category,
            ..ResolveOptions::default()
        };
        let page = search_page(items, query, &options, offset, limit);
        return Page {
            total: count(page.total),
            items: page
                .matches
                .iter()
                .filter(|item| !is_separator(item.media_name()))
                .map(Card::card)
                .collect(),
        };
    }
    let mut matching: Vec<&T> = items
        .iter()
        .filter(|item| !item.media_is_adult())
        .filter(|item| !is_separator(item.media_name()))
        .filter(|item| {
            category
                .as_ref()
                .is_none_or(|wanted| item.media_category() == Some(wanted))
        })
        .collect();
    match params.order {
        Order::Panel => {}
        // Stable, so items added at the same second keep panel order.
        Order::Recent => matching.sort_by_key(|item| Reverse(item.added_at())),
    }
    let page = Page {
        total: count(matching.len()),
        items: matching
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|item| item.card())
            .collect(),
    };
    assert!(page.items.len() <= limit);
    page
}

/// Panels list heading rows as channels ("##### NFL LIVE EVENTS #####").
/// They play nothing.
fn is_separator(name: &str) -> bool {
    let trimmed = name.trim_start();
    trimmed.starts_with("##") || trimmed.starts_with("==") || trimmed.starts_with("--")
}

fn sports_error(err: SportsError) -> BridgeError {
    BridgeError::new(ErrorKind::Network, err.to_string())
}

fn clamp_limit(limit: u32) -> usize {
    let limit = limit.clamp(1, PAGE_ITEMS_MAX);
    usize::try_from(limit).expect("u32 fits in usize on 64-bit targets")
}

fn count(total: usize) -> u32 {
    u32::try_from(total).unwrap_or(u32::MAX)
}

fn group_seasons(episodes: Vec<xtream::Episode>) -> Vec<Season> {
    let mut seasons: BTreeMap<u32, Vec<Episode>> = BTreeMap::new();
    for episode in episodes {
        let number = episode
            .season
            .and_then(|season| u32::try_from(season).ok())
            .unwrap_or(0);
        let info = episode.info.unwrap_or_default();
        seasons.entry(number).or_default().push(Episode {
            id: episode.id,
            number: episode
                .episode_num
                .and_then(|number| u32::try_from(number).ok()),
            title: episode.title,
            plot: info.plot,
            image: info.movie_image,
            duration_secs: info.duration_secs.and_then(|secs| u32::try_from(secs).ok()),
            ext: episode.container_extension,
        });
    }
    seasons
        .into_iter()
        .map(|(number, episodes)| Season { number, episodes })
        .collect()
}

fn programme(listing: &EpgListing) -> Option<Programme> {
    let start = listing.start_timestamp?;
    let stop = listing.stop_timestamp?;
    if stop <= start {
        return None;
    }
    Some(Programme {
        title: listing.title.clone(),
        description: listing.description.clone(),
        start,
        stop,
    })
}

fn now_next(stream_id: u64, programmes: &[Programme], now: i64) -> NowNext {
    let current = programmes
        .iter()
        .position(|programme| programme.start <= now && now < programme.stop);
    let upcoming = match current {
        Some(index) => programmes.get(index + 1),
        None => programmes.iter().find(|programme| programme.start > now),
    };
    NowNext {
        stream_id,
        now: current.map(|index| programmes[index].clone()),
        next: upcoming.cloned(),
    }
}

/// "HH:MM:SS" or "MM:SS", as panels send durations, in seconds.
fn parse_clock(text: &str) -> Option<u32> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let mut seconds: u32 = 0;
    for part in parts {
        let value: u32 = part.parse().ok()?;
        seconds = seconds.checked_mul(60)?.checked_add(value)?;
    }
    if seconds == 0 { None } else { Some(seconds) }
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn programme_at(start: i64, stop: i64) -> Programme {
        Programme {
            title: Some(format!("{start}")),
            description: None,
            start,
            stop,
        }
    }

    #[test]
    fn clock_durations_parse_and_reject_garbage() {
        assert_eq!(parse_clock("01:58:30"), Some(7110));
        assert_eq!(parse_clock("42:10"), Some(2530));
        assert_eq!(parse_clock("00:00:00"), None);
        assert_eq!(parse_clock("90"), None);
        assert_eq!(parse_clock("1:2:3:4"), None);
        assert_eq!(parse_clock("aa:bb"), None);
        assert_eq!(parse_clock(""), None);
    }

    #[test]
    fn now_next_finds_airing_and_following_programme() {
        let guide = [
            programme_at(0, 100),
            programme_at(100, 200),
            programme_at(200, 300),
        ];
        let airing = now_next(7, &guide, 150);
        assert_eq!(airing.now.map(|p| p.start), Some(100));
        assert_eq!(airing.next.map(|p| p.start), Some(200));

        let last = now_next(7, &guide, 250);
        assert_eq!(last.now.map(|p| p.start), Some(200));
        assert!(last.next.is_none());

        // A gap in the guide: nothing on now, the next one still shows.
        let gap = now_next(7, &[programme_at(500, 600)], 400);
        assert!(gap.now.is_none());
        assert_eq!(gap.next.map(|p| p.start), Some(500));

        assert!(now_next(7, &[], 400).now.is_none());
    }

    #[test]
    fn programmes_without_a_valid_span_are_dropped() {
        let listing = |start, stop| EpgListing {
            title: None,
            description: None,
            start_timestamp: start,
            stop_timestamp: stop,
        };
        assert!(programme(&listing(Some(10), Some(20))).is_some());
        assert!(programme(&listing(Some(20), Some(20))).is_none());
        assert!(programme(&listing(None, Some(20))).is_none());
        assert!(programme(&listing(Some(10), None)).is_none());
    }

    #[test]
    fn separator_rows_are_recognized() {
        assert!(is_separator("##### NFL LIVE EVENTS #####"));
        assert!(is_separator("  ==== FRANCE ===="));
        assert!(!is_separator("FR: TF1 HD"));
        assert!(!is_separator("#1 HITS"));
    }

    #[test]
    fn limits_clamp_to_the_page_bounds() {
        assert_eq!(clamp_limit(0), 1);
        assert_eq!(clamp_limit(30), 30);
        assert_eq!(clamp_limit(u32::MAX), PAGE_ITEMS_MAX as usize);
    }

    #[test]
    fn pasted_playlist_url_fills_in_credentials() {
        let params = |host: &str, username: &str, password: &str| OpenParams {
            account_name: "default".into(),
            host: host.into(),
            username: username.into(),
            password: password.into(),
            cache_dir: "/tmp".into(),
            state_path: "/tmp/state.toml".into(),
        };
        let pasted = params(
            "http://panel.example:8080/get.php?username=alice&password=hunter2&type=m3u_plus",
            "",
            "",
        );
        let account = account_from(&pasted).unwrap();
        assert_eq!(account.host, "http://panel.example:8080");
        assert_eq!(account.username, "alice");
        assert_eq!(account.password.expose(), "hunter2");

        // Typed values win over the URL's.
        let typed = params(&pasted.host, "bob", "secret");
        let account = account_from(&typed).unwrap();
        assert_eq!(account.username, "bob");
        assert_eq!(account.password.expose(), "secret");

        let missing = params("panel.example", "bob", " ");
        let err = account_from(&missing).unwrap_err();
        assert_eq!(err.kind, ErrorKind::AuthFailed);

        let bad_host = params("ftp://panel.example", "bob", "secret");
        assert_eq!(
            account_from(&bad_host).unwrap_err().kind,
            ErrorKind::InvalidHost
        );
    }
}
