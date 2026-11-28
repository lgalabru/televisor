//! What an HEVC sequence parameter set says about colour, for HLS
//! `VIDEO-RANGE`. Matroska `Colour` tags are often missing or wrong; the
//! SPS is what the decoder itself obeys (ITU-T H.265, section 7.3.2.2).

use crate::bits::BitReader;

/// NAL unit type of a sequence parameter set.
const NAL_SPS: u8 = 33;
/// Bytes before the arrays in an HEVCDecoderConfigurationRecord.
const HVCC_HEADER_BYTES: usize = 22;
/// H.265 limits: 64 short-term reference picture sets, 16 pictures in
/// each direction, 32 long-term pictures, 7 sub-layers.
const SHORT_TERM_SETS_MAX: u32 = 64;
const DELTA_PICTURES_MAX: u32 = 16;
const LONG_TERM_PICTURES_MAX: u32 = 32;
const SUB_LAYERS_MAX: u32 = 7;

/// `transfer_characteristics` from the first SPS in an `hvcC`, when its
/// VUI gives one: 16 is PQ, 18 is HLG, 1 is BT.709.
pub fn transfer_characteristics(hvcc: &[u8]) -> Option<u8> {
    let sps = first_sps(hvcc)?;
    let rbsp = without_emulation_prevention(sps.get(2..)?);
    parse_sps_transfer(&rbsp)
}

fn first_sps(hvcc: &[u8]) -> Option<&[u8]> {
    let mut position = HVCC_HEADER_BYTES;
    let arrays = *hvcc.get(position)?;
    position += 1;
    for _ in 0..arrays {
        let nal_type = hvcc.get(position)? & 0x3F;
        let count = u16::from_be_bytes([*hvcc.get(position + 1)?, *hvcc.get(position + 2)?]);
        position += 3;
        for _ in 0..count {
            let length = usize::from(u16::from_be_bytes([
                *hvcc.get(position)?,
                *hvcc.get(position + 1)?,
            ]));
            let nal = hvcc.get(position + 2..position + 2 + length)?;
            if nal_type == NAL_SPS {
                return Some(nal);
            }
            position += 2 + length;
        }
    }
    None
}

/// Drops the `03` in every `00 00 03`, which encoders insert so the
/// payload never looks like a start code.
fn without_emulation_prevention(nal: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(nal.len());
    let mut zeros = 0;
    for &byte in nal {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        out.push(byte);
    }
    out
}

fn parse_sps_transfer(rbsp: &[u8]) -> Option<u8> {
    let mut bits = BitReader::new(rbsp);
    bits.skip(4); // sps_video_parameter_set_id
    let sub_layers = bits.read(3);
    bits.skip(1); // sps_temporal_id_nesting_flag
    skip_profile_tier_level(&mut bits, sub_layers)?;
    bits.unsigned_golomb()?; // sps_seq_parameter_set_id
    if bits.unsigned_golomb()? == 3 {
        bits.skip(1); // separate_colour_plane_flag
    }
    bits.unsigned_golomb()?; // pic_width_in_luma_samples
    bits.unsigned_golomb()?; // pic_height_in_luma_samples
    if bits.flag() {
        for _ in 0..4 {
            bits.unsigned_golomb()?; // conformance window offsets
        }
    }
    bits.unsigned_golomb()?; // bit_depth_luma_minus8
    bits.unsigned_golomb()?; // bit_depth_chroma_minus8
    let poc_lsb_bits = bits.unsigned_golomb()?.checked_add(4)?;
    let first_layer = if bits.flag() { 0 } else { sub_layers };
    for _ in first_layer..=sub_layers {
        for _ in 0..3 {
            bits.unsigned_golomb()?; // max_dec_pic_buffering, num_reorder_pics, max_latency_increase
        }
    }
    for _ in 0..6 {
        bits.unsigned_golomb()?; // coding and transform block sizes, hierarchy depths
    }
    if bits.flag() && bits.flag() {
        skip_scaling_list_data(&mut bits)?;
    }
    bits.skip(2); // amp_enabled_flag, sample_adaptive_offset_enabled_flag
    if bits.flag() {
        bits.skip(8); // PCM sample bit depths
        bits.unsigned_golomb()?;
        bits.unsigned_golomb()?;
        bits.skip(1);
    }
    skip_short_term_ref_pic_sets(&mut bits)?;
    if bits.flag() {
        let long_term = bits.unsigned_golomb()?;
        if long_term > LONG_TERM_PICTURES_MAX || poc_lsb_bits > 16 {
            return None;
        }
        for _ in 0..long_term {
            bits.skip(poc_lsb_bits as usize + 1);
        }
    }
    bits.skip(2); // sps_temporal_mvp_enabled_flag, strong_intra_smoothing_enabled_flag
    let transfer = vui_transfer(&mut bits)?;
    (!bits.is_exhausted()).then_some(transfer)
}

