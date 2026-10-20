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
pub struct DownloadSettings {
    pub folder: Option<PathBuf>,
    pub audio_only: bool,
    pub embed_thumbnail: bool,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            folder: None,
            audio_only: true,
            embed_thumbnail: true,
        }
    }
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

    #[serde(default)]
    pub download: DownloadSettings,
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub sounds: HashMap<Uuid, SoundEntry>,
    pub folders: Vec<Folder>,
    pub active_folder: Option<Uuid>,
    pub settings: AppSettings,
}

#[derive(Serialize, Deserialize)]
struct GeneralConfig {
    pub settings: AppSettings,
}

#[derive(Serialize, Deserialize)]
struct SoundsConfig {
    #[serde(default)]
    pub folders: Vec<Folder>,
    #[serde(default)]
    pub active_folder: Option<Uuid>,
    #[serde(default)]
    pub sounds: HashMap<Uuid, SoundEntry>,
}

impl AppState {
    pub fn default_colors() -> HashMap<String, [u8; 3]> {
        let mut colors = HashMap::new();
        colors.insert("Background".to_string(), [30, 36, 48]);
        colors.insert("Panels".to_string(), [30, 36, 48]);
        colors.insert("Card Background".to_string(), [25, 33, 45]);
        colors.insert("Card Missing File".to_string(), [100, 60, 60]);
        colors.insert("Card Missing Selected".to_string(), [159, 81, 95]);
        colors.insert("Card Selected".to_string(), [57, 80, 109]);
        colors.insert("Button Normal".to_string(), [33, 45, 63]);
        colors.insert("Button Hovered".to_string(), [33, 45, 63]);
        colors.insert("Button Active".to_string(), [57, 80, 109]);
        colors.insert("Text".to_string(), [205, 206, 207]);
        colors.insert("Accent / Selection".to_string(), [57, 80, 109]);
        colors.insert("Borders".to_string(), [43, 59, 81]);
        colors.insert("Window Background".to_string(), [30, 36, 48]);
        colors
    }

    pub fn ensure_default_colors(&mut self) {
        let defaults = Self::default_colors();
        for (k, v) in defaults {
            self.settings.colors.entry(k).or_insert(v);
        }
    }

    pub fn new_empty() -> Self {
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
                colors: Self::default_colors(),
                download: DownloadSettings::default(),
            },
        }
    }

    pub fn load_or_create() -> Self {
        let dir = config_dir();
        let toml_file = dir.join("config.toml");
        let sounds_file = dir.join("sounds.json");

        let mut state = Self::new_empty();

        if toml_file.exists() {
            if let Ok(toml_str) = std::fs::read_to_string(&toml_file) {
                if let Ok(gen_cfg) = toml::from_str::<GeneralConfig>(&toml_str) {
                    state.settings = gen_cfg.settings;
                }
            }
        }

        if sounds_file.exists() {
            if let Ok(json_str) = std::fs::read_to_string(&sounds_file) {
                if let Ok(sounds_cfg) = serde_json::from_str::<SoundsConfig>(&json_str) {
                    state.folders = sounds_cfg.folders;
                    state.active_folder = sounds_cfg.active_folder;
                    state.sounds = sounds_cfg.sounds;
                }
            }
        }

        state.ensure_default_colors();
        state.validate_and_index();
        state
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);

        let gen_cfg = GeneralConfig {
            settings: self.settings.clone(),
        };

        if let Ok(toml_str) = toml::to_string_pretty(&gen_cfg) {
            let _ = std::fs::write(dir.join("config.toml"), toml_str);
        }

        let sounds_cfg = SoundsConfig {
            folders: self.folders.clone(),
            active_folder: self.active_folder.clone(),
            sounds: self.sounds.clone(),
        };

        if let Ok(json_str) = serde_json::to_string_pretty(&sounds_cfg) {
            let _ = std::fs::write(dir.join("sounds.json"), json_str);
        }
    }

    pub fn validate_and_index(&mut self) {
        for sound in self.sounds.values_mut() {
            sound.exists = sound.path.exists();
        }
    }
}

pub fn rgb_to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

pub fn hex_to_rgb(hex: &str) -> Option<[u8; 3]> {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
    let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
    let b = u8::from_str_radix(&clean[3..6], 16).ok()?;
    Some([r, g, b])
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

fn config_dir() -> PathBuf {
    let mut path = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
    path.push(".config/nisound");
    path
}
