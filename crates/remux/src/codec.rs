//! The codecs AVFoundation plays from fragmented MP4, recognised from
//! Matroska codec ids, and the per-codec facts MP4 and HLS need: decoder
//! configuration boxes, frame lengths and `CODECS` strings.

use crate::bits::BitReader;
use crate::matroska::Track;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    Hevc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    Aac,
    Ac3,
    Eac3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleCodec {
    /// SubRip: plain text with `<i>`-style tags.
    Srt,
    /// SSA and ASS: text behind eight comma-separated fields, with
    /// `{\...}` style overrides.
    Ass,
}

impl VideoCodec {
    pub fn from_matroska(codec_id: &str) -> Option<Self> {
        match codec_id {
            "V_MPEG4/ISO/AVC" => Some(Self::H264),
            "V_MPEGH/ISO/HEVC" => Some(Self::Hevc),
            _ => None,
        }
    }
}

impl AudioCodec {
    pub fn from_matroska(codec_id: &str) -> Option<Self> {
        match codec_id {
            "A_AC3" => Some(Self::Ac3),
            "A_EAC3" => Some(Self::Eac3),
            // `A_AAC/MPEG4/LC` and friends predate CodecPrivate for AAC.
            id if id == "A_AAC" || id.starts_with("A_AAC/") => Some(Self::Aac),
            _ => None,
        }
    }
}

impl SubtitleCodec {
    pub fn from_matroska(codec_id: &str) -> Option<Self> {
        match codec_id {
            "S_TEXT/UTF8" => Some(Self::Srt),
            "S_TEXT/ASS" | "S_TEXT/SSA" => Some(Self::Ass),
            _ => None,
        }
    }
}

/// AAC sampling frequencies by index (ISO/IEC 14496-3, table 1.18).
const AAC_SAMPLE_RATES: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];
/// Samples per AAC frame (AAC-LC; HE-AAC reports its core rate, so the
/// same count holds in its own timescale).
pub const AAC_FRAME_SAMPLES: u32 = 1024;
/// Samples per AC-3 frame: six blocks of 256.
pub const AC3_FRAME_SAMPLES: u32 = 1536;
/// Bytes of an AC-3 or E-AC-3 header needed to read everything we use.
const AC3_HEADER_BYTES: usize = 8;

/// AudioSpecificConfig for an AAC track. Old files carry none, so build
/// an AAC-LC one from the track's rate and channel count.
pub fn aac_config(track: &Track) -> Result<Vec<u8>, String> {
    if track.codec_private.len() >= 2 {
        return Ok(track.codec_private.clone());
    }
    let rate = track.sample_rate.round() as u32;
    let index = AAC_SAMPLE_RATES
        .iter()
        .position(|candidate| *candidate == rate)
        .ok_or_else(|| format!("AAC at {rate} Hz"))? as u16;
    let channels = u16::try_from(track.channels)
        .ok()
        .filter(|channels| (1..=7).contains(channels))
        .ok_or_else(|| format!("AAC with {} channels", track.channels))?;
    // Object type 2 (LC), 5 bits; rate index, 4; channel config, 4; 3 zero bits.
    let config = (2u16 << 11) | (index << 7) | (channels << 3);
    Ok(config.to_be_bytes().to_vec())
}

/// The audio object type, for `mp4a.40.<type>`.
pub fn aac_object_type(config: &[u8]) -> u8 {
    assert!(!config.is_empty(), "AudioSpecificConfig is never empty");
    let object_type = config[0] >> 3;
    if object_type == 31 && config.len() >= 2 {
        // Escape: 32 plus the next 6 bits.
        32 + (((config[0] & 0b111) << 3) | (config[1] >> 5))
    } else {
        object_type
    }
}

/// What an AC-3 or E-AC-3 frame header says, for `dac3` and `dec3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ac3Header {
    pub fscod: u8,
    pub bsid: u8,
    pub bsmod: u8,
    pub acmod: u8,
    pub lfeon: u8,
    /// AC-3 only: the bit rate code, `frmsizecod >> 1`.
    pub bit_rate_code: u8,
    /// Samples per frame: 1536 for AC-3, 256 per block for E-AC-3.
    pub frame_samples: u32,
    /// E-AC-3 only: the frame size in bytes, to compute the data rate.
    pub frame_bytes: u32,
    pub sample_rate: u32,
}

