//! Capture Profiles: named, switchable capture setups (region/monitor/
//! output/naming/format or video settings), so switching what you're doing
//! (e.g. a "Teams" screenshot profile vs. a "Video Recording" profile)
//! doesn't require manually re-pointing Target Monitor / ROI / output
//! directory / filename every time.
//!
//! Deliberately a thin layer on top of `AppConfig` rather than a rewrite of
//! how capture works: `do_capture()`/`start_video()`/`save_captured_image()`
//! etc. keep reading the same flat `AppConfig` fields they always have.
//! Switching profiles just writes the currently-active profile's fields
//! back from the live config (so in-progress edits aren't lost), then
//! copies the newly-selected profile's saved values into the live config.

use crate::config::{AppConfig, OutputFormat, RectRegion};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileKind {
    Screenshot,
    Video,
}

impl ProfileKind {
    pub fn label(&self) -> &'static str {
        match self {
            ProfileKind::Screenshot => "📸 Screenshot",
            ProfileKind::Video => "🎬 Video",
        }
    }
}

/// Everything a capture profile owns. See the module doc for what's
/// deliberately *not* here (hotkeys, tray behavior, autostart, ffmpeg path
/// — those stay global, one setting for the whole app).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureProfile {
    pub id: String,
    pub name: String,
    pub kind: ProfileKind,
    pub monitor_index: usize,
    /// The monitor's name when this profile was last saved. `monitor_index`
    /// is what capture actually uses; `reconcile_monitor` re-points it by
    /// this name when Windows reorders monitors.
    pub monitor_name: String,
    pub region: Option<RectRegion>,
    pub output_dir: PathBuf,
    pub file_prefix: String,
    pub padding_digits: usize,
    pub counter: u64,
    pub start_index: u64,
    pub session_index: u64,
    pub session_prefix: String,
    pub use_session_subfolders: bool,
    /// Screenshot-only, but harmless to carry on a Video profile too (kept
    /// simple rather than making these `Option`s).
    pub format: OutputFormat,
    pub jpeg_quality: u8,
    /// Video-only.
    pub video_fps: u32,
    pub cleanup_video_frames_after_encode: bool,
    pub auto_copy_to_clipboard: bool,
    pub auto_export_pdf_on_session: bool,
}

impl CaptureProfile {
    /// Snapshots the current live config into a new profile with the given
    /// id/name/kind.
    pub fn from_config(config: &AppConfig, id: String, name: String, kind: ProfileKind, monitor_name: String) -> Self {
        Self {
            id,
            name,
            kind,
            monitor_index: config.monitor_index,
            monitor_name,
            region: config.region,
            output_dir: config.output_dir.clone(),
            file_prefix: config.file_prefix.clone(),
            padding_digits: config.padding_digits,
            counter: config.counter,
            start_index: config.start_index,
            session_index: config.session_index,
            session_prefix: config.session_prefix.clone(),
            use_session_subfolders: config.use_session_subfolders,
            format: config.format,
            jpeg_quality: config.jpeg_quality,
            video_fps: config.video_fps,
            cleanup_video_frames_after_encode: config.cleanup_video_frames_after_encode,
            auto_copy_to_clipboard: config.auto_copy_to_clipboard,
            auto_export_pdf_on_session: config.auto_export_pdf_on_session,
        }
    }

    /// Re-points `monitor_index` at whichever monitor currently has this
    /// profile's recorded `monitor_name`. Windows can hand out monitor
    /// indices in a different order after a reconnect or a driver change,
    /// which would otherwise silently aim the profile at the wrong screen.
    ///
    /// If the named monitor isn't present (unplugged), the index is left
    /// alone unless it's now out of range, in which case it falls back to 0.
    /// A profile with no recorded name just adopts the name of whatever its
    /// index currently points at. Returns whether anything changed.
    pub fn reconcile_monitor(&mut self, monitor_names: &[String]) -> bool {
        if self.monitor_name.is_empty() {
            if let Some(name) = monitor_names.get(self.monitor_index) {
                self.monitor_name = name.clone();
                return true;
            }
            return false;
        }
        if let Some(idx) = monitor_names.iter().position(|n| *n == self.monitor_name) {
            if idx != self.monitor_index {
                self.monitor_index = idx;
                return true;
            }
            return false;
        }
        if self.monitor_index >= monitor_names.len() && !monitor_names.is_empty() {
            self.monitor_index = 0;
            return true;
        }
        false
    }

