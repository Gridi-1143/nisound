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
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// A single output stream feeding one PulseAudio sink.
pub struct PulseSink {
    stop_flag: Arc<AtomicBool>,
    finished_flag: Arc<AtomicBool>,
    pub volume: Arc<AtomicU32>,
    pub is_mic: bool,
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

pub const DEFAULT_DEVICE: &str = "Default";

impl AudioEngine {
    pub fn init() -> Self {
        Self {
            active_sounds: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn stop_all(&self) {
        if let Ok(mut active) = self.active_sounds.lock() {
            for sound in active.drain(..) {
                for sink in sound.sinks {
                    sink.stop();
                }
            }
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
        allow_overlap: bool,
    ) {
        if !path.exists() {
            return;
        }

        if !allow_overlap {
            self.stop_all();
        }

        let play_id = Uuid::new_v4();
        let mut sinks = Vec::new();

        if headphones {
            if let Some(sink) = self.spawn_sink(path, volume_playback, default_headphone_device, false) {
                sinks.push(sink);
            }
        }

        if mic {
            if let Some(sink) = self.spawn_sink(path, volume_out, default_mic_sink, true) {
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

    fn spawn_sink(&self, path: &Path, initial_volume: f32, device_name: &str, is_mic: bool) -> Option<PulseSink> {
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

        let target_sink: Option<String> = if device_name.is_empty() || device_name == DEFAULT_DEVICE
        {
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

        if let Some(sink_name) = &target_sink {
            let priming_ms = 50u32;
            let priming_frames = (sample_rate * priming_ms / 1000) as usize;
            let silence = vec![0u8; priming_frames * channels as usize * 2];
            let _ = simple.write(&silence);

            move_stream_to_sink(&stream_name, sink_name);
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let finished_flag = Arc::new(AtomicBool::new(false));
        let volume_atomic = Arc::new(AtomicU32::new(initial_volume.to_bits()));

        let thread_stop = stop_flag.clone();
        let thread_finished = finished_flag.clone();
        let thread_volume = volume_atomic.clone();

        std::thread::spawn(move || {
            const CHUNK_FRAMES: usize = 1024;
            let chunk_samples = CHUNK_FRAMES * channels as usize;
            let mut decoder = decoder;
            let mut byte_buf: Vec<u8> = Vec::with_capacity(chunk_samples * 2);

            'outer: loop {
                byte_buf.clear();
                let current_vol = f32::from_bits(thread_volume.load(Ordering::Relaxed)).clamp(0.0, 2.0);
                
                for _ in 0..chunk_samples {
                    match decoder.next() {
                        Some(sample) => {
                            let scaled = (sample as f32 * current_vol)
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
            volume: volume_atomic,
            is_mic,
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
