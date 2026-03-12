use crate::config::*;
use crate::framer::Decoded;
use std::f64::consts::TAU;

/// Decode PCM samples back to the original filename and payload bytes.
pub fn decode_progress(
    samples: &[f64],
    on_progress: impl Fn(f32) + Clone,
) -> Result<Decoded, String> {
    let spb = SAMPLE_RATE as f64 / BAUD_RATE as f64;
    let spb_int = spb.round() as usize;

    for offset in 0..spb_int {
        let bits = samples_to_bits(samples, offset, on_progress.clone());
        if let Ok(decoded) = find_frame_in_bits(&bits) {
            on_progress(1.0);
            return Ok(decoded);
        }
    }

    Err("could not decode signal — sync word not found at any clock phase".into())
}

/// Convenience wrapper with no progress reporting.
pub fn decode(samples: &[f64]) -> Result<Decoded, String> {
    decode_progress(samples, |_| {})
}

// ---------------------------------------------------------------------------
// Step 1 — sample → bit stream via Goertzel
// ---------------------------------------------------------------------------

fn samples_to_bits(samples: &[f64], offset: usize, on_progress: impl Fn(f32)) -> Vec<bool> {
    let spb = SAMPLE_RATE as f64 / BAUD_RATE as f64;
    let total = samples.len().max(1);
    let mut bits = Vec::new();
    let mut idx: usize = 0;

    loop {
        let start = offset + (idx as f64 * spb).round() as usize;
        let end = offset + ((idx + 1) as f64 * spb).round() as usize;
        if end > samples.len() {
            break;
        }

        let w = &samples[start..end];
        bits.push(goertzel(w, MARK_FREQ, SAMPLE_RATE) > goertzel(w, SPACE_FREQ, SAMPLE_RATE));

        if idx % 32 == 0 {
            on_progress(end as f32 / total as f32);
        }
        idx += 1;
    }
    bits
}

// ---------------------------------------------------------------------------
// Step 2 — search the bit stream for the frame
// ---------------------------------------------------------------------------

fn find_frame_in_bits(bits: &[bool]) -> Result<Decoded, String> {
    let sync_bits: Vec<bool> = [0x7E_u8, 0x7E]
        .iter()
        .flat_map(|&b| (0..8u8).rev().map(move |i| (b >> i) & 1 == 1))
        .collect();
    let sync_len = sync_bits.len(); // 16

    let mut search = 0usize;
    while search + sync_len <= bits.len() {
        let Some(rel) = bits[search..]
            .windows(sync_len)
            .position(|w| w == sync_bits.as_slice())
        else {
            break;
        };

        let sync_start = search + rel;
        let mut cursor = sync_start + sync_len;

        // ── name_len (u16 LE) ──────────────────────────────────────────
        if cursor + 16 > bits.len() {
            break;
        }
        let name_len = {
            let b = bits_to_bytes(&bits[cursor..cursor + 16]);
            u16::from_le_bytes(b.try_into().unwrap()) as usize
        };
        cursor += 16;

        // Sanity: filenames shouldn't exceed 255 bytes
        if name_len > 255 {
            search = sync_start + 1;
            continue;
        }

        // ── name bytes ────────────────────────────────────────────────
        let name_bits = name_len * 8;
        if cursor + name_bits > bits.len() {
            search = sync_start + 1;
            continue;
        }
        let name_bytes = bits_to_bytes(&bits[cursor..cursor + name_bits]);
        let filename = String::from_utf8_lossy(&name_bytes).into_owned();
        cursor += name_bits;

        // ── payload_len (u32 LE) ──────────────────────────────────────
        if cursor + 32 > bits.len() {
            break;
        }
        let payload_len = {
            let b = bits_to_bytes(&bits[cursor..cursor + 32]);
            u32::from_le_bytes(b.try_into().unwrap()) as usize
        };
        cursor += 32;

        if payload_len > 1_000_000 {
            search = sync_start + 1;
            continue;
        }

        // ── payload ───────────────────────────────────────────────────
        let payload_start = cursor;
        let payload_end = payload_start + payload_len * 8;
        let crc_end = payload_end + 16;
        if crc_end > bits.len() {
            search = sync_start + 1;
            continue;
        }

        // ── CRC check ─────────────────────────────────────────────────
        // CRC covers: sync(16) + name_len(16) + name(N*8) + payload_len(32) + payload(M*8)
        let frame_bits_end = payload_end;
        let frame_bytes = bits_to_bytes(&bits[sync_start..frame_bits_end]);
        let computed = crate::framer::crc16(&frame_bytes);
        let stored = {
            let b = bits_to_bytes(&bits[payload_end..crc_end]);
            u16::from_le_bytes(b.try_into().unwrap())
        };

        if stored == computed {
            return Ok(Decoded {
                filename,
                data: bits_to_bytes(&bits[payload_start..payload_end]),
            });
        }

        search = sync_start + 1;
    }

    Err("sync word not found".into())
}

