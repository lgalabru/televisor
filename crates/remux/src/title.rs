//! A Matroska title made ready for HLS: which tracks play, where each
//! segment's bytes are, and how a segment's frames become fMP4 samples or
//! WebVTT cues.

use crate::codec::{self, AudioCodec, SubtitleCodec, VideoCodec};
use crate::error::{ParseError, RemuxError};
use crate::matroska::{self, CuePoint, Encoding, Frame, Head, Info, Place, Track, TrackKind, id};
use crate::mp4::{self, DolbyVisionBox, Sample, SampleEntry, TrackFormat};
use crate::source::Source;

/// Segment length we aim for; Apple's HLS authoring spec recommends 6 s.
/// Segments only split at keyframes, so some run longer.
const SEGMENT_DURATION_TARGET_NS: u64 = 6_000_000_000;
/// Largest segment read in one go. 10 s of an 80 Mb/s UHD remux is
/// 100 MB; past this a "segment" means a broken index, not a real title.
pub const SEGMENT_BYTES_MAX: u64 = 128 * 1024 * 1024;
/// The first read: EBML header, SeekHead, Info and Tracks fit in a few
/// KiB in almost every file, so one read usually covers them all.
const HEAD_READ_BYTES: u64 = 256 * 1024;
/// Info or Tracks larger than this are not a real file's.
const HEADER_ELEMENT_BYTES_MAX: u64 = 4 * 1024 * 1024;
/// Cues for a three-hour film with a keyframe every second take ~300 KB.
const CUES_BYTES_MAX: u64 = 32 * 1024 * 1024;
/// Read from the first cluster to find each AC-3 track's first frame.
const FIRST_FRAMES_READ_BYTES: u64 = 2 * 1024 * 1024;
/// Most tracks of each kind offered; real files have a handful.
const RENDITIONS_MAX: usize = 32;
/// HLS video timestamps tick at 90 kHz, like MPEG-TS.
pub const VIDEO_TIMESCALE: u32 = 90_000;
/// Shown when a subtitle block gives no duration.
const SUBTITLE_DURATION_DEFAULT_NS: u64 = 3_000_000_000;
const NS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Debug)]
pub struct Title {
    pub info: Info,
    pub video: VideoOut,
    pub audio: Vec<AudioOut>,
    pub subtitles: Vec<SubtitleOut>,
    pub segments: Vec<Span>,
    /// Average bytes per second over the whole file, for `BANDWIDTH`.
    pub bytes_per_second: u64,
}

#[derive(Debug)]
pub struct VideoOut {
    pub track: Track,
    pub format: TrackFormat,
    pub codec_string: String,
    /// Dolby Vision, for players that understand it (`SUPPLEMENTAL-CODECS`).
    pub supplemental_codec: Option<String>,
    pub range: VideoRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoRange {
    Sdr,
    Pq,
    Hlg,
}

#[derive(Debug)]
pub struct AudioOut {
    pub track: Track,
    pub codec: AudioCodec,
    pub format: TrackFormat,
    pub codec_string: &'static str,
    pub frame_samples: u32,
}

#[derive(Debug)]
pub struct SubtitleOut {
    pub track: Track,
    pub codec: SubtitleCodec,
}

/// One HLS segment: keyframe to keyframe, and the bytes holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Presentation time of its first keyframe, in timestamp units.
    pub start_time: u64,
    pub duration_ns: u64,
    /// The keyframe's block: where the segment starts, in file offsets.
    /// A `relative` of 0 means the index didn't say: the segment starts
    /// at the first video keyframe in or after that cluster.
    pub start: Place,
    /// The next segment's start; `None` for the last.
    pub end: Option<Place>,
    /// File offset the read stops at: past the cluster header and blocks
    /// before `end` when `end` is inside a cluster.
    pub read_end: u64,
}

impl Span {
    /// File offset the read starts at: the start of `start`'s cluster,
    /// whose timestamp every block in it counts from.
    pub fn read_start(&self) -> u64 {
        self.start.cluster
    }
}

