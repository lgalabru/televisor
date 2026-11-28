//! Plays Matroska titles through AVPlayer, which has no Matroska demuxer:
//! the title is remuxed on the device, without transcoding, into HLS with
//! fragmented MP4 segments, served from a loopback HTTP server.
//!
//! Opening reads the file's headers and keyframe index (`Cues`) with HTTP
//! range requests and plans one HLS segment per run of keyframes. Each
//! segment request range-reads its clusters from the panel and repackages
//! the frames, copying the compressed samples untouched. See
//! `docs/rfd/0001-native-playback-for-mkv.md`.

mod bits;
pub mod codec;
pub mod ebml;
pub mod error;
pub mod hevc;
pub mod hls;
pub mod matroska;
pub mod mp4;
pub mod server;
pub mod source;
pub mod subtitles;
pub mod title;

pub use error::RemuxError;
pub use server::Server;
pub use source::{HttpSource, MemorySource, Source};