fn skip_profile_tier_level(bits: &mut BitReader<'_>, sub_layers: u32) -> Option<()> {
    if sub_layers >= SUB_LAYERS_MAX {
        return None;
    }
    // General profile (88 bits) and level (8 bits).
    bits.skip(96);
    let mut present = Vec::with_capacity(sub_layers as usize);
    for _ in 0..sub_layers {
        present.push((bits.flag(), bits.flag()));
    }
    if sub_layers > 0 {
        bits.skip(2 * (8 - sub_layers as usize));
    }
    for (has_profile, has_level) in present {
        bits.skip(if has_profile { 88 } else { 0 } + if has_level { 8 } else { 0 });
    }
    Some(())
}

fn skip_scaling_list_data(bits: &mut BitReader<'_>) -> Option<()> {
    for size in 0..4u32 {
        let step = if size == 3 { 3 } else { 1 };
        for _ in (0..6).step_by(step) {
            if !bits.flag() {
                bits.unsigned_golomb()?; // scaling_list_pred_matrix_id_delta
                continue;
            }
            let coefficients = 64.min(1 << (4 + (size << 1)));
            if size > 1 {
                bits.skip_signed_golomb()?; // scaling_list_dc_coef_minus8
            }
            for _ in 0..coefficients {
                bits.skip_signed_golomb()?;
            }
        }
    }
    Some(())
}

/// `st_ref_pic_set` for every set; each may be predicted from the one
/// before, so the sizes are tracked.
fn skip_short_term_ref_pic_sets(bits: &mut BitReader<'_>) -> Option<()> {
    let sets = bits.unsigned_golomb()?;
    if sets > SHORT_TERM_SETS_MAX {
        return None;
    }
    let mut delta_pictures: Vec<u32> = Vec::with_capacity(sets as usize);
    for index in 0..sets {
        let is_predicted = index != 0 && bits.flag();
        let count = if is_predicted {
            bits.skip(1); // delta_rps_sign
            bits.unsigned_golomb()?; // abs_delta_rps_minus1
            let reference = *delta_pictures.last()?;
            let mut count = 0;
            for _ in 0..=reference {
                let is_used = bits.flag();
                if is_used || bits.flag() {
                    count += 1;
                }
            }
            count
        } else {
            let negative = bits.unsigned_golomb()?;
            let positive = bits.unsigned_golomb()?;
            if negative > DELTA_PICTURES_MAX || positive > DELTA_PICTURES_MAX {
                return None;
            }
            for _ in 0..negative + positive {
                bits.unsigned_golomb()?; // delta_poc_minus1
                bits.skip(1); // used_by_curr_pic_flag
            }
            negative + positive
        };
        delta_pictures.push(count);
    }
    Some(())
}

/// The transfer function from `vui_parameters`, or `None` when the SPS has
/// no VUI or the VUI has no colour description.
fn vui_transfer(bits: &mut BitReader<'_>) -> Option<u8> {
    if !bits.flag() {
        return None;
    }
    if bits.flag() && bits.read(8) == 255 {
        bits.skip(32); // sar_width, sar_height
    }
    if bits.flag() {
        bits.skip(1); // overscan_appropriate_flag
    }
    if !bits.flag() {
        return None; // no video_signal_type
    }
    bits.skip(4); // video_format, video_full_range_flag
    if !bits.flag() {
        return None; // no colour_description
    }
    bits.skip(8); // colour_primaries
    Some(bits.read(8) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_emulation_prevention_bytes() {
        assert_eq!(
            without_emulation_prevention(&[0, 0, 3, 1, 0, 0, 3]),
            [0, 0, 1, 0, 0]
        );
        assert_eq!(
            without_emulation_prevention(&[0, 3, 0, 0, 0, 3]),
            [0, 3, 0, 0, 0]
        );
    }

    #[test]
    fn finds_nothing_in_a_record_without_sps() {
        assert_eq!(transfer_characteristics(&[1; 22]), None);
        let mut record = vec![1; 22];
        record.extend_from_slice(&[1, 0x20, 0, 1, 0, 2, 0x40, 1]); // one VPS
        assert_eq!(transfer_characteristics(&record), None);
        assert_eq!(transfer_characteristics(&[]), None);
    }
}
