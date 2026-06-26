use rodio::{cpal::traits::{DeviceTrait, HostTrait}, Decoder, OutputStream, OutputStreamHandle, Sink};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct ActiveSound {
    pub play_id: Uuid,
    pub sound_id: Uuid,
    pub name: String,
    pub sinks: Vec<Sink>,
}

pub struct AudioEngine {
    streams: Arc<Mutex<HashMap<String, (OutputStream, OutputStreamHandle)>>>,
    pub active_sounds: Arc<Mutex<Vec<ActiveSound>>>,
}

impl AudioEngine {
    pub fn init() -> Self {
        Self {
            streams: Arc::new(Mutex::new(HashMap::new())),
            active_sounds: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn available_output_devices() -> Vec<String> {
        let mut devices = vec!["Default".to_string()];
        let host = rodio::cpal::default_host();
        if let Ok(devs) = host.output_devices() {
            for dev in devs {
                if let Ok(name) = dev.name() {
                    devices.push(name);
                }
            }
        }
        devices
    }

    fn get_or_create_stream(&self, device_name: &str) -> Option<OutputStreamHandle> {
        let mut streams = self.streams.lock().unwrap();

        if let Some((_, handle)) = streams.get(device_name) {
            return Some(handle.clone());
        }

        let host = rodio::cpal::default_host();
        let device = if device_name == "Default" {
            host.default_output_device()
        } else {
            host.output_devices()
                .ok()?
                .find(|d| d.name().unwrap_or_default() == device_name)
        };

        if let Some(dev) = device {
            if let Ok((stream, handle)) = OutputStream::try_from_device(&dev) {
                streams.insert(device_name.to_string(), (stream, handle.clone()));
                return Some(handle);
            }
        }
        None
    }

    pub fn play_sound(
        &self,
        sound_id: Uuid,
        name: &str,
        path: &Path,
        volume_playback: f32,
        volume_out: f32,
        headphones: bool,
        mic: bool,
        default_headphone_device: &str,
        default_mic_device: &str,
    ) {
        if !path.exists() {
            return;
        }

        let play_id = Uuid::new_v4();
        let mut sinks = Vec::new();

        if headphones {
            if let Some(sink) = self.spawn_sink(path, volume_playback, default_headphone_device) {
                sinks.push(sink);
            }
        }

        if mic {
            if let Some(sink) = self.spawn_sink(path, volume_out, default_mic_device) {
                sinks.push(sink);
            }
        }

        if !sinks.is_empty() {
            if let Ok(mut active) = self.active_sounds.lock() {
                active.push(ActiveSound {
                    play_id,
                    sound_id,
                    name: name.to_string(),
                    sinks,
                });
            }
        }
    }

    fn spawn_sink(&self, path: &Path, volume: f32, device_name: &str) -> Option<Sink> {
        let file = File::open(path).ok()?;
        let source = Decoder::new(BufReader::new(file)).ok()?;
        let handle = self.get_or_create_stream(device_name)?;
        let sink = Sink::try_new(&handle).ok()?;
        sink.set_volume(volume);
        sink.append(source);
        Some(sink)
    }

    pub fn stop_sound(&self, play_id: Uuid) {
        if let Ok(mut active) = self.active_sounds.lock() {
            if let Some(pos) = active.iter().position(|s| s.play_id == play_id) {
                let sound = active.remove(pos);
                for sink in sound.sinks {
                    sink.stop();
                }
            }
        }
    }

    pub fn clean_dead_sinks(&self) {
        if let Ok(mut active) = self.active_sounds.lock() {
            active.retain(|s| s.sinks.iter().any(|sink| !sink.empty()));
        }
    }
}
