//! Feature extraction tool for the judge router.
//!
//! For each track in a baseline TSV (track_id, gt_bpm, det_bpm, ...), this
//! tool decodes the audio and emits a row of training features:
//!
//!   - Per-estimator BPM and confidence (ioi, comb, ac, low_ac, hopf, spectral)
//!   - Final fused BPM and confidence
//!   - Acoustic passport (crest factor, transient density, machine-timed flag,
//!     low/high band onset densities)
//!   - Round 3 metrics (phase coherence, empty slot, median energy, IOI multiple)
//!     evaluated at gt, det, det/2, det*2, det*3
//!   - Label: which candidate matches gt within 4% (0=det, 1=det/2, 2=det*2,
//!     3=det*3, -1=none)
//!
//! Usage:
//!   extract_features <baseline.tsv> <audio_root> <layout> > features.tsv
//!
//! layout: "flat" (audio_root/track_id.{mp3,wav}) or "subdir" (audio_root/*/track_id.wav)

use open_bpm::{bouncer, onset, tempo, DetectOptions};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: {} <baseline.tsv> <audio_root> <layout>", args[0]);
        eprintln!("  layout: flat | subdir");
        std::process::exit(1);
    }

    let baseline_path = &args[1];
    let audio_root = &args[2];
    let layout = args[3].as_str();
    if layout != "flat" && layout != "subdir" {
        eprintln!("Error: layout must be 'flat' or 'subdir'");
        std::process::exit(1);
    }

    // Build flat lookup of basename → full audio path. We accept both mp3 and wav.
    let lookup = if layout == "subdir" {
        build_subdir_lookup(audio_root)
    } else {
        Vec::new()
    };

    let baseline = std::fs::read_to_string(baseline_path)
        .expect("Failed to read baseline TSV");

    print_header();

    let mut count = 0usize;
    let mut errors = 0usize;
    let mut missing = 0usize;

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

        let audio_path = if layout == "flat" {
            // Try mp3 first, then wav
            let mp3 = PathBuf::from(format!("{}/{}.mp3", audio_root, track_id));
            let wav = PathBuf::from(format!("{}/{}.wav", audio_root, track_id));
            if mp3.exists() {
                mp3
            } else if wav.exists() {
                wav
            } else {
                missing += 1;
                continue;
            }
        } else {
            match find_in_lookup(&lookup, track_id) {
                Some(p) => p,
                None => {
                    missing += 1;
                    continue;
                }
            }
        };

        let (samples, sample_rate) = match decode_audio(&audio_path) {
            Ok(r) => r,
            Err(_) => {
                errors += 1;
                continue;
            }
        };

        // Run the full detector to get all estimator outputs
        let opts = DetectOptions::default();
        let result = open_bpm::detect_with_options(&samples, sample_rate, &opts);
        let det_bpm = result.bpm;
        let confidence = result.confidence;

        if det_bpm <= 0.0 {
            errors += 1;
            continue;
        }

        // Acoustic passport (recompute -- detect() doesn't expose it)
        let onsets_full = onset::detect_onsets_multiband(&samples, sample_rate);
        let onset_pairs: Vec<(f64, f64)> =
            onsets_full.iter().map(|o| (o.time, o.strength)).collect();
        let band_counts = onset::count_per_band(&onsets_full);
        let passport =
            bouncer::extract_passport(&samples, sample_rate, &onset_pairs, band_counts);

        let duration = samples.len() as f64 / sample_rate as f64;

        // Per-estimator features (use 0,0 for missing estimators)
        let est = &result.estimators;
        let (ioi_bpm, ioi_conf) = est.ioi.map(|e| (e.bpm, e.confidence)).unwrap_or((0.0, 0.0));
        let (comb_bpm, comb_conf) =
            est.comb.map(|e| (e.bpm, e.confidence)).unwrap_or((0.0, 0.0));
        let (ac_bpm, ac_conf) = est
            .autocorrelation
            .map(|e| (e.bpm, e.confidence))
            .unwrap_or((0.0, 0.0));
        let (lowac_bpm, lowac_conf) = est
            .low_band_ac
            .map(|e| (e.bpm, e.confidence))
            .unwrap_or((0.0, 0.0));
        let (hopf_bpm, hopf_conf) = est.hopf.map(|e| (e.bpm, e.confidence)).unwrap_or((0.0, 0.0));
        let (spec_bpm, spec_conf) =
            est.spectral.map(|e| (e.bpm, e.confidence)).unwrap_or((0.0, 0.0));

        // Round 3 metrics at the four candidates
        let candidates = [det_bpm, det_bpm / 2.0, det_bpm * 2.0, det_bpm * 3.0];
        let mut pc = [0.0f64; 4];
        let mut es = [0.0f64; 4];
        let mut me = [0.0f64; 4];
        let mut io = [0.0f64; 4];
        for (k, &c) in candidates.iter().enumerate() {
            pc[k] = tempo::phase_coherence_r(&onsets_full, c);
            es[k] = tempo::empty_slot_score(&onsets_full, c, duration);
            me[k] = tempo::median_energy_ratio_score(&onsets_full, c);
            io[k] = tempo::ioi_multiple_score(&onsets_full, c);
        }

        // Label: which candidate matches gt within 4%?
        // 0 = det, 1 = det/2, 2 = det*2, 3 = det*3, -1 = none
        let label: i32 = candidates
            .iter()
            .enumerate()
            .find(|(_, &c)| within_pct(c, gt_bpm, 0.04))
            .map(|(idx, _)| idx as i32)
            .unwrap_or(-1);

        println!(
            "{tid}\t{gt}\t{det}\t{conf:.4}\t\
             {ioi_bpm:.2}\t{ioi_conf:.4}\t\
             {comb_bpm:.2}\t{comb_conf:.4}\t\
             {ac_bpm:.2}\t{ac_conf:.4}\t\
             {lowac_bpm:.2}\t{lowac_conf:.4}\t\
             {hopf_bpm:.2}\t{hopf_conf:.4}\t\
             {spec_bpm:.2}\t{spec_conf:.4}\t\
             {cf:.2}\t{td:.2}\t{ts:.4}\t{dlow:.2}\t{dhigh:.2}\t{mt}\t{drumless}\t\
             {dur:.2}\t{n_onsets}\t\
             {pc0:.4}\t{pc1:.4}\t{pc2:.4}\t{pc3:.4}\t\
             {es0:.4}\t{es1:.4}\t{es2:.4}\t{es3:.4}\t\
             {me0:.4}\t{me1:.4}\t{me2:.4}\t{me3:.4}\t\
             {io0:.4}\t{io1:.4}\t{io2:.4}\t{io3:.4}\t\
             {label}",
            tid = track_id,
            gt = gt_bpm,
            det = det_bpm,
            conf = confidence,
            ioi_bpm = ioi_bpm,
            ioi_conf = ioi_conf,
            comb_bpm = comb_bpm,
            comb_conf = comb_conf,
            ac_bpm = ac_bpm,
            ac_conf = ac_conf,
            lowac_bpm = lowac_bpm,
            lowac_conf = lowac_conf,
            hopf_bpm = hopf_bpm,
            hopf_conf = hopf_conf,
            spec_bpm = spec_bpm,
            spec_conf = spec_conf,
            cf = passport.crest_factor_db,
            td = passport.transient_density,
            ts = passport.tempo_stability,
            dlow = passport.d_low,
            dhigh = passport.d_high,
            mt = if passport.is_machine_timed { 1 } else { 0 },
            drumless = if passport.is_drumless { 1 } else { 0 },
            dur = duration,
            n_onsets = onsets_full.len(),
            pc0 = pc[0], pc1 = pc[1], pc2 = pc[2], pc3 = pc[3],
            es0 = es[0], es1 = es[1], es2 = es[2], es3 = es[3],
            me0 = me[0], me1 = me[1], me2 = me[2], me3 = me[3],
            io0 = io[0], io1 = io[1], io2 = io[2], io3 = io[3],
            label = label,
        );

        count += 1;
        if count % 50 == 0 {
            eprintln!("  ... {} tracks processed", count);
        }
    }

    eprintln!(
        "Done: {} tracks processed, {} decode errors, {} missing audio",
        count, errors, missing
    );
}

