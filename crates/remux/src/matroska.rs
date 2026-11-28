//! The parts of Matroska a remuxer needs: where things are (SeekHead,
//! Cues), what the tracks hold (Tracks, Info) and the frames themselves
//! (Clusters). Element ids are from the Matroska spec (RFC 9559).

use crate::ebml::{self, Cursor, Header};
use crate::error::ParseError;

pub mod id {
    pub const EBML: u32 = 0x1A45_DFA3;
    pub const DOC_TYPE: u32 = 0x4282;
    pub const SEGMENT: u32 = 0x1853_8067;
    pub const SEEK_HEAD: u32 = 0x114D_9B74;
    pub const SEEK: u32 = 0x4DBB;
    pub const SEEK_ID: u32 = 0x53AB;
    pub const SEEK_POSITION: u32 = 0x53AC;
    pub const INFO: u32 = 0x1549_A966;
    pub const TIMESTAMP_SCALE: u32 = 0x2A_D7B1;
    pub const DURATION: u32 = 0x4489;
    pub const TRACKS: u32 = 0x1654_AE6B;
    pub const TRACK_ENTRY: u32 = 0xAE;
    pub const TRACK_NUMBER: u32 = 0xD7;
    pub const TRACK_TYPE: u32 = 0x83;
    pub const FLAG_ENABLED: u32 = 0xB9;
    pub const FLAG_DEFAULT: u32 = 0x88;
    pub const FLAG_FORCED: u32 = 0x55AA;
    pub const DEFAULT_DURATION: u32 = 0x23_E383;
    pub const CODEC_ID: u32 = 0x86;
    pub const CODEC_PRIVATE: u32 = 0x63A2;
    pub const LANGUAGE: u32 = 0x22_B59C;
    pub const LANGUAGE_BCP47: u32 = 0x22_B59D;
    pub const NAME: u32 = 0x536E;
    pub const VIDEO: u32 = 0xE0;
    pub const PIXEL_WIDTH: u32 = 0xB0;
    pub const PIXEL_HEIGHT: u32 = 0xBA;
    pub const COLOUR: u32 = 0x55B0;
    pub const TRANSFER_CHARACTERISTICS: u32 = 0x55BA;
    pub const AUDIO: u32 = 0xE1;
    pub const SAMPLING_FREQUENCY: u32 = 0xB5;
    pub const CHANNELS: u32 = 0x9F;
    pub const CONTENT_ENCODINGS: u32 = 0x6D80;
    pub const CONTENT_ENCODING: u32 = 0x6240;
    pub const CONTENT_COMPRESSION: u32 = 0x5034;
    pub const CONTENT_COMP_ALGO: u32 = 0x4254;
    pub const CONTENT_COMP_SETTINGS: u32 = 0x4255;
    pub const CONTENT_ENCRYPTION: u32 = 0x5035;
    pub const BLOCK_ADDITION_MAPPING: u32 = 0x41E4;
    pub const BLOCK_ADD_ID_TYPE: u32 = 0x41E7;
    pub const BLOCK_ADD_ID_EXTRA_DATA: u32 = 0x41ED;
    pub const CUES: u32 = 0x1C53_BB6B;
    pub const CUE_POINT: u32 = 0xBB;
    pub const CUE_TIME: u32 = 0xB3;
    pub const CUE_TRACK_POSITIONS: u32 = 0xB7;
    pub const CUE_TRACK: u32 = 0xF7;
    pub const CUE_CLUSTER_POSITION: u32 = 0xF1;
    pub const CUE_RELATIVE_POSITION: u32 = 0xF0;
    pub const CLUSTER: u32 = 0x1F43_B675;
    pub const TIMESTAMP: u32 = 0xE7;
    pub const SIMPLE_BLOCK: u32 = 0xA3;
    pub const BLOCK_GROUP: u32 = 0xA0;
    pub const BLOCK: u32 = 0xA1;
    pub const BLOCK_DURATION: u32 = 0x9B;
    pub const REFERENCE_BLOCK: u32 = 0xFB;
}

