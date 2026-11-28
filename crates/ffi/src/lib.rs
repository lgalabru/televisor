//! C ABI over `televisor-core`, for native frontends such as the Apple TV
//! app in `apps/appletv`.
//!
//! The surface is two functions (see `include/televisor.h`): a frontend
//! sends a JSON request and gets a JSON response back. JSON keeps the ABI
//! tiny and free of generated bindings; the payloads are small next to the
//! panel round trips behind them. The wire format lives in [`protocol`].
//!
//! Calls block the calling thread on a shared Tokio runtime, so frontends
//! call from background threads, several at once if they like. One session
//! (one signed-in account) is open at a time.

pub mod error;
pub mod protocol;
mod remux;
mod session;

use std::ffi::{CStr, CString, c_char};
use std::sync::{Arc, OnceLock, RwLock};

use serde::Serialize;

use crate::error::{BridgeError, Result};
use crate::protocol::{Envelope, ErrorKind, REQUEST_BYTES_MAX, Request};
use crate::session::Session;

/// Run one JSON request and return the JSON response.
///
/// # Safety
///
/// `request` must point to a NUL-terminated string that stays valid for the
/// duration of the call. The returned string must be released with
/// [`televisor_string_free`], exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn televisor_call(request: *const c_char) -> *mut c_char {
    assert!(!request.is_null(), "televisor_call needs a request");
    // SAFETY: the caller promises a valid NUL-terminated string.
    let request = unsafe { CStr::from_ptr(request) };
    let response = call(request.to_bytes());
    CString::new(response)
        .expect("serde_json escapes NUL, so a response never contains one")
        .into_raw()
}

/// Release a string returned by [`televisor_call`].
///
/// # Safety
///
/// `string` must be NULL or a pointer returned by [`televisor_call`] that
/// has not been released yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn televisor_string_free(string: *mut c_char) {
    if string.is_null() {
        return;
    }
    // SAFETY: the caller promises the pointer came from `CString::into_raw`.
    drop(unsafe { CString::from_raw(string) });
}

/// Safe entry point behind the C ABI: bytes in, JSON out. Public so tests
/// and Rust hosts can skip the pointers.
pub fn call(request: &[u8]) -> String {
    let outcome = parse(request).and_then(|request| runtime().block_on(handle(request)));
    let response = match outcome {
        Ok(value) => serde_json::to_string(&Envelope::Ok(value)),
        Err(err) => serde_json::to_string(&Envelope::<()>::Error(err.into_body())),
    };
    response.expect("response types always serialize")
}

fn parse(request: &[u8]) -> Result<Request> {
    if request.len() > REQUEST_BYTES_MAX {
        return Err(BridgeError::new(
            ErrorKind::InvalidRequest,
            format!(
                "request is {} bytes, the limit is {REQUEST_BYTES_MAX}",
                request.len()
            ),
        ));
    }
    serde_json::from_slice(request)
        .map_err(|err| BridgeError::new(ErrorKind::InvalidRequest, err.to_string()))
}

async fn handle(request: Request) -> Result<serde_json::Value> {
    match request {
        Request::Open(params) => {
            let (session, overview) = Session::open(params).await?;
            *SESSION.write().expect("session lock is never poisoned") = Some(Arc::new(session));
            json(&overview)
        }
        Request::Close => {
            *SESSION.write().expect("session lock is never poisoned") = None;
            json(&())
        }
        Request::Status => json(&session()?.status().await?),
        Request::Refresh => json(&session()?.refresh().await?),
        Request::Categories { library } => json(&session()?.categories(library).await?),
        Request::Browse(params) => json(&session()?.browse(&params).await?),
        Request::Search { query, limit } => json(&session()?.search(&query, limit).await?),
        Request::Movie { id } => json(&session()?.movie(id).await?),
        Request::Series { id } => json(&session()?.series_detail(id).await?),
        Request::Epg { stream_id, limit } => json(&session()?.epg(stream_id, limit).await?),
        Request::EpgNow { stream_ids } => json(&session()?.epg_now(&stream_ids).await?),
        Request::Play(params) => json(&session()?.play(params).await?),
        Request::RemuxOpen { url } => json(&remux::open(&url).await?),
        Request::RemuxClose { url } => {
            remux::close(Some(&url));
            json(&())
        }
        Request::History => json(&session()?.history()?),
        Request::Favorites => json(&session()?.favorites()?),
        Request::SetFavorite(params) => json(&session()?.set_favorite(params)?),
        Request::SportsToday { region } => json(&session()?.sports_today(region.as_deref()).await?),
    }
}

fn json<T: Serialize>(value: &T) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(value).expect("response types always serialize"))
}

static SESSION: RwLock<Option<Arc<Session>>> = RwLock::new(None);

fn session() -> Result<Arc<Session>> {
    SESSION
        .read()
        .expect("session lock is never poisoned")
        .clone()
        .ok_or_else(|| BridgeError::new(ErrorKind::NotSignedIn, "no account is signed in"))
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            // Work is network-bound; Apple TV has 2 to 6 cores.
            .worker_threads(2)
            .thread_name("televisor")
            .enable_all()
            .build()
            .expect("the Tokio runtime starts on every supported platform")
    })
}
