//! Fragmented MP4 (ISO/IEC 14496-12), as HLS uses it: an init segment
//! (`ftyp` + `moov`) per track, then media segments (`moof` + `mdat`).
//! Only what AVFoundation needs to play one track per playlist.

/// How a track's samples are described to the decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SampleEntry {
    Avc {
        avcc: Vec<u8>,
    },
    /// `dvh1` carries Dolby Vision profile 5, whose picture is meaningless
    /// without it; `hvc1` (with an optional configuration box) everything
    /// else, so players without Dolby Vision show the HDR10 or SDR base.
    Hevc {
        hvcc: Vec<u8>,
        dolby_vision: Option<DolbyVisionBox>,
        is_dolby_vision_only: bool,
    },
    Aac {
        config: Vec<u8>,
    },
    Ac3 {
        dac3: Vec<u8>,
    },
    Eac3 {
        dec3: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DolbyVisionBox {
    /// `dvcC` or `dvvC`.
    pub kind: [u8; 4],
    pub record: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackFormat {
    pub entry: SampleEntry,
    pub timescale: u32,
    pub width: u16,
    pub height: u16,
    pub channels: u16,
    pub sample_rate: u32,
}

impl TrackFormat {
    fn is_video(&self) -> bool {
        matches!(
            self.entry,
            SampleEntry::Avc { .. } | SampleEntry::Hevc { .. }
        )
    }
}

/// One sample of a fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    /// In the track's timescale.
    pub duration: u32,
    pub size: u32,
    pub is_sync: bool,
    /// Presentation minus decode time, in the timescale. Negative when a
    /// frame shows before its decode-order slot would suggest.
    pub composition_offset: i32,
}

/// The only track in each init segment.
const TRACK_ID: u32 = 1;
/// Movie timescale; only `mvhd` uses it, and its duration is zero.
const MOVIE_TIMESCALE: u32 = 1000;
/// `sample_depends_on` = 2 (an I-frame).
const SAMPLE_FLAGS_SYNC: u32 = 0x0200_0000;
/// `sample_depends_on` = 1 and `sample_is_non_sync_sample`.
const SAMPLE_FLAGS_NON_SYNC: u32 = 0x0101_0000;
/// The identity matrix of `mvhd` and `tkhd`, in 16.16 and 2.30 fixed point.
const MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

/// `ftyp` and `moov` for one track.
pub fn init_segment(format: &TrackFormat) -> Vec<u8> {
    let mut writer = Writer::default();
    writer.open(b"ftyp");
    writer.bytes(b"iso6");
    writer.u32(0);
    for brand in [b"iso6", b"cmfc", b"mp41"] {
        writer.bytes(brand);
    }
    writer.close();
    writer.open(b"moov");
    write_mvhd(&mut writer);
    write_trak(&mut writer, format);
    writer.open(b"mvex");
    writer.open_full(b"trex", 0, 0);
    writer.u32(TRACK_ID);
    writer.u32(1); // default_sample_description_index
    writer.u32(0);
    writer.u32(0);
    writer.u32(0);
    writer.close();
    writer.close();
    writer.close();
    writer.finish()
}

/// `moof` and `mdat` for samples starting at `base_decode_time`. `fill`
/// appends the samples' bytes, in order; it must write exactly the sum of
/// their sizes.
pub fn fragment(
    sequence: u32,
    base_decode_time: u64,
    samples: &[Sample],
    fill: impl FnOnce(&mut Vec<u8>),
) -> Vec<u8> {
    assert!(!samples.is_empty(), "a fragment holds at least one sample");
    let has_offsets = samples.iter().any(|sample| sample.composition_offset != 0);
    let mut writer = Writer::default();
    writer.open(b"moof");
    writer.open_full(b"mfhd", 0, 0);
    writer.u32(sequence);
    writer.close();
    writer.open(b"traf");
    // default-base-is-moof: data offsets count from the start of `moof`.
    writer.open_full(b"tfhd", 0, 0x02_0000);
    writer.u32(TRACK_ID);
    writer.close();
    writer.open_full(b"tfdt", 1, 0);
    writer.u64(base_decode_time);
    writer.close();
    // data offset, duration, size and flags per sample, plus offsets if any.
    let flags = 0x0001 | 0x0100 | 0x0200 | 0x0400 | if has_offsets { 0x0800 } else { 0 };
    // Version 1 makes composition offsets signed.
    writer.open_full(b"trun", 1, flags);
    writer.u32(samples.len() as u32);
    let data_offset_at = writer.buffer.len();
    writer.u32(0);
    for sample in samples {
        writer.u32(sample.duration);
        writer.u32(sample.size);
        writer.u32(if sample.is_sync {
            SAMPLE_FLAGS_SYNC
        } else {
            SAMPLE_FLAGS_NON_SYNC
        });
        if has_offsets {
            writer.bytes(&sample.composition_offset.to_be_bytes());
        }
    }
    writer.close();
    writer.close();
    writer.close();
    let moof_bytes = writer.buffer.len();
    let payload_bytes: u64 = samples.iter().map(|sample| u64::from(sample.size)).sum();
    let mdat_bytes = u32::try_from(payload_bytes + 8).expect("segments are capped far below 4 GiB");
    let data_offset = moof_bytes as u32 + 8;
    writer.buffer[data_offset_at..data_offset_at + 4].copy_from_slice(&data_offset.to_be_bytes());
    let mut buffer = writer.finish();
    buffer.reserve(mdat_bytes as usize);
    buffer.extend_from_slice(&mdat_bytes.to_be_bytes());
    buffer.extend_from_slice(b"mdat");
    let before = buffer.len();
    fill(&mut buffer);
    assert_eq!(
        (buffer.len() - before) as u64,
        payload_bytes,
        "fill wrote what the sample sizes promise"
    );
    buffer
}

fn write_mvhd(writer: &mut Writer) {
    writer.open_full(b"mvhd", 0, 0);
    writer.u32(0); // creation_time
    writer.u32(0); // modification_time
    writer.u32(MOVIE_TIMESCALE);
    writer.u32(0); // duration: fragments carry it
    writer.u32(0x0001_0000); // rate 1.0
    writer.u16(0x0100); // volume 1.0
    writer.zeros(10);
    for value in MATRIX {
        writer.u32(value);
    }
    writer.zeros(24); // pre_defined
    writer.u32(TRACK_ID + 1); // next_track_ID
    writer.close();
}

fn write_trak(writer: &mut Writer, format: &TrackFormat) {
    writer.open(b"trak");
    // Flags: enabled, in movie, in preview.
    writer.open_full(b"tkhd", 0, 0x7);
    writer.u32(0);
    writer.u32(0);
    writer.u32(TRACK_ID);
    writer.u32(0);
    writer.u32(0); // duration
    writer.zeros(8);
    writer.u16(0); // layer
    writer.u16(0); // alternate_group
    writer.u16(if format.is_video() { 0 } else { 0x0100 });
    writer.u16(0);
    for value in MATRIX {
        writer.u32(value);
    }
    writer.u32(u32::from(format.width) << 16);
    writer.u32(u32::from(format.height) << 16);
    writer.close();
    writer.open(b"mdia");
    writer.open_full(b"mdhd", 0, 0);
    writer.u32(0);
    writer.u32(0);
    writer.u32(format.timescale);
    writer.u32(0);
    // "und" packed as three 5-bit letters; HLS names the language.
    writer.u16(0x55C4);
    writer.u16(0);
    writer.close();
    writer.open_full(b"hdlr", 0, 0);
    writer.u32(0);
    writer.bytes(if format.is_video() { b"vide" } else { b"soun" });
    writer.zeros(12);
    writer.bytes(b"televisor\0");
    writer.close();
    writer.open(b"minf");
    if format.is_video() {
        writer.open_full(b"vmhd", 0, 1);
        writer.zeros(8);
    } else {
        writer.open_full(b"smhd", 0, 0);
        writer.zeros(4);
    }
    writer.close();
    writer.open(b"dinf");
    writer.open_full(b"dref", 0, 0);
    writer.u32(1);
    // Flag 1: the media is in this file.
    writer.open_full(b"url ", 0, 1);
    writer.close();
    writer.close();
    writer.close();
    write_stbl(writer, format);
    writer.close();
    writer.close();
    writer.close();
}

fn write_stbl(writer: &mut Writer, format: &TrackFormat) {
    writer.open(b"stbl");
    writer.open_full(b"stsd", 0, 0);
    writer.u32(1);
    write_sample_entry(writer, format);
    writer.close();
    // Empty tables: every sample lives in fragments.
    for kind in [b"stts", b"stsc", b"stco"] {
        writer.open_full(kind, 0, 0);
        writer.u32(0);
        writer.close();
    }
    writer.open_full(b"stsz", 0, 0);
    writer.u32(0);
    writer.u32(0);
    writer.close();
    writer.close();
}

fn write_sample_entry(writer: &mut Writer, format: &TrackFormat) {
    match &format.entry {
        SampleEntry::Avc { avcc } => {
            write_visual_entry(writer, b"avc1", format);
            writer.child(b"avcC", avcc);
        }
        SampleEntry::Hevc {
            hvcc,
            dolby_vision,
            is_dolby_vision_only,
        } => {
            write_visual_entry(
                writer,
                if *is_dolby_vision_only {
                    b"dvh1"
                } else {
                    b"hvc1"
                },
                format,
            );
            writer.child(b"hvcC", hvcc);
            if let Some(config) = dolby_vision {
                writer.child(&config.kind, &config.record);
            }
        }
        SampleEntry::Aac { config } => {
            write_audio_entry(writer, b"mp4a", format);
            write_esds(writer, config);
        }
        SampleEntry::Ac3 { dac3 } => {
            write_audio_entry(writer, b"ac-3", format);
            writer.child(b"dac3", dac3);
        }
        SampleEntry::Eac3 { dec3 } => {
            write_audio_entry(writer, b"ec-3", format);
            writer.child(b"dec3", dec3);
        }
    }
    writer.close();
}

/// Opens a VisualSampleEntry; the caller adds the codec box and closes.
fn write_visual_entry(writer: &mut Writer, kind: &[u8; 4], format: &TrackFormat) {
    writer.open(kind);
    writer.zeros(6);
    writer.u16(1); // data_reference_index
    writer.zeros(16);
    writer.u16(format.width);
    writer.u16(format.height);
    writer.u32(0x0048_0000); // 72 dpi
    writer.u32(0x0048_0000);
    writer.u32(0);
    writer.u16(1); // frame_count
    writer.zeros(32); // compressorname
    writer.u16(0x0018); // depth
    writer.u16(0xFFFF); // pre_defined = -1
}

/// Opens an AudioSampleEntry; the caller adds the codec box and closes.
fn write_audio_entry(writer: &mut Writer, kind: &[u8; 4], format: &TrackFormat) {
    writer.open(kind);
    writer.zeros(6);
    writer.u16(1);
    writer.zeros(8);
    writer.u16(format.channels);
    writer.u16(16); // samplesize
    writer.zeros(4);
    // 16.16 fixed point; rates above 65535 Hz don't occur in AAC or AC-3 here.
    writer.u32(format.sample_rate.min(0xFFFF) << 16);
}

/// `esds` for AAC (ISO/IEC 14496-1, 7.2.6.5).
fn write_esds(writer: &mut Writer, config: &[u8]) {
    let mut decoder_specific = vec![0x05];
    descriptor_length(&mut decoder_specific, config.len());
    decoder_specific.extend_from_slice(config);
    let mut decoder_config = vec![0x40, 0x15]; // MPEG-4 audio, audio stream
    decoder_config.extend_from_slice(&[0; 3 + 4 + 4]); // buffer size, max and average bit rate
    decoder_config.extend_from_slice(&decoder_specific);
    let mut es = vec![0, 0, 0]; // ES_ID, flags
    es.push(0x04);
    descriptor_length(&mut es, decoder_config.len());
    es.extend_from_slice(&decoder_config);
    es.extend_from_slice(&[0x06, 0x01, 0x02]); // SLConfigDescriptor, predefined MP4
    writer.open_full(b"esds", 0, 0);
    writer.u8(0x03);
    let mut length = Vec::new();
    descriptor_length(&mut length, es.len());
    writer.bytes(&length);
    writer.bytes(&es);
    writer.close();
}

/// Descriptor lengths are 7 bits per byte, high bit meaning "more".
fn descriptor_length(out: &mut Vec<u8>, length: usize) {
    assert!(length < 1 << 28, "descriptor lengths fit four bytes");
    let mut started = false;
    for shift in [21, 14, 7] {
        let part = ((length >> shift) & 0x7F) as u8;
        if part != 0 || started {
            out.push(part | 0x80);
            started = true;
        }
    }
    out.push((length & 0x7F) as u8);
}

/// Writes nested boxes, patching each size when it closes.
#[derive(Default)]
struct Writer {
    buffer: Vec<u8>,
    open: Vec<usize>,
}

impl Writer {
    fn open(&mut self, kind: &[u8; 4]) {
        self.open.push(self.buffer.len());
        self.u32(0);
        self.bytes(kind);
    }

    fn open_full(&mut self, kind: &[u8; 4], version: u8, flags: u32) {
        assert!(flags < 1 << 24, "full box flags are 24 bits");
        self.open(kind);
        self.u32((u32::from(version) << 24) | flags);
    }

    fn close(&mut self) {
        let start = self.open.pop().expect("close matches an open");
        let size = u32::try_from(self.buffer.len() - start).expect("boxes stay under 4 GiB");
        self.buffer[start..start + 4].copy_from_slice(&size.to_be_bytes());
    }

    fn child(&mut self, kind: &[u8; 4], payload: &[u8]) {
        self.open(kind);
        self.bytes(payload);
        self.close();
    }

    fn finish(self) -> Vec<u8> {
        assert!(self.open.is_empty(), "every box is closed");
        self.buffer
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    fn zeros(&mut self, count: usize) {
        self.buffer.resize(self.buffer.len() + count, 0);
    }

    fn u8(&mut self, value: u8) {
        self.buffer.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_be_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_be_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Top-level boxes as (type, size).
    fn boxes(data: &[u8]) -> Vec<(String, usize)> {
        let mut found = Vec::new();
        let mut position = 0;
        while position < data.len() {
            let size =
                u32::from_be_bytes(data[position..position + 4].try_into().unwrap()) as usize;
            found.push((
                String::from_utf8_lossy(&data[position + 4..position + 8]).into_owned(),
                size,
            ));
            assert!(size >= 8);
            position += size;
        }
        assert_eq!(position, data.len(), "boxes tile the buffer");
        found
    }

    #[test]
    fn init_segment_is_ftyp_then_moov() {
        let format = TrackFormat {
            entry: SampleEntry::Aac {
                config: vec![0x11, 0x90],
            },
            timescale: 48000,
            width: 0,
            height: 0,
            channels: 2,
            sample_rate: 48000,
        };
        let init = init_segment(&format);
        let kinds: Vec<String> = boxes(&init).into_iter().map(|(kind, _)| kind).collect();
        assert_eq!(kinds, ["ftyp", "moov"]);
        assert!(init.windows(4).any(|window| window == b"esds"));
    }

    #[test]
    fn fragment_points_trun_at_the_mdat_payload() {
        let samples = [
            Sample {
                duration: 3750,
                size: 3,
                is_sync: true,
                composition_offset: 0,
            },
            Sample {
                duration: 3750,
                size: 2,
                is_sync: false,
                composition_offset: -3750,
            },
        ];
        let fragment = fragment(7, 90_000, &samples, |out| out.extend_from_slice(b"abcde"));
        let found = boxes(&fragment);
        assert_eq!(found[0].0, "moof");
        assert_eq!(found[1], (String::from("mdat"), 8 + 5));
        let trun = fragment
            .windows(4)
            .position(|window| window == b"trun")
            .unwrap();
        // size, type, version and flags, sample count, then the data offset.
        let data_offset =
            u32::from_be_bytes(fragment[trun + 12..trun + 16].try_into().unwrap()) as usize;
        assert_eq!(&fragment[data_offset..], b"abcde");
    }

    #[test]
    #[should_panic(expected = "fill wrote what the sample sizes promise")]
    fn fragment_refuses_a_short_fill() {
        let samples = [Sample {
            duration: 1,
            size: 4,
            is_sync: true,
            composition_offset: 0,
        }];
        fragment(1, 0, &samples, |out| out.extend_from_slice(b"abc"));
    }

    #[test]
    fn descriptor_lengths_use_continuation_bits() {
        let mut out = Vec::new();
        descriptor_length(&mut out, 5);
        assert_eq!(out, [5]);
        out.clear();
        descriptor_length(&mut out, 200);
        assert_eq!(out, [0x81, 0x48]);
    }
}
