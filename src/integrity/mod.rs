use crc::{Crc, CRC_32_MPEG_2};
use sha2::{Digest, Sha256};

/// CRC-32/MPEG-2 calculator.
const CRC_MPEG2: Crc<u32> = Crc::<u32>::new(&CRC_32_MPEG_2);

/// Compute CRC-32/MPEG-2 over a byte slice.
pub fn crc32_mpeg2(data: &[u8]) -> u32 {
    CRC_MPEG2.checksum(data)
}

/// Compute CRC-32/MPEG-2 for a packet: header (with CRC field zeroed) + payload.
pub fn packet_crc32(header: &[u8], crc_field_offset: usize, payload: &[u8]) -> u32 {
    let mut digest = CRC_MPEG2.digest();

    // Feed header bytes before the CRC field
    digest.update(&header[..crc_field_offset]);
    // Feed 4 zero bytes in place of the CRC field
    digest.update(&[0u8; 4]);
    // Feed header bytes after the CRC field
    if crc_field_offset + 4 < header.len() {
        digest.update(&header[crc_field_offset + 4..]);
    }
    // Feed the payload
    digest.update(payload);

    digest.finalize()
}

/// Verify the CRC field in a packet.
pub fn verify_packet_crc(
    header: &[u8],
    crc_field_offset: usize,
    payload: &[u8],
    expected_crc: u32,
) -> bool {
    packet_crc32(header, crc_field_offset, payload) == expected_crc
}

/// SHA-256 digest type.
pub type Sha256Digest = [u8; 32];

/// Compute SHA-256 hash of a byte slice.
pub fn sha256(data: &[u8]) -> Sha256Digest {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&result);
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_mpeg2_known_value() {
        // "123456789" has a well-known CRC-32/MPEG-2 checksum
        let data = b"123456789";
        let crc = crc32_mpeg2(data);
        assert_eq!(crc, 0x0376E6E7);
    }

    #[test]
    fn test_crc32_empty() {
        let crc = crc32_mpeg2(b"");
        assert_eq!(crc, 0xFFFFFFFF);
    }

    #[test]
    fn test_sha256_known_value() {
        let hash = sha256(b"hello");
        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(hex, expected);
    }

    #[test]
    fn test_packet_crc_roundtrip() {
        let header = vec![0x01, 0x02, 0x03, 0x04, 0x00, 0x00, 0x00, 0x00, 0x09, 0x0A];
        let payload = b"test payload";
        let crc_offset = 4;

        let crc = packet_crc32(&header, crc_offset, payload);
        assert!(verify_packet_crc(&header, crc_offset, payload, crc));
        assert!(!verify_packet_crc(&header, crc_offset, payload, crc ^ 1));
    }
}