/// A segment's bytes and the frames found in them.
#[derive(Debug)]
pub struct Demuxed {
    pub bytes: Vec<u8>,
    pub frames: Vec<Frame>,
}

impl Title {
    /// Reads what HLS needs from the file: headers, tracks, the keyframe
    /// index and, for AC-3 tracks, one frame each.
    pub async fn open(source: &impl Source) -> Result<Self, RemuxError> {
        let head_bytes = source.read(0, HEAD_READ_BYTES).await?;
        let head = matroska::parse_head(&head_bytes)?;
        let info_offset = head
            .find(id::INFO)
            .ok_or(ParseError::Invalid("no Info element"))?;
        let info = matroska::parse_info(
            &read_element(source, &head_bytes, info_offset, HEADER_ELEMENT_BYTES_MAX).await?,
        )?;
        let tracks_offset = head
            .find(id::TRACKS)
            .ok_or(ParseError::Invalid("no Tracks element"))?;
        let tracks = matroska::parse_tracks(
            &read_element(source, &head_bytes, tracks_offset, HEADER_ELEMENT_BYTES_MAX).await?,
        )?;
        let cues_offset = head.find(id::CUES).ok_or_else(|| {
            RemuxError::Unsupported(String::from("the file has no keyframe index"))
        })?;
        let cues_payload = read_element(source, &head_bytes, cues_offset, CUES_BYTES_MAX).await?;
        let cues = matroska::parse_cues(&cues_payload, head.segment_offset)?;
        let file_end = head
            .segment_end
            .or(source.length())
            .ok_or(ParseError::Invalid("file of unknown length"))?;
        let (video, video_track) = choose_video(&tracks)?;
        let segments = plan_segments(
            &cues,
            video_track.number,
            &info,
            &head,
            cues_offset,
            file_end,
        )?;
        let first_frames = source
            .read(segments[0].read_start(), FIRST_FRAMES_READ_BYTES)
            .await?;
        let audio = choose_audio(&tracks, &first_frames)?;
        let subtitles = tracks
            .iter()
            .filter(|track| {
                track.kind == TrackKind::Subtitle && track.encoding != Encoding::Unsupported
            })
            .filter_map(|track| {
                SubtitleCodec::from_matroska(&track.codec_id).map(|codec| SubtitleOut {
                    track: track.clone(),
                    codec,
                })
            })
            .take(RENDITIONS_MAX)
            .collect();
        let duration_seconds = (info.duration_ns().unwrap_or(1) / NS_PER_SECOND).max(1);
        Ok(Self {
            info,
            video,
            audio,
            subtitles,
            segments,
            bytes_per_second: file_end / duration_seconds,
        })
    }

    /// The frames of segment `index`, from its bytes: those stored from
    /// its keyframe up to the next segment's. Muxers store audio up to a
    /// few hundred milliseconds after the keyframe that follows it, and
    /// some put keyframes mid-cluster, so splitting by storage, not by
    /// time or by cluster, gives every frame to exactly one segment.
    pub fn demux(&self, index: usize, bytes: Vec<u8>) -> Result<Demuxed, RemuxError> {
        let span = &self.segments[index];
        let mut frames = matroska::parse_clusters(&bytes)?;
        for frame in &mut frames {
            frame.place.cluster += span.read_start();
        }
        let video = self.video.track.number;
        let start = keyframe_place(span.start, &frames, video).unwrap_or(span.start);
        // An end not found was beyond the read: everything read is ours.
        let end = span.end.and_then(|end| keyframe_place(end, &frames, video));
        frames.retain(|frame| frame.place >= start && end.is_none_or(|end| frame.place < end));
        Ok(Demuxed { bytes, frames })
    }

