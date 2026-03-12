/// PCM sample rate used for all WAV files (Hz).
pub const SAMPLE_RATE: u32 = 44_100;

/// Baud rate: symbols (bits) transmitted per second.
pub const BAUD_RATE: u32 = 1_200;

/// Audio frequency used to represent a mark bit (1) — Hz.
pub const MARK_FREQ: f64 = 1_200.0;

/// Audio frequency used to represent a space bit (0) — Hz.
pub const SPACE_FREQ: f64 = 2_200.0;

/// Sine-wave amplitude in the range [0, 1].
pub const AMPLITUDE: f64 = 0.9;

/// Number of 0xAA preamble bytes prepended before the sync word.
/// These alternating bits let the decoder lock onto the bit clock.
pub const PREAMBLE_LEN: usize = 24;

/// Two-byte sync word that marks the start of the frame header.
pub const SYNC: [u8; 2] = [0x7E, 0x7E];
