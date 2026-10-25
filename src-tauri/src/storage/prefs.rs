//! User preferences stored as JSON with atomic write.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub selected_profile_id: Option<i64>,
    pub mic_gain: f32,
    pub input_device_name: Option<String>,
    pub recordings_folder: Option<String>,
    pub language: String,
    pub launch_at_login: bool,
    pub resume_on_launch: bool,
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
            resume_on_launch: false,
        }
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
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => AppSettings::default(),
        }
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
}