    /// The video samples of segment `index` as one fMP4 fragment.
    pub fn video_fragment(&self, index: usize, demuxed: &Demuxed) -> Result<Vec<u8>, RemuxError> {
        let (frames, skipped) = select_video(&demuxed.frames, self.video.track.number);
        if skipped > 0 {
            // The index pointed into a cluster instead of at its start;
            // the frames before the keyframe can't be decoded here.
            tracing::warn!(
                index,
                skipped,
                "video frames before the segment's keyframe dropped"
            );
        }
        if frames.is_empty() {
            return Err(RemuxError::Parse(ParseError::Invalid(
                "segment without video frames",
            )));
        }
        let scale_ns = self.info.timestamp_scale_ns;
        let presentation: Vec<i64> = frames
            .iter()
            .map(|frame| ticks(frame.time, scale_ns, VIDEO_TIMESCALE))
            .collect();
        // Blocks are in decode order and carry presentation times; the
        // sorted presentation times are a valid decode timeline.
        let mut decode = presentation.clone();
        decode.sort_unstable();
        let last_duration = self
            .video
            .track
            .default_duration_ns
            .map(|ns| ns_to_ticks(ns, VIDEO_TIMESCALE))
            .filter(|ticks| *ticks > 0);
        let prefix = stripped_prefix(&self.video.track);
        let mut samples = Vec::with_capacity(frames.len());
        for (position, frame) in frames.iter().enumerate() {
            let duration = match decode.get(position + 1) {
                Some(next) => next - decode[position],
                None => last_duration.unwrap_or_else(|| {
                    samples
                        .last()
                        .map_or(1, |sample: &Sample| i64::from(sample.duration))
                }),
            };
            samples.push(Sample {
                duration: u32::try_from(duration.max(0))
                    .map_err(|_| ParseError::Invalid("frame longer than a day"))?,
                size: (frame.length + prefix.len()) as u32,
                is_sync: frame.is_keyframe,
                composition_offset: i32::try_from(presentation[position] - decode[position])
                    .map_err(|_| ParseError::Invalid("frame reordered by hours"))?,
            });
        }
        let base =
            u64::try_from(decode[0]).map_err(|_| ParseError::Invalid("video before time zero"))?;
        Ok(mp4::fragment(sequence(index), base, &samples, |out| {
            copy_frames(out, &demuxed.bytes, &frames, prefix)
        }))
    }

    /// The samples of audio track `audio` in segment `index`: every frame
    /// in the segment's bytes, not those within its times, because muxers
    /// store audio up to a few hundred milliseconds after the keyframe that
    /// starts the next segment. Frames get the codec's exact length rather
    /// than millisecond time differences, which would put gaps between them.
    pub fn audio_fragment(
        &self,
        audio: usize,
        index: usize,
        demuxed: &Demuxed,
    ) -> Result<Vec<u8>, RemuxError> {
        let out = &self.audio[audio];
        let frame_ns =
            u64::from(out.frame_samples) * NS_PER_SECOND / u64::from(out.format.timescale);
        let frames: Vec<Frame> = demuxed
            .frames
            .iter()
            .filter(|frame| frame.track == out.track.number)
            .copied()
            .collect();
        let Some(first) = frames.first() else {
            return Err(RemuxError::Parse(ParseError::Invalid(
                "segment without audio frames",
            )));
        };
        let first_ns = first.time * self.info.timestamp_scale_ns as i64
            + i64::from(first.lace_index) * frame_ns as i64;
        let base = u64::try_from(ns_to_ticks(first_ns.max(0) as u64, out.format.timescale))
            .expect("ticks of a positive time are positive");
        let prefix = stripped_prefix(&out.track);
        let samples: Vec<Sample> = frames
            .iter()
            .map(|frame| Sample {
                duration: out.frame_samples,
                size: (frame.length + prefix.len()) as u32,
                is_sync: true,
                composition_offset: 0,
            })
            .collect();
        Ok(mp4::fragment(sequence(index), base, &samples, |out| {
            copy_frames(out, &demuxed.bytes, &frames, prefix)
        }))
    }

