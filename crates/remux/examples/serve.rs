//! Serves one Matroska title as HLS on loopback, to try it in a player:
//!
//! ```sh
//! cargo run -p televisor-remux --example serve -- movie.mkv
//! TELEVISOR_REMUX_URL='http://panel/movie/user/pass/1.mkv' cargo run -p televisor-remux --example serve
//! ```
//!
//! A panel URL comes from the environment so it stays out of shell history
//! and process lists; it embeds the account's credentials.

use televisor_remux::{HttpSource, MemorySource, RemuxError, Server, Source};

#[tokio::main]
async fn main() -> Result<(), RemuxError> {
    // Segment failures are logged as warnings; show them.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    match std::env::args().nth(1) {
        Some(path) => serve(MemorySource(std::fs::read(path)?)).await,
        None => {
            let url = std::env::var("TELEVISOR_REMUX_URL")
                .expect("a file path argument or TELEVISOR_REMUX_URL");
            let url = url.parse().expect("TELEVISOR_REMUX_URL is a URL");
            serve(HttpSource::new(url)?).await
        }
    }
}

async fn serve(source: impl Source) -> Result<(), RemuxError> {
    let server = Server::start(source).await?;
    let title = server.title();
    eprintln!(
        "{} segments; video {} {:?} {:?} (Dolby Vision record: {}); audio {}; subtitles {}",
        title.segments.len(),
        title.video.codec_string,
        title.video.range,
        title.video.supplemental_codec,
        title.video.track.dolby_vision.is_some(),
        title
            .audio
            .iter()
            .map(|out| format!("{} ({})", out.codec_string, out.track.language))
            .collect::<Vec<_>>()
            .join(", "),
        title.subtitles.len()
    );
    println!("{}", server.url());
    tokio::signal::ctrl_c().await?;
    Ok(())
}
