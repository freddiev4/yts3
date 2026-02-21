use byteorder::{ByteOrder, LittleEndian};
use thiserror::Error;

use crate::config;
use crate::integrity;

#[derive(Error, Debug)]
pub enum PacketError {
    #[error("invalid magic: expected 0x{expected:08X}, got 0x{got:08X}")]
    InvalidMagic { expected: u32, got: u32 },
    #[error("unsupported version: {0}")]
    UnsupportedVersion(u8),
    #[error("CRC mismatch: expected 0x{expected:08X}, got 0x{computed:08X}")]
    CrcMismatch { expected: u32, computed: u32 },
    #[error("buffer too short: need {need} bytes, have {have}")]
    BufferTooShort { need: usize, have: usize },
    #[error("payload length mismatch")]
    PayloadLengthMismatch,
}

/// Parsed packet header fields.
#[derive(Debug, Clone)]
pub struct PacketHeader {
    pub magic: u32,
    pub version: u8,
    pub flags: u8,
    pub file_id: [u8; config::FILE_ID_SIZE],
    pub chunk_index: u32,
    pub chunk_size: u32,
    pub original_size: u32,
    pub symbol_size: u16,
    pub k: u32,
    pub esi: u32,
    pub payload_length: u16,
    pub crc: u32,
}

/// A complete packet: header + payload.
#[derive(Debug, Clone)]
pub struct Packet {
    pub header: PacketHeader,
    pub payload: Vec<u8>,
}

// Header field offsets (V2, 50 bytes total)
const OFF_MAGIC: usize = 0;
const OFF_VERSION: usize = 4;
const OFF_FLAGS: usize = 5;
const OFF_FILE_ID: usize = 6;
const OFF_CHUNK_INDEX: usize = 22;
const OFF_CHUNK_SIZE: usize = 26;
const OFF_ORIGINAL_SIZE: usize = 30;
const OFF_SYMBOL_SIZE: usize = 34;
const OFF_K: usize = 36;
const OFF_ESI: usize = 40;
const OFF_PAYLOAD_LEN: usize = 44;
const OFF_CRC: usize = 46;

impl PacketHeader {
    pub fn is_repair(&self) -> bool {
        self.flags & config::FLAG_REPAIR_SYMBOL != 0
    }

    pub fn is_last_chunk(&self) -> bool {
        self.flags & config::FLAG_LAST_CHUNK != 0
    }

    pub fn is_encrypted(&self) -> bool {
        self.flags & config::FLAG_ENCRYPTED != 0
    }
}

/// Serialize a packet header + payload into bytes.
pub fn serialize_packet(
    file_id: &[u8; config::FILE_ID_SIZE],
    chunk_index: u32,
    chunk_size: u32,
    original_size: u32,
    symbol_size: u16,
    k: u32,
    esi: u32,
    flags: u8,
    payload: &[u8],
) -> Vec<u8> {
    let mut header = vec![0u8; config::PACKET_HEADER_SIZE];

    LittleEndian::write_u32(&mut header[OFF_MAGIC..], config::MAGIC);
    header[OFF_VERSION] = config::PACKET_VERSION;
    header[OFF_FLAGS] = flags;
    header[OFF_FILE_ID..OFF_FILE_ID + config::FILE_ID_SIZE].copy_from_slice(file_id);
    LittleEndian::write_u32(&mut header[OFF_CHUNK_INDEX..], chunk_index);
    LittleEndian::write_u32(&mut header[OFF_CHUNK_SIZE..], chunk_size);
    LittleEndian::write_u32(&mut header[OFF_ORIGINAL_SIZE..], original_size);
    LittleEndian::write_u16(&mut header[OFF_SYMBOL_SIZE..], symbol_size);
    LittleEndian::write_u32(&mut header[OFF_K..], k);
    LittleEndian::write_u32(&mut header[OFF_ESI..], esi);
    LittleEndian::write_u16(&mut header[OFF_PAYLOAD_LEN..], payload.len() as u16);

    // Compute CRC over header (with CRC field zeroed) + payload
    let crc = integrity::packet_crc32(&header, OFF_CRC, payload);
    LittleEndian::write_u32(&mut header[OFF_CRC..], crc);

    let mut packet_bytes = Vec::with_capacity(config::PACKET_HEADER_SIZE + payload.len());
    packet_bytes.extend_from_slice(&header);
    packet_bytes.extend_from_slice(payload);
    packet_bytes
}