/// Parses the header of the AC-3 or E-AC-3 frame at the start of `frame`.
pub fn parse_ac3_header(frame: &[u8], codec: AudioCodec) -> Result<Ac3Header, String> {
    if frame.len() < AC3_HEADER_BYTES || frame[0] != 0x0B || frame[1] != 0x77 {
        return Err(String::from("audio frame without an AC-3 sync word"));
    }
    let mut bits = BitReader::new(&frame[2..]);
    match codec {
        AudioCodec::Ac3 => {
            bits.skip(16); // crc1
            let fscod = bits.read(2) as u8;
            let frmsizecod = bits.read(6) as u8;
            let bsid = bits.read(5) as u8;
            let bsmod = bits.read(3) as u8;
            let acmod = bits.read(3) as u8;
            if acmod & 1 != 0 && acmod != 1 {
                bits.skip(2); // cmixlev
            }
            if acmod & 4 != 0 {
                bits.skip(2); // surmixlev
            }
            if acmod == 2 {
                bits.skip(2); // dsurmod
            }
            let lfeon = bits.read(1) as u8;
            let sample_rate = [48000, 44100, 32000]
                .get(usize::from(fscod))
                .copied()
                .ok_or("AC-3 with a reserved sample rate")?;
            Ok(Ac3Header {
                fscod,
                bsid,
                bsmod,
                acmod,
                lfeon,
                bit_rate_code: frmsizecod >> 1,
                frame_samples: AC3_FRAME_SAMPLES,
                frame_bytes: 0,
                sample_rate,
            })
        }
        AudioCodec::Eac3 => {
            bits.skip(2 + 3); // strmtyp, substreamid
            let frame_bytes = (bits.read(11) + 1) * 2;
            let fscod = bits.read(2) as u8;
            let (sample_rate, blocks) = if fscod == 3 {
                let fscod2 = bits.read(2);
                let rate = [24000, 22050, 16000]
                    .get(fscod2 as usize)
                    .copied()
                    .ok_or("E-AC-3 with a reserved sample rate")?;
                (rate, 6)
            } else {
                (
                    ([48000, 44100, 32000])[usize::from(fscod)],
                    [1, 2, 3, 6][bits.read(2) as usize],
                )
            };
            let acmod = bits.read(3) as u8;
            let lfeon = bits.read(1) as u8;
            let bsid = bits.read(5) as u8;
            Ok(Ac3Header {
                fscod,
                bsid,
                bsmod: 0,
                acmod,
                lfeon,
                bit_rate_code: 0,
                frame_samples: 256 * blocks,
                frame_bytes,
                sample_rate,
            })
        }
        AudioCodec::Aac => unreachable!("AAC has no AC-3 header; callers branch on the codec"),
    }
}

/// The `dac3` box payload (ETSI TS 102 366, annex F.4).
pub fn dac3(header: &Ac3Header) -> Vec<u8> {
    let bits = (u32::from(header.fscod) << 22)
        | (u32::from(header.bsid) << 17)
        | (u32::from(header.bsmod) << 14)
        | (u32::from(header.acmod) << 11)
        | (u32::from(header.lfeon) << 10)
        | (u32::from(header.bit_rate_code) << 5);
    bits.to_be_bytes()[1..].to_vec()
}

/// The `dec3` box payload for one independent substream (annex F.6).
/// Dependent substreams (7.1) still play: the decoder reads them from the
/// frames; only the box leaves them out.
pub fn dec3(header: &Ac3Header) -> Vec<u8> {
    let data_rate_kbps = (u64::from(header.frame_bytes) * 8 * u64::from(header.sample_rate)
        / u64::from(header.frame_samples)
        / 1000) as u16;
    let mut payload = Vec::with_capacity(5);
    // data_rate (13 bits), num_ind_sub (3 bits) = 0, meaning one.
    payload.extend_from_slice(&(data_rate_kbps.min(0x1FFF) << 3).to_be_bytes());
    // fscod 2, bsid 5, reserved 1 | asvc 1, bsmod 3, acmod 3, lfeon 1 |
    // reserved 3, num_dep_sub 4, reserved 1.
    payload.push((header.fscod << 6) | (header.bsid << 1));
    payload.push((header.bsmod << 4) | (header.acmod << 1) | header.lfeon);
    payload.push(0);
    payload
}

/// `avc1.PPCCLL` from an AVCDecoderConfigurationRecord.
pub fn avc_codec_string(avcc: &[u8]) -> Result<String, String> {
    if avcc.len() < 4 || avcc[0] != 1 {
        return Err(String::from("H.264 track without a valid avcC"));
    }
    Ok(format!(
        "avc1.{:02x}{:02x}{:02x}",
        avcc[1], avcc[2], avcc[3]
    ))
}

/// `hvc1.<profile>.<compatibility>.<tier><level>.<constraints>` from an
/// HEVCDecoderConfigurationRecord (ISO/IEC 14496-15, annex E.3).
pub fn hevc_codec_string(hvcc: &[u8], sample_entry: &str) -> Result<String, String> {
    // configurationVersion, then profile, compatibility (4), constraints (6), level.
    if hvcc.len() < 13 || hvcc[0] != 1 {
        return Err(String::from("HEVC track without a valid hvcC"));
    }
    let space = ["", "A", "B", "C"][usize::from(hvcc[1] >> 6)];
    let tier = if hvcc[1] & 0x20 != 0 { "H" } else { "L" };
    let profile = hvcc[1] & 0x1F;
    let compatibility = u32::from_be_bytes([hvcc[2], hvcc[3], hvcc[4], hvcc[5]]).reverse_bits();
    let level = hvcc[12];
    let mut constraints = &hvcc[6..12];
    while let [rest @ .., 0] = constraints {
        constraints = rest;
    }
    let mut text = format!("{sample_entry}.{space}{profile}.{compatibility:X}.{tier}{level}");
    for byte in constraints {
        text.push_str(&format!(".{byte:X}"));
    }
    Ok(text)
}