/// Matroska's default: timestamps count milliseconds.
const TIMESTAMP_SCALE_NS_DEFAULT: u64 = 1_000_000;
/// Most frames one laced block may hold: the lace count is one byte.
const LACE_FRAMES_MAX: usize = 256;
/// Dolby Vision configuration record types (`dvcC` for profiles up to 7,
/// `dvvC` for 8 and up), as BlockAddIDType values.
pub const DOLBY_VISION_CONFIG: u32 = u32::from_be_bytes(*b"dvcC");
pub const DOLBY_VISION_CONFIG_EXTENDED: u32 = u32::from_be_bytes(*b"dvvC");

/// What the start of the file says: where the Segment's payload starts and
/// where its top-level elements are.
#[derive(Debug, Default)]
pub struct Head {
    /// File offset of the Segment's payload. SeekHead and Cues positions
    /// count from here.
    pub segment_offset: u64,
    /// File offset just past the Segment, when its size is known.
    pub segment_end: Option<u64>,
    /// Top-level elements met in the bytes read, in file order.
    pub elements: Vec<Located>,
    /// Positions the SeekHead lists, as file offsets.
    pub seeks: Vec<(u32, u64)>,
}

/// A top-level element: its id, file offset and total size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Located {
    pub id: u32,
    pub offset: u64,
    pub header_bytes: u64,
    /// Payload size; `None` when unknown.
    pub size: Option<u64>,
}

impl Head {
    /// Where element `id` starts, from the bytes read or the SeekHead.
    pub fn find(&self, id: u32) -> Option<u64> {
        self.elements
            .iter()
            .find(|element| element.id == id)
            .map(|element| element.offset)
            .or_else(|| {
                self.seeks
                    .iter()
                    .find(|(seek, _)| *seek == id)
                    .map(|(_, offset)| *offset)
            })
    }
}

/// Parses the start of a file: EBML header, Segment header, then each
/// top-level element up to the first Cluster or the end of `data`. A
/// SeekHead cut off by the end of `data` is skipped, not an error.
pub fn parse_head(data: &[u8]) -> Result<Head, ParseError> {
    let mut cursor = Cursor::new(data);
    let ebml_header = cursor.header()?;
    if ebml_header.id != id::EBML {
        return Err(ParseError::Invalid("not an EBML file"));
    }
    let doc_type = doc_type(cursor.payload(&ebml_header)?)?;
    if doc_type != "matroska" && doc_type != "webm" {
        return Err(ParseError::Invalid("EBML file that is not Matroska"));
    }
    let segment = cursor.header()?;
    if segment.id != id::SEGMENT {
        return Err(ParseError::Invalid("no Segment after the EBML header"));
    }
    let segment_offset = cursor.position() as u64;
    let mut head = Head {
        segment_offset,
        segment_end: segment.size.map(|size| segment_offset + size),
        ..Head::default()
    };
    while !cursor.is_empty() {
        let offset = cursor.position() as u64;
        let header = match cursor.header() {
            Ok(header) => header,
            Err(ParseError::Truncated { .. }) => break,
            Err(error) => return Err(error),
        };
        head.elements.push(Located {
            id: header.id,
            offset,
            header_bytes: header.header_bytes as u64,
            size: header.size,
        });
        if header.id == id::CLUSTER {
            break;
        }
        let Ok(payload) = cursor.payload(&header) else {
            break;
        };
        if header.id == id::SEEK_HEAD {
            head.seeks.extend(
                parse_seek_head(payload)?
                    .into_iter()
                    .map(|(seek, position)| (seek, segment_offset + position)),
            );
        }
    }
    Ok(head)
}

fn doc_type(payload: &[u8]) -> Result<String, ParseError> {
    let mut doc_type = String::from("matroska");
    ebml::children(payload, |header, body| {
        if header.id == id::DOC_TYPE {
            doc_type = ebml::string(body);
        }
        Ok(())
    })?;
    Ok(doc_type)
}

