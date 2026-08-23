// src/video.rs
//! Video capture module (prototype)
//!
//! Captures frames via the DXGI Desktop Duplication API (through xcap's
//! `VideoRecorder`) — a real continuous capture stream driven by actual
//! screen updates, not a poll-and-screenshot loop — and stores them as PNG
//! files in a dedicated sub-folder, while a parallel thread records system
//! audio loopback (see `crate::audio`) to a WAV file. After capture stops,
//! it attempts to mux both into an MP4 via an external `ffmpeg` binary.
//! The full in-process H.264 pipeline is future work.

use crate::audio::{self, AudioCaptureOutcome};
use crate::capture::{capture_region, save_image_with_format};
use crate::config::{AppConfig, OutputFormat, RectRegion};
use crate::dxgi_capture::{self, DuplicationSession};
use crossbeam_channel::{unbounded, Receiver, Sender};
use image::RgbaImage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Condvar, Mutex, OnceLock,
};
use std::thread;
use std::time::{Duration, Instant};
use xcap::Monitor;

/// Appends a timestamped line to `%TEMP%\shotgun_video_debug.log` for
/// diagnosing capture issues that don't reproduce outside the full app.
pub fn debug_log(msg: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("shotgun_video_debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let now = chrono::Local::now().format("%H:%M:%S%.3f");
        let _ = writeln!(f, "[{now}] {msg}");
    }
}

/// Per-frame state for whichever recording session is currently active on a
/// given monitor's duplication stream (`None` between sessions).
struct FrameSink {
    output_dir: PathBuf,
    filename_prefix: String,
    region: Option<RectRegion>,
    frame_count: Arc<AtomicU32>,
    /// Real wall-clock instant each frame was written, in order. Used after
    /// the session ends to give each frame its true display duration
    /// (screen-change-driven capture is inherently bursty — averaging
    /// frame_count/elapsed into one `-framerate` would stretch fast bursts
    /// into slow motion and compress quiet stretches).
    frame_times: Arc<Mutex<Vec<Instant>>>,
    /// Number of PNG-encode jobs handed to the writer pool that haven't
    /// finished yet. `start_video_capture` waits for this to reach 0 before
    /// building the concat file, so encoding lag never races the mux step.
    pending_encodes: Arc<AtomicU32>,
    min_frame_interval: Duration,
    last_write: Instant,
}

struct EncodeJob {
    img: RgbaImage,
    path: PathBuf,
    pending: Arc<AtomicU32>,
}

/// A small pool of dedicated PNG-encoder threads, created once and shared
/// across every recording. PNG encoding a large frame can take tens to
/// hundreds of milliseconds (see the `png_encode_speed_benchmark` test) —
/// doing that synchronously inside the DXGI polling loop would throttle
/// capture to the encoder's speed regardless of how fast the screen is
/// actually changing. Dispatching jobs here lets DXGI keep acquiring frames
/// at full speed while multiple cores encode in parallel.
fn encode_queue() -> &'static Sender<EncodeJob> {
    static QUEUE: OnceLock<Sender<EncodeJob>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (tx, rx) = unbounded::<EncodeJob>();
        let worker_count = thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 6);
        debug_log(&format!("starting {worker_count} PNG-encoder worker thread(s)"));
        for i in 0..worker_count {
            let rx = rx.clone();
            let _ = thread::Builder::new()
                .name(format!("shotgun-png-writer-{i}"))
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        if let Err(e) = save_image_with_format(&job.img, OutputFormat::Png, 100, &job.path) {
                            debug_log(&format!("async PNG write failed for {}: {e}", job.path.display()));
                        }
                        job.pending.fetch_sub(1, Ordering::SeqCst);
                    }
                });
        }
        tx
    })
}

/// A monitor's DXGI duplication session, kept alive for the lifetime of the
/// app and reused across recordings. DXGI only allows one active
/// duplication interface per output at a time, so it must be created once
/// and reused via the `active` pause/resume signal, never recreated
/// per-session.
struct MonitorRecorder {
    active: Arc<(Mutex<bool>, Condvar)>,
    frame_sink: Arc<Mutex<Option<FrameSink>>>,
}

