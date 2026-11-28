//! Wire format between a native frontend and the bridge.
//!
//! A request is one JSON object, `{"method": "<name>", "params": {...}}`
//! (`params` is omitted for methods that take none). A response is either
//! `{"ok": <value>}` or `{"error": {"kind": "<kind>", "message": "..."}}`.
//! Frontends decode `kind` into their own enum, so adding a variant here is a
//! breaking change for them: update `apps/appletv` in the same change.

use serde::{Deserialize, Serialize};
use televisor_core::sports::{EventState, League, Team};
use televisor_core::state::MediaKind;

/// Largest request accepted, in bytes. The biggest real request is an EPG
/// batch of [`EPG_BATCH_MAX`] ids, well under 4 KiB; 64 KiB leaves room for
/// long passwords and hostnames without letting a bug send megabytes.
pub const REQUEST_BYTES_MAX: usize = 64 * 1024;

/// Most items one `browse` or `search` page returns. A tvOS shelf shows
/// about 8 posters at once and a grid about 30; 200 covers a few screens of
/// scrolling per round trip.
pub const PAGE_ITEMS_MAX: u32 = 200;

/// Most programmes returned per channel. 24 covers a day at hourly slots.
pub const EPG_LISTINGS_MAX: u32 = 24;

/// Most feeds listed per game: enough choice of language and quality
/// without a wall of near-identical rows.
pub const GAME_CHANNELS_MAX: usize = 8;

/// Most channels in one `epg_now` batch: one screen of the channel guide
/// is about 10 rows, so 50 covers prefetching a few screens ahead.
pub const EPG_BATCH_MAX: usize = 50;

#[derive(Debug, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    /// Sign in and start a session, replacing any open one.
    Open(OpenParams),
    /// End the session. Cached catalogs stay on disk.
    Close,
    /// Refresh the subscription summary of the open session.
    Status,
    /// Drop every cached catalog of the open session, so the next request
    /// refetches from the panel.
    Refresh,
    Categories {
        library: Library,
    },
    Browse(BrowseParams),
    Search {
        query: String,
        limit: u32,
    },
    Movie {
        id: u64,
    },
    Series {
        id: u64,
    },
    Epg {
        stream_id: u64,
        limit: u32,
    },
    /// What is on now and next, for many channels in one round trip.
    EpgNow {
        stream_ids: Vec<u64>,
    },
    /// Resolve a playback URL and record the play in history.
    Play(PlayParams),
    /// Serve a Matroska title as HLS on loopback, so AVPlayer can play it.
    /// `url` is a `play` result. Replaces any title already served: the
    /// plan's one connection goes to the newest. Fails with `unsupported`
    /// when the title needs VLC.
    RemuxOpen {
        url: String,
    },
    /// Stop serving the remuxed title at `url` (a `remux_open` result),
    /// freeing the panel connection. Naming it keeps a late close from
    /// stopping a newer title; a stale `url` is a no-op.
    RemuxClose {
        url: String,
    },
    History,
    Favorites,
    SetFavorite(SetFavoriteParams),
    /// Big games live now or starting soon, with the channels carrying them.
    SportsToday {
        /// Channel-name prefix to rank first, like `FR`.
        #[serde(default)]
        region: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
pub struct OpenParams {
    /// Names the per-account cache directory: letters, digits, `-` and `_`.
    pub account_name: String,
    /// A host (`host:port`), an origin, or a full provider playlist URL whose
    /// embedded credentials fill in `username` and `password` when empty.
    pub host: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// Directory for catalog caches; the host app's sandboxed caches folder.
    pub cache_dir: String,
    /// File holding favorites and history.
    pub state_path: String,
}

/// The three catalogs a panel serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Library {
    Live,
    Movies,
    Series,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    /// The order the panel lists items in, usually its channel numbering.
    #[default]
    Panel,
    /// Newest additions first.
    Recent,
}

