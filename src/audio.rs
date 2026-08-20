use libpulse_binding as pulse;
use pulse::context::{Context, FlagSet as ContextFlagSet};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::proplist::Proplist;
use pulse::sample::{Format, Spec};
use pulse::stream::Direction;
use libpulse_simple_binding::Simple;
use rodio::{Decoder, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct PulseSink {
    stop_flag: Arc<AtomicBool>,
    finished_flag: Arc<AtomicBool>,
    pub is_enabled: Arc<AtomicBool>,
    pub is_paused: Arc<AtomicBool>,
    pub is_looping: Arc<AtomicBool>,
    pub progress_frames: Arc<AtomicU32>,
    pub volume: Arc<AtomicU32>,
    pub is_mic: bool,
    pub sample_rate: u32,
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
    pub duration_secs: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct QueueItem {
    pub queue_id: Uuid,
    pub sound_id: Uuid,
    pub name: String,
    pub path: PathBuf,
    pub headphones: bool,
    pub mic: bool,
    pub default_headphone_device: String,
    pub default_mic_sink: String,
    pub duration_secs: Option<u64>,
}

pub struct AudioEngine {
    pub active_sounds: Arc<Mutex<Vec<ActiveSound>>>,
    pub pending_queue: Arc<Mutex<Vec<QueueItem>>>,
    pub routing_mode: Arc<Mutex<crate::config::RoutingMode>>,
    pub direct_targets: Arc<Mutex<Vec<String>>>,
}

pub const DEFAULT_DEVICE: &str = "Default";

impl AudioEngine {
    pub fn init(routing_mode: crate::config::RoutingMode, direct_targets: Vec<String>) -> Self {
        Self {
            active_sounds: Arc::new(Mutex::new(Vec::new())),
            pending_queue: Arc::new(Mutex::new(Vec::new())),
            routing_mode: Arc::new(Mutex::new(routing_mode)),
            direct_targets: Arc::new(Mutex::new(direct_targets)),
        }
    }

    pub fn has_active_sounds(&self) -> bool {
        self.active_sounds.lock().map(|a| !a.is_empty()).unwrap_or(false)
    }

    pub fn stop_all(&self) {
        if let Ok(mut active) = self.active_sounds.lock() {
            for sound in active.drain(..) {
                for sink in sound.sinks {
                    sink.stop();
                }
            }
        }
        if let Ok(mut queue) = self.pending_queue.lock() {
            queue.clear();
        }
    }

    pub fn update_live_volume(&self, sound_id: Uuid, local_vol: f32, mic_vol: f32) {
        if let Ok(active) = self.active_sounds.lock() {
            for s in active.iter().filter(|s| s.sound_id == sound_id) {
                for sink in &s.sinks {
                    let v = if sink.is_mic { mic_vol } else { local_vol };
                    sink.volume.store(v.to_bits(), Ordering::Relaxed);
                }
            }
        }
    }

    pub fn update_live_channels(&self, sound_id: Uuid, headphones_enabled: bool, mic_enabled: bool) {
        if let Ok(active) = self.active_sounds.lock() {
            for s in active.iter().filter(|s| s.sound_id == sound_id) {
                for sink in &s.sinks {
                    let enabled = if sink.is_mic {
                        mic_enabled
                    } else {
                        headphones_enabled
                    };
                    sink.is_enabled.store(enabled, Ordering::Relaxed);
                }
            }
        }
    }

    pub fn toggle_pause(&self, play_id: Uuid) {
        if let Ok(active) = self.active_sounds.lock() {
            if let Some(sound) = active.iter().find(|s| s.play_id == play_id) {
                for sink in &sound.sinks {
                    let current = sink.is_paused.load(Ordering::Relaxed);
                    sink.is_paused.store(!current, Ordering::Relaxed);
                }
            }
        }
    }

    pub fn toggle_loop(&self, play_id: Uuid) {
        if let Ok(active) = self.active_sounds.lock() {
            if let Some(sound) = active.iter().find(|s| s.play_id == play_id) {
                for sink in &sound.sinks {
                    let current = sink.is_looping.load(Ordering::Relaxed);
                    sink.is_looping.store(!current, Ordering::Relaxed);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
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
        default_mic_sink: &str,
        duration_secs: Option<u64>,
        allow_overlap: bool,
        queue_sounds: bool,
    ) {
        if !path.exists() {
            return;
        }

        if !allow_overlap {
            let is_playing = !self.active_sounds.lock().unwrap().is_empty();
            if is_playing {
                if queue_sounds {
                    if let Ok(mut queue) = self.pending_queue.lock() {
                        queue.push(QueueItem {
                            queue_id: Uuid::new_v4(),
                            sound_id,
                            name: name.to_string(),
                            path: path.to_path_buf(),
                            headphones,
                            mic,
                            default_headphone_device: default_headphone_device.to_string(),
                            default_mic_sink: default_mic_sink.to_string(),
                            duration_secs,
                        });
                    }
                    return;
                } else {
                    self.stop_all();
                }
            }
        }

        let play_id = Uuid::new_v4();
        let mut sinks = Vec::new();

        // Створюємо обидва sinks, щоб користувач міг динамічно вмикати/вимикати канали на ходу
        if let Some(sink) = self.spawn_sink(path, volume_playback, default_headphone_device, false, headphones) {
            sinks.push(sink);
        }

        if let Some(sink) = self.spawn_sink(path, volume_out, default_mic_sink, true, mic) {
            sinks.push(sink);
        }

        if !sinks.is_empty() {
            if let Ok(mut active) = self.active_sounds.lock() {
                active.push(ActiveSound {
                    play_id,
                    sound_id,
                    name: name.to_string(),
                    sinks,
                    duration_secs,
                });
            }
        }
    }

    fn spawn_sink(
        &self,
        path: &Path,
        initial_volume: f32,
        device_name: &str,
        is_mic: bool,
        initially_enabled: bool,
    ) -> Option<PulseSink> {
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

        let target_sink: Option<String> = if device_name.is_empty() || device_name == DEFAULT_DEVICE {
            None
        } else {
            Some(device_name.to_string())
        };

        let stream_name = format!("nisound-{}", Uuid::new_v4());

        let simple = Simple::new(
            None,
            "Nisound",
            Direction::Playback,
            target_sink.as_deref(),
            &stream_name,
            &spec,
            None,
            None,
        )
        .ok()?;

        let mode = self.routing_mode.lock().unwrap().clone();
        let targets = self.direct_targets.lock().unwrap().clone();

        if let Some(sink_name) = &target_sink {
            let priming_ms = 50u32;
            let priming_frames = (sample_rate * priming_ms / 1000) as usize;
            let silence = vec![0u8; priming_frames * channels as usize * 2];
            let _ = simple.write(&silence);

            if is_mic && mode == crate::config::RoutingMode::DirectTarget {
                move_stream_to_sink(&stream_name, "nisound_mic_sink");
                let active_targets = crate::pipewire_routing::list_active_targets();
                let selected: Vec<crate::pipewire_routing::PwTargetNode> = active_targets
                    .into_iter()
                    .filter(|t| targets.contains(&t.display_name))
                    .collect();

                std::thread::spawn(move || {
                    crate::pipewire_routing::link_stream_to_targets(&selected);
                });
            } else {
                move_stream_to_sink(&stream_name, sink_name);
            }
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let finished_flag = Arc::new(AtomicBool::new(false));
        let is_enabled = Arc::new(AtomicBool::new(initially_enabled));
        let volume_atomic = Arc::new(AtomicU32::new(initial_volume.to_bits()));
        let is_paused = Arc::new(AtomicBool::new(false));
        let is_looping = Arc::new(AtomicBool::new(false));
        let progress_frames = Arc::new(AtomicU32::new(0));

        let thread_stop = stop_flag.clone();
        let thread_finished = finished_flag.clone();
        let thread_enabled = is_enabled.clone();
        let thread_volume = volume_atomic.clone();
        let thread_paused = is_paused.clone();
        let thread_looping = is_looping.clone();
        let thread_progress = progress_frames.clone();
        let path_clone = path.to_path_buf();

        std::thread::spawn(move || {
            const CHUNK_FRAMES: usize = 2048;
            let chunk_samples = CHUNK_FRAMES * channels as usize;
            let mut decoder = decoder;
            let mut byte_buf: Vec<u8> = Vec::with_capacity(chunk_samples * 2);
            let mut frames_played = 0;

            'outer: loop {
                if thread_paused.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }

                byte_buf.clear();
                let channel_on = thread_enabled.load(Ordering::Relaxed);
                let current_vol = if channel_on {
                    f32::from_bits(thread_volume.load(Ordering::Relaxed)).clamp(0.0, 2.0)
                } else {
                    0.0
                };
                
                for _ in 0..chunk_samples {
                    match decoder.next() {
                        Some(sample) => {
                            let scaled = (sample as f32 * current_vol)
                                .clamp(i16::MIN as f32, i16::MAX as f32)
                                as i16;
                            byte_buf.extend_from_slice(&scaled.to_ne_bytes());
                        }
                        None => {
                            if thread_looping.load(Ordering::Relaxed) {
                                if let Ok(file) = std::fs::File::open(&path_clone) {
                                    if let Ok(new_dec) = rodio::Decoder::new(std::io::BufReader::new(file)) {
                                        decoder = new_dec;
                                        frames_played = 0;
                                        thread_progress.store(0, Ordering::Relaxed);
                                        continue 'outer;
                                    }
                                }
                            }
                            break 'outer;
                        }
                    }
                    if thread_stop.load(Ordering::Relaxed) {
                        break 'outer;
                    }
                }
                
                frames_played += CHUNK_FRAMES as u32;
                thread_progress.store(frames_played, Ordering::Relaxed);

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
            is_enabled,
            is_paused,
            is_looping,
            progress_frames,
            volume: volume_atomic,
            is_mic,
            sample_rate,
        })
    }

    pub fn stop_sound(&self, play_id: Uuid, state: &crate::config::AppState) {
        let mut trigger_queue = false;
        if let Ok(mut active) = self.active_sounds.lock() {
            if let Some(pos) = active.iter().position(|s| s.play_id == play_id) {
                let sound = active.remove(pos);
                for sink in sound.sinks {
                    sink.stop();
                }
            }
            if active.is_empty() {
                trigger_queue = true;
            }
        }

        if trigger_queue {
            self.play_next_in_queue(state);
        }
    }

    pub fn play_next_in_queue(&self, state: &crate::config::AppState) {
        let next_item = {
            if let Ok(mut queue) = self.pending_queue.lock() {
                if !queue.is_empty() {
                    Some(queue.remove(0))
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(item) = next_item {
            let (v_local, v_mic) = if let Some(s) = state.sounds.get(&item.sound_id) {
                if s.use_global_volume {
                    (state.settings.global_volume_playback, state.settings.global_volume_out)
                } else {
                    (s.volume_playback, s.volume_out)
                }
            } else {
                (1.0, 1.0)
            };

            self.play_sound(
                item.sound_id,
                &item.name,
                &item.path,
                v_local,
                v_mic,
                item.headphones,
                item.mic,
                &item.default_headphone_device,
                &item.default_mic_sink,
                item.duration_secs,
                false,
                true,
            );
        }
    }

    pub fn clean_dead_sinks(&self, state: &crate::config::AppState) {
        let mut trigger_queue = false;

        if let Ok(mut active) = self.active_sounds.lock() {
            if !active.is_empty() {
                let was_playing = true;
                active.retain(|s| s.sinks.iter().any(|sink| !sink.empty()));
                if was_playing && active.is_empty() {
                    trigger_queue = true;
                }
            }
        }

        if trigger_queue {
            self.play_next_in_queue(state);
        }
    }
}

fn move_stream_to_sink(stream_name: &str, sink_name: &str) {
    let mut proplist = match Proplist::new() {
        Some(p) => p,
        None => return,
    };
    let _ = proplist.set_str(pulse::proplist::properties::APPLICATION_NAME, "Nisound");

    let mut mainloop = match Mainloop::new() {
        Some(m) => m,
        None => return,
    };

    let context = match Context::new_with_proplist(&mainloop, "NisoundMoveStream", &proplist) {
        Some(c) => std::cell::RefCell::new(c),
        None => return,
    };

    if context
        .borrow_mut()
        .connect(None, ContextFlagSet::NOFLAGS, None)
        .is_err()
    {
        return;
    }

    loop {
        match mainloop.iterate(true) {
            IterateResult::Quit(_) | IterateResult::Err(_) => return,
            IterateResult::Success(_) => {}
        }
        match context.borrow().get_state() {
            pulse::context::State::Ready => break,
            pulse::context::State::Failed | pulse::context::State::Terminated => return,
            _ => {}
        }
    }

    let found_index: std::rc::Rc<std::cell::RefCell<Option<u32>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let done: std::rc::Rc<std::cell::RefCell<bool>> = std::rc::Rc::new(std::cell::RefCell::new(false));

    let found_for_cb = found_index.clone();
    let done_for_cb = done.clone();
    let target_name = stream_name.to_string();
    let op = context
        .borrow()
        .introspect()
        .get_sink_input_info_list(move |res| {
            if let pulse::callbacks::ListResult::Item(info) = res {
                let name = info.name.as_ref().map(|c| c.to_string()).unwrap_or_default();
                if name == target_name {
                    *found_for_cb.borrow_mut() = Some(info.index);
                }
            } else if matches!(res, pulse::callbacks::ListResult::End) {
                *done_for_cb.borrow_mut() = true;
            }
        });
    while !*done.borrow() {
        mainloop.iterate(true);
    }
    drop(op);

    let index = match *found_index.borrow() {
        Some(i) => i,
        None => {
            context.borrow_mut().disconnect();
            return;
        }
    };

    let move_done: std::rc::Rc<std::cell::RefCell<bool>> =
        std::rc::Rc::new(std::cell::RefCell::new(false));
    let move_done_cb = move_done.clone();
    let op = {
        let mut introspector = context.borrow().introspect();
        introspector.move_sink_input_by_name(
            index,
            sink_name,
            Some(Box::new(move |_success| {
                *move_done_cb.borrow_mut() = true;
            })),
        )
    };
    while !*move_done.borrow() {
        mainloop.iterate(true);
    }
    drop(op);

    context.borrow_mut().disconnect();
}
