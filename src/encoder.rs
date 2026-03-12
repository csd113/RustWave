use crate::config::*;
use std::f64::consts::TAU;

/// Encode `framed` bytes into PCM audio samples.
/// Calls `on_progress` with a value in 0.0..=1.0 as bits are processed.
pub fn encode_progress(framed: &[u8], on_progress: impl Fn(f32)) -> Vec<f64> {
    let spb = SAMPLE_RATE as f64 / BAUD_RATE as f64;

    let bits: Vec<bool> = framed
        .iter()
        .flat_map(|&byte| (0..8u8).rev().map(move |i| (byte >> i) & 1 == 1))
        .collect();

    let total_bits = bits.len().max(1);
    let silence_len = (SAMPLE_RATE as f64 * 0.05) as usize;
    let signal_len = ((bits.len() as f64) * spb).round() as usize;
    let mut samples = Vec::with_capacity(silence_len * 2 + signal_len);

    samples.extend(std::iter::repeat(0.0_f64).take(silence_len));

    let mut phase = 0.0_f64;
    for (idx, &bit) in bits.iter().enumerate() {
        let freq = if bit { MARK_FREQ } else { SPACE_FREQ };
        let phase_inc = TAU * freq / SAMPLE_RATE as f64;

        let start = (idx as f64 * spb).round() as usize;
        let end = ((idx + 1) as f64 * spb).round() as usize;

        for _ in start..end {
            samples.push(AMPLITUDE * phase.sin());
            phase = (phase + phase_inc) % TAU;
        }

        // Report progress every 64 bits to avoid lock contention
        if idx % 64 == 0 {
            on_progress(idx as f32 / total_bits as f32);
        }
    }

    samples.extend(std::iter::repeat(0.0_f64).take(silence_len));
    on_progress(1.0);
    samples
}

/// Convenience wrapper with no progress reporting (used by CLI and tests).
pub fn encode(framed: &[u8]) -> Vec<f64> {
    encode_progress(framed, |_| {})
}