/// Deserialize a packet from a byte buffer. Returns the packet and the number of bytes consumed.
pub fn deserialize_packet(data: &[u8]) -> Result<(Packet, usize), PacketError> {
    if data.len() < config::PACKET_HEADER_SIZE {
        return Err(PacketError::BufferTooShort {
            need: config::PACKET_HEADER_SIZE,
            have: data.len(),
        });
    }

    let header_bytes = &data[..config::PACKET_HEADER_SIZE];

    let magic = LittleEndian::read_u32(&header_bytes[OFF_MAGIC..]);
    if magic != config::MAGIC {
        return Err(PacketError::InvalidMagic {
            expected: config::MAGIC,
            got: magic,
        });
    }

    let version = header_bytes[OFF_VERSION];
    if version != config::PACKET_VERSION {
        return Err(PacketError::UnsupportedVersion(version));
    }

    let flags = header_bytes[OFF_FLAGS];
    let mut file_id = [0u8; config::FILE_ID_SIZE];
    file_id.copy_from_slice(&header_bytes[OFF_FILE_ID..OFF_FILE_ID + config::FILE_ID_SIZE]);
    let chunk_index = LittleEndian::read_u32(&header_bytes[OFF_CHUNK_INDEX..]);
    let chunk_size = LittleEndian::read_u32(&header_bytes[OFF_CHUNK_SIZE..]);
    let original_size = LittleEndian::read_u32(&header_bytes[OFF_ORIGINAL_SIZE..]);
    let symbol_size = LittleEndian::read_u16(&header_bytes[OFF_SYMBOL_SIZE..]);
    let k = LittleEndian::read_u32(&header_bytes[OFF_K..]);
    let esi = LittleEndian::read_u32(&header_bytes[OFF_ESI..]);
    let payload_length = LittleEndian::read_u16(&header_bytes[OFF_PAYLOAD_LEN..]);
    let crc = LittleEndian::read_u32(&header_bytes[OFF_CRC..]);

    let total_len = config::PACKET_HEADER_SIZE + payload_length as usize;
    if data.len() < total_len {
        return Err(PacketError::BufferTooShort {
            need: total_len,
            have: data.len(),
        });
    }

    let payload = data[config::PACKET_HEADER_SIZE..total_len].to_vec();

    // Verify CRC
    let computed_crc = integrity::packet_crc32(header_bytes, OFF_CRC, &payload);
    if computed_crc != crc {
        return Err(PacketError::CrcMismatch {
            expected: crc,
            computed: computed_crc,
        });
    }

    let header = PacketHeader {
        magic,
        version,
        flags,
        file_id,
        chunk_index,
        chunk_size,
        original_size,
        symbol_size,
        k,
        esi,
        payload_length,
        crc,
    };

    Ok((Packet { header, payload }, total_len))
}

/// Scan a byte buffer for packets by looking for the magic number.
pub fn scan_for_packets(data: &[u8]) -> Vec<Packet> {
    let mut packets = Vec::new();
    let mut offset = 0;
    let magic_bytes = config::MAGIC.to_le_bytes();

    while offset + config::PACKET_HEADER_SIZE <= data.len() {
        // Search for magic number
        if let Some(pos) = find_magic(&data[offset..], &magic_bytes) {
            let abs_pos = offset + pos;
            match deserialize_packet(&data[abs_pos..]) {
                Ok((packet, consumed)) => {
                    packets.push(packet);
                    offset = abs_pos + consumed;
                }
                Err(_) => {
                    offset = abs_pos + 1; // Skip past this false magic match
                }
            }
        } else {
            break;
        }
    }

    packets
}

fn find_magic(data: &[u8], magic: &[u8; 4]) -> Option<usize> {
    data.windows(4).position(|w| w == magic)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_file_id() -> [u8; 16] {
        let mut id = [0u8; 16];
        for (i, b) in id.iter_mut().enumerate() {
            *b = i as u8;
        }
        id
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let file_id = make_test_file_id();
        let payload = vec![0xAA; 256];

        let data = serialize_packet(
            &file_id,
            3,     // chunk_index
            1024,  // chunk_size
            900,   // original_size
            256,   // symbol_size
            4,     // k
            3,     // esi
            config::FLAG_LAST_CHUNK,
            &payload,
        );

        let (packet, consumed) = deserialize_packet(&data).unwrap();
        assert_eq!(consumed, config::PACKET_HEADER_SIZE + 256);
        assert_eq!(packet.header.magic, config::MAGIC);
        assert_eq!(packet.header.version, config::PACKET_VERSION);
        assert_eq!(packet.header.chunk_index, 3);
        assert_eq!(packet.header.chunk_size, 1024);
        assert_eq!(packet.header.original_size, 900);
        assert_eq!(packet.header.k, 4);
        assert_eq!(packet.header.esi, 3);
        assert!(packet.header.is_last_chunk());
        assert!(!packet.header.is_repair());
        assert!(!packet.header.is_encrypted());
        assert_eq!(packet.payload, payload);
    }

    #[test]
    fn test_crc_tamper_detection() {
        let file_id = make_test_file_id();
        let payload = vec![0xBB; 128];
        let mut data = serialize_packet(&file_id, 0, 512, 512, 128, 4, 0, 0, &payload);

        // Tamper with the payload
        data[config::PACKET_HEADER_SIZE + 10] ^= 0xFF;

        let result = deserialize_packet(&data);
        assert!(matches!(result, Err(PacketError::CrcMismatch { .. })));
    }

    #[test]
    fn test_scan_for_packets() {
        let file_id = make_test_file_id();
        let p1 = serialize_packet(&file_id, 0, 256, 200, 64, 4, 0, 0, &vec![1u8; 64]);
        let p2 = serialize_packet(&file_id, 0, 256, 200, 64, 4, 1, 0, &vec![2u8; 64]);

        // Concatenate with some garbage in between
        let mut stream = Vec::new();
        stream.extend_from_slice(&[0xFF; 10]);
        stream.extend_from_slice(&p1);
        stream.extend_from_slice(&[0x00; 5]);
        stream.extend_from_slice(&p2);
        stream.extend_from_slice(&[0xAA; 20]);

        let packets = scan_for_packets(&stream);
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].header.esi, 0);
        assert_eq!(packets[1].header.esi, 1);
    }
}
