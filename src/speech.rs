//! Speech to text on this computer: the microphone, audio files, and the Whisper model.
//!
//! The model is downloaded once from the whisper.cpp project, checked against its published
//! SHA-256, and kept in the app's data folder. Nothing is sent anywhere to transcribe.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Once,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

/// Whisper wants 16 kHz mono.
pub const RATE: u32 = 16_000;
/// A dictation stops itself after this long.
pub const LONGEST: Duration = Duration::from_secs(5 * 60);

pub struct ModelFile {
    pub name: &'static str,
    pub url: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

/// Whisper base, quantised: multilingual, about 60 MB, and fast enough on a laptop CPU.
pub const MODEL: ModelFile = ModelFile {
    name: "ggml-base-q5_1.bin",
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin",
    bytes: 59_707_625,
    sha256: "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898",
};

pub fn model_path(data: &Path) -> PathBuf {
    data.join("models").join(MODEL.name)
}

/// The model, when it has been downloaded and is the expected size.
pub fn installed(data: &Path) -> Option<PathBuf> {
    let path = model_path(data);
    fs::metadata(&path)
        .is_ok_and(|meta| meta.len() == MODEL.bytes)
        .then_some(path)
}

pub fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// Downloads the model into `data`. `received` counts bytes as they arrive; setting `cancel`
/// stops the download and removes the partial file.
pub fn download(data: &Path, received: &AtomicU64, cancel: &AtomicBool) -> Result<PathBuf, String> {
    let destination = model_path(data);
    let folder = destination.parent().expect("model path has a folder");
    fs::create_dir_all(folder)
        .map_err(|error| format!("Couldn’t create {}: {error}", folder.display()))?;
    let partial = destination.with_extension("part");
    let result = (|| {
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(20)))
            .build();
        let agent: ureq::Agent = config.into();
        let mut response = agent
            .get(MODEL.url)
            .call()
            .map_err(|error| format!("The download didn’t start: {error}"))?;
        let mut body = response
            .body_mut()
            .with_config()
            .limit(MODEL.bytes + 1)
            .reader();
        let mut file = fs::File::create(&partial)
            .map_err(|error| format!("Couldn’t save the model: {error}"))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled".to_owned());
            }
            let count = body
                .read(&mut buffer)
                .map_err(|error| format!("The download stopped: {error}"))?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
            file.write_all(&buffer[..count])
                .map_err(|error| format!("Couldn’t save the model: {error}"))?;
            received.fetch_add(count as u64, Ordering::Relaxed);
        }
        file.sync_all()
            .map_err(|error| format!("Couldn’t save the model: {error}"))?;
        let digest: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if digest != MODEL.sha256 {
            return Err("The downloaded model didn’t match its published checksum, so it was discarded. Try again.".to_owned());
        }
        fs::rename(&partial, &destination)
            .map_err(|error| format!("Couldn’t save the model: {error}"))?;
        Ok(destination.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

/// Mixes interleaved frames down to one channel.
fn mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

/// Converts a sample rate by averaging each output sample's share of the input. Averaging
/// filters out most of what would otherwise alias when lowering the rate.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let step = from as f64 / to as f64;
    let length = (input.len() as f64 / step).floor() as usize;
    (0..length)
        .map(|index| {
            let start = index as f64 * step;
            if step <= 1.0 {
                // Raising the rate: interpolate between neighbours.
                let base = start.floor() as usize;
                let next = (base + 1).min(input.len() - 1);
                let weight = (start - base as f64) as f32;
                input[base] * (1.0 - weight) + input[next] * weight
            } else {
                let first = start.floor() as usize;
                let last = ((start + step).ceil() as usize).min(input.len());
                let span = &input[first..last.max(first + 1)];
                span.iter().sum::<f32>() / span.len() as f32
            }
        })
        .collect()
}

/// Decodes an audio file to 16 kHz mono. Also returns its length in whole seconds.
pub fn decode_file(path: &Path) -> Result<(Vec<f32>, u64), String> {
    use symphonia::core::{
        codecs::audio::AudioDecoderOptions,
        errors::Error,
        formats::{FormatOptions, TrackType, probe::Hint},
        io::MediaSourceStream,
        meta::MetadataOptions,
    };

    let file = fs::File::open(path).map_err(|error| format!("Couldn’t open it: {error}"))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(ext);
    }
    let unreadable = |_| {
        "This audio format can’t be decoded here. Convert it to WAV, MP3, M4A, or FLAC.".to_owned()
    };
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(unreadable)?;
    // Some writers don't mark a default track; the first audio track is the one to read.
    let track = format
        .default_track(TrackType::Audio)
        .or_else(|| format.first_track(TrackType::Audio))
        .ok_or("It has no audio track.")?;
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or("It has no audio track.")?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(parameters, &AudioDecoderOptions::default())
        .map_err(unreadable)?;
    let track_id = track.id;

    let mut samples = Vec::new();
    let mut rate = 0;
    let mut buffer: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(Error::ResetRequired) => break,
            Err(error) => return Err(format!("Couldn’t read the audio: {error}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = decoded.spec();
                rate = spec.rate();
                let channels = spec.channels().count().max(1);
                buffer.resize(decoded.samples_interleaved(), 0.0);
                decoded.copy_to_slice_interleaved(&mut buffer);
                samples.extend(mono(&buffer, channels));
            }
            // A damaged packet is skipped, as players do.
            Err(Error::DecodeError(_)) => continue,
            Err(error) => return Err(format!("Couldn’t decode the audio: {error}")),
        }
    }
    if samples.is_empty() || rate == 0 {
        return Err("It contains no audio.".into());
    }
    let seconds = samples.len() as u64 / rate as u64;
    Ok((resample(&samples, rate, RATE), seconds))
}

