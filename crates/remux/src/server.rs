//! A loopback HTTP server that answers AVPlayer's HLS requests. It binds
//! `127.0.0.1` on a random port and serves only under a random path
//! token, so other apps on the device can't use it to reach the panel.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OnceCell, Semaphore};

use crate::error::RemuxError;
use crate::hls;
use crate::source::Source;
use crate::title::{Demuxed, Title};

/// Largest request head read. AVPlayer's are about 300 bytes.
const REQUEST_HEAD_BYTES_MAX: usize = 8 * 1024;
/// Connections served at once. AVPlayer opens a few per playlist.
const CONNECTIONS_MAX: usize = 16;
/// An idle keep-alive connection is closed after this.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Demuxed segments kept. The video, audio and subtitle requests for one
/// segment come close together; three covers them plus one prefetched.
/// At the 128 MiB read cap that is at most 384 MiB, in practice ~30 MiB.
const CACHED_SEGMENTS_MAX: usize = 3;
/// Bytes of randomness in the path token.
const TOKEN_BYTES: usize = 16;

/// What a request path asks for, under `/<token>/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Multivariant,
    VideoPlaylist,
    VideoInit,
    VideoSegment(usize),
    AudioPlaylist(usize),
    AudioInit(usize),
    AudioSegment(usize, usize),
    SubtitlePlaylist(usize),
    SubtitleSegment(usize, usize),
}

impl Route {
    /// The path relative to the token, as playlists write it.
    pub fn path(&self) -> String {
        match self {
            Self::Multivariant => String::from("index.m3u8"),
            Self::VideoPlaylist => String::from("video.m3u8"),
            Self::VideoInit => String::from("video.mp4"),
            Self::VideoSegment(index) => format!("video-{index}.m4s"),
            Self::AudioPlaylist(track) => format!("audio-{track}.m3u8"),
            Self::AudioInit(track) => format!("audio-{track}.mp4"),
            Self::AudioSegment(track, index) => format!("audio-{track}-{index}.m4s"),
            Self::SubtitlePlaylist(track) => format!("subtitles-{track}.m3u8"),
            Self::SubtitleSegment(track, index) => format!("subtitles-{track}-{index}.vtt"),
        }
    }

    pub fn parse(path: &str) -> Option<Self> {
        let (stem, extension) = path.rsplit_once('.')?;
        let mut parts = stem.split('-');
        let kind = parts.next()?;
        let numbers: Vec<usize> = parts.map(str::parse).collect::<Result<_, _>>().ok()?;
        let route = match (kind, numbers.as_slice(), extension) {
            ("index", [], "m3u8") => Self::Multivariant,
            ("video", [], "m3u8") => Self::VideoPlaylist,
            ("video", [], "mp4") => Self::VideoInit,
            ("video", [index], "m4s") => Self::VideoSegment(*index),
            ("audio", [track], "m3u8") => Self::AudioPlaylist(*track),
            ("audio", [track], "mp4") => Self::AudioInit(*track),
            ("audio", [track, index], "m4s") => Self::AudioSegment(*track, *index),
            ("subtitles", [track], "m3u8") => Self::SubtitlePlaylist(*track),
            ("subtitles", [track, index], "vtt") => Self::SubtitleSegment(*track, *index),
            _ => return None,
        };
        // One spelling per route: `video-01.m4s` is not `video-1.m4s`.
        (route.path() == path).then_some(route)
    }
}

/// One title being served. Dropping it stops the server.
pub struct Server<S: Source> {
    state: Arc<State<S>>,
    port: u16,
    token: String,
    task: tokio::task::JoinHandle<()>,
}

impl<S: Source> Server<S> {
    /// Opens the title and starts serving it. Must run inside a Tokio runtime.
    pub async fn start(source: S) -> Result<Self, RemuxError> {
        let title = Title::open(&source).await?;
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let token = random_token()?;
        let state = Arc::new(State {
            title,
            source,
            cache: Mutex::new(VecDeque::new()),
        });
        let task = tokio::spawn(accept(listener, Arc::clone(&state), token.clone()));
        Ok(Self {
            state,
            port,
            token,
            task,
        })
    }

