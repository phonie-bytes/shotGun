use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    Png,
    Jpeg,
    Bmp,
    WebP,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 4] = [
        OutputFormat::Png,
        OutputFormat::Jpeg,
        OutputFormat::Bmp,
        OutputFormat::WebP,
    ];

    pub fn extension(&self) -> &'static str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Jpeg => "jpg",
            OutputFormat::Bmp => "bmp",
            OutputFormat::WebP => "webp",
        }
    }

    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            OutputFormat::Png => "PNG (Lossless)",
            OutputFormat::Jpeg => "JPEG (Compressed)",
            OutputFormat::Bmp => "BMP (Bitmap)",
            OutputFormat::WebP => "WebP (Modern)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RectRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub vk_code: u32,
    pub key_name: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            vk_code: 0x78, // VK_F9
            key_name: "F9".to_string(),
        }
    }
}

impl HotkeyConfig {
    pub fn display_string(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.win {
            parts.push("Win");
        }
        if !self.key_name.is_empty() {
            parts.push(&self.key_name);
        } else {
            parts.push("None");
        }
        parts.join(" + ")
    }

    pub fn win32_modifiers(&self) -> u32 {
        let mut mods = 0x4000; // MOD_NOREPEAT
        if self.alt {
            mods |= 0x0001; // MOD_ALT
        }
        if self.ctrl {
            mods |= 0x0002; // MOD_CONTROL
        }
        if self.shift {
            mods |= 0x0004; // MOD_SHIFT
        }
        if self.win {
            mods |= 0x0008; // MOD_WIN
        }
        mods
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub monitor_index: usize,
    pub region: Option<RectRegion>,
    pub output_dir: PathBuf,
    pub format: OutputFormat,
    pub jpeg_quality: u8,
    pub file_prefix: String,
    pub padding_digits: usize,
    pub counter: u64,
    pub start_index: u64,
    pub session_index: u64,
    pub session_prefix: String,
    pub use_session_subfolders: bool,
    pub prompt_on_new_session: bool,
    pub play_sound: bool,
    pub auto_increment: bool,
    pub overwrite_existing: bool,
    pub minimize_to_tray: bool,
    pub close_to_tray: bool,
    pub capture_hotkey: HotkeyConfig,
    pub new_session_hotkey: HotkeyConfig,
    #[serde(default = "default_video_start_hotkey")]
    pub video_start_hotkey: HotkeyConfig,
    #[serde(default = "default_video_stop_hotkey")]
    pub video_stop_hotkey: HotkeyConfig,
    #[serde(default = "default_toggle_window_hotkey")]
    pub toggle_window_hotkey: HotkeyConfig,
    #[serde(default = "default_video_fps")]
    pub video_fps: u32,
    /// Path to the ffmpeg executable. Empty string = auto-detect (next to the
    /// app, then fall back to PATH).
    #[serde(default)]
    pub ffmpeg_path: String,
    /// Delete the intermediate PNG frames, audio.wav, and concat script once
    /// the MP4 has been successfully created.
    #[serde(default)]
    pub cleanup_video_frames_after_encode: bool,
    /// When starting a New Session, automatically bundle the screenshots
    /// from the session that just ended into a PDF (in addition to keeping
    /// the individual image files).
    #[serde(default)]
    pub auto_export_pdf_on_session: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        let default_dir = dirs_default_pictures_dir().unwrap_or_else(|| PathBuf::from("./captures"));

        Self {
            monitor_index: 0,
            region: None,
            output_dir: default_dir,
            format: OutputFormat::Png,
            jpeg_quality: 90,
            file_prefix: "shot_".to_string(),
            padding_digits: 3,
            counter: 1,
            start_index: 1,
            session_index: 1,
            session_prefix: "session_".to_string(),
            use_session_subfolders: false,
            prompt_on_new_session: true,
            play_sound: true,
            auto_increment: true,
            overwrite_existing: false,
            minimize_to_tray: true,
            close_to_tray: false,
            capture_hotkey: HotkeyConfig {
                ctrl: false,
                alt: false,
                shift: false,
                win: false,
                vk_code: 0x78, // F9
                key_name: "F9".to_string(),
            },
            new_session_hotkey: HotkeyConfig {
                ctrl: false,
                alt: false,
                shift: false,
                win: false,
                vk_code: 0x79, // F10
                key_name: "F10".to_string(),
            },
            video_start_hotkey: HotkeyConfig {
                ctrl: false,
                alt: false,
                shift: false,
                win: false,
                vk_code: 0x7B, // F12
                key_name: "F12".to_string(),
            },
            video_stop_hotkey: HotkeyConfig {
                ctrl: false,
                alt: false,
                shift: true,
                win: false,
                vk_code: 0x7B, // F12 (with Shift)
                key_name: "F12".to_string(),
            },
            toggle_window_hotkey: default_toggle_window_hotkey(),
            video_fps: 30,
            ffmpeg_path: String::new(),
            cleanup_video_frames_after_encode: false,
            auto_export_pdf_on_session: false,
        }
    }
}