// ---------------------------------------------------------------------------
// Non-integer Goertzel DFT
// ---------------------------------------------------------------------------

fn goertzel(samples: &[f64], freq: f64, sample_rate: u32) -> f64 {
    let w = TAU * freq / sample_rate as f64;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0_f64, 0.0_f64);
    for &x in samples {
        let s0 = x + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let real = s1 - s2 * w.cos();
    let imag = s2 * w.sin();
    real * real + imag * imag
}

// ---------------------------------------------------------------------------
// Bit / byte helpers
// ---------------------------------------------------------------------------

pub(crate) fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |acc, (i, &b)| acc | if b { 1 << (7 - i) } else { 0 })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{encoder, framer};

    #[test]
    fn goertzel_discriminates_mark_vs_space() {
        let spb = (SAMPLE_RATE as f64 / BAUD_RATE as f64).round() as usize;
        let mark_samples: Vec<f64> = (0..spb)
            .map(|i| (TAU * MARK_FREQ * i as f64 / SAMPLE_RATE as f64).sin())
            .collect();
        let space_samples: Vec<f64> = (0..spb)
            .map(|i| (TAU * SPACE_FREQ * i as f64 / SAMPLE_RATE as f64).sin())
            .collect();

        assert!(
            goertzel(&mark_samples, MARK_FREQ, SAMPLE_RATE)
                > goertzel(&mark_samples, SPACE_FREQ, SAMPLE_RATE)
        );
        assert!(
            goertzel(&space_samples, SPACE_FREQ, SAMPLE_RATE)
                > goertzel(&space_samples, MARK_FREQ, SAMPLE_RATE)
        );
    }

    #[test]
    fn full_round_trip_text() {
        let payload = b"Hello, AFSK!";
        let samples = encoder::encode(&framer::frame(payload, "hello.txt"));
        let decoded = decode(&samples).expect("decode failed");
        assert_eq!(decoded.data, payload);
        assert_eq!(decoded.filename, "hello.txt");
    }

    #[test]
    fn full_round_trip_all_bytes() {
        let payload: Vec<u8> = (0u8..=255).collect();
        let samples = encoder::encode(&framer::frame(&payload, "all.bin"));
        let decoded = decode(&samples).expect("decode failed");
        assert_eq!(decoded.data, payload);
        assert_eq!(decoded.filename, "all.bin");
    }

    #[test]
    fn full_round_trip_empty() {
        let samples = encoder::encode(&framer::frame(&[], "empty.bin"));
        let decoded = decode(&samples).expect("decode failed");
        assert!(decoded.data.is_empty());
    }

    #[test]
    fn filename_with_dots_preserved() {
        let payload = b"compressed archive";
        let samples = encoder::encode(&framer::frame(payload, "archive.tar.gz"));
        let decoded = decode(&samples).expect("decode failed");
        assert_eq!(decoded.filename, "archive.tar.gz");
    }
}
