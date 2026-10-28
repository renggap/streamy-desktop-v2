//! User preferences stored as JSON with atomic write.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

pub const MIN_GAIN: f32 = 0.0;
pub const MAX_GAIN: f32 = 4.0;

/// Single source of truth for app settings, shared verbatim with the UI.
/// `serde(default)` lets older/partial files (and older UIs) load without
/// wiping everything back to defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub selected_profile_id: Option<i64>,
    pub mic_gain: f32,
    pub input_device_name: Option<String>,
    pub recordings_folder: Option<String>,
    pub language: String,
    pub launch_at_login: bool,
    pub start_minimized: bool,
    pub auto_reconnect: bool,
    pub record_with_stream: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            selected_profile_id: None,
            mic_gain: 1.0,
            input_device_name: None,
            recordings_folder: None,
            language: "en".into(),
            launch_at_login: false,
            start_minimized: false,
            auto_reconnect: true,
            record_with_stream: false,
        }
    }
}

impl AppSettings {
    /// Clamp and tidy values coming from the UI or an edited file.
    pub fn sanitized(mut self) -> Self {
        if !self.mic_gain.is_finite() {
            self.mic_gain = 1.0;
        }
        self.mic_gain = self.mic_gain.clamp(MIN_GAIN, MAX_GAIN);
        self.input_device_name = self.input_device_name.filter(|s| !s.trim().is_empty());
        self.recordings_folder = self
            .recordings_folder
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if self.language != "en" && self.language != "id" {
            self.language = "en".into();
        }
        self
    }
}

pub struct SettingsStorage {
    path: PathBuf,
}

impl SettingsStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn load(&self) -> AppSettings {
        if !self.path.exists() {
            return AppSettings::default();
        }
        match std::fs::read_to_string(&self.path) {
            Ok(content) => match serde_json::from_str::<AppSettings>(&content) {
                Ok(s) => s.sanitized(),
                Err(e) => {
                    tracing::warn!("settings file unreadable, using defaults: {e}");
                    AppSettings::default()
                }
            },
            Err(_) => AppSettings::default(),
        }
    }

    /// Load, apply `f`, save. Keeps read-modify-write in one place.
    pub fn update(&self, f: impl FnOnce(&mut AppSettings)) -> Result<AppSettings, String> {
        let mut s = self.load();
        f(&mut s);
        let s = s.sanitized();
        self.save(&s)?;
        Ok(s)
    }

    /// Atomic write: write to temp file in the same directory, then rename.
    pub fn save(&self, settings: &AppSettings) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let tmp_path = self.path.with_extension("tmp");
        let content = serde_json::to_string_pretty(settings)
            .map_err(|e| format!("Failed to serialize settings: {:?}", e))?;

        {
            let mut file = File::create(&tmp_path)
                .map_err(|e| format!("Failed to create temp settings file: {:?}", e))?;
            file.write_all(content.as_bytes())
                .map_err(|e| format!("Failed to write settings: {:?}", e))?;
            file.flush()
                .map_err(|e| format!("Failed to flush settings: {:?}", e))?;
        }

        std::fs::rename(&tmp_path, &self.path)
            .map_err(|e| format!("Failed to atomically replace settings file: {:?}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_settings_atomic_save_and_load() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        let storage = SettingsStorage::new(&path);
        let mut settings = storage.load();
        assert_eq!(settings.mic_gain, 1.0);

        settings.mic_gain = 1.45;
        settings.language = "id".into();
        settings.selected_profile_id = Some(42);

        storage.save(&settings).unwrap();

        let loaded = storage.load();
        assert_eq!(loaded.mic_gain, 1.45);
        assert_eq!(loaded.language, "id");
        assert_eq!(loaded.selected_profile_id, Some(42));
    }

    #[test]
    fn partial_and_legacy_files_keep_known_values() {
        // Legacy file: unknown key, missing new keys.
        let s: AppSettings =
            serde_json::from_str(r#"{"mic_gain":2.0,"resume_on_launch":true}"#).unwrap();
        assert_eq!(s.mic_gain, 2.0);
        assert!(s.auto_reconnect);
        assert_eq!(s.language, "en");
    }

    #[test]
    fn sanitize_clamps_gain_and_language() {
        let s = AppSettings {
            mic_gain: 99.0,
            language: "fr".into(),
            input_device_name: Some("  ".into()),
            ..Default::default()
        }
        .sanitized();
        assert_eq!(s.mic_gain, MAX_GAIN);
        assert_eq!(s.language, "en");
        assert_eq!(s.input_device_name, None);
    }
}