    /// Copies this profile's fields into the live config — the "switch to"
    /// half of a profile switch.
    pub fn write_into_config(&self, config: &mut AppConfig) {
        config.monitor_index = self.monitor_index;
        config.region = self.region;
        config.output_dir = self.output_dir.clone();
        config.file_prefix = self.file_prefix.clone();
        config.padding_digits = self.padding_digits;
        config.counter = self.counter;
        config.start_index = self.start_index;
        config.session_index = self.session_index;
        config.session_prefix = self.session_prefix.clone();
        config.use_session_subfolders = self.use_session_subfolders;
        config.format = self.format;
        config.jpeg_quality = self.jpeg_quality;
        config.video_fps = self.video_fps;
        config.cleanup_video_frames_after_encode = self.cleanup_video_frames_after_encode;
        config.auto_copy_to_clipboard = self.auto_copy_to_clipboard;
        config.auto_export_pdf_on_session = self.auto_export_pdf_on_session;
    }

    /// Copies the live config's fields back into this profile — the
    /// "switch away from" half, so nothing typed while this profile was
    /// active gets lost.
    pub fn update_from_config(&mut self, config: &AppConfig) {
        self.monitor_index = config.monitor_index;
        self.region = config.region;
        self.output_dir = config.output_dir.clone();
        self.file_prefix = config.file_prefix.clone();
        self.padding_digits = config.padding_digits;
        self.counter = config.counter;
        self.start_index = config.start_index;
        self.session_index = config.session_index;
        self.session_prefix = config.session_prefix.clone();
        self.use_session_subfolders = config.use_session_subfolders;
        self.format = config.format;
        self.jpeg_quality = config.jpeg_quality;
        self.video_fps = config.video_fps;
        self.cleanup_video_frames_after_encode = config.cleanup_video_frames_after_encode;
        self.auto_copy_to_clipboard = config.auto_copy_to_clipboard;
        self.auto_export_pdf_on_session = config.auto_export_pdf_on_session;
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfilesFile {
    pub active_profile_id: Option<String>,
    pub profiles: Vec<CaptureProfile>,
}

impl ProfilesFile {
    pub fn config_path() -> PathBuf {
        if let Some(proj_dirs) = directories::ProjectDirs::from("com", "noerotech", "shotgun") {
            let config_dir = proj_dirs.config_dir();
            let _ = std::fs::create_dir_all(config_dir);
            config_dir.join("profiles.json")
        } else {
            PathBuf::from("shotgun_profiles.json")
        }
    }

    pub fn load() -> Option<Self> {
        let path = Self::config_path();
        if !path.exists() {
            return None;
        }
        let data = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let data = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, data).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Loads `profiles.json` if it exists; otherwise creates it with a
    /// single "Default" profile snapshotting whatever `config` (already
    /// loaded from `config.json`) has, so upgrading users see zero
    /// disruption if they never open the Profiles UI. Does *not* apply
    /// anything back onto `config` — `config.json` is already the
    /// authoritative live state for whichever profile was active when the
    /// app last closed.
    pub fn load_or_migrate(config: &AppConfig, monitor_name: String) -> Self {
        if let Some(existing) = Self::load() {
            return existing;
        }
        let default_profile = CaptureProfile::from_config(
            config,
            "default".to_string(),
            "Default".to_string(),
            ProfileKind::Screenshot,
            monitor_name,
        );
        let file = ProfilesFile {
            active_profile_id: Some(default_profile.id.clone()),
            profiles: vec![default_profile],
        };
        let _ = file.save();
        file
    }

    /// Runs `reconcile_monitor` over every profile; true if any changed.
    pub fn reconcile_monitors(&mut self, monitor_names: &[String]) -> bool {
        let mut changed = false;
        for p in &mut self.profiles {
            changed |= p.reconcile_monitor(monitor_names);
        }
        changed
    }

    pub fn active(&self) -> Option<&CaptureProfile> {
        let id = self.active_profile_id.as_ref()?;
        self.profiles.iter().find(|p| &p.id == id)
    }

    pub fn active_mut(&mut self) -> Option<&mut CaptureProfile> {
        let id = self.active_profile_id.clone()?;
        self.profiles.iter_mut().find(|p| p.id == id)
    }

    pub fn find(&self, id: &str) -> Option<&CaptureProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut CaptureProfile> {
        self.profiles.iter_mut().find(|p| p.id == id)
    }

    /// Slugifies `name` into an id, disambiguating with a numeric suffix on
    /// collision. No UUID crate needed — profiles are few and user-named.
    pub fn unique_id_from_name(&self, name: &str) -> String {
        let base: String = name
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        let base = if base.is_empty() { "profile".to_string() } else { base };
        if self.find(&base).is_none() {
            return base;
        }
        let mut n = 2;
        loop {
            let candidate = format!("{base}-{n}");
            if self.find(&candidate).is_none() {
                return candidate;
            }
            n += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.monitor_index = 2;
        cfg.file_prefix = "teams_".to_string();
        cfg.counter = 7;
        cfg
    }

    #[test]
    fn round_trips_config_into_profile_and_back() {
        let cfg = sample_config();
        let profile = CaptureProfile::from_config(&cfg, "teams".to_string(), "Teams".to_string(), ProfileKind::Screenshot, "Display 1".to_string());
        assert_eq!(profile.monitor_index, 2);
        assert_eq!(profile.file_prefix, "teams_");
        assert_eq!(profile.counter, 7);

        let mut fresh = AppConfig::default();
        profile.write_into_config(&mut fresh);
        assert_eq!(fresh.monitor_index, 2);
        assert_eq!(fresh.file_prefix, "teams_");
        assert_eq!(fresh.counter, 7);
    }

    #[test]
    fn update_from_config_reflects_live_edits() {
        let cfg = sample_config();
        let mut profile = CaptureProfile::from_config(&cfg, "teams".to_string(), "Teams".to_string(), ProfileKind::Screenshot, String::new());

        let mut edited = cfg.clone();
        edited.counter = 42;
        edited.file_prefix = "teams_meeting_".to_string();
        profile.update_from_config(&edited);

        assert_eq!(profile.counter, 42);
        assert_eq!(profile.file_prefix, "teams_meeting_");
    }

    /// Mirrors what the profile editor does for the *active* profile: pull
    /// live state in first (`update_from_config`), apply the user's edit,
    /// then push it back out (`write_into_config`). The live counter —
    /// which advances with every capture, not in the stored profile — must
    /// survive that round trip instead of being rolled back to whatever
    /// the profile last saw.
    #[test]
    fn editing_active_profile_writes_through_without_rolling_back_live_counter() {
        let mut live = sample_config();
        live.counter = 7;
        let mut profile = CaptureProfile::from_config(&live, "teams".to_string(), "Teams".to_string(), ProfileKind::Screenshot, String::new());

        // Captures happen; the live counter moves on, the profile hasn't seen it.
        live.counter = 9;

        profile.update_from_config(&live);
        profile.file_prefix = "teams_edited_".to_string();
        profile.write_into_config(&mut live);

        assert_eq!(live.counter, 9, "live counter must not be rolled back by the write-through");
        assert_eq!(live.file_prefix, "teams_edited_", "the edit must reach the live config");
    }

    #[test]
    fn unique_id_from_name_disambiguates_collisions() {
        let mut file = ProfilesFile::default();
        file.profiles.push(CaptureProfile::from_config(&AppConfig::default(), "teams".to_string(), "Teams".to_string(), ProfileKind::Screenshot, String::new()));

        assert_eq!(file.unique_id_from_name("Video Recording"), "video-recording");
        assert_eq!(file.unique_id_from_name("Teams"), "teams-2");
    }

    #[test]
    fn serializes_and_deserializes_round_trip() {
        let cfg = sample_config();
        let profile = CaptureProfile::from_config(&cfg, "teams".to_string(), "Teams".to_string(), ProfileKind::Screenshot, String::new());
        let file = ProfilesFile { active_profile_id: Some("teams".to_string()), profiles: vec![profile] };

        let json = serde_json::to_string(&file).expect("serialize");
        let restored: ProfilesFile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.active_profile_id.as_deref(), Some("teams"));
        assert_eq!(restored.profiles[0].file_prefix, "teams_");
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reconcile_follows_the_monitor_by_name_when_indices_reorder() {
        let mut p = CaptureProfile::from_config(&AppConfig::default(), "t".into(), "T".into(), ProfileKind::Screenshot, "DISPLAY5".into());
        p.monitor_index = 1;
        // DISPLAY5 used to be index 1; after a reconnect it is index 2.
        assert!(p.reconcile_monitor(&names(&["DISPLAY1", "DISPLAY7", "DISPLAY5"])));
        assert_eq!(p.monitor_index, 2);
        // Already correct: nothing to do.
        assert!(!p.reconcile_monitor(&names(&["DISPLAY1", "DISPLAY7", "DISPLAY5"])));
    }

    #[test]
    fn reconcile_keeps_index_when_named_monitor_is_unplugged_unless_out_of_range() {
        let mut p = CaptureProfile::from_config(&AppConfig::default(), "t".into(), "T".into(), ProfileKind::Screenshot, "GONE".into());
        p.monitor_index = 1;
        assert!(!p.reconcile_monitor(&names(&["DISPLAY1", "DISPLAY7"])), "named monitor missing but index valid: leave it");
        p.monitor_index = 5;
        assert!(p.reconcile_monitor(&names(&["DISPLAY1", "DISPLAY7"])));
        assert_eq!(p.monitor_index, 0, "out-of-range index falls back to the first monitor");
    }

    #[test]
    fn reconcile_adopts_a_name_for_profiles_that_have_none() {
        let mut p = CaptureProfile::from_config(&AppConfig::default(), "t".into(), "T".into(), ProfileKind::Screenshot, String::new());
        p.monitor_index = 1;
        assert!(p.reconcile_monitor(&names(&["DISPLAY1", "DISPLAY7"])));
        assert_eq!(p.monitor_name, "DISPLAY7");
    }
}
