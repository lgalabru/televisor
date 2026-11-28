//! EBML, the binary layout under Matroska: every element is an id, a size
//! and a payload, and ids and sizes are variable-length integers whose
//! first byte says how many bytes follow (RFC 8794).

use crate::error::ParseError;

/// Longest variable-length integer EBML allows (RFC 8794, section 4).
const VARINT_BYTES_MAX: usize = 8;

/// An element's id and payload size, as read from its header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// The id with its length marker kept, as the Matroska spec writes it
    /// (`0x1F43B675` for Cluster).
    pub id: u32,
    /// `None` for "unknown size", which streaming muxers write for a
    /// Segment or Cluster they cannot seek back to patch.
    pub size: Option<u64>,
    /// Bytes taken by the id and the size together.
    pub header_bytes: usize,
}

/// Reads elements out of a byte slice. Positions are relative to the
/// slice; callers that need file offsets add the slice's own offset.
#[derive(Debug, Clone)]
pub struct Cursor<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.position
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Reads an element header, leaving the cursor at its payload.
    pub fn header(&mut self) -> Result<Header, ParseError> {
        let start = self.position;
        let id = self.id()?;
        let size = self.size()?;
        Ok(Header {
            id,
            size,
            header_bytes: self.position - start,
        })
    }

    /// The next `length` bytes, as a slice, advancing past them.
    pub fn take(&mut self, length: usize) -> Result<&'a [u8], ParseError> {
        if length > self.remaining() {
            return Err(ParseError::Truncated {
                needed_bytes: (length - self.remaining()) as u64,
            });
        }
        let bytes = &self.data[self.position..self.position + length];
        self.position += length;
        Ok(bytes)
    }

    /// The payload of an element whose header was just read. An element of
    /// unknown size runs to the end of the slice.
    pub fn payload(&mut self, header: &Header) -> Result<&'a [u8], ParseError> {
        match header.size {
            Some(size) => {
                let length = usize::try_from(size)
                    .map_err(|_| ParseError::Invalid("element larger than memory"))?;
                self.take(length)
            }
            None => self.take(self.remaining()),
        }
    }

    pub fn byte(&mut self) -> Result<u8, ParseError> {
        Ok(self.take(1)?[0])
    }

    /// A big-endian integer of `length` bytes, at most 8.
    pub fn uint_bytes(&mut self, length: usize) -> Result<u64, ParseError> {
        if length > 8 {
            return Err(ParseError::Invalid("integer wider than 8 bytes"));
        }
        let bytes = self.take(length)?;
        Ok(bytes
            .iter()
            .fold(0u64, |value, byte| (value << 8) | u64::from(*byte)))
    }

    /// A variable-length integer with its marker bit removed, as block
    /// headers use for track numbers and lace sizes. Also returns its width.
    pub fn varint(&mut self) -> Result<(u64, usize), ParseError> {
        let first = self.byte()?;
        let width = first.leading_zeros() as usize + 1;
        if width > VARINT_BYTES_MAX {
            return Err(ParseError::Invalid(
                "variable-length integer wider than 8 bytes",
            ));
        }
        let mut value = u64::from(first) & (0xFF >> width);
        for _ in 1..width {
            value = (value << 8) | u64::from(self.byte()?);
        }
        Ok((value, width))
    }

    fn id(&mut self) -> Result<u32, ParseError> {
        let first = self.byte()?;
        let width = first.leading_zeros() as usize + 1;
        // Matroska ids are 1 to 4 bytes (RFC 8794, section 5).
        if width > 4 {
            return Err(ParseError::Invalid("element id wider than 4 bytes"));
        }
        let mut id = u32::from(first);
        for _ in 1..width {
            id = (id << 8) | u32::from(self.byte()?);
        }
        Ok(id)
    }

    fn size(&mut self) -> Result<Option<u64>, ParseError> {
        let (value, width) = self.varint()?;
        // All value bits set means "unknown size" (RFC 8794, section 6.2).
        let unknown = (1u64 << (7 * width)) - 1;
        if value == unknown {
            Ok(None)
        } else {
            Ok(Some(value))
        }
    }
}

/// An unsigned integer element's payload: 0 to 8 bytes, big-endian.
pub fn uint(payload: &[u8]) -> Result<u64, ParseError> {
    Cursor::new(payload).uint_bytes(payload.len())
}

/// A float element's payload: 0, 4 or 8 bytes.
pub fn float(payload: &[u8]) -> Result<f64, ParseError> {
    match payload.len() {
        0 => Ok(0.0),
        4 => Ok(f64::from(f32::from_be_bytes(
            payload.try_into().expect("length checked"),
        ))),
        8 => Ok(f64::from_be_bytes(
            payload.try_into().expect("length checked"),
        )),
        _ => Err(ParseError::Invalid("float element neither 4 nor 8 bytes")),
    }
}

/// A string element's payload. Matroska pads strings with NULs.
pub fn string(payload: &[u8]) -> String {
    let end = payload
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(payload.len());
    String::from_utf8_lossy(&payload[..end]).into_owned()
}

/// Calls `visit` with each child element of `payload`, in order.
pub fn children<'a>(
    payload: &'a [u8],
    mut visit: impl FnMut(Header, &'a [u8]) -> Result<(), ParseError>,
) -> Result<(), ParseError> {
    let mut cursor = Cursor::new(payload);
    while !cursor.is_empty() {
        let header = cursor.header()?;
        let body = cursor.payload(&header)?;
        visit(header, body)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ids_and_sizes_of_each_width() {
        // Cluster id (4 bytes), size 0x123 written on 2 bytes.
        let data = [0x1F, 0x43, 0xB6, 0x75, 0x41, 0x23];
        let header = Cursor::new(&data).header().unwrap();
        assert_eq!(header.id, 0x1F43_B675);
        assert_eq!(header.size, Some(0x123));
        assert_eq!(header.header_bytes, 6);
    }

    #[test]
    fn all_ones_size_is_unknown() {
        let data = [
            0x1F, 0x43, 0xB6, 0x75, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        ];
        assert_eq!(Cursor::new(&data).header().unwrap().size, None);
        // One byte of all ones is unknown too.
        assert_eq!(Cursor::new(&[0xA3, 0xFF]).header().unwrap().size, None);
    }

    #[test]
    fn refuses_a_zero_first_byte() {
        let error = Cursor::new(&[0x00, 0x81]).header().unwrap_err();
        assert!(matches!(error, ParseError::Invalid(_)));
    }

    #[test]
    fn reports_how_many_bytes_are_missing() {
        let data = [0xA3, 0x85, 1, 2];
        let mut cursor = Cursor::new(&data);
        let header = cursor.header().unwrap();
        assert_eq!(
            cursor.payload(&header).unwrap_err(),
            ParseError::Truncated { needed_bytes: 3 }
        );
    }

    #[test]
    fn decodes_floats_and_padded_strings() {
        assert_eq!(float(&1.5f32.to_be_bytes()).unwrap(), 1.5);
        assert_eq!(float(&2.25f64.to_be_bytes()).unwrap(), 2.25);
        assert!(float(&[0, 0, 0]).is_err());
        assert_eq!(string(b"eng\0\0"), "eng");
    }
}
