use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SoundEntry {
    pub id: Uuid,
    pub name: String,
    pub path: PathBuf,
    pub hotkey: Option<String>,
    pub volume_out: f32,
    pub volume_playback: f32,
    pub mic_enabled: bool,
    pub headphones_enabled: bool,
    pub custom_channels: Option<CustomChannels>,
    #[serde(skip)]
    pub exists: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CustomChannels {
    pub input_device: String,
    pub output_device: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    pub sound_ids: Vec<Uuid>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    pub default_output: String,
    pub default_input: String,
    pub colors: HashMap<String, [u8; 3]>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppState {
    pub sounds: HashMap<Uuid, SoundEntry>,
    pub folders: Vec<Folder>,
    pub active_folder: Option<Uuid>,
    pub settings: AppSettings,
}

impl AppState {
    pub fn new_empty() -> Self {
        let mut colors = HashMap::new();
        colors.insert("bg".to_string(), [30, 30, 46]);
        colors.insert("accent".to_string(), [137, 180, 250]);

        Self {
            sounds: HashMap::new(),
            folders: vec![],
            active_folder: None,
            settings: AppSettings {
                default_output: "Default".to_string(),
                default_input: "Default".to_string(),
                colors,
            },
        }
    }

    pub fn load_or_create() -> Self {
        let config_path = sys_config_path();
        let mut state = if config_path.exists() {
            std::fs::read_to_string(config_path)
                .ok()
                .and_then(|json| serde_json::from_str::<AppState>(&json).ok())
                .unwrap_or_else(Self::new_empty)
        } else {
            Self::new_empty()
        };

        state.validate_and_index();
        state
    }

    pub fn save(&self) {
        if let Some(parent) = sys_config_path().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(sys_config_path(), json);
        }
    }

    pub fn validate_and_index(&mut self) {
        for sound in self.sounds.values_mut() {
            sound.exists = sound.path.exists();
        }
    }
}

fn sys_config_path() -> PathBuf {
    let mut path = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
    path.push(".config/lnx-soundboard/config.json");
    path
}