    /// Subtitle track `subtitle` in the segment `demuxed`, as a WebVTT
    /// document. Like audio, cues go by where they are stored.
    pub fn subtitle_document(&self, subtitle: usize, demuxed: &Demuxed) -> String {
        let out = &self.subtitles[subtitle];
        let scale_ns = self.info.timestamp_scale_ns;
        // Cue times are media times; MPEGTS 0 maps them one to one.
        let mut document = String::from("WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:0,LOCAL:00:00:00.000\n\n");
        for frame in demuxed
            .frames
            .iter()
            .filter(|frame| frame.track == out.track.number)
        {
            let start_ns = frame.time * scale_ns as i64;
            if start_ns < 0 {
                continue;
            }
            let raw = &demuxed.bytes[frame.start..frame.start + frame.length];
            let text = crate::subtitles::to_webvtt_text(raw, out.codec);
            if text.is_empty() {
                continue;
            }
            let duration_ns = frame
                .duration
                .map_or(SUBTITLE_DURATION_DEFAULT_NS, |duration| duration * scale_ns);
            document.push_str(&format!(
                "{} --> {}\n{}\n\n",
                crate::subtitles::timestamp(start_ns as u64),
                crate::subtitles::timestamp(start_ns as u64 + duration_ns),
                text
            ));
        }
        document
    }
}

/// HLS media sequence numbers start at 1 for fragments, as `mfhd` wants.
fn sequence(index: usize) -> u32 {
    u32::try_from(index + 1).expect("titles have far fewer than 4 billion segments")
}

/// Video frames of a segment in decode order, from its first keyframe,
/// and how many came before it.
fn select_video(frames: &[Frame], track: u64) -> (Vec<Frame>, usize) {
    let video: Vec<Frame> = frames
        .iter()
        .filter(|frame| frame.track == track)
        .copied()
        .collect();
    let skipped = video
        .iter()
        .position(|frame| frame.is_keyframe)
        .unwrap_or(video.len());
    (video[skipped..].to_vec(), skipped)
}

fn copy_frames(out: &mut Vec<u8>, bytes: &[u8], frames: &[Frame], prefix: &[u8]) {
    for frame in frames {
        out.extend_from_slice(prefix);
        out.extend_from_slice(&bytes[frame.start..frame.start + frame.length]);
    }
}

fn stripped_prefix(track: &Track) -> &[u8] {
    match &track.encoding {
        Encoding::StrippedHeader(prefix) => prefix,
        Encoding::Plain => &[],
        Encoding::Unsupported => unreachable!("tracks with other encodings are refused at open"),
    }
}

/// A Matroska time in `timescale` ticks.
fn ticks(time: i64, scale_ns: u64, timescale: u32) -> i64 {
    let ns = i128::from(time) * i128::from(scale_ns);
    (ns * i128::from(timescale) / i128::from(NS_PER_SECOND)) as i64
}

fn ns_to_ticks(ns: u64, timescale: u32) -> i64 {
    (u128::from(ns) * u128::from(timescale) / u128::from(NS_PER_SECOND)) as i64
}

/// Reads the payload of the top-level element at `offset`, from the head
/// bytes when it is there already.
async fn read_element(
    source: &impl Source,
    head: &[u8],
    offset: u64,
    bytes_max: u64,
) -> Result<Vec<u8>, RemuxError> {
    let start = usize::try_from(offset).unwrap_or(usize::MAX);
    let local = if start < head.len() {
        head[start..].to_vec()
    } else {
        source
            .read(offset, HEAD_READ_BYTES.min(bytes_max + 16))
            .await?
    };
    let mut cursor = crate::ebml::Cursor::new(&local);
    let header = cursor.header()?;
    let size = header
        .size
        .ok_or(ParseError::Invalid("header element of unknown size"))?;
    if size > bytes_max {
        return Err(ParseError::Invalid("header element larger than any real file's").into());
    }
    let header_bytes = header.header_bytes as u64;
    let total = header_bytes + size;
    if total as usize <= local.len() {
        return Ok(local[header.header_bytes..total as usize].to_vec());
    }
    let payload = source.read(offset + header_bytes, size).await?;
    if (payload.len() as u64) < size {
        return Err(ParseError::Truncated {
            needed_bytes: size - payload.len() as u64,
        }
        .into());
    }
    Ok(payload)
}