/// What a Dolby Vision configuration record means for playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DolbyVision {
    pub profile: u8,
    pub level: u8,
    /// 0: none, 1: HDR10, 2: SDR, 4: HLG.
    pub compatibility: u8,
}

pub fn parse_dolby_vision(record: &[u8]) -> Option<DolbyVision> {
    if record.len() < 5 {
        return None;
    }
    let profile = record[2] >> 1;
    let level = ((record[2] & 1) << 5) | (record[3] >> 3);
    let compatibility = record[4] >> 4;
    Some(DolbyVision {
        profile,
        level,
        compatibility,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matroska::{Encoding, TrackKind};

    fn audio_track(codec_private: Vec<u8>, sample_rate: f64, channels: u32) -> Track {
        Track {
            number: 2,
            kind: TrackKind::Audio,
            codec_id: String::from("A_AAC"),
            codec_private,
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
            sample_rate,
            channels,
        }
    }

    #[test]
    fn builds_aac_lc_config_when_the_file_has_none() {
        assert_eq!(
            aac_config(&audio_track(Vec::new(), 48000.0, 2)).unwrap(),
            [0x11, 0x90]
        );
        assert_eq!(
            aac_config(&audio_track(vec![0x12, 0x10], 44100.0, 2)).unwrap(),
            [0x12, 0x10]
        );
        assert!(aac_config(&audio_track(Vec::new(), 47000.0, 2)).is_err());
        assert!(aac_config(&audio_track(Vec::new(), 48000.0, 0)).is_err());
        assert_eq!(aac_object_type(&[0x11, 0x90]), 2);
        assert_eq!(aac_object_type(&[0x2B, 0x92, 0x08]), 5);
    }

    #[test]
    fn reads_an_ac3_header_into_dac3() {
        // 48 kHz, 448 kb/s (frmsizecod 28), bsid 8, 3/2 with LFE.
        let frame = [0x0B, 0x77, 0, 0, 0b0001_1100, 0b0100_0000, 0b1110_0001, 0];
        let header = parse_ac3_header(&frame, AudioCodec::Ac3).unwrap();
        assert_eq!(
            (header.fscod, header.bsid, header.acmod, header.lfeon),
            (0, 8, 7, 1)
        );
        assert_eq!(header.bit_rate_code, 14);
        assert_eq!(header.frame_samples, 1536);
        assert_eq!(dac3(&header), [0x10, 0x3D, 0xC0]);
    }

    #[test]
    fn reads_an_eac3_header_into_dec3() {
        // Independent, 1536-byte frames (frmsiz 767), 48 kHz, 6 blocks, 3/2 + LFE, bsid 16.
        let frame = [0x0B, 0x77, 0x02, 0xFF, 0b0011_1111, 0b1000_0000, 0, 0];
        let header = parse_ac3_header(&frame, AudioCodec::Eac3).unwrap();
        assert_eq!((header.acmod, header.lfeon, header.bsid), (7, 1, 16));
        assert_eq!(header.frame_samples, 1536);
        assert_eq!(header.frame_bytes, 1536);
        // 1536 bytes every 32 ms is 384 kb/s.
        assert_eq!(dec3(&header), [0x0C, 0x00, 0x20, 0x0F, 0x00]);
    }

    #[test]
    fn refuses_audio_without_a_sync_word() {
        assert!(parse_ac3_header(&[0; 8], AudioCodec::Ac3).is_err());
        assert!(parse_ac3_header(&[0x0B, 0x77], AudioCodec::Eac3).is_err());
    }

    #[test]
    fn writes_hls_codec_strings() {
        assert_eq!(
            avc_codec_string(&[1, 0x64, 0x00, 0x28, 0xFF]).unwrap(),
            "avc1.640028"
        );
        // Main 10, compatibility flag 2 set, level 5.1, progressive and frame-only.
        let hvcc = [1, 0x02, 0x20, 0, 0, 0, 0x90, 0, 0, 0, 0, 0, 153];
        assert_eq!(
            hevc_codec_string(&hvcc, "hvc1").unwrap(),
            "hvc1.2.4.L153.90"
        );
        assert!(hevc_codec_string(&hvcc[..5], "hvc1").is_err());
    }

    #[test]
    fn reads_dolby_vision_profile_and_level() {
        // Profile 8, level 6, compatibility 1 (HDR10).
        let record = [1, 0, 0b0001_0000, 0b0011_0111, 0b0001_0000];
        let config = parse_dolby_vision(&record).unwrap();
        assert_eq!(
            config,
            DolbyVision {
                profile: 8,
                level: 6,
                compatibility: 1
            }
        );
    }
}
