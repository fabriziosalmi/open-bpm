//! CLI for open-bpm.

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "open-bpm", version, about = "High-accuracy BPM detection")]
struct Cli {
    /// Audio file path (WAV, MP3, FLAC, OGG, AAC)
    file: PathBuf,

    /// Minimum BPM to detect
    #[arg(long, default_value = "60")]
    min_bpm: f64,

    /// Maximum BPM to detect
    #[arg(long, default_value = "200")]
    max_bpm: f64,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    format: String,

    /// Disable segmented analysis (analyze full track at once)
    #[arg(long)]
    no_segments: bool,

    /// Show per-estimator diagnostics
    #[arg(long)]
    verbose: bool,
}

fn main() {
    let cli = Cli::parse();

    // Decode audio file
    let (samples, sample_rate) = match decode_audio(&cli.file) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let opts = open_bpm::DetectOptions {
        min_bpm: cli.min_bpm,
        max_bpm: cli.max_bpm,
        segmented: !cli.no_segments,
        ..Default::default()
    };

    let start = std::time::Instant::now();
    let result = open_bpm::detect_with_options(&samples, sample_rate, &opts);
    let elapsed = start.elapsed();

    match cli.format.as_str() {
        "json" => {
            println!("{{");
            println!("  \"bpm\": {},", result.bpm);
            println!("  \"confidence\": {:.4},", result.confidence);
            println!("  \"grid_offset\": {:.6},", result.grid_offset);
            println!("  \"elapsed_ms\": {:.1}", elapsed.as_secs_f64() * 1000.0);
            if cli.verbose {
                println!("  ,\"estimators\": {{");
                if let Some(ioi) = result.estimators.ioi {
                    println!(
                        "    \"ioi\": {{ \"bpm\": {:.2}, \"confidence\": {:.4} }},",
                        ioi.bpm, ioi.confidence
                    );
                }
                if let Some(comb) = result.estimators.comb {
                    println!(
                        "    \"comb\": {{ \"bpm\": {:.2}, \"confidence\": {:.4} }},",
                        comb.bpm, comb.confidence
                    );
                }
                if let Some(ac) = result.estimators.autocorrelation {
                    println!(
                        "    \"autocorrelation\": {{ \"bpm\": {:.2}, \"confidence\": {:.4} }},",
                        ac.bpm, ac.confidence
                    );
                }
                if let Some(tg) = result.estimators.tempogram {
                    println!(
                        "    \"tempogram\": {{ \"bpm\": {:.2}, \"confidence\": {:.4} }}",
                        tg.bpm, tg.confidence
                    );
                }
                println!("  }}");
            }
            println!("}}");
        }
        _ => {
            println!("{:.2} BPM", result.bpm);
            if result.confidence < 0.3 {
                println!("  (low confidence: {:.0}%)", result.confidence * 100.0);
            } else if cli.verbose {
                println!("  confidence: {:.0}%", result.confidence * 100.0);
            }
            if cli.verbose {
                println!("  grid offset: {:.3}s", result.grid_offset);
                println!("  elapsed: {:.1}ms", elapsed.as_secs_f64() * 1000.0);
                if let Some(ioi) = result.estimators.ioi {
                    println!("  IOI histogram: {:.2} BPM ({:.0}%)", ioi.bpm, ioi.confidence * 100.0);
                }
                if let Some(comb) = result.estimators.comb {
                    println!("  Comb filter:   {:.2} BPM ({:.0}%)", comb.bpm, comb.confidence * 100.0);
                }
                if let Some(ac) = result.estimators.autocorrelation {
                    println!("  Autocorrel.:   {:.2} BPM ({:.0}%)", ac.bpm, ac.confidence * 100.0);
                }
                if let Some(h) = result.estimators.hopf {
                    println!("  Hopf SBERN:    {:.2} BPM ({:.0}%)", h.bpm, h.confidence * 100.0);
                }
                println!(
                    "  duration: {:.1}s ({} samples @ {}Hz)",
                    samples.len() as f64 / sample_rate as f64,
                    samples.len(),
                    sample_rate
                );
            }
        }
    }
}

fn decode_audio(path: &PathBuf) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error>> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or("No audio track found")?;

    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or("Unknown sample rate")?;
    let n_channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2);
    let track_id = track.id;

    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut all_samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(_) => break,
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let spec = *decoded.spec();
        let n_frames = decoded.capacity();

        let mut sample_buf = SampleBuffer::<f32>::new(n_frames as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);
        let interleaved = sample_buf.samples();

        // Mix to mono
        if n_channels == 1 {
            all_samples.extend_from_slice(interleaved);
        } else {
            for chunk in interleaved.chunks(n_channels) {
                let mono: f32 = chunk.iter().sum::<f32>() / n_channels as f32;
                all_samples.push(mono);
            }
        }
    }

    if all_samples.is_empty() {
        return Err("No audio samples decoded".into());
    }

    Ok((all_samples, sample_rate))
}