fn choose_video(tracks: &[Track]) -> Result<(VideoOut, &Track), RemuxError> {
    let track = tracks
        .iter()
        .find(|track| track.kind == TrackKind::Video && track.is_enabled)
        .ok_or_else(|| RemuxError::Unsupported(String::from("no video track")))?;
    let codec = VideoCodec::from_matroska(&track.codec_id)
        .ok_or_else(|| RemuxError::Unsupported(format!("video codec {}", track.codec_id)))?;
    if track.encoding == Encoding::Unsupported {
        return Err(RemuxError::Unsupported(String::from(
            "compressed video track",
        )));
    }
    let dolby_vision = track.dolby_vision.as_ref().and_then(|(kind, record)| {
        codec::parse_dolby_vision(record).map(|config| (*kind, record, config))
    });
    let (entry, codec_string, supplemental_codec, mut range) = match codec {
        VideoCodec::H264 => (
            SampleEntry::Avc {
                avcc: track.codec_private.clone(),
            },
            codec::avc_codec_string(&track.codec_private).map_err(RemuxError::Unsupported)?,
            None,
            VideoRange::Sdr,
        ),
        VideoCodec::Hevc => hevc_entry(track, dolby_vision)?,
    };
    if range == VideoRange::Sdr {
        // The bitstream is what the decoder obeys; container tags are
        // often missing, and sometimes wrong.
        let bitstream = match codec {
            VideoCodec::Hevc => crate::hevc::transfer_characteristics(&track.codec_private),
            VideoCodec::H264 => None,
        };
        range = match bitstream.or(track.transfer) {
            Some(16) => VideoRange::Pq,
            Some(18) => VideoRange::Hlg,
            _ => VideoRange::Sdr,
        };
    }
    let format = TrackFormat {
        entry,
        timescale: VIDEO_TIMESCALE,
        width: u16::try_from(track.width)
            .map_err(|_| RemuxError::Unsupported(String::from("video wider than 65535 pixels")))?,
        height: u16::try_from(track.height)
            .map_err(|_| RemuxError::Unsupported(String::from("video taller than 65535 pixels")))?,
        channels: 0,
        sample_rate: 0,
    };
    Ok((
        VideoOut {
            track: track.clone(),
            format,
            codec_string,
            supplemental_codec,
            range,
        },
        track,
    ))
}

type HevcEntry = (SampleEntry, String, Option<String>, VideoRange);

/// Dolby Vision profile 5 needs the `dvh1` entry: its base layer is not
/// viewable alone. Profiles 8 and up keep `hvc1` with the configuration
/// attached, so every player shows the HDR10 or HLG base. Profile 7 (two
/// layers, from discs) plays as its HDR10 base layer.
fn hevc_entry(
    track: &Track,
    dolby_vision: Option<(u32, &Vec<u8>, codec::DolbyVision)>,
) -> Result<HevcEntry, RemuxError> {
    let hvcc = track.codec_private.clone();
    let hevc = codec::hevc_codec_string(&hvcc, "hvc1").map_err(RemuxError::Unsupported)?;
    let Some((kind, record, config)) = dolby_vision else {
        return Ok((
            SampleEntry::Hevc {
                hvcc,
                dolby_vision: None,
                is_dolby_vision_only: false,
            },
            hevc,
            None,
            VideoRange::Sdr,
        ));
    };
    let config_box = Some(DolbyVisionBox {
        kind: kind.to_be_bytes(),
        record: record.clone(),
    });
    let dolby_string = format!("dvh1.{:02}.{:02}", config.profile, config.level);
    match (config.profile, config.compatibility) {
        (5, _) => Ok((
            SampleEntry::Hevc {
                hvcc,
                dolby_vision: config_box,
                is_dolby_vision_only: true,
            },
            dolby_string,
            None,
            VideoRange::Pq,
        )),
        (8, 1) => Ok((
            SampleEntry::Hevc {
                hvcc,
                dolby_vision: config_box,
                is_dolby_vision_only: false,
            },
            hevc,
            Some(format!("{dolby_string}/db1p")),
            VideoRange::Pq,
        )),
        (8, 4) => Ok((
            SampleEntry::Hevc {
                hvcc,
                dolby_vision: config_box,
                is_dolby_vision_only: false,
            },
            hevc,
            Some(format!("{dolby_string}/db4h")),
            VideoRange::Hlg,
        )),
        _ => Ok((
            SampleEntry::Hevc {
                hvcc,
                dolby_vision: None,
                is_dolby_vision_only: false,
            },
            hevc,
            None,
            VideoRange::Sdr,
        )),
    }
}

