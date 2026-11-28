use thiserror::Error;

/// What went wrong reading Matroska bytes.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseError {
    /// The bytes end before the element does. Opening reads more and
    /// retries; inside a cluster read it is a short read from the panel.
    #[error("need {needed_bytes} more bytes")]
    Truncated { needed_bytes: u64 },
    #[error("malformed Matroska: {0}")]
    Invalid(&'static str),
}

/// Why a title can't be remuxed, or a request failed.
#[derive(Debug, Error)]
pub enum RemuxError {
    /// The panel could not be reached or refused a read.
    #[error("the panel did not answer: {0}")]
    Upstream(String),
    #[error(transparent)]
    Parse(#[from] ParseError),
    /// The file is fine but AVFoundation can't play it this way: a codec
    /// it lacks, no keyframe index, compressed tracks. The caller falls
    /// back to VLC.
    #[error("can't play natively: {0}")]
    Unsupported(String),
    /// A local failure: the loopback server could not start.
    #[error("local server: {0}")]
    Io(#[from] std::io::Error),
}