#[derive(Debug, Deserialize)]
pub struct BrowseParams {
    pub library: Library,
    #[serde(default)]
    pub category_id: Option<String>,
    /// Filters by name and ranks by relevance when not empty.
    #[serde(default)]
    pub query: String,
    /// Ignored when `query` is not empty: matches are ranked instead.
    #[serde(default)]
    pub order: Order,
    #[serde(default)]
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Deserialize)]
pub struct PlayParams {
    pub kind: MediaKind,
    pub id: u64,
    /// Title recorded in history.
    pub name: String,
    /// Container extension of a movie or episode (`mp4`, `mkv`).
    #[serde(default)]
    pub ext: Option<String>,
    /// Artwork recorded in history.
    #[serde(default)]
    pub image: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetFavoriteParams {
    pub kind: MediaKind,
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub image: Option<String>,
    pub is_favorite: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Envelope<T> {
    Ok(T),
    Error(ErrorBody),
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub kind: ErrorKind,
    /// English detail for logs and secondary text. Never holds credentials:
    /// core errors strip URLs before they get here.
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The request was malformed: a bug in the frontend.
    InvalidRequest,
    /// No session is open; call `open` first.
    NotSignedIn,
    /// The panel rejected the username or password.
    AuthFailed,
    /// The host could not be parsed as a URL.
    InvalidHost,
    /// The panel could not be reached, or stalled.
    Network,
    /// The panel answered with an error status or unreadable data.
    Server,
    /// The panel is behind a stablecoin payment gateway.
    PaymentRequired,
    /// The account is a plain M3U playlist; only live TV is available.
    LiveOnly,
    /// No item with that id.
    NotFound,
    /// Reading or writing local files failed.
    Storage,
    /// The title can't be played natively (codec, missing keyframe
    /// index); play it with VLC instead.
    Unsupported,
}

#[derive(Debug, Serialize)]
pub struct AccountOverview {
    /// Free-form panel status: "Active", "Expired", "Banned".
    pub status: Option<String>,
    /// Epoch seconds; `None` means the subscription never expires.
    pub expires_at: Option<i64>,
    pub is_trial: bool,
    pub connections_active: Option<i64>,
    pub connections_max: Option<i64>,
    /// IANA name of the panel's timezone.
    pub timezone: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Category {
    pub id: String,
    pub name: String,
}

/// A browsable title: a channel, a movie, or a show.
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub library: Library,
    pub id: u64,
    pub name: String,
    /// Poster, or channel logo.
    pub image: Option<String>,
    /// Wide artwork, when the panel lists one (series only).
    pub backdrop: Option<String>,
    /// 0 to 10.
    pub rating: Option<f64>,
    pub genre: Option<String>,
    /// Epoch seconds the item was added or last changed.
    pub added_at: Option<i64>,
    /// Container extension of a movie.
    pub ext: Option<String>,
    /// Catch-up replay is available for this channel.
    pub has_archive: bool,
    /// The panel category listing it, to open from a search result.
    pub category_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Page {
    /// Matching items before paging.
    pub total: u32,
    pub items: Vec<Item>,
}

#[derive(Debug, Serialize)]
pub struct SearchResults {
    pub live: Vec<Item>,
    pub movies: Vec<Item>,
    pub series: Vec<Item>,
}

#[derive(Debug, Serialize)]
pub struct MovieDetail {
    pub plot: Option<String>,
    pub genre: Option<String>,
    pub cast: Option<String>,
    pub director: Option<String>,
    pub duration_secs: Option<u32>,
    pub rating: Option<f64>,
    pub released: Option<String>,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    /// YouTube video id.
    pub trailer: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SeriesDetail {
    pub plot: Option<String>,
    pub genre: Option<String>,
    pub cast: Option<String>,
    pub rating: Option<f64>,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    /// Ascending by season number.
    pub seasons: Vec<Season>,
}

#[derive(Debug, Serialize)]
pub struct Season {
    /// 0 for specials, or when the panel gives no number.
    pub number: u32,
    /// Ascending by episode number.
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Serialize)]
pub struct Episode {
    pub id: u64,
    pub number: Option<u32>,
    pub title: Option<String>,
    pub plot: Option<String>,
    pub image: Option<String>,
    pub duration_secs: Option<u32>,
    pub ext: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Programme {
    pub title: Option<String>,
    pub description: Option<String>,
    /// Epoch seconds.
    pub start: i64,
    /// Epoch seconds.
    pub stop: i64,
}

#[derive(Debug, Serialize)]
pub struct NowNext {
    pub stream_id: u64,
    pub now: Option<Programme>,
    pub next: Option<Programme>,
}

#[derive(Debug, Serialize)]
pub struct Playback {
    /// Embeds the panel credentials: hand it to the player, never log it.
    pub url: String,
    pub is_live: bool,
}

#[derive(Debug, Serialize)]
pub struct RemuxedStream {
    /// Loopback HLS playlist for AVPlayer. Unguessable, but still never
    /// logged: it reaches the panel with the account's credentials.
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct Entry {
    pub kind: MediaKind,
    pub id: u64,
    pub name: String,
    pub image: Option<String>,
    pub ext: Option<String>,
    /// Epoch seconds; `None` for favorites.
    pub played_at: Option<u64>,
}

/// A game and the feeds of it found in the catalog.
#[derive(Debug, Serialize)]
pub struct Game {
    pub id: String,
    pub league: League,
    pub league_name: &'static str,
    /// Epoch seconds.
    pub start: i64,
    pub state: EventState,
    /// "Q3 5:12", "Half", "Final", "8:20 PM EST".
    pub status: Option<String>,
    pub home: Team,
    pub away: Team,
    /// Never empty: games nobody carries are left out.
    pub channels: Vec<Item>,
}
