use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;
use rodio::{Decoder, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// A single output stream feeding one PulseAudio sink or source (monitor).
/// Replaces the old rodio::Sink — same "fire and forget + poll" API
/// (`stop()` / `empty()`), but writes PCM straight to Pulse/PipeWire so it
/// can target any named device (e.g. "alsa_output.pci-..." or a virtual
/// mic sink), not just what cpal happens to enumerate on the ALSA host.
pub struct PulseSink {
    stop_flag: Arc<AtomicBool>,
    finished_flag: Arc<AtomicBool>,
}

impl PulseSink {
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    pub fn empty(&self) -> bool {
        self.finished_flag.load(Ordering::Relaxed)
    }
}

impl Drop for PulseSink {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct ActiveSound {
    pub play_id: Uuid,
    pub sound_id: Uuid,
    pub name: String,
    pub sinks: Vec<PulseSink>,
}

pub struct AudioEngine {
    pub active_sounds: Arc<Mutex<Vec<ActiveSound>>>,
}

/// Sentinel value meaning "let PulseAudio/PipeWire pick the default
/// device", used both as the initial config value and as a synthetic
/// entry the UI prepends to the device list.
pub const DEFAULT_DEVICE: &str = "Default";

impl AudioEngine {
    pub fn init() -> Self {
        Self {
            active_sounds: Arc::new(Mutex::new(Vec::new())),
        }
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

    /// Opens a fresh Pulse/PipeWire playback stream targeting `device_name`
    /// (a technical Pulse name like "alsa_output.pci-0000_00_1f.3.analog-stereo",
    /// or DEFAULT_DEVICE to let the server pick), decodes `path` with rodio,
    /// and streams PCM into it on a background thread.
    fn spawn_sink(&self, path: &Path, volume: f32, device_name: &str) -> Option<PulseSink> {
        let file = File::open(path).ok()?;
        let decoder = Decoder::new(BufReader::new(file)).ok()?;

        let channels = decoder.channels();
        let sample_rate = decoder.sample_rate();

        let spec = Spec {
            format: Format::S16NE,
            channels: channels as u8,
            rate: sample_rate,
        };
        if !spec.is_valid() {
            return None;
        }

        let device = if device_name.is_empty() || device_name == DEFAULT_DEVICE {
            None
        } else {
            Some(device_name)
        };

        let simple = Simple::new(
            None, // default server (works for both PulseAudio and pipewire-pulse)
            "Nisound",
            Direction::Playback,
            device,
            "sound effect",
            &spec,
            None,
            None,
        )
        .ok()?;

        let stop_flag = Arc::new(AtomicBool::new(false));
        let finished_flag = Arc::new(AtomicBool::new(false));
        let thread_stop = stop_flag.clone();
        let thread_finished = finished_flag.clone();
        let volume = volume.clamp(0.0, 2.0);

        std::thread::spawn(move || {
            const CHUNK_FRAMES: usize = 1024;
            let chunk_samples = CHUNK_FRAMES * channels as usize;
            let mut decoder = decoder;
            let mut byte_buf: Vec<u8> = Vec::with_capacity(chunk_samples * 2);

            'outer: loop {
                byte_buf.clear();
                for _ in 0..chunk_samples {
                    match decoder.next() {
                        Some(sample) => {
                            let scaled = (sample as f32 * volume)
                                .clamp(i16::MIN as f32, i16::MAX as f32)
                                as i16;
                            byte_buf.extend_from_slice(&scaled.to_ne_bytes());
                        }
                        None => break 'outer,
                    }
                    if thread_stop.load(Ordering::Relaxed) {
                        break 'outer;
                    }
                }
                if byte_buf.is_empty() {
                    break;
                }
                if simple.write(&byte_buf).is_err() {
                    break;
                }
            }

            if thread_stop.load(Ordering::Relaxed) {
                let _ = simple.flush();
            } else {
                let _ = simple.drain();
            }

            thread_finished.store(true, Ordering::Relaxed);
        });

        Some(PulseSink {
            stop_flag,
            finished_flag,
        })
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