/// Every audio track AVFoundation can play, default first. AC-3 and
/// E-AC-3 decoder boxes come from a real frame, found in `first_frames`.
fn choose_audio(tracks: &[Track], first_frames: &[u8]) -> Result<Vec<AudioOut>, RemuxError> {
    let frames = matroska::parse_clusters(first_frames)?;
    let mut audio = Vec::new();
    let mut refused = Vec::new();
    for track in tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Audio && track.is_enabled)
    {
        let Some(codec) = AudioCodec::from_matroska(&track.codec_id)
            .filter(|_| track.encoding != Encoding::Unsupported)
        else {
            refused.push(track.codec_id.clone());
            continue;
        };
        let frame = frames
            .iter()
            .find(|frame| frame.track == track.number)
            .map(|frame| {
                let mut bytes = stripped_prefix(track).to_vec();
                bytes.extend_from_slice(&first_frames[frame.start..frame.start + frame.length]);
                bytes
            });
        match audio_out(track, codec, frame.as_deref()) {
            Ok(out) => audio.push(out),
            Err(reason) => refused.push(reason),
        }
        if audio.len() == RENDITIONS_MAX {
            break;
        }
    }
    if audio.is_empty() {
        return Err(RemuxError::Unsupported(format!(
            "audio codecs {}",
            refused.join(", ")
        )));
    }
    // A stable sort keeps file order among equals.
    audio.sort_by_key(|out| !out.track.is_default);
    Ok(audio)
}

fn audio_out(track: &Track, codec: AudioCodec, frame: Option<&[u8]>) -> Result<AudioOut, String> {
    let channels =
        u16::try_from(track.channels).map_err(|_| String::from("audio with too many channels"))?;
    let (entry, codec_string, frame_samples, sample_rate) = match codec {
        AudioCodec::Aac => {
            let config = codec::aac_config(track)?;
            let codec_string = match codec::aac_object_type(&config) {
                5 => "mp4a.40.5",
                29 => "mp4a.40.29",
                _ => "mp4a.40.2",
            };
            (
                SampleEntry::Aac { config },
                codec_string,
                codec::AAC_FRAME_SAMPLES,
                track.sample_rate.round() as u32,
            )
        }
        AudioCodec::Ac3 | AudioCodec::Eac3 => {
            let frame = frame
                .ok_or_else(|| format!("{} track with no frame near the start", track.codec_id))?;
            let header = codec::parse_ac3_header(frame, codec)?;
            if codec == AudioCodec::Ac3 {
                (
                    SampleEntry::Ac3 {
                        dac3: codec::dac3(&header),
                    },
                    "ac-3",
                    header.frame_samples,
                    header.sample_rate,
                )
            } else {
                (
                    SampleEntry::Eac3 {
                        dec3: codec::dec3(&header),
                    },
                    "ec-3",
                    header.frame_samples,
                    header.sample_rate,
                )
            }
        }
    };
    if sample_rate == 0 {
        return Err(String::from("audio with a sample rate of zero"));
    }
    let format = TrackFormat {
        entry,
        timescale: sample_rate,
        width: 0,
        height: 0,
        channels,
        sample_rate,
    };
    Ok(AudioOut {
        track: track.clone(),
        codec,
        format,
        codec_string,
        frame_samples,
    })
}