fn print_header() {
    println!(
        "track_id\tgt_bpm\tdet_bpm\tdet_conf\t\
         ioi_bpm\tioi_conf\t\
         comb_bpm\tcomb_conf\t\
         ac_bpm\tac_conf\t\
         lowac_bpm\tlowac_conf\t\
         hopf_bpm\thopf_conf\t\
         spec_bpm\tspec_conf\t\
         crest_db\ttransient_density\ttempo_stability\td_low\td_high\tmachine_timed\tdrumless\t\
         duration_s\tn_onsets\t\
         pc_det\tpc_half\tpc_double\tpc_triple\t\
         es_det\tes_half\tes_double\tes_triple\t\
         me_det\tme_half\tme_double\tme_triple\t\
         io_det\tio_half\tio_double\tio_triple\t\
         label"
    );
}

fn within_pct(a: f64, b: f64, pct: f64) -> bool {
    if b <= 0.0 {
        return false;
    }
    ((a - b) / b).abs() <= pct
}

fn build_subdir_lookup(root: &str) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let walk = std::fs::read_dir(root).ok();
    if let Some(entries) = walk {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Ok(sub) = std::fs::read_dir(&p) {
                    for f in sub.flatten() {
                        let fp = f.path();
                        if let Some(stem) = fp.file_stem().and_then(|s| s.to_str()) {
                            out.push((stem.to_string(), fp));
                        }
                    }
                }
            } else if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                out.push((stem.to_string(), p));
            }
        }
    }
    out
}

fn find_in_lookup(lookup: &[(String, PathBuf)], track_id: &str) -> Option<PathBuf> {
    lookup
        .iter()
        .find(|(stem, _)| stem == track_id)
        .map(|(_, p)| p.clone())
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
