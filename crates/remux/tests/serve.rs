//! The server end to end over loopback HTTP, on a small Matroska file
//! made with ffmpeg: H.264 with B-frames, AAC (French), AC-3 (English) and
//! SubRip (French), 14 s with a keyframe every second.

use televisor_remux::matroska;
use televisor_remux::{MemorySource, RemuxError, Server};

const FIXTURE: &[u8] = include_bytes!("fixtures/small.mkv");

async fn get(url: &str) -> (u16, Vec<u8>) {
    let response = reqwest::get(url)
        .await
        .expect("the loopback server answers");
    let status = response.status().as_u16();
    (
        status,
        response.bytes().await.expect("a whole body").to_vec(),
    )
}

async fn text(base: &str, path: &str) -> String {
    let (status, body) = get(&format!("{base}{path}")).await;
    assert_eq!(status, 200, "{path}");
    String::from_utf8(body).expect("playlists are UTF-8")
}

/// Segment paths of a media playlist, in order.
fn segment_paths(playlist: &str) -> Vec<String> {
    playlist
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `(tfdt, sample count)` of a `moof` + `mdat` fragment.
fn fragment_summary(fragment: &[u8]) -> (u64, u32) {
    let find = |kind: &[u8]| {
        fragment
            .windows(4)
            .position(|window| window == kind)
            .expect("box present")
    };
    let tfdt = find(b"tfdt");
    let trun = find(b"trun");
    let decode_time = u64::from_be_bytes(fragment[tfdt + 8..tfdt + 16].try_into().unwrap());
    let samples = u32::from_be_bytes(fragment[trun + 8..trun + 12].try_into().unwrap());
    (decode_time, samples)
}

/// Frames per track number in the whole file, by our own parser.
fn frames_in_fixture(track: u64) -> u32 {
    let head = matroska::parse_head(FIXTURE).unwrap();
    let first_cluster = head
        .elements
        .iter()
        .find(|element| element.id == matroska::id::CLUSTER)
        .unwrap()
        .offset as usize;
    let frames = matroska::parse_clusters(&FIXTURE[first_cluster..]).unwrap();
    frames.iter().filter(|frame| frame.track == track).count() as u32
}

#[tokio::test]
async fn serves_every_frame_of_every_track_exactly_once() {
    let server = Server::start(MemorySource(FIXTURE.to_vec())).await.unwrap();
    let url = server.url();
    let base = url.strip_suffix("index.m3u8").unwrap().to_owned();
    let title = server.title();
    assert_eq!(title.video.codec_string.split('.').next(), Some("avc1"));
    assert_eq!(title.audio.len(), 2);
    assert_eq!(title.subtitles.len(), 1);

    let master = text(&base, "index.m3u8").await;
    assert!(master.contains("LANGUAGE=\"fr\""), "{master}");
    assert!(master.contains("LANGUAGE=\"en\""), "{master}");
    assert!(master.contains("TYPE=SUBTITLES"), "{master}");
    assert!(
        master.contains("mp4a.40.2") && master.contains("ac-3"),
        "{master}"
    );

    let video = text(&base, "video.m3u8").await;
    let total: f64 = video
        .lines()
        .filter_map(|line| line.strip_prefix("#EXTINF:"))
        .map(|line| line.trim_end_matches(',').parse::<f64>().unwrap())
        .sum();
    assert!((total - 14.0).abs() < 0.1, "playlist lasts {total} s");

    let tracks = [
        ("video", title.video.track.number),
        ("audio-0", title.audio[0].track.number),
        ("audio-1", title.audio[1].track.number),
    ];
    for (name, number) in tracks {
        let (status, init) = get(&format!("{base}{name}.mp4")).await;
        assert_eq!(status, 200);
        assert_eq!(&init[4..8], b"ftyp");
        let mut served = 0;
        let mut last_decode_time = None;
        for path in segment_paths(&text(&base, &format!("{name}.m3u8")).await) {
            let (status, fragment) = get(&format!("{base}{path}")).await;
            assert_eq!(status, 200, "{path}");
            let (decode_time, samples) = fragment_summary(&fragment);
            assert!(
                last_decode_time < Some(decode_time),
                "{name} goes forward in time"
            );
            last_decode_time = Some(decode_time);
            served += samples;
        }
        assert_eq!(served, frames_in_fixture(number), "{name} frames");
    }

    let mut subtitles = String::new();
    for path in segment_paths(&text(&base, "subtitles-0.m3u8").await) {
        let document = text(&base, &path).await;
        assert!(document.starts_with("WEBVTT\n"));
        subtitles.push_str(&document);
    }
    assert_eq!(
        subtitles.matches("<i>Bonjour</i>").count(),
        1,
        "{subtitles}"
    );
    assert_eq!(
        subtitles
            .matches("00:00:08.000 --> 00:00:09.500\nSecond")
            .count(),
        1,
        "{subtitles}"
    );
}

#[tokio::test]
async fn refuses_requests_outside_its_token_and_segments() {
    let server = Server::start(MemorySource(FIXTURE.to_vec())).await.unwrap();
    let url = server.url();
    let wrong_token = url.replace(
        url.split('/').nth(3).unwrap(),
        "00000000000000000000000000000000",
    );
    assert_eq!(get(&wrong_token).await.0, 404);
    let base = url.strip_suffix("index.m3u8").unwrap();
    assert_eq!(get(&format!("{base}video-999.m4s")).await.0, 404);
    assert_eq!(get(&format!("{base}audio-7.m3u8")).await.0, 404);
    assert_eq!(get(&format!("{base}video-01.m4s")).await.0, 404);
}

#[tokio::test]
async fn stops_serving_when_dropped() {
    let server = Server::start(MemorySource(FIXTURE.to_vec())).await.unwrap();
    let url = server.url();
    drop(server);
    tokio::task::yield_now().await;
    assert!(reqwest::get(&url).await.is_err());
}

#[tokio::test]
async fn refuses_files_it_cannot_remux() {
    let not_matroska = Server::start(MemorySource(b"RIFF....AVI LIST".to_vec())).await;
    assert!(matches!(not_matroska, Err(RemuxError::Parse(_))));
    // Headers without the Cues at the end of the file: no keyframe index.
    let cut = Server::start(MemorySource(FIXTURE[..FIXTURE.len() / 2].to_vec())).await;
    assert!(matches!(
        cut,
        Err(RemuxError::Unsupported(_) | RemuxError::Parse(_))
    ));
}
