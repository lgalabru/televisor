//! The one remuxed title being served, if any.

use std::sync::Mutex;

use televisor_remux::{HttpSource, Server};
use url::Url;

use crate::error::{BridgeError, Result};
use crate::protocol::{ErrorKind, RemuxedStream};

static SERVER: Mutex<Option<Server<HttpSource>>> = Mutex::new(None);

pub async fn open(url: &str) -> Result<RemuxedStream> {
    // The plan allows one stream: free the previous title's connection
    // before opening this one.
    close(None);
    let url = Url::parse(url).map_err(|_| {
        BridgeError::new(ErrorKind::InvalidRequest, "the stream address is not a URL")
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(BridgeError::new(
            ErrorKind::InvalidRequest,
            "the stream address is not HTTP",
        ));
    }
    let server = Server::start(HttpSource::new(url)?).await?;
    let stream = RemuxedStream { url: server.url() };
    *SERVER.lock().expect("never poisoned") = Some(server);
    Ok(stream)
}

/// Stops the title being served, if it is the one at `url` (any title
/// for `None`).
pub fn close(url: Option<&str>) {
    let server = {
        let mut current = SERVER.lock().expect("never poisoned");
        let is_match = match (current.as_ref(), url) {
            (Some(server), Some(url)) => server.url() == url,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if is_match { current.take() } else { None }
    };
    // Dropped outside the lock: stopping the server takes a moment.
    drop(server);
}
