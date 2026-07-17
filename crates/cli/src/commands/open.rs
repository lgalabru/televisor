use clap::Args;
use televisor_core::player::{self, OpenOptions};

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

    /// Quit the player when the stream ends
    #[clap(long)]
    pub exit_on_end: bool,

    /// Player: vlc (default), mpv, iina, ffplay, system, or a command template
    #[clap(long)]
    pub player: Option<String>,
}

impl OpenCommand {
    pub async fn execute(&self) -> televisor_core::Result<()> {
        let opts = OpenOptions {
            fullscreen: self.fullscreen,
            caching_ms: self.caching,
            exit_on_end: self.exit_on_end,
            title: None,
        };
        player::open_stream(&self.url, &opts, &super::player_for(self.player.as_deref()))?;
        Ok(())
    }
}