fn default_video_fps() -> u32 {
    30
}

fn default_video_start_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
        vk_code: 0x7B, // F12
        key_name: "F12".to_string(),
    }
}

fn default_video_stop_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        ctrl: false,
        alt: false,
        shift: true,
        win: false,
        vk_code: 0x7B, // F12 (with Shift)
        key_name: "F12".to_string(),
    }
}

fn default_toggle_window_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        ctrl: true,
        alt: true,
        shift: false,
        win: false,
        vk_code: 0x2D, // Insert
        key_name: "Insert".to_string(),
    }
}

fn dirs_default_pictures_dir() -> Option<PathBuf> {
    if let Some(user_dirs) = directories::UserDirs::new() {
        if let Some(pic) = user_dirs.picture_dir() {
            return Some(pic.join("shotGun"));
        }
    }
    None
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        if let Some(proj_dirs) = directories::ProjectDirs::from("com", "noerotech", "shotgun") {
            let config_dir = proj_dirs.config_dir();
            let _ = std::fs::create_dir_all(config_dir);
            config_dir.join("config.json")
        } else {
            PathBuf::from("shotgun_config.json")
        }
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(cfg) = serde_json::from_str::<AppConfig>(&data) {
                    return cfg;
                }
            }
        }
        let default_cfg = Self::default();
        let _ = default_cfg.save();
        default_cfg
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_display_string() {
        let hk = HotkeyConfig {
            ctrl: true,
            alt: true,
            shift: false,
            win: false,
            vk_code: 0x78,
            key_name: "F9".to_string(),
        };
        assert_eq!(hk.display_string(), "Ctrl + Alt + F9");
    }

    #[test]
    fn test_hotkey_modifiers() {
        let hk = HotkeyConfig {
            ctrl: true,
            alt: false,
            shift: true,
            win: false,
            vk_code: 0x53,
            key_name: "S".to_string(),
        };
        let mods = hk.win32_modifiers();
        assert_ne!(mods & 0x0002, 0); // MOD_CONTROL
        assert_ne!(mods & 0x0004, 0); // MOD_SHIFT
        assert_eq!(mods & 0x0001, 0); // MOD_ALT
    }

    #[test]
    fn test_config_serialization() {
        let mut cfg = AppConfig::default();
        cfg.start_index = 0;
        cfg.counter = 0;
        let json = serde_json::to_string(&cfg).expect("Serialization failed");
        let deserialized: AppConfig = serde_json::from_str(&json).expect("Deserialization failed");
        assert_eq!(deserialized.counter, 0);
        assert_eq!(deserialized.start_index, 0);
        assert_eq!(cfg.file_prefix, deserialized.file_prefix);
        assert_eq!(cfg.format, deserialized.format);
        assert!(deserialized.prompt_on_new_session);
    }
}