fn monitor_recorders() -> &'static Mutex<HashMap<usize, MonitorRecorder>> {
    static RECORDERS: OnceLock<Mutex<HashMap<usize, MonitorRecorder>>> = OnceLock::new();
    RECORDERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Gets (creating and starting its capture thread if needed) the persistent
/// recorder for `monitor_index`.
fn get_or_create_recorder(monitor_index: usize) -> Result<(Arc<(Mutex<bool>, Condvar)>, Arc<Mutex<Option<FrameSink>>>), String> {
    debug_log(&format!("get_or_create_recorder(monitor_index={monitor_index}) called"));
    let mut recorders = monitor_recorders().lock().map_err(|e| format!("Recorder registry poisoned: {e}"))?;

    if let Some(entry) = recorders.get(&monitor_index) {
        debug_log("reusing existing persistent recorder");
        return Ok((entry.active.clone(), entry.frame_sink.clone()));
    }

    debug_log("no cached recorder; enumerating monitors");
    let monitors = Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {e}"))?;
    debug_log(&format!("found {} monitor(s)", monitors.len()));
    let target_monitor = monitors
        .get(monitor_index)
        .or_else(|| monitors.first())
        .cloned()
        .ok_or_else(|| "No monitor found for video capture".to_string())?;
    let (x, y) = (target_monitor.x(), target_monitor.y());
    debug_log(&format!(
        "target monitor: {} at ({x},{y}), {}x{}",
        target_monitor.name(), target_monitor.width(), target_monitor.height()
    ));

    let session = match dxgi_capture::open_for_point(x, y) {
        Ok(s) => {
            debug_log("dxgi_capture::open_for_point succeeded (DXGI duplication acquired)");
            s
        }
        Err(e) => {
            debug_log(&format!("open_for_point FAILED: {e}"));
            return Err(format!("Failed to start screen capture (DXGI duplication): {e}"));
        }
    };

    let frame_sink: Arc<Mutex<Option<FrameSink>>> = Arc::new(Mutex::new(None));
    let active: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));

    let frame_sink_for_thread = frame_sink.clone();
    let active_for_thread = active.clone();

    let spawn_result = thread::Builder::new()
        .name(format!("shotgun-video-frames-{monitor_index}"))
        .spawn(move || {
            debug_log("frame-writer thread started");
            run_capture_loop(session, active_for_thread, frame_sink_for_thread);
        });
    if let Err(e) = &spawn_result {
        debug_log(&format!("failed to spawn frame-writer thread: {e}"));
        return Err(format!("Failed to spawn capture thread: {e}"));
    }

    recorders.insert(
        monitor_index,
        MonitorRecorder {
            active: active.clone(),
            frame_sink: frame_sink.clone(),
        },
    );

    Ok((active, frame_sink))
}

