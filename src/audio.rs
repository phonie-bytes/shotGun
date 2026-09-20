// src/audio.rs
//! System audio (loopback) capture via WASAPI, written out as a WAV file.
//!
//! Captures whatever is playing through the default output device (the
//! "what you hear" signal), not the microphone, so recordings include
//! system/app audio the way a normal screen recorder does. `cpal` (an
//! earlier unused dependency here) doesn't expose WASAPI loopback mode, so
//! this talks to Core Audio directly via the `windows` crate.

use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use windows::core::GUID;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};

// {00000003-0000-0010-8000-00AA00389B71} KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID = GUID::from_values(
    0x0000_0003,
    0x0000,
    0x0010,
    [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71],
);

const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;

#[repr(C)]
#[derive(Clone, Copy)]
struct WaveFormatExtensible {
    format: WAVEFORMATEX,
    samples: u16,
    channel_mask: u32,
    sub_format: GUID,
}

/// Tracks how many audio frames have been written against how many *should*
/// exist by now on the wall clock, so silent stretches can be padded.
///
/// WASAPI loopback only delivers packets while something is actively
/// rendering audio — a genuinely silent stretch produces no packets at all,
/// so writing only what arrives makes the WAV shorter than the recording
/// and drifts audio/video sync. Kept as its own struct (no WASAPI in it) so
/// the accounting is unit-testable.
struct GapFiller {
    sample_rate: u64,
    start: Instant,
    frames_written: u64,
}

impl GapFiller {
    fn new(sample_rate: u32, start: Instant) -> Self {
        Self { sample_rate: sample_rate as u64, start, frames_written: 0 }
    }

    fn record(&mut self, frames: u64) {
        self.frames_written += frames;
    }

    /// Frames of silence needed to catch the file up to the wall clock, or
    /// 0 if it's within `threshold_frames` of it. The threshold keeps normal
    /// packet-arrival jitter from being "corrected" with tiny zero-padded
    /// glitches while audio is actually playing; pass 0 for the final
    /// flush at stop so the file's length matches the recording exactly.
    fn frames_owed(&self, now: Instant, threshold_frames: u64) -> u64 {
        let elapsed = now.saturating_duration_since(self.start);
        let expected = (elapsed.as_secs_f64() * self.sample_rate as f64) as u64;
        let owed = expected.saturating_sub(self.frames_written);
        if owed > threshold_frames { owed } else { 0 }
    }
}

pub enum AudioCaptureOutcome {
    Recorded { wav_path: PathBuf },
    Unavailable { reason: String },
}

/// Records system audio loopback to `wav_path` until `stop_flag` is set.
/// Meant to be run on its own thread (COM is initialized per-thread); it
/// blocks until `stop_flag` is set and cleanup completes.
pub fn record_loopback_to_wav(wav_path: PathBuf, stop_flag: Arc<AtomicBool>) -> AudioCaptureOutcome {
    // The audio timeline starts *now*, not when the stream finally starts:
    // COM init, endpoint activation and `IAudioClient::Start` together take a
    // few hundred ms before any audio can flow, and the video loop is
    // already running by then. Anchoring here turns that startup latency
    // into leading silence instead of a permanent audio-vs-video offset.
    let started = Instant::now();
    unsafe {
        if let Err(e) = CoInitializeEx(None, COINIT_MULTITHREADED).ok() {
            return AudioCaptureOutcome::Unavailable {
                reason: format!("CoInitializeEx failed: {e}"),
            };
        }
        let result = capture(&wav_path, &stop_flag, started);
        CoUninitialize();
        match result {
            Ok(()) => AudioCaptureOutcome::Recorded { wav_path },
            Err(reason) => AudioCaptureOutcome::Unavailable { reason },
        }
    }
}

