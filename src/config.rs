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
    #[serde(default)]
    pub time_added: u64,
    #[serde(skip)]
    pub exists: bool,
}

/// Per-sound override of the global routing. Both fields are technical
/// Pulse *sink* names (playback always targets a sink, never a source) —
/// `mic_sink` is the sink that feeds the virtual/"mic" channel, the same
/// way `output_device` feeds the headphones/speakers channel.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CustomChannels {
    pub mic_sink: String,
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
    /// Technical Pulse sink name for the "headphones/speakers" channel.
    pub default_output: String,
    /// Technical Pulse sink name for the "microphone" channel — this is a
    /// sink too (e.g. the Nisound-managed virtual sink), never a source.
    pub mic_sink: String,
    /// If false, starting a new sound stops every currently playing sound
    /// first (both channels). If true, sounds overlap freely.
    pub allow_overlap: bool,
    /// Whether Nisound should keep a `module-loopback` running from
    /// `mic_loopback_source` into `mic_sink`, so the user's real voice and
    /// the soundboard effects end up mixed together for listeners.
    pub mic_loopback_enabled: bool,
    /// Technical Pulse *source* name of the real microphone to loop into
    /// `mic_sink` when `mic_loopback_enabled` is true.
    pub mic_loopback_source: String,
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
                mic_sink: "Default".to_string(),
                allow_overlap: true,
                mic_loopback_enabled: false,
                mic_loopback_source: "Default".to_string(),
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