/// Runs forever on its dedicated thread: blocks (no CPU use) while inactive,
/// and while active pulls frames from the duplication session, writing any
/// that pass the fps throttle to whatever session is currently in `frame_sink`.
fn run_capture_loop(session: DuplicationSession, active: Arc<(Mutex<bool>, Condvar)>, frame_sink: Arc<Mutex<Option<FrameSink>>>) {
    let (lock, cvar) = &*active;
    loop {
        {
            let mut is_active = match lock.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            while !*is_active {
                is_active = match cvar.wait(is_active) {
                    Ok(g) => g,
                    Err(_) => return,
                };
            }
        }

        match dxgi_capture::acquire_frame(&session, 200) {
            Ok(None) => {} // timeout / no new content this tick — normal, keep looping
            Ok(Some((fw, fh, rgba))) => {
                let mut guard = match frame_sink.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                let Some(sink) = guard.as_mut() else {
                    continue; // no session active right now; drop the frame
                };

                let now = Instant::now();
                if now.duration_since(sink.last_write) < sink.min_frame_interval {
                    continue;
                }
                sink.last_write = now;

                let idx = sink.frame_count.fetch_add(1, Ordering::SeqCst);
                if idx == 0 {
                    debug_log(&format!("first frame received: {fw}x{fh}, {} bytes raw", rgba.len()));
                }
                match RgbaImage::from_raw(fw, fh, rgba) {
                    Some(img) => {
                        let cropped = capture_region(&img, sink.region);
                        let filename = format!("{}{:05}.png", sink.filename_prefix, idx);
                        let path = sink.output_dir.join(filename);
                        // Timing is recorded now (when the frame was
                        // actually captured), not when the async encode
                        // below finishes — encoding a large frame can take
                        // tens of milliseconds, and gating capture on that
                        // would throttle the DXGI poll loop right back down.
                        if let Ok(mut times) = sink.frame_times.lock() {
                            times.push(now);
                        }
                        sink.pending_encodes.fetch_add(1, Ordering::SeqCst);
                        let _ = encode_queue().send(EncodeJob {
                            img: cropped,
                            path,
                            pending: sink.pending_encodes.clone(),
                        });
                    }
                    None => {
                        debug_log(&format!("RgbaImage::from_raw failed for frame {idx} ({fw}x{fh})"));
                    }
                }
            }
            Err(e) => {
                debug_log(&format!("acquire_frame error: {e}"));
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

/// Configuration for a video capture run.
pub struct VideoConfig {
    /// Target frames per second.
    pub fps: u32,
    /// Destination folder for the recorded frames.
    pub output_dir: PathBuf,
    /// Monitor to capture from.
    pub monitor_index: usize,
    /// Region of interest (same as in image capture); `None` = full monitor.
    pub region: Option<RectRegion>,
    /// Prefix for generated frame files.
    pub filename_prefix: String,
    /// User-configured ffmpeg path; empty = auto-detect.
    pub ffmpeg_path: String,
    /// Delete PNG frames, audio.wav, and the concat script once the MP4 is
    /// successfully created.
    pub cleanup_after_encode: bool,
}

/// Outcome of the encode step, reported back to the UI after `stop()`.
pub enum VideoResult {
    /// Frames were captured and successfully muxed into an MP4.
    Encoded { path: PathBuf, frame_count: u32, has_audio: bool },
    /// Frames were captured, but ffmpeg was not found or failed. PNG frames
    /// remain in `frames_dir`.
    EncodeFailed { frames_dir: PathBuf, frame_count: u32, reason: String },
}

/// Handle returned from `start_video_capture`. Used to stop the capture.
pub struct VideoHandle {
    stop_flag: Arc<AtomicBool>,
    thread_handle: thread::JoinHandle<()>,
}

impl VideoHandle {
    /// Stop the video capture and wait for the worker thread (and the
    /// ffmpeg encode step) to finish. The result is delivered on the
    /// `Receiver<VideoResult>` returned alongside this handle.
    pub fn stop(self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        let _ = self.thread_handle.join();
    }
}

/// Locates the ffmpeg binary to use, in priority order:
/// 1. An explicit user-configured path (if it points at an existing file).
/// 2. `ffmpeg.exe` sitting next to the running executable (bundled install).
/// 3. Bare `"ffmpeg"`, resolved via PATH by the OS when the process spawns.
fn resolve_ffmpeg_path(configured: &str) -> String {
    if !configured.trim().is_empty() {
        let p = Path::new(configured.trim());
        if p.exists() {
            return configured.trim().to_string();
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let bundled = exe_dir.join("ffmpeg.exe");
            if bundled.exists() {
                return bundled.to_string_lossy().to_string();
            }
        }
    }

    "ffmpeg".to_string()
}

/// Escapes a path for ffmpeg's concat-demuxer `file '...'` directive
/// (single quotes are escaped as `'\''`, matching its documented rules).
fn escape_concat_path(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "'\\''")
}

/// Removes the intermediate PNG frames, `audio.wav`, and the concat script
/// from `output_dir` after a successful encode, leaving only `output.mp4`.
fn cleanup_intermediate_files(output_dir: &Path, filename_prefix: &str) {
    let Ok(entries) = std::fs::read_dir(output_dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let is_frame_png = name.starts_with(filename_prefix) && name.ends_with(".png");
        let is_side_file = name == "audio.wav" || name == "frames.concat.txt";
        if is_frame_png || is_side_file {
            if let Err(e) = std::fs::remove_file(&path) {
                debug_log(&format!("cleanup: failed to remove {}: {e}", path.display()));
            }
        }
    }
}

/// Writes an ffmpeg concat-demuxer script giving each frame its true
/// real-world display duration (the gap until the next frame was captured,
/// or until `stop_instant` for the last one), so encoded playback speed
/// matches what actually happened rather than a single averaged rate.
fn write_concat_file(output_dir: &Path, filename_prefix: &str, times: &[Instant], stop_instant: Instant) -> Result<PathBuf, String> {
    if times.is_empty() {
        return Err("no frame timestamps recorded".to_string());
    }

    let mut content = String::new();
    for (i, &t) in times.iter().enumerate() {
        let duration = if i + 1 < times.len() {
            times[i + 1].duration_since(t).as_secs_f64()
        } else {
            stop_instant.duration_since(t).as_secs_f64()
        }
        .max(0.001); // concat demuxer requires a positive duration

        let path = output_dir.join(format!("{filename_prefix}{i:05}.png"));
        content.push_str(&format!("file '{}'\n", escape_concat_path(&path)));
        content.push_str(&format!("duration {duration:.3}\n"));
    }
    // The concat demuxer ignores the duration on the final listed entry, so
    // per its documented workaround, repeat the last file once more.
    let last_idx = times.len() - 1;
    let last_path = output_dir.join(format!("{filename_prefix}{last_idx:05}.png"));
    content.push_str(&format!("file '{}'\n", escape_concat_path(&last_path)));

    let concat_path = output_dir.join("frames.concat.txt");
    std::fs::write(&concat_path, content).map_err(|e| format!("failed to write {}: {e}", concat_path.display()))?;
    Ok(concat_path)
}

/// Starts a video capture session in a background thread. Returns a handle
/// used to stop the capture, and a receiver that yields the encode result
/// once `stop()` completes.
pub fn start_video_capture(cfg: VideoConfig) -> (VideoHandle, Receiver<VideoResult>) {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();
    let (result_tx, result_rx): (Sender<VideoResult>, Receiver<VideoResult>) = unbounded();

    let thread_handle = thread::spawn(move || {
        debug_log(&format!(
            "=== start_video_capture: output_dir={} monitor_index={} fps={} region={:?} ===",
            cfg.output_dir.display(), cfg.monitor_index, cfg.fps, cfg.region
        ));
        if std::fs::create_dir_all(&cfg.output_dir).is_err() {
            debug_log("create_dir_all FAILED");
            let _ = result_tx.send(VideoResult::EncodeFailed {
                frames_dir: cfg.output_dir.clone(),
                frame_count: 0,
                reason: "Failed to create output directory".to_string(),
            });
            return;
        }

        let (active, frame_sink) = match get_or_create_recorder(cfg.monitor_index) {
            Ok(r) => r,
            Err(reason) => {
                let _ = result_tx.send(VideoResult::EncodeFailed {
                    frames_dir: cfg.output_dir.clone(),
                    frame_count: 0,
                    reason,
                });
                return;
            }
        };

        // Audio loopback capture runs on its own thread, sharing the same
        // stop flag so it starts/stops in lockstep with the frame capture.
        let audio_wav_path = cfg.output_dir.join("audio.wav");
        let audio_stop = stop_clone.clone();
        let audio_wav_for_thread = audio_wav_path.clone();
        let audio_thread = thread::spawn(move || {
            audio::record_loopback_to_wav(audio_wav_for_thread, audio_stop)
        });

        // DXGI delivers a frame on every screen update, which on a fast
        // monitor with lots of motion can be far more than the configured
        // target; the frame sink throttles writes to roughly `cfg.fps` to
        // bound disk/CPU load, while still being purely event-driven (no
        // wasted frames during a static/idle screen).
        let frame_count = Arc::new(AtomicU32::new(0));
        let frame_times: Arc<Mutex<Vec<Instant>>> = Arc::new(Mutex::new(Vec::new()));
        let pending_encodes = Arc::new(AtomicU32::new(0));
        let min_frame_interval = Duration::from_secs_f64(1.0 / cfg.fps.max(1) as f64);
        {
            let mut guard = match frame_sink.lock() {
                Ok(g) => g,
                Err(e) => {
                    let _ = result_tx.send(VideoResult::EncodeFailed {
                        frames_dir: cfg.output_dir.clone(),
                        frame_count: 0,
                        reason: format!("Frame sink poisoned: {e}"),
                    });
                    return;
                }
            };
            *guard = Some(FrameSink {
                output_dir: cfg.output_dir.clone(),
                filename_prefix: cfg.filename_prefix.clone(),
                region: cfg.region,
                frame_count: frame_count.clone(),
                frame_times: frame_times.clone(),
                pending_encodes: pending_encodes.clone(),
                min_frame_interval,
                last_write: Instant::now() - min_frame_interval,
            });
        }

        debug_log("activating capture loop");
        {
            let (lock, cvar) = &*active;
            match lock.lock() {
                Ok(mut is_active) => {
                    *is_active = true;
                    cvar.notify_all();
                }
                Err(e) => {
                    debug_log(&format!("active-flag mutex poisoned: {e}"));
                    if let Ok(mut guard) = frame_sink.lock() {
                        *guard = None;
                    }
                    let _ = result_tx.send(VideoResult::EncodeFailed {
                        frames_dir: cfg.output_dir.clone(),
                        frame_count: 0,
                        reason: format!("Failed to start screen recorder: active-flag mutex poisoned: {e}"),
                    });
                    return;
                }
            }
        }

        debug_log("capture loop active, entering wait loop");
        let capture_start = Instant::now();
        while !stop_clone.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(50));
        }
        debug_log("stop signal received, deactivating capture loop");
        let stop_instant = Instant::now();
        {
            let (lock, cvar) = &*active;
            if let Ok(mut is_active) = lock.lock() {
                *is_active = false;
                cvar.notify_all();
            }
        }
        if let Ok(mut guard) = frame_sink.lock() {
            *guard = None;
        }

        let audio_outcome = audio_thread
            .join()
            .unwrap_or(AudioCaptureOutcome::Unavailable { reason: "audio capture thread panicked".to_string() });
        match &audio_outcome {
            AudioCaptureOutcome::Recorded { wav_path } => debug_log(&format!("audio recorded to {}", wav_path.display())),
            AudioCaptureOutcome::Unavailable { reason } => debug_log(&format!("audio unavailable: {reason}")),
        }

        let frame_index = frame_count.load(Ordering::SeqCst);
        debug_log(&format!("final frame_count = {frame_index}"));

        // Frames may still be encoding asynchronously in the writer pool;
        // wait for them so every PNG the concat file references actually
        // exists before we hand it to ffmpeg.
        let drain_deadline = Instant::now() + Duration::from_secs(30);
        while pending_encodes.load(Ordering::SeqCst) > 0 && Instant::now() < drain_deadline {
            thread::sleep(Duration::from_millis(20));
        }
        let still_pending = pending_encodes.load(Ordering::SeqCst);
        if still_pending > 0 {
            debug_log(&format!("WARNING: {still_pending} frame(s) still encoding after 30s drain timeout; proceeding anyway"));
        }

        if frame_index == 0 {
            debug_log("no frames captured, aborting before ffmpeg");
            let _ = result_tx.send(VideoResult::EncodeFailed {
                frames_dir: cfg.output_dir.clone(),
                frame_count: 0,
                reason: "No frames captured".to_string(),
            });
            return;
        }

        let _ = capture_start; // kept for the debug log above; timing now comes from frame_times

        // Give each frame its *actual* real-world display duration rather
        // than a single averaged `-framerate`. Screen-change-driven capture
        // is inherently bursty: averaging frame_count/elapsed into one rate
        // stretches fast bursts into slow motion and compresses quiet
        // stretches. A concat-demuxer script with an explicit duration per
        // frame keeps playback speed matching what actually happened.
        let times = frame_times.lock().map(|v| v.clone()).unwrap_or_default();
        let concat_path = match write_concat_file(&cfg.output_dir, &cfg.filename_prefix, &times, stop_instant) {
            Ok(p) => p,
            Err(e) => {
                debug_log(&format!("failed to write concat file: {e}"));
                let _ = result_tx.send(VideoResult::EncodeFailed {
                    frames_dir: cfg.output_dir.clone(),
                    frame_count: frame_index,
                    reason: format!("Failed to prepare frame timing for encode: {e}"),
                });
                return;
            }
        };

        let mp4_path = cfg.output_dir.join("output.mp4");

        let audio_wav_str = match &audio_outcome {
            AudioCaptureOutcome::Recorded { wav_path } => {
                // A file-size heuristic isn't reliable here: WASAPI's mix
                // format is WAVE_FORMAT_EXTENSIBLE, whose header alone is
                // already 68 bytes even with zero recorded samples (nothing
                // was playing during capture). Parse the actual sample
                // count instead.
                let sample_count = hound::WavReader::open(wav_path).map(|r| r.len()).unwrap_or(0);
                debug_log(&format!("audio.wav sample count: {sample_count}"));
                if sample_count > 0 {
                    Some(wav_path.to_string_lossy().to_string())
                } else {
                    debug_log("audio.wav has no samples (nothing was playing during capture) - encoding video-only");
                    None
                }
            }
            AudioCaptureOutcome::Unavailable { .. } => None,
        };
        let has_audio = audio_wav_str.is_some();

        let concat_path_str = concat_path.to_string_lossy().to_string();
        let mut args: Vec<&str> = vec!["-y", "-f", "concat", "-safe", "0", "-i", &concat_path_str];
        if let Some(wav) = &audio_wav_str {
            args.push("-i");
            args.push(wav);
        }
        // libx264 with yuv420p requires even width/height (4:2:0 chroma
        // subsampling halves both dimensions); a hand-dragged ROI is often
        // an odd size, so crop off at most 1px per side to the nearest even
        // dimensions rather than failing outright.
        args.extend(["-vf", "crop=floor(iw/2)*2:floor(ih/2)*2"]);
        args.extend(["-c:v", "libx264", "-pix_fmt", "yuv420p"]);
        // Resample the concat file's variable per-frame durations into a
        // standard constant frame rate (duplicating/dropping frames as
        // needed) — matches what every player/platform (e.g. YouTube)
        // expects, while the concat durations still keep overall timing
        // faithful to what actually happened on screen.
        let fps_arg = cfg.fps.max(1).to_string();
        args.extend(["-r", &fps_arg]);
        if has_audio {
            args.extend(["-c:a", "aac", "-b:a", "192k", "-shortest"]);
        }
        let mp4_path_str = mp4_path.to_str().unwrap_or("output.mp4");
        args.push(mp4_path_str);

        let ffmpeg_bin = resolve_ffmpeg_path(&cfg.ffmpeg_path);
        debug_log(&format!("running ffmpeg: '{ffmpeg_bin}' {}", args.join(" ")));
        let mut ffmpeg_cmd = std::process::Command::new(&ffmpeg_bin);
        ffmpeg_cmd.args(&args);
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            ffmpeg_cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let ffmpeg_status = ffmpeg_cmd.output();

        match ffmpeg_status {
            Ok(out) if out.status.success() => {
                debug_log(&format!("ffmpeg succeeded, wrote {}", mp4_path.display()));
                if cfg.cleanup_after_encode {
                    cleanup_intermediate_files(&cfg.output_dir, &cfg.filename_prefix);
                }
                let _ = result_tx.send(VideoResult::Encoded {
                    path: mp4_path,
                    frame_count: frame_index,
                    has_audio,
                });
            }
            Ok(out) => {
                debug_log(&format!("ffmpeg exited with failure status {:?}\nstderr: {}", out.status, String::from_utf8_lossy(&out.stderr)));
                let mut reason = String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("ffmpeg encode failed").to_string();
                if let AudioCaptureOutcome::Unavailable { reason: audio_reason } = &audio_outcome {
                    reason = format!("{reason} (audio also unavailable: {audio_reason})");
                }
                let _ = result_tx.send(VideoResult::EncodeFailed {
                    frames_dir: cfg.output_dir.clone(),
                    frame_count: frame_index,
                    reason,
                });
            }
            Err(e) => {
                debug_log(&format!("failed to spawn ffmpeg process: {e}"));
                let _ = result_tx.send(VideoResult::EncodeFailed {
                    frames_dir: cfg.output_dir.clone(),
                    frame_count: frame_index,
                    reason: format!("ffmpeg not found ('{ffmpeg_bin}'): {e}"),
                });
            }
        }
    });

    (
        VideoHandle {
            stop_flag,
            thread_handle,
        },
        result_rx,
    )
}

/// Convenience wrapper used by the UI to start a capture with the global config.
pub fn start_from_global(config: &AppConfig) -> (VideoHandle, Receiver<VideoResult>) {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let out_dir = config.output_dir.join(format!("video_{timestamp}"));
    let cfg = VideoConfig {
        fps: config.video_fps,
        output_dir: out_dir,
        monitor_index: config.monitor_index,
        region: config.region,
        filename_prefix: "frame_".to_string(),
        ffmpeg_path: config.ffmpeg_path.clone(),
        cleanup_after_encode: config.cleanup_video_frames_after_encode,
    };
    start_video_capture(cfg)
}

// NOTE: This implementation records frames as PNGs and audio as a WAV file
// for simplicity, then shells out to ffmpeg to mux + encode. Future work
// could replace that with an in-process H.264 encoder to drop the external
// ffmpeg dependency.

#[cfg(test)]
mod tests {
    use super::*;

    /// Not a correctness check — measures how long PNG-encoding a realistic
    /// frame takes with default settings (Adaptive filter) vs a fast filter,
    /// to quantify the per-frame cost that gates how many frames per second
    /// the capture loop can actually sustain.
    #[test]
    fn png_encode_speed_benchmark() {
        for (w, h, label) in [(2701u32, 1541u32, "2701x1541 (reported ROI)"), (3840, 2160, "3840x2160 (4K monitor)")] {
            let img = RgbaImage::from_fn(w, h, |x, y| {
                image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255])
            });

            let dir = std::env::temp_dir();

            let path_default = dir.join("bench_default.png");
            let start = Instant::now();
            save_image_with_format(&img, OutputFormat::Png, 100, &path_default).unwrap();
            let default_elapsed = start.elapsed();

            let path_fast = dir.join("bench_fast.png");
            let start = Instant::now();
            let file = std::fs::File::create(&path_fast).unwrap();
            let writer = std::io::BufWriter::new(file);
            let encoder = image::codecs::png::PngEncoder::new_with_quality(
                writer,
                image::codecs::png::CompressionType::Fast,
                image::codecs::png::FilterType::NoFilter,
            );
            image::ImageEncoder::write_image(encoder, img.as_raw(), w, h, image::ExtendedColorType::Rgba8).unwrap();
            let fast_elapsed = start.elapsed();

            println!(
                "{label}: default={default_elapsed:?} ({:.1} fps max) | fast/nofilter={fast_elapsed:?} ({:.1} fps max)",
                1.0 / default_elapsed.as_secs_f64(),
                1.0 / fast_elapsed.as_secs_f64(),
            );

            let _ = std::fs::remove_file(&path_default);
            let _ = std::fs::remove_file(&path_fast);
        }
    }

    fn run_one_session(dir: &std::path::Path) -> u32 {
        run_one_session_on(dir, 0)
    }

    fn run_one_session_on(dir: &std::path::Path, monitor_index: usize) -> u32 {
        let cfg = VideoConfig {
            fps: 30,
            output_dir: dir.to_path_buf(),
            monitor_index,
            region: None,
            filename_prefix: "frame_".to_string(),
            ffmpeg_path: String::new(),
            cleanup_after_encode: false,
        };
        let (handle, result_rx) = start_video_capture(cfg);
        std::thread::sleep(Duration::from_secs(2));
        handle.stop();

        let result = result_rx.recv().expect("no result from video capture");
        match result {
            VideoResult::Encoded { frame_count, .. } => frame_count,
            VideoResult::EncodeFailed { frame_count, reason, .. } => {
                println!("Encode failed (expected if ffmpeg isn't installed here): {reason}");
                frame_count
            }
        }
    }

    #[test]
    fn dxgi_capture_produces_multiple_frames() {
        let dir = std::env::temp_dir().join("shotgun_video_test_1");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);

        let frame_count = run_one_session(&dir);
        println!("Captured {frame_count} frames via DXGI duplication");
        assert!(frame_count >= 1, "expected at least one frame to be captured");

        let png_count = std::fs::read_dir(&dir)
            .map(|entries| entries.filter_map(|e| e.ok()).filter(|e| e.path().extension().map(|x| x == "png").unwrap_or(false)).count())
            .unwrap_or(0);
        assert_eq!(png_count as u32, frame_count, "PNG file count on disk should match reported frame count");
    }

    /// Regression test: DXGI only allows one active duplication session per
    /// monitor. A second recording in the same process used to silently
    /// capture zero frames because each call created a brand-new duplication
    /// session without releasing the first. `get_or_create_recorder` fixes
    /// this by reusing one persistent session across recordings.
    #[test]
    fn second_recording_in_same_process_still_captures_frames() {
        let dir_a = std::env::temp_dir().join("shotgun_video_test_2a");
        let dir_b = std::env::temp_dir().join("shotgun_video_test_2b");
        for d in [&dir_a, &dir_b] {
            let _ = std::fs::remove_dir_all(d);
            let _ = std::fs::create_dir_all(d);
        }

        let first = run_one_session(&dir_a);
        println!("First session: {first} frames");
        assert!(first >= 1, "first recording should capture at least one frame");

        let second = run_one_session(&dir_b);
        println!("Second session: {second} frames");
        assert!(second >= 1, "second recording in the same process should also capture frames (regression check)");
    }

    /// Regression test for the actual reported bug: xcap's `video_recorder()`
    /// creates its D3D11 device against whatever adapter is "default," then
    /// only searches that adapter's outputs — so on a multi-monitor/
    /// multi-adapter system, a monitor attached to a non-default adapter
    /// fails with DXGI_ERROR_NOT_FOUND. This exercises every monitor xcap
    /// reports, to catch that regardless of which adapter each is on.
    #[test]
    fn every_monitor_captures_at_least_one_frame() {
        let monitor_count = Monitor::all().map(|m| m.len()).unwrap_or(0);
        assert!(monitor_count >= 1, "expected at least one monitor to be detected");
        println!("Testing {monitor_count} monitor(s)");

        for idx in 0..monitor_count {
            let dir = std::env::temp_dir().join(format!("shotgun_video_test_monitor_{idx}"));
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(&dir);

            let frame_count = run_one_session_on(&dir, idx);
            println!("Monitor {idx}: {frame_count} frames");
            assert!(frame_count >= 1, "monitor index {idx} failed to capture any frames");
        }
    }

    /// Regression test for the actual reported bug: libx264 + yuv420p
    /// requires even width/height, but a hand-dragged ROI is often odd
    /// (e.g. the user's real 2701x1541 region), which used to make ffmpeg
    /// fail outright and write a 0-byte file despite frames capturing fine.
    #[test]
    fn odd_dimension_region_still_encodes_to_a_playable_mp4() {
        let ffmpeg_path = "R:\\repos\\ai-testing\\TainGester\\convert\\ffmpeg\\bin\\ffmpeg.exe";
        if !std::path::Path::new(ffmpeg_path).exists() {
            println!("Skipping: ffmpeg not found at expected dev-machine path");
            return;
        }

        let dir = std::env::temp_dir().join("shotgun_video_test_odd_dims");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);

        let cfg = VideoConfig {
            fps: 30,
            output_dir: dir.clone(),
            monitor_index: 0,
            region: Some(RectRegion { x: 3, y: 3, width: 301, height: 201 }), // both odd
            filename_prefix: "frame_".to_string(),
            ffmpeg_path: ffmpeg_path.to_string(),
            cleanup_after_encode: false,
        };
        let (handle, result_rx) = start_video_capture(cfg);
        std::thread::sleep(Duration::from_secs(2));
        handle.stop();

        let result = result_rx.recv().expect("no result from video capture");
        match result {
            VideoResult::Encoded { path, frame_count, .. } => {
                println!("Encoded {frame_count} frames to {}", path.display());
                let size = std::fs::metadata(&path).expect("mp4 file missing").len();
                println!("Output mp4 size: {size} bytes");
                assert!(size > 1000, "mp4 file is suspiciously small ({size} bytes) - encode likely failed silently");
            }
            VideoResult::EncodeFailed { reason, .. } => {
                panic!("Expected successful encode with odd dimensions, got failure: {reason}");
            }
        }
    }
}