    /// The multivariant playlist, for `AVPlayer`.
    pub fn url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/{}/{}",
            self.port,
            self.token,
            Route::Multivariant.path()
        )
    }

    pub fn title(&self) -> &Title {
        &self.state.title
    }
}

impl<S: Source> Drop for Server<S> {
    fn drop(&mut self) {
        // Open connections hold the state; they end with their next request.
        self.task.abort();
    }
}

type Slot = Arc<OnceCell<Arc<Demuxed>>>;

struct State<S: Source> {
    title: Title,
    source: S,
    /// Most recently used last.
    cache: Mutex<VecDeque<(usize, Slot)>>,
}

impl<S: Source> State<S> {
    /// Segment `index`, demuxed once however many requests want it.
    async fn segment(&self, index: usize) -> Result<Arc<Demuxed>, RemuxError> {
        let slot = {
            let mut cache = self.cache.lock().expect("never poisoned");
            let found = cache.iter().position(|(cached, _)| *cached == index);
            let slot = match found {
                Some(position) => cache.remove(position).expect("position is in range").1,
                None => Arc::new(OnceCell::new()),
            };
            cache.push_back((index, Arc::clone(&slot)));
            while cache.len() > CACHED_SEGMENTS_MAX {
                cache.pop_front();
            }
            slot
        };
        let demuxed = slot
            .get_or_try_init(|| async {
                let span = self.title.segments[index];
                let bytes = self
                    .source
                    .read(span.read_start(), span.read_end - span.read_start())
                    .await?;
                self.title.demux(index, bytes).map(Arc::new)
            })
            .await?;
        Ok(Arc::clone(demuxed))
    }

    async fn respond(&self, route: Route) -> Result<(Vec<u8>, &'static str), Failure> {
        let title = &self.title;
        let segments = title.segments.len();
        let playlist = "application/vnd.apple.mpegurl";
        match route {
            Route::Multivariant => Ok((hls::multivariant(title).into_bytes(), playlist)),
            Route::VideoPlaylist => Ok((
                hls::media(title, Some(Route::VideoInit), Route::VideoSegment).into_bytes(),
                playlist,
            )),
            Route::AudioPlaylist(track) if track < title.audio.len() => Ok((
                hls::media(title, Some(Route::AudioInit(track)), |index| {
                    Route::AudioSegment(track, index)
                })
                .into_bytes(),
                playlist,
            )),
            Route::SubtitlePlaylist(track) if track < title.subtitles.len() => Ok((
                hls::media(title, None, |index| Route::SubtitleSegment(track, index)).into_bytes(),
                playlist,
            )),
            Route::VideoInit => Ok((crate::mp4::init_segment(&title.video.format), "video/mp4")),
            Route::AudioInit(track) if track < title.audio.len() => Ok((
                crate::mp4::init_segment(&title.audio[track].format),
                "audio/mp4",
            )),
            Route::VideoSegment(index) if index < segments => {
                let demuxed = self.segment(index).await?;
                Ok((title.video_fragment(index, &demuxed)?, "video/iso.segment"))
            }
            Route::AudioSegment(track, index) if track < title.audio.len() && index < segments => {
                let demuxed = self.segment(index).await?;
                Ok((
                    title.audio_fragment(track, index, &demuxed)?,
                    "audio/iso.segment",
                ))
            }
            Route::SubtitleSegment(track, index)
                if track < title.subtitles.len() && index < segments =>
            {
                let demuxed = self.segment(index).await?;
                Ok((
                    title.subtitle_document(track, &demuxed).into_bytes(),
                    "text/vtt",
                ))
            }
            _ => Err(Failure::NotFound),
        }
    }
}

enum Failure {
    NotFound,
    Remux(RemuxError),
}

impl From<RemuxError> for Failure {
    fn from(error: RemuxError) -> Self {
        Self::Remux(error)
    }
}

