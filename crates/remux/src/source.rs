//! Where the Matroska bytes come from: the panel, over HTTP range
//! requests, one at a time.

use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, LOCATION, RANGE};
use url::Url;

use crate::error::RemuxError;

/// Random access to a file's bytes.
pub trait Source: Send + Sync + 'static {
    /// Up to `length` bytes from `offset`; fewer only at the end of the file.
    fn read(
        &self,
        offset: u64,
        length: u64,
    ) -> impl Future<Output = Result<Vec<u8>, RemuxError>> + Send;
    /// The file's size, once a read has revealed it.
    fn length(&self) -> Option<u64>;
}

/// Tries per read. The panel's CDN links expire (HTTP 509 after tens of
/// seconds idle, seen 2026-11-27), so a failed read resolves a fresh link
/// and tries again.
const READ_ATTEMPTS_MAX: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(500);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A whole segment read; 128 MiB at 20 Mb/s takes under a minute.
const READ_TIMEOUT: Duration = Duration::from_secs(90);
/// The panel redirects once, to its CDN; more is a loop.
const REDIRECTS_MAX: u32 = 4;

/// Reads from the panel. Plans allow one stream at a time, so reads are
/// serialised: AVPlayer's parallel segment requests share one upstream
/// connection instead of opening a second the panel would refuse.
pub struct HttpSource {
    client: reqwest::Client,
    /// The panel URL. Embeds credentials: never logged.
    origin: Url,
    /// The CDN link the panel last redirected to.
    resolved: Mutex<Option<Url>>,
    length: Mutex<Option<u64>>,
    serial: tokio::sync::Mutex<()>,
}

impl HttpSource {
    pub fn new(origin: Url) -> Result<Self, RemuxError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(READ_TIMEOUT)
            .build()
            .map_err(|error| RemuxError::Upstream(error.without_url().to_string()))?;
        Ok(Self {
            client,
            origin,
            resolved: Mutex::new(None),
            length: Mutex::new(None),
            serial: tokio::sync::Mutex::new(()),
        })
    }

    async fn attempt(&self, offset: u64, length: u64) -> Result<Vec<u8>, RemuxError> {
        let mut url = self
            .resolved
            .lock()
            .expect("never poisoned")
            .clone()
            .unwrap_or_else(|| self.origin.clone());
        let range = format!("bytes={}-{}", offset, offset + length - 1);
        for _ in 0..REDIRECTS_MAX {
            let response = self
                .client
                .get(url.clone())
                .header(RANGE, &range)
                .send()
                .await
                .map_err(upstream)?;
            let status = response.status();
            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| {
                        RemuxError::Upstream(String::from("redirect without a location"))
                    })?;
                url = url.join(location).map_err(|_| {
                    RemuxError::Upstream(String::from("redirect to an invalid location"))
                })?;
                continue;
            }
            if status == StatusCode::RANGE_NOT_SATISFIABLE {
                return Ok(Vec::new());
            }
            if status != StatusCode::PARTIAL_CONTENT {
                *self.resolved.lock().expect("never poisoned") = None;
                return Err(if status == StatusCode::OK {
                    RemuxError::Unsupported(String::from("the panel ignores range requests"))
                } else {
                    RemuxError::Upstream(format!("HTTP {}", status.as_u16()))
                });
            }
            if let Some(total) = total_length(&response) {
                *self.length.lock().expect("never poisoned") = Some(total);
            }
            *self.resolved.lock().expect("never poisoned") = Some(url);
            let body = response.bytes().await.map_err(upstream)?;
            if body.len() as u64 > length {
                return Err(RemuxError::Upstream(String::from(
                    "the panel sent more than the range asked",
                )));
            }
            return Ok(body.to_vec());
        }
        Err(RemuxError::Upstream(String::from("too many redirects")))
    }
}

impl Source for HttpSource {
    async fn read(&self, offset: u64, length: u64) -> Result<Vec<u8>, RemuxError> {
        assert!(length > 0, "reads ask for at least one byte");
        let _serial = self.serial.lock().await;
        let mut attempt = 1;
        loop {
            match self.attempt(offset, length).await {
                Ok(bytes) => return Ok(bytes),
                Err(RemuxError::Upstream(reason)) if attempt < READ_ATTEMPTS_MAX => {
                    tracing::debug!(
                        attempt,
                        reason,
                        "panel read failed; retrying with a fresh link"
                    );
                    *self.resolved.lock().expect("never poisoned") = None;
                    tokio::time::sleep(RETRY_DELAY).await;
                    attempt += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn length(&self) -> Option<u64> {
        *self.length.lock().expect("never poisoned")
    }
}

/// The total in `Content-Range: bytes 0-99/3168566082`.
fn total_length(response: &reqwest::Response) -> Option<u64> {
    let value = response.headers().get(CONTENT_RANGE)?.to_str().ok()?;
    value.rsplit('/').next()?.parse().ok()
}

/// Drops the URL from reqwest errors: it carries the panel credentials.
fn upstream(error: reqwest::Error) -> RemuxError {
    RemuxError::Upstream(error.without_url().to_string())
}

/// A file in memory, for tests and tools.
pub struct MemorySource(pub Vec<u8>);

impl Source for MemorySource {
    async fn read(&self, offset: u64, length: u64) -> Result<Vec<u8>, RemuxError> {
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.0.len());
        let end = usize::try_from(offset.saturating_add(length))
            .unwrap_or(usize::MAX)
            .min(self.0.len());
        Ok(self.0[start..end].to_vec())
    }

    fn length(&self) -> Option<u64> {
        Some(self.0.len() as u64)
    }
}