/// Languages dictation can be set to, as Whisper codes and names. Detection covers the rest.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("zh", "Chinese"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("pt", "Portuguese"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("hi", "Hindi"),
    ("ar", "Arabic"),
    ("ru", "Russian"),
    ("it", "Italian"),
];

/// Turns 16 kHz mono speech into text in `language`, or the language it detects.
pub fn transcribe(model: &Path, samples: &[f32], language: Option<&str>) -> Result<String, String> {
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    static QUIET: Once = Once::new();
    // whisper.cpp otherwise writes its progress to the terminal.
    QUIET.call_once(whisper_rs::install_logging_hooks);

    if samples.len() < RATE as usize / 4 {
        return Ok(String::new());
    }
    let context = WhisperContext::new_with_params(model, WhisperContextParameters::default())
        .map_err(|error| format!("Couldn’t load the speech model: {error}"))?;
    let mut state = context
        .create_state()
        .map_err(|error| format!("Couldn’t start transcribing: {error}"))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(language.unwrap_or("auto")));
    let threads = thread::available_parallelism().map_or(4, |count| count.get().min(8));
    params.set_n_threads(threads as i32);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);
    state
        .full(params, samples)
        .map_err(|error| format!("Transcribing failed: {error}"))?;
    let mut text = String::new();
    for segment in state.as_iter() {
        let part = segment
            .to_str_lossy()
            .map_err(|error| format!("Transcribing failed: {error}"))?;
        let part = part.trim();
        // Whisper marks silence and noise in brackets; those are not words.
        if part.is_empty() || (part.starts_with('[') && part.ends_with(']')) {
            continue;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(part);
    }
    Ok(text)
}

/// Records from the default microphone until finished or cancelled.
pub struct Recorder {
    stop: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<f32>>>,
    /// The latest input level, 0 to 1, as `f32` bits.
    level: Arc<AtomicU32>,
    rate: u32,
    worker: Option<thread::JoinHandle<()>>,
    started: Instant,
}

impl Recorder {
    pub fn start() -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let samples = Arc::new(Mutex::new(Vec::new()));
        let level = Arc::new(AtomicU32::new(0));
        let (ready, started) = mpsc::channel();
        let worker = {
            let (stop, samples, level) = (stop.clone(), samples.clone(), level.clone());
            // Audio streams are not sendable on every platform, so the stream lives and dies
            // on this thread.
            thread::spawn(move || {
                let stream = match open_microphone(samples, level) {
                    Ok((stream, rate)) => {
                        let _ = ready.send(Ok(rate));
                        stream
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                while !stop.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(20));
                }
                drop(stream);
            })
        };
        let rate = started
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "The microphone didn’t respond.".to_owned())??;
        Ok(Self {
            stop,
            samples,
            level,
            rate,
            worker: Some(worker),
            started: Instant::now(),
        })
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    /// Stops recording and returns the audio at 16 kHz mono.
    pub fn finish(mut self) -> Vec<f32> {
        self.halt();
        let samples = std::mem::take(&mut *self.samples.lock().unwrap_or_else(|e| e.into_inner()));
        resample(&samples, self.rate, RATE)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.halt();
    }
}

