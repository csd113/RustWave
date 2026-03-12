```markdown
# RustWave

Encode arbitrary bytes into a WAV file using Bell-202-style **Audio Frequency-Shift Keying**, and decode them back — losslessly.

```
rustwave encode -i data.bin -o signal.wav
rustwave decode -i signal.wav -o data.bin
```

Launch the GUI:

```
./rustwave-cli --gui
```

---

## How it works

```
[ your bytes ]
      │
   FRAMER      preamble (24 × 0xAA) | sync (0x7E 0x7E) | length (u32 LE) | payload | CRC-16
      │
  ENCODER      each bit → sine wave at 1200 Hz (mark=1) or 2200 Hz (space=0)
               continuous-phase FSK at 1200 baud, 44100 Hz / 16-bit mono WAV
      │
  [ .wav ]
      │
  DECODER      non-integer Goertzel filter per bit-window; bit-level sync-word
               search (handles any byte offset); CRC verification
      │
   FRAMER      reconstruct original bytes
      │
[ your bytes ]
```

### Signal parameters

| Parameter      | Value                        |
|----------------|------------------------------|
| Sample rate    | 44 100 Hz                    |
| Bit rate       | 1 200 baud                   |
| Mark (1)       | 1 200 Hz                     |
| Space (0)      | 2 200 Hz                     |
| Modulation     | CPFSK (continuous-phase FSK) |
| WAV format     | 16-bit signed PCM, mono      |
| Frame overhead | 28 bytes (preamble + sync + length + CRC) |

---

## Building

Requires Rust 1.75+ (2021 edition).

```bash
cargo build --release
# binary is at target/release/rustwave-cli
```

## Running tests

```bash
cargo test
```

12 unit + integration tests cover:
- Framer round-trips (empty, ASCII, all-256-bytes, corrupt CRC)
- CRC-16/CCITT known-value check
- WAV write/read round-trip (silence and sine wave)
- Goertzel mark-vs-space discrimination
- Full encode→decode round-trips (empty, text, all byte values)

---

## Project layout

```
src/
  main.rs       CLI (clap — encode / decode subcommands, --gui flag)
  config.rs     Shared constants (sample rate, baud rate, frequencies)
  framer.rs     Byte envelope: preamble, sync word, length, CRC-16/CCITT
  wav.rs        WAV file I/O via hound
  encoder.rs    Bytes → CPFSK audio samples
  decoder.rs    Audio samples → bytes (Goertzel filter, bit-level sync search)
```
```