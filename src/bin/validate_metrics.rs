//! Validation tool: computes harmonic_template_score and mdl_score at
//! ground-truth BPM vs detected BPM for each track in the baseline TSV.
//!
//! Usage:
//!   cargo run --release --bin validate_metrics -- <baseline.tsv> <audio_dir> > validation.tsv

use open_bpm::{onset, tempo};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <baseline.tsv> <audio_dir>", args[0]);
        std::process::exit(1);
    }

    let baseline_path = &args[1];
    let audio_dir = &args[2];

    let baseline = std::fs::read_to_string(baseline_path)
        .expect("Failed to read baseline TSV");

    // Output header
    println!("track_id\tgt_bpm\tdet_bpm\tacc1\tht_at_gt\tht_at_det\tht_at_half\tht_at_double\tmdl_at_gt\tmdl_at_det\tmdl_at_half\tmdl_at_double");

    let mut count = 0;
    let mut errors = 0;

    for (i, line) in baseline.lines().enumerate() {
        if i == 0 {
            continue; // skip header
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 6 {
            continue;
        }

        let track_id = parts[0];
        let gt_bpm: f64 = match parts[1].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let det_bpm: f64 = match parts[2].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let acc1 = parts[4];

        if det_bpm <= 0.0 {
            continue;
        }

        let audio_path = PathBuf::from(format!("{}/{}.mp3", audio_dir, track_id));
        if !audio_path.exists() {
            continue;
        }

        let (samples, sample_rate) = match decode_audio(&audio_path) {
            Ok(r) => r,
            Err(_) => {
                errors += 1;
                continue;
            }
        };

        let sr = sample_rate as f64;
        let onset_env = onset::onset_strength_envelope(&samples, sample_rate);
        let onsets = onset::detect_onsets_multiband(&samples, sample_rate);
        let duration = samples.len() as f64 / sr;

        let half_bpm = det_bpm / 2.0;
        let double_bpm = det_bpm * 2.0;

        let ht_at_gt = tempo::harmonic_template_score(&onset_env, sr, gt_bpm);
        let ht_at_det = tempo::harmonic_template_score(&onset_env, sr, det_bpm);
        let ht_at_half = tempo::harmonic_template_score(&onset_env, sr, half_bpm);
        let ht_at_double = tempo::harmonic_template_score(&onset_env, sr, double_bpm);
        let mdl_at_gt = tempo::mdl_score(&onsets, gt_bpm, duration);
        let mdl_at_det = tempo::mdl_score(&onsets, det_bpm, duration);
        let mdl_at_half = tempo::mdl_score(&onsets, half_bpm, duration);
        let mdl_at_double = tempo::mdl_score(&onsets, double_bpm, duration);

        println!(
            "{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}",
            track_id, gt_bpm, det_bpm, acc1,
            ht_at_gt, ht_at_det, ht_at_half, ht_at_double,
            mdl_at_gt, mdl_at_det, mdl_at_half, mdl_at_double
        );

        count += 1;
        if count % 50 == 0 {
            eprintln!("  ... {} tracks processed", count);
        }
    }

    eprintln!("Done: {} tracks processed, {} errors", count, errors);
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