/// Cuts the title at keyframes, about every [`SEGMENT_DURATION_TARGET_NS`].
fn plan_segments(
    cues: &[CuePoint],
    video_track: u64,
    info: &Info,
    head: &Head,
    cues_offset: u64,
    file_end: u64,
) -> Result<Vec<Span>, RemuxError> {
    let mut points: Vec<&CuePoint> = cues.iter().filter(|cue| cue.track == video_track).collect();
    if points.is_empty() {
        return Err(RemuxError::Unsupported(String::from(
            "the keyframe index lists no video keyframes",
        )));
    }
    points.dedup_by_key(|cue| place(cue));
    let scale_ns = info.timestamp_scale_ns;
    let target = SEGMENT_DURATION_TARGET_NS / scale_ns;
    let mut boundaries = vec![points[0]];
    for cue in &points[1..] {
        let last = boundaries.last().expect("starts with one boundary");
        if cue.time >= last.time + target && place(cue) > place(last) {
            boundaries.push(cue);
        }
    }
    let last_cluster = boundaries
        .last()
        .expect("at least one boundary")
        .cluster_offset;
    // Clusters end where Cues, Tags or the file begin.
    let clusters_end = [cues_offset, file_end]
        .into_iter()
        .chain(head.elements.iter().map(|element| element.offset))
        .filter(|offset| *offset > last_cluster)
        .min()
        .unwrap_or(file_end);
    let total_ns = info.duration_ns();
    let mut segments = Vec::with_capacity(boundaries.len());
    for (index, cue) in boundaries.iter().enumerate() {
        let next = boundaries.get(index + 1);
        let read_end = match next {
            // The index doesn't say where in its cluster the next keyframe
            // is: read on until it is likely found.
            Some(next) if next.relative == 0 => {
                (next.cluster_offset + KEYFRAME_LOOKAHEAD_BYTES).min(clusters_end)
            }
            // Mid-cluster: read the blocks before it, which are ours.
            Some(next) => next.cluster_offset + CLUSTER_HEADER_BYTES_MAX + next.relative,
            None => clusters_end,
        };
        let duration_ns = match next {
            Some(next) => (next.time - cue.time) * scale_ns,
            None => total_ns.map_or(SEGMENT_DURATION_TARGET_NS, |total| {
                total.saturating_sub(cue.time * scale_ns).max(1)
            }),
        };
        if read_end.saturating_sub(cue.cluster_offset) > SEGMENT_BYTES_MAX {
            return Err(RemuxError::Unsupported(String::from(
                "a keyframe interval larger than any segment we read",
            )));
        }
        segments.push(Span {
            start_time: cue.time,
            duration_ns,
            start: place(cue),
            end: next.map(|next| place(next)),
            read_end,
        });
    }
    assert!(
        segments
            .windows(2)
            .all(|pair| pair[0].end == Some(pair[1].start)),
        "segments tile the file"
    );
    Ok(segments)
}

/// Widest cluster header: a 4-byte id and an 8-byte size.
const CLUSTER_HEADER_BYTES_MAX: u64 = 12;
/// Read past the next segment's cluster start when the index gives no
/// block position. Muxers that omit it cut clusters by time, leaving a
/// few frames before the keyframe: 7 KB on a 1080p title seen
/// 2026-11-27. 256 KiB is a dozen 1080p frames, 5 % of a 6 s segment.
const KEYFRAME_LOOKAHEAD_BYTES: u64 = 256 * 1024;

/// Where a segment boundary is in `frames`: the boundary itself when the
/// index gave the block, else the first video keyframe from its cluster
/// on. `None` when that keyframe is not in `frames`.
fn keyframe_place(boundary: Place, frames: &[Frame], video: u64) -> Option<Place> {
    if boundary.relative != 0 {
        return Some(boundary);
    }
    frames
        .iter()
        .find(|frame| frame.track == video && frame.is_keyframe && frame.place >= boundary)
        .map(|frame| frame.place)
}

