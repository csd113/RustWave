mod config;
mod decoder;
mod encoder;
mod framer;
mod gui;
mod wav;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "afsk", version, about = "AFSK audio codec", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Encode a file into an AFSK WAV (original filename is stored in the signal)
    Encode {
        #[arg(short, long, value_name = "FILE")]
        input: PathBuf,
        #[arg(short, long, value_name = "FILE")]
        output: PathBuf,
    },
    /// Decode an AFSK WAV — restores the original filename automatically.
    /// If -o is omitted the file is written next to the WAV with its original name.
    Decode {
        #[arg(short, long, value_name = "FILE")]
        input: PathBuf,
        /// Output path (optional — defaults to original filename next to the WAV)
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
}

fn main() {
    if std::env::args().any(|a| a == "-gui" || a == "--gui") {
        gui::run().unwrap_or_else(|e| {
            eprintln!("GUI error: {e}");
            std::process::exit(1);
        });
        return;
    }

    let cli = Cli::parse();

    match cli.command {
        Command::Encode { input, output } => {
            let data = std::fs::read(&input).unwrap_or_else(|e| {
                eprintln!("error: cannot read '{}': {e}", input.display());
                std::process::exit(1);
            });

            // Store just the filename (not full path) in the frame
            let filename = input
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            let framed = framer::frame(&data, &filename);
            let samples = encoder::encode(&framed);

            if let Err(e) = wav::write(&output, &samples) {
                eprintln!("error: cannot write '{}': {e}", output.display());
                std::process::exit(1);
            }

            let duration = samples.len() as f64 / config::SAMPLE_RATE as f64;
            eprintln!(
                "encoded '{}' ({} byte{}) -> {} ({:.2} s)",
                filename,
                data.len(),
                plural(data.len()),
                output.display(),
                duration,
            );
        }

        Command::Decode { input, output } => {
            let samples = wav::read(&input).unwrap_or_else(|e| {
                eprintln!("error: cannot read '{}': {e}", input.display());
                std::process::exit(1);
            });

            let decoded = decoder::decode(&samples).unwrap_or_else(|e| {
                eprintln!("error: decode failed: {e}");
                std::process::exit(1);
            });

            // Use caller-supplied path, or reconstruct from stored filename
            let out_path = output.unwrap_or_else(|| {
                input
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(&decoded.filename)
            });

            if let Err(e) = std::fs::write(&out_path, &decoded.data) {
                eprintln!("error: cannot write '{}': {e}", out_path.display());
                std::process::exit(1);
            }

            eprintln!(
                "decoded {} byte{} -> '{}' (original filename: '{}')",
                decoded.data.len(),
                plural(decoded.data.len()),
                out_path.display(),
                decoded.filename,
            );
        }
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
