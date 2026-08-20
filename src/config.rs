use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use uuid::Uuid;

fn default_true() -> bool { true }
fn default_global_volume() -> f32 { 1.0 }
fn default_routing_mode() -> RoutingMode { RoutingMode::DirectTarget }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum RoutingMode {
    VirtualMic,
    DirectTarget,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SoundEntry {
    pub id: Uuid,
    pub name: String,
    pub path: PathBuf,
    pub hotkey: Option<String>,
    pub volume_out: f32,
    pub volume_playback: f32,
    #[serde(default = "default_true")]
    pub use_global_volume: bool,
    pub mic_enabled: bool,
    pub headphones_enabled: bool,
    pub custom_channels: Option<CustomChannels>,
    #[serde(default)]
    pub time_added: u64,
    #[serde(default)]
    pub duration_secs: Option<u64>,
    #[serde(skip)]
    pub exists: bool,
}

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
    pub default_output: String,
    pub mic_sink: String,
    pub allow_overlap: bool,
    #[serde(default)]
    pub queue_sounds: bool,
    pub mic_loopback_enabled: bool,
    pub mic_loopback_source: String,
    #[serde(default = "default_global_volume")]
    pub global_volume_playback: f32,
    #[serde(default = "default_global_volume")]
    pub global_volume_out: f32,
    
    #[serde(default = "default_routing_mode")]
    pub routing_mode: RoutingMode,
    #[serde(default)]
    pub direct_targets: Vec<String>,

    pub stop_all_hotkey: Option<String>,

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
                queue_sounds: false,
                mic_loopback_enabled: false,
                mic_loopback_source: "Default".to_string(),
                global_volume_playback: 1.0,
                global_volume_out: 1.0,
                routing_mode: RoutingMode::DirectTarget,
                direct_targets: Vec::new(),
                stop_all_hotkey: None,
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

pub fn compute_duration_fast(path: &Path) -> Option<u64> {
    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .ok()?;

    let format = probed.format;
    let track = format.default_track()?;
    let time_base = track.codec_params.time_base?;
    let n_frames = track.codec_params.n_frames?;

    let time = time_base.calc_time(n_frames);
    Some(time.seconds)
}

fn sys_config_path() -> PathBuf {
    let mut path = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
    path.push(".config/nisound/config.json");
    path
}