fn open_microphone(
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
) -> Result<(cpal::Stream, u32), String> {
    use cpal::{
        FromSample, Sample, SampleFormat, SizedSample,
        traits::{DeviceTrait, HostTrait, StreamTrait},
    };

    let device = cpal::default_host()
        .default_input_device()
        .ok_or("No microphone was found.")?;
    let supported = device
        .default_input_config()
        .map_err(|error| format!("The microphone isn’t available: {error}"))?;
    let channels = supported.channels() as usize;
    let rate = supported.sample_rate();
    let format = supported.sample_format();
    let config = supported.config();

    fn handler<T>(
        channels: usize,
        samples: Arc<Mutex<Vec<f32>>>,
        level: Arc<AtomicU32>,
    ) -> impl FnMut(&[T], &cpal::InputCallbackInfo) + Send + 'static
    where
        T: Sample,
        f32: FromSample<T>,
    {
        move |data: &[T], _| {
            let frames: Vec<f32> = data.iter().map(|&value| f32::from_sample(value)).collect();
            let mixed = mono(&frames, channels);
            let peak = mixed
                .iter()
                .fold(0.0f32, |peak, value| peak.max(value.abs()));
            level.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
            if let Ok(mut store) = samples.lock() {
                // Stop growing once the dictation limit is reached.
                if store.len() < (LONGEST.as_secs() as usize + 5) * 192_000 {
                    store.extend(mixed);
                }
            }
        }
    }
    fn build<T>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        channels: usize,
        samples: Arc<Mutex<Vec<f32>>>,
        level: Arc<AtomicU32>,
    ) -> Result<cpal::Stream, cpal::Error>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        device.build_input_stream(config, handler::<T>(channels, samples, level), |_| {}, None)
    }

    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, channels, samples, level),
        SampleFormat::I16 => build::<i16>(&device, config, channels, samples, level),
        SampleFormat::I32 => build::<i32>(&device, config, channels, samples, level),
        SampleFormat::U16 => build::<u16>(&device, config, channels, samples, level),
        SampleFormat::I8 => build::<i8>(&device, config, channels, samples, level),
        SampleFormat::U8 => build::<u8>(&device, config, channels, samples, level),
        other => {
            return Err(format!(
                "The microphone uses an unsupported format ({other})."
            ));
        }
    }
    .map_err(|error| format!("The microphone isn’t available: {error}"))?;
    stream
        .play()
        .map_err(|error| format!("The microphone didn’t start: {error}"))?;
    Ok((stream, rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_length_and_level() {
        let tone: Vec<f32> = (0..48_000).map(|_| 0.5).collect();
        let lower = resample(&tone, 48_000, RATE);
        assert_eq!(lower.len(), 16_000);
        assert!(lower.iter().all(|value| (value - 0.5).abs() < 1e-6));
        let higher = resample(&[0.0, 1.0], 8_000, RATE);
        assert_eq!(higher, vec![0.0, 0.5, 1.0, 1.0]);
        assert_eq!(resample(&[0.25; 10], RATE, RATE), vec![0.25; 10]);
    }

    #[test]
    fn mixing_averages_each_frame() {
        assert_eq!(mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(mono(&[0.2, 0.4], 1), vec![0.2, 0.4]);
    }

    #[test]
    fn decodes_a_wav_file_to_sixteen_kilohertz() {
        // One second of 44.1 kHz stereo silence with a 440 Hz tone on the left.
        let rate = 44_100u32;
        let mut data = Vec::new();
        for index in 0..rate {
            let tone = ((index as f32 / rate as f32) * 440.0 * std::f32::consts::TAU).sin();
            data.extend(((tone * 16_000.0) as i16).to_le_bytes());
            data.extend(0i16.to_le_bytes());
        }
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((36 + data.len() as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(rate.to_le_bytes());
        wav.extend((rate * 4).to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((data.len() as u32).to_le_bytes());
        wav.extend(data);
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("tone.wav");
        fs::write(&path, wav).unwrap();

        let (samples, seconds) = decode_file(&path).unwrap();
        assert_eq!(seconds, 1);
        assert!(
            (15_990..=16_000).contains(&samples.len()),
            "{}",
            samples.len()
        );
        let peak = samples
            .iter()
            .fold(0.0f32, |peak, value| peak.max(value.abs()));
        assert!(peak > 0.2 && peak < 0.3, "{peak}");
        assert!(decode_file(&folder.path().join("missing.wav")).is_err());
    }

    /// Transcribes recorded speech with the downloaded model:
    /// `SUPER_OPENCODE_FIXTURES=<folder> SUPER_OPENCODE_SPEECH_MODEL=<model> cargo test --release
    /// transcribes_real_speech -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn transcribes_real_speech() {
        let folder = std::env::var("SUPER_OPENCODE_FIXTURES").expect("fixture folder");
        let model = std::env::var("SUPER_OPENCODE_SPEECH_MODEL").expect("speech model");
        for name in ["speech.wav", "speech-aac.m4a"] {
            let started = Instant::now();
            let (samples, seconds) = decode_file(&Path::new(&folder).join(name)).unwrap();
            let text = transcribe(Path::new(&model), &samples, None).unwrap();
            println!(
                "--- {name} ({seconds} s of audio, {:.1} s to transcribe): {text}",
                started.elapsed().as_secs_f32()
            );
            assert!(text.contains("Thursday"), "{text}");
            assert!(text.to_lowercase().contains("release notes"), "{text}");
        }
    }

    #[test]
    fn a_missing_model_is_not_installed() {
        let folder = tempfile::tempdir().unwrap();
        assert!(installed(folder.path()).is_none());
        let path = model_path(folder.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"too short").unwrap();
        assert!(installed(folder.path()).is_none());
    }
}