async fn accept<S: Source>(listener: TcpListener, state: Arc<State<S>>, token: String) {
    let permits = Arc::new(Semaphore::new(CONNECTIONS_MAX));
    let token: Arc<str> = token.into();
    loop {
        let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
            unreachable!("the semaphore is never closed");
        };
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(error) => {
                // Out of file descriptors and the like: wait, don't spin.
                tracing::warn!(%error, "loopback accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let state = Arc::clone(&state);
        let token = Arc::clone(&token);
        tokio::spawn(async move {
            serve(stream, &state, &token).await;
            drop(permit);
        });
    }
}

/// Answers requests on one connection until it closes or idles.
async fn serve<S: Source>(mut stream: TcpStream, state: &State<S>, token: &str) {
    let mut buffer = Vec::with_capacity(1024);
    loop {
        let head =
            match tokio::time::timeout(IDLE_TIMEOUT, read_head(&mut stream, &mut buffer)).await {
                Ok(Some(head)) => head,
                // Closed, idle, or a request too large to be AVPlayer's.
                Ok(None) | Err(_) => return,
            };
        let Some((method, path)) = request_line(&head) else {
            // The connection closes either way; a failed write changes nothing.
            let _ = write_response(&mut stream, "400 Bad Request", "text/plain", b"", false).await;
            return;
        };
        let is_head = method == "HEAD";
        let route = path
            .strip_prefix('/')
            .and_then(|path| path.split_once('/'))
            .filter(|(given, _)| *given == token)
            .and_then(|(_, rest)| Route::parse(rest));
        let (status, content_type, body) = match route {
            None => ("404 Not Found", "text/plain", Vec::new()),
            Some(route) if method == "GET" || is_head => match state.respond(route).await {
                Ok((body, content_type)) => ("200 OK", content_type, body),
                Err(Failure::NotFound) => ("404 Not Found", "text/plain", Vec::new()),
                Err(Failure::Remux(error)) => {
                    tracing::warn!(%error, route = route.path(), "remux request failed");
                    (
                        "502 Bad Gateway",
                        "text/plain",
                        error.to_string().into_bytes(),
                    )
                }
            },
            Some(_) => ("405 Method Not Allowed", "text/plain", Vec::new()),
        };
        if write_response(&mut stream, status, content_type, &body, is_head)
            .await
            .is_err()
        {
            // AVPlayer cancels requests when seeking; nothing to clean up.
            return;
        }
    }
}

/// Reads one request head, keeping any bytes after it in `buffer`.
async fn read_head(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> Option<String> {
    loop {
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..end]).into_owned();
            buffer.drain(..end + 4);
            return Some(head);
        }
        if buffer.len() >= REQUEST_HEAD_BYTES_MAX {
            return None;
        }
        let mut chunk = [0u8; 1024];
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

fn request_line(head: &str) -> Option<(&str, &str)> {
    let mut parts = head.lines().next()?.split(' ');
    let method = parts.next()?;
    let target = parts.next()?;
    // Query strings carry nothing we use.
    let path = target.split('?').next()?;
    Some((method, path))
}

async fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    is_head: bool,
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    if !is_head {
        stream.write_all(body).await?;
    }
    stream.flush().await
}

fn random_token() -> Result<String, RemuxError> {
    let mut bytes = [0u8; TOKEN_BYTES];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_print_back_what_they_parse() {
        for route in [
            Route::Multivariant,
            Route::VideoPlaylist,
            Route::VideoInit,
            Route::VideoSegment(12),
            Route::AudioPlaylist(1),
            Route::AudioInit(1),
            Route::AudioSegment(1, 0),
            Route::SubtitlePlaylist(3),
            Route::SubtitleSegment(3, 7),
        ] {
            assert_eq!(Route::parse(&route.path()), Some(route));
        }
    }

    #[test]
    fn refuses_unknown_and_malformed_paths() {
        for path in [
            "",
            "video",
            "video-01.m4s",
            "video-x.m4s",
            "audio.m4s",
            "video-1-2.m4s",
            "../etc/passwd",
            "index.m3u8.bak",
        ] {
            assert_eq!(Route::parse(path), None, "{path}");
        }
    }

    #[test]
    fn reads_method_and_path_without_query() {
        assert_eq!(
            request_line("GET /abc/index.m3u8?x=1 HTTP/1.1\r\nHost: a"),
            Some(("GET", "/abc/index.m3u8"))
        );
        assert_eq!(request_line(""), None);
    }
}