unsafe fn capture(wav_path: &Path, stop_flag: &Arc<AtomicBool>, started: Instant) -> Result<(), String> {
    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
        .map_err(|e| format!("Failed to create device enumerator: {e}"))?;

    let device = enumerator
        .GetDefaultAudioEndpoint(eRender, eConsole)
        .map_err(|e| format!("No default playback device: {e}"))?;

    let audio_client: IAudioClient = device
        .Activate(CLSCTX_ALL, None)
        .map_err(|e| format!("Failed to activate audio client: {e}"))?;

    let mix_format_ptr = audio_client
        .GetMixFormat()
        .map_err(|e| format!("Failed to get mix format: {e}"))?;

    let capture_result = capture_with_format(&audio_client, mix_format_ptr, wav_path, stop_flag, started);
    CoTaskMemFree(Some(mix_format_ptr as *const _));
    capture_result
}

unsafe fn capture_with_format(
    audio_client: &IAudioClient,
    mix_format_ptr: *mut WAVEFORMATEX,
    wav_path: &Path,
    stop_flag: &Arc<AtomicBool>,
    started: Instant,
) -> Result<(), String> {
    // WAVEFORMATEX is packed; read an owned, aligned copy before touching fields.
    let mix_format = mix_format_ptr.read_unaligned();
    let channels = mix_format.nChannels;
    let sample_rate = mix_format.nSamplesPerSec;
    let bits_per_sample = mix_format.wBitsPerSample;
    let format_tag = mix_format.wFormatTag;

    let is_float = if format_tag == WAVE_FORMAT_EXTENSIBLE {
        let ext = (mix_format_ptr as *const WaveFormatExtensible).read_unaligned();
        ext.sub_format == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
    } else {
        format_tag == WAVE_FORMAT_IEEE_FLOAT
    };

    if !is_float {
        return Err(format!(
            "Unsupported audio mix format (tag={format_tag}, bits={bits_per_sample}); expected IEEE float"
        ));
    }

    // 200ms buffer, expressed in 100-nanosecond units as WASAPI expects.
    let buffer_duration_hns: i64 = 200 * 10_000;

    audio_client
        .Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            buffer_duration_hns,
            0,
            mix_format_ptr,
            None,
        )
        .map_err(|e| format!("Failed to initialize audio client: {e}"))?;

    let capture_client: IAudioCaptureClient = audio_client
        .GetService()
        .map_err(|e| format!("Failed to get capture client: {e}"))?;

    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(wav_path, spec)
        .map_err(|e| format!("Failed to create WAV file: {e}"))?;

    audio_client
        .Start()
        .map_err(|e| format!("Failed to start audio client: {e}"))?;

    let mut gap = GapFiller::new(sample_rate, started);
    // ~50ms: comfortably above packet-arrival jitter, well below anything audible as a gap.
    let gap_threshold = sample_rate as u64 / 20;

    while !stop_flag.load(Ordering::SeqCst) {
        let mut packet_size = capture_client
            .GetNextPacketSize()
            .map_err(|e| format!("GetNextPacketSize failed: {e}"))?;

        while packet_size != 0 {
            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut num_frames: u32 = 0;
            let mut flags: u32 = 0;

            capture_client
                .GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)
                .map_err(|e| format!("GetBuffer failed: {e}"))?;

            let silent = (flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0;
            let sample_count = (num_frames as usize) * (channels as usize);

            if silent || data_ptr.is_null() {
                for _ in 0..sample_count {
                    let _ = writer.write_sample(0.0f32);
                }
            } else {
                let samples = std::slice::from_raw_parts(data_ptr as *const f32, sample_count);
                for &s in samples {
                    let _ = writer.write_sample(s);
                }
            }

            gap.record(num_frames as u64);

            capture_client
                .ReleaseBuffer(num_frames)
                .map_err(|e| format!("ReleaseBuffer failed: {e}"))?;

            packet_size = capture_client
                .GetNextPacketSize()
                .map_err(|e| format!("GetNextPacketSize failed: {e}"))?;
        }

        // No packet for a while (nothing rendering) — pad silence so the file
        // keeps up with the wall clock instead of falling behind the video.
        let owed = gap.frames_owed(Instant::now(), gap_threshold);
        if owed > 0 {
            for _ in 0..owed * channels as u64 {
                let _ = writer.write_sample(0.0f32);
            }
            gap.record(owed);
        }

        std::thread::sleep(Duration::from_millis(10));
    }

    // Final flush: make the file's length match the recording exactly, even
    // if the last stretch was silent.
    let owed = gap.frames_owed(Instant::now(), 0);
    for _ in 0..owed * channels as u64 {
        let _ = writer.write_sample(0.0f32);
    }

    let _ = audio_client.Stop();
    writer
        .finalize()
        .map_err(|e| format!("Failed to finalize WAV file: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    #[test]
    fn loopback_capture_produces_a_wav_file() {
        let dir = std::env::temp_dir().join("shotgun_audio_test");
        let _ = std::fs::create_dir_all(&dir);
        let wav_path = dir.join("smoke_test.wav");

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();
        let wav_clone = wav_path.clone();
        let spawned = std::time::Instant::now();
        let handle = std::thread::spawn(move || record_loopback_to_wav(wav_clone, stop_clone));

        // The audio timeline is anchored at thread entry (see
        // `record_loopback_to_wav`), so time the recording from just before
        // spawn. Device setup latency must show up as leading silence, not as
        // a shorter file.
        std::thread::sleep(StdDuration::from_secs(2));
        stop_flag.store(true, Ordering::SeqCst);
        let ran_for = spawned.elapsed().as_secs_f64();
        let outcome = handle.join().expect("capture thread panicked");

        match outcome {
            AudioCaptureOutcome::Recorded { wav_path } => {
                // Length must track the wall clock whether or not anything was
                // playing (the old `size > 44` check passed even for a header-only
                // file, since WASAPI's extensible header alone is ~68 bytes).
                let reader = hound::WavReader::open(&wav_path).expect("wav should open");
                let spec = reader.spec();
                let secs = reader.duration() as f64 / spec.sample_rate as f64;
                println!("Captured WAV: {secs:.2}s for a {ran_for:.2}s recording at {}", wav_path.display());
                assert!(
                    (secs - ran_for).abs() < 0.25,
                    "WAV is {secs:.2}s but the capture ran {ran_for:.2}s — silent stretches aren't being padded"
                );
            }
            AudioCaptureOutcome::Unavailable { reason } => {
                panic!("Loopback capture unavailable: {reason}");
            }
        }
    }

    #[test]
    fn gap_filler_owes_the_wall_clock_minus_what_was_written() {
        let t0 = Instant::now();
        let mut g = GapFiller::new(48_000, t0);
        // 2s in, nothing written: owe 2s of frames.
        assert_eq!(g.frames_owed(t0 + Duration::from_secs(2), 0), 96_000);
        // After writing 1s worth, owe the remaining second.
        g.record(48_000);
        assert_eq!(g.frames_owed(t0 + Duration::from_secs(2), 0), 48_000);
    }

    #[test]
    fn gap_filler_ignores_jitter_below_threshold_but_final_flush_does_not() {
        let t0 = Instant::now();
        let mut g = GapFiller::new(48_000, t0);
        g.record(48_000);
        let now = t0 + Duration::from_millis(1_030); // 30ms behind
        assert_eq!(g.frames_owed(now, 2_400), 0, "30ms behind is within the 50ms jitter threshold");
        assert_eq!(g.frames_owed(now, 0), 1_440, "final flush (threshold 0) pads the last 30ms");
    }

    #[test]
    fn gap_filler_never_owes_when_ahead_of_the_clock() {
        let t0 = Instant::now();
        let mut g = GapFiller::new(48_000, t0);
        g.record(100_000);
        assert_eq!(g.frames_owed(t0 + Duration::from_secs(1), 0), 0);
    }
}
