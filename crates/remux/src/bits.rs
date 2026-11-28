//! A bit reader for codec headers.

/// Longest Exp-Golomb prefix read. Values in the headers we parse fit in
/// 32 bits; a longer run of zeros means corrupt data.
const EXP_GOLOMB_ZEROS_MAX: u32 = 31;

/// Reads bits most significant first. Reading past the end yields zeros
/// and marks the reader exhausted; callers check before trusting a value.
pub struct BitReader<'a> {
    data: &'a [u8],
    position_bits: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            position_bits: 0,
        }
    }

    pub fn read(&mut self, count: u32) -> u32 {
        assert!(count <= 32, "reads are at most 32 bits");
        let mut value = 0u32;
        for _ in 0..count {
            let byte = self.data.get(self.position_bits / 8).copied().unwrap_or(0);
            let bit = (byte >> (7 - self.position_bits % 8)) & 1;
            value = (value << 1) | u32::from(bit);
            self.position_bits += 1;
        }
        value
    }

    pub fn flag(&mut self) -> bool {
        self.read(1) == 1
    }

    pub fn skip(&mut self, count: usize) {
        self.position_bits += count;
    }

    /// An unsigned Exp-Golomb value, `ue(v)` in the H.26x specs; `None`
    /// on a prefix longer than any valid header holds.
    pub fn unsigned_golomb(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while !self.flag() {
            zeros += 1;
            if zeros > EXP_GOLOMB_ZEROS_MAX || self.is_exhausted() {
                return None;
            }
        }
        Some(((1u64 << zeros) - 1 + u64::from(self.read(zeros))) as u32)
    }

    /// A signed Exp-Golomb value, `se(v)`; only its presence matters here.
    pub fn skip_signed_golomb(&mut self) -> Option<()> {
        self.unsigned_golomb().map(|_| ())
    }

    pub fn is_exhausted(&self) -> bool {
        self.position_bits > self.data.len() * 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_exp_golomb_codes() {
        // 1 | 010 | 011 | 00100 | 0001000: 0, 1, 2, 3, 7.
        let data = [0b1010_0110, 0b0100_0001, 0b0000_0000];
        let mut reader = BitReader::new(&data);
        let values: Vec<u32> = (0..5).map(|_| reader.unsigned_golomb().unwrap()).collect();
        assert_eq!(values, [0, 1, 2, 3, 7]);
        assert!(!reader.is_exhausted());
    }

    #[test]
    fn refuses_endless_zeros() {
        let mut reader = BitReader::new(&[0; 8]);
        assert_eq!(reader.unsigned_golomb(), None);
        assert!(BitReader::new(&[0; 2]).unsigned_golomb().is_none());
    }
}
