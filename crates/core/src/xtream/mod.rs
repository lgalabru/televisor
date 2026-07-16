//! Xtream Codes API: lenient deserialization, typed client, playback URLs.

pub mod client;
pub mod de;
pub mod types;

pub use client::{DEFAULT_USER_AGENT, XtreamClient, XtreamError};
pub use types::{
    AccountSummary, Category, CategoryId, Episode, LiveStream, Series, SeriesInfo, ServerInfo,
    StreamFormat, StreamId, UserInfo, VodStream,
};
