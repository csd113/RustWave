/// Frame layout (v2 — stores original filename)
///
/// ┌──────────────┬────────┬──────────────┬──────┬──────────────┬─────────┬────────┐
/// │ preamble N×AA│ 7E 7E  │ `name_len` u16 │ name │ `payload_len`  │ payload │ CRC-16 │
/// └──────────────┴────────┴──────────────┴──────┴──────────────┴─────────┴────────┘
///                 ◄──────────────────── CRC covers this span ──────────────────────►
use crate::config::{PREAMBLE_LEN, SYNC};

/// Wrap `data` in a transmittable frame, embedding `filename` so the decoder
/// can reconstruct the file with the correct name and extension.
#[allow(
    clippy::arithmetic_side_effects, // capacity arithmetic is safe; values are small by construction
    clippy::indexing_slicing,        // out[PREAMBLE_LEN..] is valid: preamble bytes are always pushed first
)]
pub fn frame(data: &[u8], filename: &str) -> Vec<u8> {
    let name_bytes = filename.as_bytes();
    let name_len = name_bytes.len().min(255); // cap at 255 bytes
    let name_bytes = name_bytes.get(..name_len).unwrap_or(name_bytes);

    let capacity = PREAMBLE_LEN + 2 + 2 + name_len + 4 + data.len() + 2;
    let mut out = Vec::with_capacity(capacity);

    // Preamble
    out.extend(std::iter::repeat_n(0xAA_u8, PREAMBLE_LEN));

    // Sync word
    out.extend_from_slice(&SYNC);

    // Filename length (u16 LE) + filename bytes
    out.extend_from_slice(&u16::try_from(name_len).unwrap_or(255).to_le_bytes());
    out.extend_from_slice(name_bytes);

    // Payload length (u32 LE) + payload
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(data);

    // CRC-16/CCITT over everything from sync word onwards (not the preamble)
    let crc = crc16(&out[PREAMBLE_LEN..]);
    out.extend_from_slice(&crc.to_le_bytes());

    out
}

/// Decoded frame: original filename and payload bytes.
pub struct Decoded {
    pub filename: String,
    pub data: Vec<u8>,
}

/// Find and validate a frame inside `raw`, returning the embedded filename and payload.
///
/// Used only by the byte-level path (tests / CLI verification).
/// The audio decoder uses `find_frame_in_bits` in decoder.rs directly.
#[allow(
    dead_code,
    clippy::arithmetic_side_effects, // cursor arithmetic is bounds-checked before each use
    clippy::indexing_slicing,        // all slices are bounds-checked immediately above each access
)]
pub fn deframe(raw: &[u8]) -> Result<Decoded, String> {
    let sync_pos = raw
        .windows(SYNC.len())
        .position(|w| w == SYNC)
        .ok_or_else(|| "sync word not found".to_string())?;

    let mut cursor = sync_pos + SYNC.len();

    // name_len (u16)
    if raw.len() < cursor + 2 {
        return Err("frame too short: missing name_len".into());
    }
    let name_len = u16::from_le_bytes(
        raw[cursor..cursor + 2]
            .try_into()
            .map_err(|_| "internal: name_len slice error".to_string())?,
    ) as usize;
    cursor += 2;

    // name bytes
    if raw.len() < cursor + name_len {
        return Err("frame too short: missing filename".into());
    }
    let filename = String::from_utf8_lossy(&raw[cursor..cursor + name_len]).into_owned();
    cursor += name_len;

    // payload_len (u32)
    if raw.len() < cursor + 4 {
        return Err("frame too short: missing payload_len".into());
    }
    let payload_len = u32::from_le_bytes(
        raw[cursor..cursor + 4]
            .try_into()
            .map_err(|_| "internal: payload_len slice error".to_string())?,
    ) as usize;
    cursor += 4;

    let payload_end = cursor + payload_len;
    let crc_end = payload_end + 2;

    if raw.len() < crc_end {
        return Err(format!(
            "frame truncated: need {} bytes after sync, have {}",
            crc_end - sync_pos,
            raw.len() - sync_pos,
        ));
    }

    let stored_crc = u16::from_le_bytes(
        raw[payload_end..crc_end]
            .try_into()
            .map_err(|_| "internal: CRC slice error".to_string())?,
    );
    let computed_crc = crc16(&raw[sync_pos..payload_end]);

    if stored_crc != computed_crc {
        return Err(format!(
            "CRC mismatch: stored {stored_crc:#06x}, computed {computed_crc:#06x}"
        ));
    }

    Ok(Decoded {
        filename,
        data: raw[cursor..payload_end].to_vec(),
    })
}

// ---------------------------------------------------------------------------
// CRC-16/CCITT  (polynomial 0x1021, init 0xFFFF, no bit-reflection)
// ---------------------------------------------------------------------------

#[allow(clippy::arithmetic_side_effects)] // bit-shifting in CRC polynomial; no panic risk
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(data: &[u8], name: &str) -> Decoded {
        #[allow(clippy::unwrap_used)]
        deframe(&frame(data, name)).unwrap()
    }

    #[test]
    fn round_trip_empty_payload() {
        let d = rt(&[], "empty.bin");
        assert!(d.data.is_empty());
        assert_eq!(d.filename, "empty.bin");
    }

    #[test]
    fn round_trip_ascii() {
        let d = rt(b"Hello, AFSK!", "hello.txt");
        assert_eq!(d.data, b"Hello, AFSK!");
        assert_eq!(d.filename, "hello.txt");
    }

    #[test]
    fn round_trip_binary() {
        let data: Vec<u8> = (0u8..=255).collect();
        let d = rt(&data, "all_bytes.bin");
        assert_eq!(d.data, data);
    }

    #[test]
    fn filename_preserved() {
        let d = rt(b"data", "archive.tar.gz");
        assert_eq!(d.filename, "archive.tar.gz");
    }

    #[test]
    fn deframe_ignores_leading_garbage() {
        let mut framed = frame(b"test", "test.txt");
        framed.insert(0, 0xFF);
        framed.insert(0, 0x42);
        #[allow(clippy::unwrap_used)]
        let d = deframe(&framed).unwrap();
        assert_eq!(d.data, b"test");
        assert_eq!(d.filename, "test.txt");
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    fn crc_detects_corruption() {
        let mut framed = frame(b"integrity check", "check.txt");
        // Corrupt a payload byte (past preamble+sync+namelen+name+payloadlen)
        let corrupt_pos = PREAMBLE_LEN + 2 + 2 + "check.txt".len() + 4 + 2;
        framed[corrupt_pos] ^= 0xFF;
        assert!(deframe(&framed).is_err());
    }

    #[test]
    fn crc16_known_value() {
        assert_eq!(crc16(b"123456789"), 0x29B1);
    }
}