fn parse_seek_head(payload: &[u8]) -> Result<Vec<(u32, u64)>, ParseError> {
    let mut seeks = Vec::new();
    ebml::children(payload, |header, body| {
        if header.id != id::SEEK {
            return Ok(());
        }
        let mut seek_id = None;
        let mut position = None;
        ebml::children(body, |child, value| {
            match child.id {
                id::SEEK_ID => seek_id = Some(ebml::uint(value)? as u32),
                id::SEEK_POSITION => position = Some(ebml::uint(value)?),
                _ => {}
            }
            Ok(())
        })?;
        if let (Some(seek_id), Some(position)) = (seek_id, position) {
            seeks.push((seek_id, position));
        }
        Ok(())
    })?;
    Ok(seeks)
}

/// The Segment's Info: the timestamp unit and the title's length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Info {
    pub timestamp_scale_ns: u64,
    /// In timestamp units.
    pub duration: Option<f64>,
}

impl Info {
    pub fn duration_ns(&self) -> Option<u64> {
        self.duration
            .map(|duration| (duration * self.timestamp_scale_ns as f64) as u64)
    }
}

pub fn parse_info(payload: &[u8]) -> Result<Info, ParseError> {
    let mut info = Info {
        timestamp_scale_ns: TIMESTAMP_SCALE_NS_DEFAULT,
        duration: None,
    };
    ebml::children(payload, |header, body| {
        match header.id {
            id::TIMESTAMP_SCALE => info.timestamp_scale_ns = ebml::uint(body)?,
            id::DURATION => info.duration = Some(ebml::float(body)?),
            _ => {}
        }
        Ok(())
    })?;
    if info.timestamp_scale_ns == 0 {
        return Err(ParseError::Invalid("timestamp scale of zero"));
    }
    Ok(info)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
    Other,
}

/// How a track's frames are stored, beyond the codec itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Encoding {
    Plain,
    /// "Header stripping": every frame lost the same leading bytes, which
    /// must be put back.
    StrippedHeader(Vec<u8>),
    /// zlib, bzip2, LZO or encryption: nothing AVFoundation can take.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub number: u64,
    pub kind: TrackKind,
    pub codec_id: String,
    pub codec_private: Vec<u8>,
    pub is_enabled: bool,
    pub is_default: bool,
    /// Subtitles meant to show whenever the audio is in their language.
    pub is_forced: bool,
    pub default_duration_ns: Option<u64>,
    /// ISO 639-2 (`fre`) or BCP 47 (`fr-CA`); `und` when absent.
    pub language: String,
    pub name: Option<String>,
    pub encoding: Encoding,
    pub width: u32,
    pub height: u32,
    /// ITU-T H.273 transfer characteristics: 16 is PQ, 18 is HLG.
    pub transfer: Option<u8>,
    /// A Dolby Vision configuration record and its box type.
    pub dolby_vision: Option<(u32, Vec<u8>)>,
    pub sample_rate: f64,
    pub channels: u32,
}

pub fn parse_tracks(payload: &[u8]) -> Result<Vec<Track>, ParseError> {
    let mut tracks = Vec::new();
    ebml::children(payload, |header, body| {
        if header.id == id::TRACK_ENTRY {
            tracks.push(parse_track(body)?);
        }
        Ok(())
    })?;
    Ok(tracks)
}