fn place(cue: &CuePoint) -> Place {
    Place {
        cluster: cue.cluster_offset,
        relative: cue.relative,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(time: u64, cluster_offset: u64) -> CuePoint {
        CuePoint {
            time,
            track: 1,
            cluster_offset,
            relative: 0,
        }
    }

    fn info() -> Info {
        Info {
            timestamp_scale_ns: 1_000_000,
            duration: Some(20_000.0),
        }
    }

    #[test]
    fn segments_merge_keyframes_up_to_the_target() {
        let cues = [
            cue(0, 100),
            cue(2000, 200),
            cue(4000, 300),
            cue(6000, 400),
            cue(9000, 500),
            cue(12500, 600),
            cue(14000, 700),
        ];
        let segments = plan_segments(&cues, 1, &info(), &Head::default(), 1000, 2000).unwrap();
        let starts: Vec<u64> = segments.iter().map(|span| span.start_time).collect();
        assert_eq!(starts, [0, 6000, 12500]);
        // No block positions in this index: the read looks past the next
        // cluster's start for its keyframe, up to the Cues here.
        assert_eq!(
            (segments[0].read_start(), segments[0].read_end),
            (100, 1000)
        );
        assert_eq!(segments[0].end, Some(segments[1].start));
        assert_eq!(segments[0].duration_ns, 6_000_000_000);
        // The last runs to the Cues and lasts until the title's end.
        assert_eq!((segments[2].end, segments[2].read_end), (None, 1000));
        assert_eq!(segments[2].duration_ns, 7_500_000_000);
    }

    #[test]
    fn a_keyframe_inside_a_cluster_ends_the_read_past_it() {
        let mut next = cue(6000, 400);
        next.relative = 50;
        let segments = plan_segments(
            &[cue(0, 100), next],
            1,
            &info(),
            &Head::default(),
            1000,
            2000,
        )
        .unwrap();
        // The cluster header and the 50 bytes of blocks before the keyframe.
        assert_eq!(segments[0].read_end, 400 + 12 + 50);
        assert_eq!(
            segments[1].start,
            Place {
                cluster: 400,
                relative: 50
            }
        );
        assert_eq!(segments[1].read_start(), 400);
    }

    #[test]
    fn refuses_an_index_without_video_cues() {
        let cues = [CuePoint {
            time: 0,
            track: 2,
            cluster_offset: 100,
            relative: 0,
        }];
        let error = plan_segments(&cues, 1, &info(), &Head::default(), 1000, 2000).unwrap_err();
        assert!(matches!(error, RemuxError::Unsupported(_)));
    }

    #[test]
    fn refuses_a_segment_beyond_the_read_limit() {
        let cues = [cue(0, 100), cue(7000, 100 + SEGMENT_BYTES_MAX + 1)];
        assert!(plan_segments(&cues, 1, &info(), &Head::default(), u64::MAX, u64::MAX).is_err());
    }

    fn frame(time: i64, is_keyframe: bool) -> Frame {
        Frame {
            track: 1,
            time,
            is_keyframe,
            start: 0,
            length: 0,
            duration: None,
            lace_index: 0,
            place: Place {
                cluster: 0,
                relative: 0,
            },
        }
    }

    #[test]
    fn a_boundary_without_a_block_position_is_its_clusters_first_keyframe() {
        let at = |cluster, relative, is_keyframe| Frame {
            place: Place { cluster, relative },
            ..frame(0, is_keyframe)
        };
        // Two frames of the previous GOP open the cluster at 500.
        let frames = [
            at(100, 20, true),
            at(500, 10, false),
            at(500, 90, false),
            at(500, 200, true),
            at(900, 10, true),
        ];
        let boundary = Place {
            cluster: 500,
            relative: 0,
        };
        assert_eq!(
            keyframe_place(boundary, &frames, 1),
            Some(Place {
                cluster: 500,
                relative: 200
            })
        );
        // Not read far enough to see it.
        assert_eq!(keyframe_place(boundary, &frames[..3], 1), None);
        // The index's own position wins.
        let indexed = Place {
            cluster: 500,
            relative: 90,
        };
        assert_eq!(keyframe_place(indexed, &frames, 1), Some(indexed));
    }

    #[test]
    fn video_starts_at_the_first_keyframe() {
        let frames = [
            frame(960, false),
            frame(1000, true),
            frame(1080, false),
            frame(1040, false),
            frame(2000, true),
        ];
        let (selected, skipped) = select_video(&frames, 1);
        let times: Vec<i64> = selected.iter().map(|frame| frame.time).collect();
        assert_eq!(times, [1000, 1080, 1040, 2000]);
        assert_eq!(skipped, 1);
        assert_eq!(select_video(&frames[..1], 1), (Vec::new(), 1));
    }
}
