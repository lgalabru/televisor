use clap::Args;
use televisor_core::vlc::{self, OpenOptions};

#[derive(Args)]
pub struct OpenCommand {
    /// Stream URL to open (HLS, RTSP, RTMP, UDP, file, ...)
    pub url: String,

    /// Start playback in fullscreen
    #[clap(long, short)]
    pub fullscreen: bool,

    /// Network caching in milliseconds
    #[clap(long)]
    pub caching: Option<u32>,

    /// Quit VLC when the stream ends
    #[clap(long)]
    pub exit_on_end: bool,
}

impl OpenCommand {
    pub async fn execute(&self) -> televisor_core::Result<()> {
        let opts = OpenOptions {
            fullscreen: self.fullscreen,
            caching_ms: self.caching,
            exit_on_end: self.exit_on_end,
            title: None,
        };
        vlc::open_stream(&self.url, &opts)?;
        Ok(())
    }
}