fn parse_track(payload: &[u8]) -> Result<Track, ParseError> {
    let mut track = Track {
        number: 0,
        kind: TrackKind::Other,
        codec_id: String::new(),
        codec_private: Vec::new(),
        is_enabled: true,
        is_default: true,
        is_forced: false,
        default_duration_ns: None,
        language: String::from("eng"),
        name: None,
        encoding: Encoding::Plain,
        width: 0,
        height: 0,
        transfer: None,
        dolby_vision: None,
        sample_rate: 8000.0,
        channels: 1,
    };
    let mut language_bcp47 = None;
    ebml::children(payload, |header, body| {
        match header.id {
            id::TRACK_NUMBER => track.number = ebml::uint(body)?,
            id::TRACK_TYPE => {
                track.kind = match ebml::uint(body)? {
                    1 => TrackKind::Video,
                    2 => TrackKind::Audio,
                    17 => TrackKind::Subtitle,
                    _ => TrackKind::Other,
                }
            }
            id::FLAG_ENABLED => track.is_enabled = ebml::uint(body)? != 0,
            id::FLAG_DEFAULT => track.is_default = ebml::uint(body)? != 0,
            id::FLAG_FORCED => track.is_forced = ebml::uint(body)? != 0,
            id::DEFAULT_DURATION => {
                track.default_duration_ns = Some(ebml::uint(body)?).filter(|ns| *ns > 0)
            }
            id::CODEC_ID => track.codec_id = ebml::string(body),
            id::CODEC_PRIVATE => track.codec_private = body.to_vec(),
            id::LANGUAGE => track.language = ebml::string(body),
            id::LANGUAGE_BCP47 => language_bcp47 = Some(ebml::string(body)),
            id::NAME => track.name = Some(ebml::string(body)).filter(|name| !name.is_empty()),
            id::VIDEO => parse_video(body, &mut track)?,
            id::AUDIO => parse_audio(body, &mut track)?,
            id::CONTENT_ENCODINGS => track.encoding = parse_encodings(body)?,
            id::BLOCK_ADDITION_MAPPING => {
                if let Some(config) = parse_block_addition(body)? {
                    track.dolby_vision = Some(config);
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    if track.number == 0 {
        return Err(ParseError::Invalid("track without a number"));
    }
    if let Some(language) = language_bcp47 {
        track.language = language;
    }
    Ok(track)
}

fn parse_video(payload: &[u8], track: &mut Track) -> Result<(), ParseError> {
    ebml::children(payload, |header, body| {
        match header.id {
            id::PIXEL_WIDTH => track.width = ebml::uint(body)? as u32,
            id::PIXEL_HEIGHT => track.height = ebml::uint(body)? as u32,
            id::COLOUR => ebml::children(body, |colour, value| {
                if colour.id == id::TRANSFER_CHARACTERISTICS {
                    track.transfer = Some(ebml::uint(value)? as u8);
                }
                Ok(())
            })?,
            _ => {}
        }
        Ok(())
    })
}

fn parse_audio(payload: &[u8], track: &mut Track) -> Result<(), ParseError> {
    ebml::children(payload, |header, body| {
        match header.id {
            id::SAMPLING_FREQUENCY => track.sample_rate = ebml::float(body)?,
            id::CHANNELS => track.channels = ebml::uint(body)? as u32,
            _ => {}
        }
        Ok(())
    })
}

fn parse_encodings(payload: &[u8]) -> Result<Encoding, ParseError> {
    let mut encoding = Encoding::Plain;
    ebml::children(payload, |header, body| {
        if header.id != id::CONTENT_ENCODING {
            return Ok(());
        }
        if encoding != Encoding::Plain {
            // Two layers of encoding: rare and not worth undoing.
            encoding = Encoding::Unsupported;
            return Ok(());
        }
        encoding = parse_encoding(body)?;
        Ok(())
    })?;
    Ok(encoding)
}

fn parse_encoding(payload: &[u8]) -> Result<Encoding, ParseError> {
    let mut encoding = Encoding::Unsupported;
    ebml::children(payload, |header, body| {
        match header.id {
            id::CONTENT_COMPRESSION => {
                // ContentCompAlgo defaults to 0 (zlib); 3 is header stripping.
                let mut algorithm = 0;
                let mut settings = Vec::new();
                ebml::children(body, |child, value| {
                    match child.id {
                        id::CONTENT_COMP_ALGO => algorithm = ebml::uint(value)?,
                        id::CONTENT_COMP_SETTINGS => settings = value.to_vec(),
                        _ => {}
                    }
                    Ok(())
                })?;
                if algorithm == 3 {
                    encoding = Encoding::StrippedHeader(settings);
                }
            }
            id::CONTENT_ENCRYPTION => encoding = Encoding::Unsupported,
            _ => {}
        }
        Ok(())
    })?;
    Ok(encoding)
}

fn parse_block_addition(payload: &[u8]) -> Result<Option<(u32, Vec<u8>)>, ParseError> {
    let mut kind = 0u32;
    let mut extra = Vec::new();
    ebml::children(payload, |header, body| {
        match header.id {
            id::BLOCK_ADD_ID_TYPE => kind = ebml::uint(body)? as u32,
            id::BLOCK_ADD_ID_EXTRA_DATA => extra = body.to_vec(),
            _ => {}
        }
        Ok(())
    })?;
    let is_dolby_vision = kind == DOLBY_VISION_CONFIG || kind == DOLBY_VISION_CONFIG_EXTENDED;
    Ok((is_dolby_vision && !extra.is_empty()).then_some((kind, extra)))
}

/// One entry of the keyframe index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CuePoint {
    /// In timestamp units.
    pub time: u64,
    pub track: u64,
    /// File offset of the Cluster holding the keyframe.
    pub cluster_offset: u64,
    /// Offset of the keyframe's block inside the cluster's payload; 0
    /// when the index doesn't say, meaning the cluster's start.
    pub relative: u64,
}

/// Parses Cues into points sorted by time. `segment_offset` turns the
/// Segment-relative cluster positions into file offsets.
pub fn parse_cues(payload: &[u8], segment_offset: u64) -> Result<Vec<CuePoint>, ParseError> {
    let mut points = Vec::new();
    ebml::children(payload, |header, body| {
        if header.id != id::CUE_POINT {
            return Ok(());
        }
        let mut time = None;
        let mut positions = Vec::new();
        ebml::children(body, |child, value| {
            match child.id {
                id::CUE_TIME => time = Some(ebml::uint(value)?),
                id::CUE_TRACK_POSITIONS => positions.push(parse_cue_position(value)?),
                _ => {}
            }
            Ok(())
        })?;
        let Some(time) = time else {
            return Err(ParseError::Invalid("cue point without a time"));
        };
        for (track, position, relative) in positions {
            points.push(CuePoint {
                time,
                track,
                cluster_offset: segment_offset + position,
                relative,
            });
        }
        Ok(())
    })?;
    points.sort_by_key(|point| (point.time, point.cluster_offset));
    Ok(points)
}

/// `(track, cluster position, relative position)` of a CueTrackPositions.
fn parse_cue_position(payload: &[u8]) -> Result<(u64, u64, u64), ParseError> {
    let mut track = None;
    let mut position = None;
    let mut relative = 0;
    ebml::children(payload, |header, body| {
        match header.id {
            id::CUE_TRACK => track = Some(ebml::uint(body)?),
            id::CUE_CLUSTER_POSITION => position = Some(ebml::uint(body)?),
            id::CUE_RELATIVE_POSITION => relative = ebml::uint(body)?,
            _ => {}
        }
        Ok(())
    })?;
    match (track, position) {
        (Some(track), Some(position)) => Ok((track, position, relative)),
        _ => Err(ParseError::Invalid(
            "cue without a track or a cluster position",
        )),
    }
}

/// One frame, pointing into the buffer it was parsed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub track: u64,
    /// Presentation time, in timestamp units. Signed: a block may sit
    /// before its cluster's timestamp.
    pub time: i64,
    pub is_keyframe: bool,
    /// Byte range of the frame in the buffer.
    pub start: usize,
    pub length: usize,
    /// From BlockDuration, in timestamp units, spread over a lace.
    pub duration: Option<u64>,
    /// Index of the frame within its lace; later frames' times are
    /// estimates the caller refines with the codec's frame length.
    pub lace_index: u16,
    /// Where its block is stored, comparable with a cue's position.
    pub place: Place,
}

/// A block's place in the file: its cluster's offset (in the buffer
/// parsed) and the block's offset inside the cluster's payload, counted
/// as `CueRelativePosition` counts. Orders blocks in file order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Place {
    pub cluster: u64,
    pub relative: u64,
}

/// Parses consecutive Clusters from `data` and returns their frames in
/// file order (decode order). Stops quietly at the first element that is
/// not a Cluster, or at a Cluster cut off by the end of `data` (whose
/// complete blocks are still returned).
pub fn parse_clusters(data: &[u8]) -> Result<Vec<Frame>, ParseError> {
    let mut frames = Vec::new();
    let mut cursor = Cursor::new(data);
    while !cursor.is_empty() {
        let header = match cursor.header() {
            Ok(header) => header,
            Err(ParseError::Truncated { .. }) => break,
            Err(error) => return Err(error),
        };
        if header.id != id::CLUSTER {
            // Void, CRC-32, Cues or Tags between or after clusters.
            if is_skippable(header.id) && cursor.payload(&header).is_ok() {
                continue;
            }
            break;
        }
        let start = cursor.position();
        let end = match header.size {
            Some(size) => (start as u64 + size).min(data.len() as u64) as usize,
            None => data.len(),
        };
        let cluster = (start - header.header_bytes) as u64;
        let consumed = parse_cluster(
            &data[start..end],
            start,
            cluster,
            header.size.is_none(),
            &mut frames,
        )?;
        cursor = Cursor::new(data);
        cursor
            .take(start + consumed)
            .expect("consumed bytes are inside data");
    }
    Ok(frames)
}

fn is_skippable(element: u32) -> bool {
    // Void and CRC-32.
    matches!(element, 0xEC | 0xBF)
}

/// Parses one cluster's payload, appending its frames. Returns the bytes
/// consumed: for a cluster of unknown size, up to the next top-level id.
fn parse_cluster(
    payload: &[u8],
    base: usize,
    cluster: u64,
    is_unknown_size: bool,
    frames: &mut Vec<Frame>,
) -> Result<usize, ParseError> {
    let mut cursor = Cursor::new(payload);
    let mut cluster_time: Option<u64> = None;
    while !cursor.is_empty() {
        let element_start = cursor.position();
        let header = match cursor.header() {
            Ok(header) => header,
            Err(ParseError::Truncated { .. }) => return Ok(element_start),
            Err(error) => return Err(error),
        };
        if is_unknown_size && is_top_level(header.id) {
            return Ok(element_start);
        }
        let Ok(body) = cursor.payload(&header) else {
            // The read ended inside this element.
            return Ok(element_start);
        };
        let body_start = base + element_start + header.header_bytes;
        let place = Place {
            cluster,
            relative: element_start as u64,
        };
        match header.id {
            id::TIMESTAMP => cluster_time = Some(ebml::uint(body)?),
            id::SIMPLE_BLOCK => {
                let time = cluster_time
                    .ok_or(ParseError::Invalid("block before its cluster's timestamp"))?;
                parse_block(body, body_start, time, place, None, None, frames)?;
            }
            id::BLOCK_GROUP => {
                let time = cluster_time
                    .ok_or(ParseError::Invalid("block before its cluster's timestamp"))?;
                parse_block_group(body, body_start, time, place, frames)?;
            }
            _ => {}
        }
    }
    Ok(cursor.position())
}

fn is_top_level(element: u32) -> bool {
    matches!(
        element,
        id::CLUSTER
            | id::CUES
            | id::SEEK_HEAD
            | id::INFO
            | id::TRACKS
            | 0x1254_C367
            | 0x1941_A469
            | 0x1043_A770
    )
}

fn parse_block_group(
    payload: &[u8],
    base: usize,
    cluster_time: u64,
    place: Place,
    frames: &mut Vec<Frame>,
) -> Result<(), ParseError> {
    let mut block: Option<(&[u8], usize)> = None;
    let mut duration = None;
    let mut has_reference = false;
    let mut cursor = Cursor::new(payload);
    while !cursor.is_empty() {
        let start = cursor.position();
        let header: Header = cursor.header()?;
        let body = cursor.payload(&header)?;
        match header.id {
            id::BLOCK => block = Some((body, base + start + header.header_bytes)),
            id::BLOCK_DURATION => duration = Some(ebml::uint(body)?),
            id::REFERENCE_BLOCK => has_reference = true,
            _ => {}
        }
    }
    let (body, body_start) = block.ok_or(ParseError::Invalid("block group without a block"))?;
    parse_block(
        body,
        body_start,
        cluster_time,
        place,
        Some(!has_reference),
        duration,
        frames,
    )
}

/// Parses a SimpleBlock or Block. `is_keyframe` is given for a Block (from
/// its group) and read from the flags for a SimpleBlock.
fn parse_block(
    payload: &[u8],
    base: usize,
    cluster_time: u64,
    place: Place,
    is_keyframe: Option<bool>,
    duration: Option<u64>,
    frames: &mut Vec<Frame>,
) -> Result<(), ParseError> {
    let mut cursor = Cursor::new(payload);
    let (track, _) = cursor.varint()?;
    let relative = cursor.uint_bytes(2)? as u16 as i16;
    let flags = cursor.byte()?;
    let is_keyframe = is_keyframe.unwrap_or(flags & 0x80 != 0);
    let time = cluster_time as i64 + i64::from(relative);
    let sizes = lace_sizes(&mut cursor, flags)?;
    let count = sizes.len() as u64;
    let mut start = base + cursor.position();
    for (index, length) in sizes.into_iter().enumerate() {
        frames.push(Frame {
            track,
            time,
            is_keyframe,
            start,
            length,
            duration: duration.map(|duration| duration / count),
            lace_index: index as u16,
            place,
        });
        start += length;
    }
    assert_eq!(
        start,
        base + payload.len(),
        "lace sizes cover the block exactly"
    );
    Ok(())
}

/// Sizes of the frames in a block, after its header.
fn lace_sizes(cursor: &mut Cursor<'_>, flags: u8) -> Result<Vec<usize>, ParseError> {
    let lacing = (flags >> 1) & 0b11;
    if lacing == 0 {
        return Ok(vec![cursor.remaining()]);
    }
    let count = usize::from(cursor.byte()?) + 1;
    assert!(count <= LACE_FRAMES_MAX);
    let mut sizes = Vec::with_capacity(count);
    match lacing {
        // Xiph: each size is a run of 255s plus a final byte.
        0b01 => {
            for _ in 1..count {
                let mut size = 0usize;
                loop {
                    let byte = cursor.byte()?;
                    size += usize::from(byte);
                    if byte != 255 {
                        break;
                    }
                }
                sizes.push(size);
            }
        }
        // EBML: the first size, then signed differences.
        0b11 => {
            let (first, _) = cursor.varint()?;
            let mut size = first as i64;
            sizes.push(first as usize);
            for _ in 2..count {
                let (raw, width) = cursor.varint()?;
                let bias = (1i64 << (7 * width - 1)) - 1;
                size += raw as i64 - bias;
                if size < 0 {
                    return Err(ParseError::Invalid("negative EBML lace size"));
                }
                sizes.push(size as usize);
            }
        }
        // Fixed: equal sizes.
        0b10 => {
            if !cursor.remaining().is_multiple_of(count) {
                return Err(ParseError::Invalid(
                    "fixed lace not divisible by its frame count",
                ));
            }
            return Ok(vec![cursor.remaining() / count; count]);
        }
        _ => unreachable!("lacing is two bits and 0 returned above"),
    }
    let listed: usize = sizes.iter().sum();
    if listed > cursor.remaining() {
        return Err(ParseError::Invalid("lace sizes exceed the block"));
    }
    sizes.push(cursor.remaining() - listed);
    Ok(sizes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_block(track: u8, relative: i16, flags: u8, body: &[u8]) -> Vec<u8> {
        let mut block = vec![0x80 | track];
        block.extend_from_slice(&relative.to_be_bytes());
        block.push(flags);
        block.extend_from_slice(body);
        element(id::SIMPLE_BLOCK, &block)
    }

    fn element(element_id: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes: Vec<u8> = element_id
            .to_be_bytes()
            .into_iter()
            .skip_while(|byte| *byte == 0)
            .collect();
        // Sizes up to 2^56 - 2 fit in 8 bytes; tests stay far below.
        bytes.push(0x01);
        bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes()[1..]);
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn reads_simple_blocks_with_cluster_times() {
        let mut cluster = element(id::TIMESTAMP, &[0x03, 0xE8]);
        cluster.extend(simple_block(1, 0, 0x80, b"key"));
        cluster.extend(simple_block(1, -5, 0x00, b"before"));
        let data = element(id::CLUSTER, &cluster);
        let frames = parse_clusters(&data).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].time, 1000);
        assert!(frames[0].is_keyframe);
        assert_eq!(
            &data[frames[0].start..frames[0].start + frames[0].length],
            b"key"
        );
        assert_eq!(frames[1].time, 995);
        assert!(!frames[1].is_keyframe);
    }

    #[test]
    fn splits_xiph_ebml_and_fixed_laces() {
        // Xiph: 3 frames of sizes 2, 256 and the rest (1).
        let mut xiph = vec![2, 2, 255, 1];
        xiph.extend(vec![0xAA; 2 + 256 + 1]);
        // EBML: 3 frames of 4, 3 (difference -1) and the rest (2).
        let mut laced_ebml = vec![2, 0x84, 0xBE];
        laced_ebml.extend(vec![0xBB; 4 + 3 + 2]);
        let fixed = vec![1, 1, 2, 3, 4];
        let mut cluster = element(id::TIMESTAMP, &[0]);
        cluster.extend(simple_block(2, 0, 0x80 | 0b010, &xiph));
        cluster.extend(simple_block(2, 0, 0x80 | 0b110, &laced_ebml));
        cluster.extend(simple_block(2, 0, 0x80 | 0b100, &fixed));
        let frames = parse_clusters(&element(id::CLUSTER, &cluster)).unwrap();
        let lengths: Vec<usize> = frames.iter().map(|frame| frame.length).collect();
        assert_eq!(lengths, [2, 256, 1, 4, 3, 2, 2, 2]);
    }

    #[test]
    fn refuses_lace_sizes_beyond_the_block() {
        let mut cluster = element(id::TIMESTAMP, &[0]);
        cluster.extend(simple_block(1, 0, 0b010, &[1, 200, 0]));
        assert!(parse_clusters(&element(id::CLUSTER, &cluster)).is_err());
    }

    #[test]
    fn block_groups_take_keyframes_from_references() {
        let mut block = vec![0x81, 0, 10, 0];
        block.extend_from_slice(b"p");
        let mut group = element(id::BLOCK, &block);
        group.extend(element(id::REFERENCE_BLOCK, &[0xF6]));
        group.extend(element(id::BLOCK_DURATION, &[40]));
        let mut cluster = element(id::TIMESTAMP, &[0]);
        cluster.extend(element(id::BLOCK_GROUP, &group));
        let frames = parse_clusters(&element(id::CLUSTER, &cluster)).unwrap();
        assert_eq!(frames.len(), 1);
        assert!(!frames[0].is_keyframe);
        assert_eq!(frames[0].time, 10);
        assert_eq!(frames[0].duration, Some(40));
    }

    #[test]
    fn stops_at_a_cut_off_cluster_keeping_whole_blocks() {
        let mut cluster = element(id::TIMESTAMP, &[0]);
        cluster.extend(simple_block(1, 0, 0x80, b"whole"));
        cluster.extend(simple_block(1, 40, 0, b"cut off here"));
        let data = element(id::CLUSTER, &cluster);
        let frames = parse_clusters(&data[..data.len() - 4]).unwrap();
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn stops_at_the_first_element_that_is_not_a_cluster() {
        let mut cluster = element(id::TIMESTAMP, &[0]);
        cluster.extend(simple_block(1, 0, 0x80, b"a"));
        let mut data = element(id::CLUSTER, &cluster);
        data.extend(element(id::CUES, &[0; 8]));
        data.extend(element(id::CLUSTER, &cluster));
        assert_eq!(parse_clusters(&data).unwrap().len(), 1);
    }
}
