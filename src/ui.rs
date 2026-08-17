use crate::audio::AudioEngine;
use crate::config::{AppState, Folder, SoundEntry};
use crate::pulse_devices::{self, AudioDevice, DeviceFilter, DeviceKind};
use egui::{Color32, Context, Ui};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq)]
enum SortOrder {
    TimeAsc,
    TimeDesc,
    AlphaAsc,
    AlphaDesc,
}

#[derive(Clone)]
struct FolderImportRequest {
    source_path: PathBuf,
    audio_paths: Vec<PathBuf>,
}

enum PendingAction {
    DeleteSound(Vec<Uuid>),
    DeleteFolder(Uuid),
    ClearAllSounds,
}

#[derive(Clone, Debug, PartialEq)]
enum KeybindTarget {
    Sound(Uuid),
    StopAll,
}

#[derive(Clone, Debug)]
struct KeybindCapture {
    target: KeybindTarget,
    ctrl: bool,
    shift: bool,
    alt: bool,
}

pub struct SoundboardApp {
    pub state: AppState,
    pub audio: AudioEngine,
    pub selected_sounds: HashSet<Uuid>,
    pub show_settings: bool,
    pub active_settings_tab: usize,
    pub hotkey_rx: Option<std::sync::mpsc::Receiver<crate::GlobalEvent>>,
    pub search_query: String,
    sort_order: SortOrder,
    output_devices: Vec<AudioDevice>,
    input_devices: Vec<AudioDevice>,
    output_filter: DeviceFilter,
    mic_filter: DeviceFilter,
    mic_channel_exists: bool,
    mic_loopback_active: bool,
    pending_folder_import: Option<FolderImportRequest>,
    active_sound_popup: Option<Uuid>,
    active_sound_popup_tab: usize,
    error_message: Option<String>,
    action_delete_sounds: Vec<Uuid>,
    action_remove_sounds_from_folder: Vec<Uuid>,
    show_new_folder_dialog: bool,
    new_folder_name: String,
    action_delete_folder: Option<Uuid>,
    show_clear_all_confirm: bool,
    last_selected_sound: Option<Uuid>,
    confirm_action: Option<PendingAction>,
    keybind_capture: Option<KeybindCapture>,
}

impl SoundboardApp {
    pub fn new(state: AppState) -> Self {
        pulse_devices::ensure_mic_channel();

        let (output_devices, input_devices) = pulse_devices::list_devices();

        if state.settings.mic_loopback_enabled {
            pulse_devices::set_mic_loopback(true, &state.settings.mic_loopback_source);
        }

        let (mic_channel_exists, mic_loopback_active) = pulse_devices::mic_channel_status();

        let audio = AudioEngine::init(
            state.settings.routing_mode.clone(),
            state.settings.direct_targets.clone(),
        );

        Self {
            state,
            audio,
            output_devices,
            input_devices,
            output_filter: DeviceFilter::All,
            mic_filter: DeviceFilter::All,
            mic_channel_exists,
            mic_loopback_active,
            hotkey_rx: None,
            search_query: String::new(),
            sort_order: SortOrder::TimeDesc,
            selected_sounds: HashSet::new(),
            show_settings: false,
            active_settings_tab: 0,
            pending_folder_import: None,
            active_sound_popup: None,
            active_sound_popup_tab: 0,
            error_message: None,
            action_delete_sounds: Vec::new(),
            action_remove_sounds_from_folder: Vec::new(),
            show_new_folder_dialog: false,
            new_folder_name: String::new(),
            action_delete_folder: None,
            show_clear_all_confirm: false,
            last_selected_sound: None,
            confirm_action: None,
            keybind_capture: None,
        }
    }

    pub fn shutdown(&self) {
        pulse_devices::remove_mic_channel();
    }

    fn refresh_devices(&mut self) {
        let (outputs, inputs) = pulse_devices::list_devices();
        self.output_devices = outputs;
        self.input_devices = inputs;
    }

    fn refresh_mic_channel_status(&mut self) {
        let (exists, loopback) = pulse_devices::mic_channel_status();
        self.mic_channel_exists = exists;
        self.mic_loopback_active = loopback;
    }

    fn device_label<'a>(devices: &'a [AudioDevice], name: &'a str) -> &'a str {
        devices
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.description.as_str())
            .unwrap_or(name)
    }

    fn execute_action(&mut self, action: PendingAction) {
        match action {
            PendingAction::DeleteSound(ids) => {
                for id in ids {
                    self.state.sounds.remove(&id);
                    for f in &mut self.state.folders {
                        f.sound_ids.retain(|s| s != &id);
                    }
                    self.selected_sounds.remove(&id);
                }
            }
            PendingAction::DeleteFolder(id) => {
                self.state.folders.retain(|f| f.id != id);
                if self.state.active_folder == Some(id) {
                    self.state.active_folder = None;
                }
            }
            PendingAction::ClearAllSounds => {
                self.state.sounds.clear();
                for f in &mut self.state.folders {
                    f.sound_ids.clear();
                }
                self.selected_sounds.clear();
            }
        }
        self.state.save();
    }

    fn get_effective_volume(&self, sound: &SoundEntry) -> (f32, f32) {
        if sound.use_global_volume {
            (self.state.settings.global_volume_playback, self.state.settings.global_volume_out)
        } else {
            (sound.volume_playback, sound.volume_out)
        }
    }

    pub fn render_ui(&mut self, ctx: &Context) {
        self.audio.clean_dead_sinks();

        if ctx.memory(|m| m.focused().is_none()) && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(sound_id) = self.last_selected_sound {
                if let Some(sound) = self.state.sounds.get(&sound_id).cloned() {
                    let (v_local, v_mic) = self.get_effective_volume(&sound);
                    self.audio.play_sound(
                        sound.id,
                        &sound.name,
                        &sound.path,
                        v_local,
                        v_mic,
                        sound.headphones_enabled,
                        sound.mic_enabled,
                        &self.state.settings.default_output,
                        &self.state.settings.mic_sink,
                        self.state.settings.allow_overlap,
                    );
                }
            }
        }

        // ── TOP TOOLBAR ──────────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("➕ Add Sound").clicked() {
                    if let Some(paths) = rfd::FileDialog::new()
                        .set_title("Select sound files")
                        .add_filter("Audio", &["mp3", "wav", "ogg", "flac", "m4a", "aac", "opus"])
                        .pick_files()
                    {
                        self.import_sound_paths(paths, self.state.active_folder);
                    }
                }

                if ui.button("📁 Add Folder").clicked() {
                    if let Some(folder_path) = rfd::FileDialog::new()
                        .set_title("Select folder with sounds")
                        .pick_folder()
                    {
                        let mut audio_paths = Vec::new();
                        Self::collect_audio_files(&folder_path, &mut audio_paths);

                        if !audio_paths.is_empty() {
                            self.pending_folder_import = Some(FolderImportRequest {
                                source_path: folder_path,
                                audio_paths,
                            });
                        }
                    }
                }

                ui.separator();

                if ui.button("🔄").on_hover_text("Refresh device list").clicked() {
                    self.refresh_devices();
                    self.refresh_mic_channel_status();
                }

                ui.label("To others (mic):");
                let mut mic_changed = false;
                let mic_label = Self::device_label(&self.output_devices, &self.state.settings.mic_sink).to_string();
                egui::ComboBox::from_id_source("def_mic")
                    .selected_text(mic_label)
                    .show_ui(ui, |ui| {
                        mic_changed |= ui
                            .selectable_value(
                                &mut self.state.settings.mic_sink,
                                crate::audio::DEFAULT_DEVICE.to_string(),
                                "Default",
                            )
                            .changed();
                        for dev in self.output_devices.iter() {
                            mic_changed |= ui
                                .selectable_value(
                                    &mut self.state.settings.mic_sink,
                                    dev.name.clone(),
                                    &dev.description,
                                )
                                .changed();
                        }
                    });
                if mic_changed {
                    self.state.save();
                }

                ui.separator();
                ui.label("Mode:");
                
                let mut routing_changed = false;
                egui::ComboBox::from_id_source("routing_mode")
                    .selected_text(match self.state.settings.routing_mode {
                        crate::config::RoutingMode::VirtualMic => "Virtual Mic",
                        crate::config::RoutingMode::DirectTarget => "Direct Target",
                    })
                    .show_ui(ui, |ui| {
                        routing_changed |= ui.selectable_value(
                            &mut self.state.settings.routing_mode,
                            crate::config::RoutingMode::VirtualMic,
                            "Virtual Mic"
                        ).changed();
                        routing_changed |= ui.selectable_value(
                            &mut self.state.settings.routing_mode,
                            crate::config::RoutingMode::DirectTarget,
                            "Direct Target"
                        ).changed();
                    });

                if self.state.settings.routing_mode == crate::config::RoutingMode::DirectTarget {
                    ui.menu_button("Select Target Apps", |ui| {
                        let active_targets = crate::pipewire_routing::list_active_targets();
                        for target in active_targets {
                            let mut is_selected = self.state.settings.direct_targets.contains(&target.display_name);
                            if ui.checkbox(&mut is_selected, &target.display_name).changed() {
                                if is_selected {
                                    self.state.settings.direct_targets.push(target.display_name.clone());
                                } else {
                                    self.state.settings.direct_targets.retain(|t| t != &target.display_name);
                                    crate::pipewire_routing::unlink_target(&target.display_name);
                                }
                                routing_changed = true;
                            }
                        }
                    });
                }

                if routing_changed {
                    if self.state.settings.routing_mode == crate::config::RoutingMode::DirectTarget {
                        if self.mic_loopback_active {
                            self.mic_loopback_active = false;
                            self.state.settings.mic_loopback_enabled = false;
                            pulse_devices::set_mic_loopback(false, &self.state.settings.mic_loopback_source);
                        }
                    } else {
                        for target_name in &self.state.settings.direct_targets {
                            crate::pipewire_routing::unlink_target(target_name);
                        }
                    }

                    if let Ok(mut mode) = self.audio.routing_mode.lock() {
                        *mode = self.state.settings.routing_mode.clone();
                    }
                    if let Ok(mut targets) = self.audio.direct_targets.lock() {
                        *targets = self.state.settings.direct_targets.clone();
                    }
                    self.state.save();
                }

                if self.state.settings.routing_mode == crate::config::RoutingMode::VirtualMic {
                    ui.separator();
                    ui.label("Mix voice in:");
                    let loopback_label = Self::device_label(
                        &self.input_devices,
                        &self.state.settings.mic_loopback_source,
                    )
                    .to_string();
                    
                    let mut loopback_source_changed = false;
                    egui::ComboBox::from_id_source("top_loopback_source")
                        .selected_text(loopback_label)
                        .show_ui(ui, |ui| {
                            loopback_source_changed |= ui.selectable_value(
                                &mut self.state.settings.mic_loopback_source,
                                crate::audio::DEFAULT_DEVICE.to_string(),
                                "Default",
                            ).changed();
                            for dev in self.input_devices.iter().filter(|d| d.kind != DeviceKind::Monitor) {
                                loopback_source_changed |= ui.selectable_value(
                                    &mut self.state.settings.mic_loopback_source,
                                    dev.name.clone(),
                                    &dev.description,
                                ).changed();
                            }
                        });

                    if loopback_source_changed {
                        if self.mic_loopback_active {
                            let ok = pulse_devices::set_mic_loopback(
                                true,
                                &self.state.settings.mic_loopback_source,
                            );
                            if !ok {
                                self.error_message = Some("Couldn't update voice loopback source.".to_string());
                            }
                        }
                        self.state.save();
                    }

                    let loopback_text = if self.mic_loopback_active {
                        "🔁 ON"
                    } else {
                        "🔁 OFF"
                    };
                    if ui.selectable_label(self.mic_loopback_active, loopback_text)
                        .on_hover_text("Mixes your real microphone into the mic channel.")
                        .clicked()
                    {
                        let new_state = !self.mic_loopback_active;
                        let ok = pulse_devices::set_mic_loopback(
                            new_state,
                            &self.state.settings.mic_loopback_source,
                        );
                        if ok {
                            self.state.settings.mic_loopback_enabled = new_state;
                            self.state.save();
                            self.refresh_mic_channel_status();
                        } else {
                            self.error_message = Some(
                                "Couldn't toggle voice loopback.".to_string(),
                            );
                        }
                    }
                }

                ui.separator();
                
                ui.label("Global Vol:");
                let mut global_vol_changed = false;
                
                ui.label("🎧");
                let hp_slider = ui.add_sized(
                    [65.0, 16.0],
                    egui::Slider::new(&mut self.state.settings.global_volume_playback, 0.0..=2.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                );
                if hp_slider.changed() {
                    global_vol_changed = true;
                }
                if hp_slider.secondary_clicked() {
                    self.state.settings.global_volume_playback = 1.0;
                    global_vol_changed = true;
                }
                hp_slider.on_hover_text("RMB to reset to 100%");

                ui.label("🎙");
                let mic_slider = ui.add_sized(
                    [65.0, 16.0],
                    egui::Slider::new(&mut self.state.settings.global_volume_out, 0.0..=2.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                );
                if mic_slider.changed() {
                    global_vol_changed = true;
                }
                if mic_slider.secondary_clicked() {
                    self.state.settings.global_volume_out = 1.0;
                    global_vol_changed = true;
                }
                mic_slider.on_hover_text("RMB to reset to 100%");

                if global_vol_changed {
                    self.state.save();
                    for (id, s) in &self.state.sounds {
                        if s.use_global_volume {
                            self.audio.update_live_volume(*id, self.state.settings.global_volume_playback, self.state.settings.global_volume_out);
                        }
                    }
                }

                ui.separator();
                if ui.button("⚙ Settings").clicked() {
                    self.show_settings = !self.show_settings;
                }
            });
        });

        // ── RIGHT PANEL: Categories + Playback ──────────────────────────────
        egui::SidePanel::right("folders")
            .width_range(200.0..=230.0)
            .show(ctx, |ui| {
                let active_snapshot: Vec<(Uuid, String)> = self
                    .audio
                    .active_sounds
                    .lock()
                    .map(|a| a.iter().map(|s| (s.play_id, s.name.clone())).collect())
                    .unwrap_or_default();

                let total_height = ui.available_height();

                egui::ScrollArea::vertical()
                    .id_source("folders_scroll")
                    .max_height(total_height * 0.5)
                    .show(ui, |ui| {
                        let all_sounds_btn =
                            ui.selectable_label(self.state.active_folder.is_none(), "all sounds");
                        if all_sounds_btn.clicked() {
                            self.state.active_folder = None;
                        }
                        all_sounds_btn.context_menu(|ui| {
                            if ui.button("clear all sounds").clicked() {
                                self.show_clear_all_confirm = true;
                                ui.close_menu();
                            }
                        });

                        ui.separator();

                        let mut target_folder_move = None;

                        for folder in &self.state.folders {
                            let is_selected = self.state.active_folder == Some(folder.id);
                            let response = ui.selectable_label(is_selected, &folder.name);

                            if response.clicked() {
                                self.state.active_folder = Some(folder.id);
                            }

                            if response.hovered() && ctx.input(|i| i.pointer.any_released()) {
                                target_folder_move = Some(folder.id);
                            }

                            response.context_menu(|ui| {
                                if ui.button("delete folder").clicked() {
                                    self.confirm_action =
                                        Some(PendingAction::DeleteFolder(folder.id));
                                    ui.close_menu();
                                }
                            });
                        }

                        if let Some(folder_id) = target_folder_move {
                            if !self.selected_sounds.is_empty() {
                                if let Some(f) =
                                    self.state.folders.iter_mut().find(|f| f.id == folder_id)
                                {
                                    for s_id in &self.selected_sounds {
                                        if !f.sound_ids.contains(s_id) {
                                            f.sound_ids.push(*s_id);
                                        }
                                    }
                                    self.state.save();
                                    self.selected_sounds.clear();
                                }
                            }
                        }

                        let remaining_rect = ui.available_rect_before_wrap();
                        let empty_space_resp = ui.interact(
                            remaining_rect,
                            ui.id().with("empty_space"),
                            egui::Sense::click(),
                        );
                        empty_space_resp.context_menu(|ui| {
                            if ui.button("create new folder").clicked() {
                                self.show_new_folder_dialog = true;
                                ui.close_menu();
                            }
                        });
                    });

                ui.separator();

                egui::ScrollArea::vertical().id_source("playback_scroll").show(ui, |ui| {
                    ui.add_space(4.0);
                    
                    let overlap_text = if self.state.settings.allow_overlap {
                        "🎵 Overlap: ON"
                    } else {
                        "🎵 Overlap: OFF"
                    };
                    if ui.selectable_label(self.state.settings.allow_overlap, overlap_text)
                        .on_hover_text("ON: sounds can play on top of each other. OFF: starting a sound stops all others.")
                        .clicked()
                    {
                        self.state.settings.allow_overlap = !self.state.settings.allow_overlap;
                        self.state.save();
                    }
                    
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.heading("Playback");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("⏹ Stop All").clicked() {
                                self.audio.stop_all();
                            }

                            let is_capturing = self.keybind_capture.as_ref()
                                .map(|k| k.target == KeybindTarget::StopAll)
                                .unwrap_or(false);

                            if is_capturing {
                                let capture = self.keybind_capture.as_ref().unwrap();
                                let mut preview = String::new();
                                if capture.ctrl  { preview.push_str("Ctrl+"); }
                                if capture.alt   { preview.push_str("Alt+"); }
                                if capture.shift { preview.push_str("Shift+"); }
                                preview.push_str("...");

                                ui.colored_label(Color32::YELLOW, &preview);

                                let events = ctx.input(|i| i.events.clone());
                                let mods = ctx.input(|i| i.modifiers);

                                if let Some(cap) = self.keybind_capture.as_mut() {
                                    cap.ctrl  = mods.command || mods.ctrl;
                                    cap.alt   = mods.alt;
                                    cap.shift = mods.shift;
                                }

                                for e in events {
                                    if let egui::Event::Key { key, pressed: true, .. } = e {
                                        if key == egui::Key::Escape {
                                            self.keybind_capture = None;
                                        } else {
                                            let cap = self.keybind_capture.take().unwrap();
                                            let mut combo = String::new();
                                            if cap.ctrl  { combo.push_str("Ctrl+"); }
                                            if cap.alt   { combo.push_str("Alt+"); }
                                            if cap.shift { combo.push_str("Shift+"); }
                                            combo.push_str(&format!("{:?}", key));
                                            self.state.settings.stop_all_hotkey = Some(combo);
                                            self.state.save();
                                        }
                                    }
                                }
                            } else {
                                let btn_text = match &self.state.settings.stop_all_hotkey {
                                    Some(hk) => format!("⌨ {}", hk),
                                    None => "➕".to_string(),
                                };
                                let btn_resp = ui.small_button(&btn_text);
                                if btn_resp.clicked() {
                                    let mods = ctx.input(|i| i.modifiers);
                                    self.keybind_capture = Some(KeybindCapture {
                                        target: KeybindTarget::StopAll,
                                        ctrl:  mods.command || mods.ctrl,
                                        alt:   mods.alt,
                                        shift: mods.shift,
                                    });
                                } else if btn_resp.secondary_clicked() {
                                    self.state.settings.stop_all_hotkey = None;
                                    self.state.save();
                                }
                                btn_resp.on_hover_text("LMB to assign hotkey, RMB to remove");
                            }
                        });
                    });
                        
                    ui.separator();

                    if active_snapshot.is_empty() {
                        ui.weak("Nothing playing");
                    } else {
                        let mut to_stop: Option<Uuid> = None;

                        for (play_id, name) in &active_snapshot {
                            ui.horizontal(|ui| {
                                if ui.add_sized([18.0, 18.0], egui::Button::new("✕").fill(Color32::from_rgb(160, 60, 60))).clicked() {
                                    to_stop = Some(*play_id);
                                }
                                let label = Self::truncate_name(name, 22);
                                ui.label(label);
                            });
                        }

                        if let Some(pid) = to_stop {
                            self.audio.stop_sound(pid);
                        }
                    }
                });
            });

        // ── CENTRAL PANEL: Sound Grid ────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.text_edit_singleline(&mut self.search_query);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_source("sort_order")
                        .selected_text(match self.sort_order {
                            SortOrder::TimeDesc => "Time (Newest)",
                            SortOrder::TimeAsc => "Time (Oldest)",
                            SortOrder::AlphaAsc => "A-Z",
                            SortOrder::AlphaDesc => "Z-A",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.sort_order, SortOrder::TimeDesc, "Time (Newest)");
                            ui.selectable_value(&mut self.sort_order, SortOrder::TimeAsc, "Time (Oldest)");
                            ui.selectable_value(&mut self.sort_order, SortOrder::AlphaAsc, "A-Z");
                            ui.selectable_value(&mut self.sort_order, SortOrder::AlphaDesc, "Z-A");
                        });
                    ui.label("Sort:");
                });
            });
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut sound_ids_to_render: Vec<Uuid> = {
                    let mut seen = std::collections::HashSet::new();

                    match self.state.active_folder {
                        None => self
                            .state
                            .sounds
                            .keys()
                            .cloned()
                            .filter(|id| seen.insert(*id))
                            .collect(),
                        Some(f_id) => self
                            .state
                            .folders
                            .iter()
                            .find(|f| f.id == f_id)
                            .map(|f| {
                                f.sound_ids
                                    .iter()
                                    .cloned()
                                    .filter(|id| seen.insert(*id))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    }
                };

                if !self.search_query.is_empty() {
                    let q = self.search_query.to_lowercase();
                    sound_ids_to_render.retain(|id| {
                        self.state.sounds.get(id).map_or(false, |s| s.name.to_lowercase().contains(&q))
                    });
                }

                sound_ids_to_render.sort_by(|a, b| {
                    let s_a = self.state.sounds.get(a);
                    let s_b = self.state.sounds.get(b);
                    if let (Some(sa), Some(sb)) = (s_a, s_b) {
                        match self.sort_order {
                            SortOrder::TimeAsc => sa.time_added.cmp(&sb.time_added),
                            SortOrder::TimeDesc => sb.time_added.cmp(&sa.time_added),
                            SortOrder::AlphaAsc => sa.name.to_lowercase().cmp(&sb.name.to_lowercase()),
                            SortOrder::AlphaDesc => sb.name.to_lowercase().cmp(&sa.name.to_lowercase()),
                        }
                    } else {
                        std::cmp::Ordering::Equal
                    }
                });

                let cell_width = 310.0;
                let available_width = ui.available_width().max(cell_width);
                let columns = ((available_width / cell_width).floor() as usize).max(1);

                egui::Grid::new(("sounds_grid", self.state.active_folder))
                    .num_columns(columns)
                    .spacing([10.0, 10.0])
                    .min_col_width(cell_width)
                    .max_col_width(cell_width)
                    .show(ui, |ui| {
                        let mut col_count = 0;
                        for id in sound_ids_to_render.iter() {
                            let id = *id;
                            if let Some(mut sound) = self.state.sounds.get(&id).cloned() {
                                self.render_sound_widget(ui, &mut sound, ctx, &sound_ids_to_render);
                                self.state.sounds.insert(id, sound);

                                col_count += 1;
                                if col_count == columns {
                                    col_count = 0;
                                    ui.end_row();
                                }
                            }
                        }
                    });
            });
        });

        // ── GENERAL SETTINGS WINDOW ──────────────────────────────────────────
        if self.show_settings {
            let mut want_refresh_devices = false;
            let mut want_refresh_mic_status = false;

            egui::Window::new("General Settings")
                .open(&mut self.show_settings)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.active_settings_tab, 0, "Tracks");
                        ui.selectable_value(&mut self.active_settings_tab, 1, "Interface");
                    });
                    ui.separator();

                    match self.active_settings_tab {
                        0 => {
                            ui.label("Audio devices");

                            if ui.button("🔄 Update device list").clicked() {
                                want_refresh_devices = true;
                            }

                            ui.separator();
                            ui.label("Local sound — only you hear this (headphones/speakers):");
                            
                            let mut out_changed = false;
                            let out_label = Self::device_label(&self.output_devices, &self.state.settings.default_output).to_string();
                            egui::ComboBox::from_id_source("def_out_settings")
                                .selected_text(out_label)
                                .show_ui(ui, |ui| {
                                    out_changed |= ui.selectable_value(
                                        &mut self.state.settings.default_output,
                                        crate::audio::DEFAULT_DEVICE.to_string(),
                                        "Default",
                                    ).changed();
                                    for dev in self.output_devices.iter() {
                                        out_changed |= ui.selectable_value(
                                            &mut self.state.settings.default_output,
                                            dev.name.clone(),
                                            &dev.description,
                                        ).changed();
                                    }
                                });
                            if out_changed {
                                self.state.save();
                            }

                            ui.separator();
                            ui.heading("Mic channel — what others hear");
                            ui.label(
                                "Nisound keeps its own virtual sink+microphone running \
                                 automatically while the app is open, so soundboard sounds can \
                                 reach Discord/Zoom/OBS. Pick \"Nisound_Mic\" as your microphone \
                                 there once."
                            );

                            let status_text = if self.mic_channel_exists {
                                "✔ Nisound_Mic is active"
                            } else {
                                "✘ Not active (will be created automatically)"
                            };
                            ui.horizontal(|ui| {
                                ui.label(status_text);
                                if ui.small_button("🔄").on_hover_text("Refresh status").clicked() {
                                    want_refresh_mic_status = true;
                                }
                            });
                        }
                        1 => {
                            ui.label("Theme Color Editor");
                            let mut changed = false;
                            for (key, color) in self.state.settings.colors.iter_mut() {
                                ui.horizontal(|ui| {
                                    ui.label(key);
                                    changed |= ui.color_edit_button_srgb(color).changed();
                                });
                            }
                            if changed {
                                self.state.save();
                            }
                        }
                        _ => {}
                    }
                });

            if want_refresh_devices {
                self.refresh_devices();
            }
            if want_refresh_mic_status {
                self.refresh_mic_channel_status();
            }
        }

        self.render_folder_import_prompt(ctx);
        self.render_sound_popup(ctx);
        self.process_deferred_actions(ctx);
    }

    fn process_deferred_actions(&mut self, ctx: &Context) {
        if !self.action_remove_sounds_from_folder.is_empty() {
            if let Some(folder_id) = self.state.active_folder {
                if let Some(f) = self.state.folders.iter_mut().find(|f| f.id == folder_id) {
                    for id in &self.action_remove_sounds_from_folder {
                        f.sound_ids.retain(|s| s != id);
                    }
                    self.state.save();
                }
            }
            self.action_remove_sounds_from_folder.clear();
        }

        if !self.action_delete_sounds.is_empty() {
            for id in &self.action_delete_sounds {
                self.state.sounds.remove(id);
                for f in &mut self.state.folders {
                    f.sound_ids.retain(|s| s != id);
                }
                self.selected_sounds.remove(id);
            }
            self.state.save();
            self.action_delete_sounds.clear();
        }

        if let Some(id) = self.action_delete_folder.take() {
            self.state.folders.retain(|f| f.id != id);
            if self.state.active_folder == Some(id) {
                self.state.active_folder = None;
            }
            self.state.save();
        }

        if let Some(action) = &self.confirm_action {
            let mut open = true;
            let title = match action {
                PendingAction::DeleteSound(_) => "Confirm sound deletion",
                PendingAction::DeleteFolder(_) => "Confirm folder deletion",
                PendingAction::ClearAllSounds => "Confirm clear all",
            };

            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Are you sure? This action cannot be undone.");
                    ui.horizontal(|ui| {
                        if ui.button("Yes").clicked() {
                            if let Some(action_to_run) = self.confirm_action.take() {
                                self.execute_action(action_to_run);
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_action = None;
                        }
                    });
                });

            if !open {
                self.confirm_action = None;
            }
        }

        if self.show_new_folder_dialog {
            let mut open = true;
            egui::Window::new("New Category Folder")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        let resp = ui.text_edit_singleline(&mut self.new_folder_name);
                        resp.request_focus();

                        if resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            if !self.new_folder_name.trim().is_empty() {
                                self.state.folders.push(Folder {
                                    id: Uuid::new_v4(),
                                    name: self.new_folder_name.trim().to_string(),
                                    sound_ids: vec![],
                                });
                                self.state.save();
                            }
                            self.new_folder_name.clear();
                            self.show_new_folder_dialog = false;
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() {
                            if !self.new_folder_name.trim().is_empty() {
                                self.state.folders.push(Folder {
                                    id: Uuid::new_v4(),
                                    name: self.new_folder_name.trim().to_string(),
                                    sound_ids: vec![],
                                });
                                self.state.save();
                            }
                            self.new_folder_name.clear();
                            self.show_new_folder_dialog = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_new_folder_dialog = false;
                            self.new_folder_name.clear();
                        }
                    });
                });
            if !open {
                self.show_new_folder_dialog = false;
                self.new_folder_name.clear();
            }
        }

        let mut clear_error = false;
        if let Some(err) = &self.error_message {
            let mut open = true;
            egui::Window::new("Notice")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(err);
                    if ui.button("OK").clicked() {
                        clear_error = true;
                    }
                });
            if !open {
                clear_error = true;
            }
        }

        if clear_error {
            self.error_message = None;
        }
    }

    fn render_folder_import_prompt(&mut self, ctx: &Context) {
        let Some(request) = self.pending_folder_import.clone() else {
            return;
        };

        let current_folder_label = self.current_folder_label();
        let source_label = request.source_path.display().to_string();
        let audio_count = request.audio_paths.len();
        let mut open = true;

        egui::Window::new("Import folder")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(format!("Folder: {}", source_label));
                ui.label(format!("Audio files found: {}", audio_count));
                ui.separator();

                if ui.button("Create new folder for these sounds").clicked() {
                    self.finalize_folder_import(request.clone(), true);
                }

                let add_label = if self.state.active_folder.is_some() {
                    format!("Add to current folder ({})", current_folder_label)
                } else {
                    "Add to All Sounds".to_string()
                };

                if ui.button(add_label).clicked() {
                    self.finalize_folder_import(request.clone(), false);
                }

                if ui.button("Cancel").clicked() {
                    self.pending_folder_import = None;
                }
            });

        if !open {
            self.pending_folder_import = None;
        }
    }

    fn render_sound_popup(&mut self, ctx: &Context) {
        let Some(sound_id) = self.active_sound_popup else {
            return;
        };

        let Some(sound_name) = self.state.sounds.get(&sound_id).map(|s| s.name.clone()) else {
            self.active_sound_popup = None;
            return;
        };

        let mut open = true;

        let sound_clone = self.state.sounds.get(&sound_id).cloned();

        if let Some(mut sound) = sound_clone {
            let mut changed = false;

            egui::Window::new(format!("Sound: {}", sound_name))
                .id(egui::Id::new(("sound_popup", sound_id)))
                .min_width(360.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.active_sound_popup_tab, 0, "Settings");
                        ui.selectable_value(&mut self.active_sound_popup_tab, 1, "Edit");
                    });
                    ui.separator();

                    match self.active_sound_popup_tab {
                        0 => {
                            egui::Grid::new("sound_settings_grid")
                                .num_columns(2)
                                .spacing([12.0, 8.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.strong("File path:");
                                    let path_str = sound.path.display().to_string();
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(Self::truncate_name(&path_str, 40))
                                                .monospace()
                                                .weak(),
                                        )
                                        .sense(egui::Sense::hover()),
                                    )
                                    .on_hover_text(&path_str);
                                    ui.end_row();

                                    ui.strong("Status:");
                                    if sound.exists {
                                        ui.colored_label(Color32::from_rgb(100, 200, 100), "✔ File found");
                                    } else {
                                        ui.colored_label(Color32::from_rgb(220, 80, 80), "✘ File missing");
                                    }
                                    ui.end_row();

                                    ui.strong("Hotkey:");
                                    ui.horizontal(|ui| {
                                        let is_capturing = self.keybind_capture.as_ref()
                                            .map(|k| k.target == KeybindTarget::Sound(sound.id))
                                            .unwrap_or(false);

                                        if is_capturing {
                                            let capture = self.keybind_capture.as_ref().unwrap();
                                            let mut preview = String::new();
                                            if capture.ctrl  { preview.push_str("Ctrl+"); }
                                            if capture.alt   { preview.push_str("Alt+"); }
                                            if capture.shift { preview.push_str("Shift+"); }
                                            preview.push_str("...");

                                            ui.colored_label(Color32::YELLOW, &preview);
                                            ui.weak("(press key, Esc to cancel)");

                                            let events = ctx.input(|i| i.events.clone());
                                            let mods = ctx.input(|i| i.modifiers);

                                            if let Some(cap) = self.keybind_capture.as_mut() {
                                                cap.ctrl  = mods.command || mods.ctrl;
                                                cap.alt   = mods.alt;
                                                cap.shift = mods.shift;
                                            }

                                            for e in events {
                                                if let egui::Event::Key { key, pressed: true, .. } = e {
                                                    if key == egui::Key::Escape {
                                                        self.keybind_capture = None;
                                                    } else {
                                                        let cap = self.keybind_capture.take().unwrap();
                                                        let mut combo = String::new();
                                                        if cap.ctrl  { combo.push_str("Ctrl+"); }
                                                        if cap.alt   { combo.push_str("Alt+"); }
                                                        if cap.shift { combo.push_str("Shift+"); }
                                                        combo.push_str(&format!("{:?}", key));
                                                        sound.hotkey = Some(combo);
                                                        changed = true;
                                                    }
                                                }
                                            }
                                        } else {
                                            let btn_text = match &sound.hotkey {
                                                Some(hk) => format!("⌨ {}", hk),
                                                None => "➕Assign hotkey".to_string(),
                                            };
                                            let btn_resp = ui.small_button(&btn_text);
                                            if btn_resp.clicked() {
                                                let mods = ctx.input(|i| i.modifiers);
                                                self.keybind_capture = Some(KeybindCapture {
                                                    target: KeybindTarget::Sound(sound.id),
                                                    ctrl:  mods.command || mods.ctrl,
                                                    alt:   mods.alt,
                                                    shift: mods.shift,
                                                });
                                            } else if btn_resp.secondary_clicked() {
                                                sound.hotkey = None;
                                                changed = true;
                                            }
                                            btn_resp.on_hover_text("LMB to assign hotkey, RMB to remove");
                                        }
                                    });
                                    ui.end_row();

                                    ui.strong("Volume Settings:");
                                    let mut vol_changed = ui.checkbox(&mut sound.use_global_volume, "Use default volume").changed();
                                    ui.end_row();

                                    ui.strong("Volume (Local):");
                                    ui.add_enabled_ui(!sound.use_global_volume, |ui| {
                                        let s = ui.add(egui::Slider::new(&mut sound.volume_playback, 0.0..=2.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)).text("local"));
                                        if s.changed() { vol_changed = true; }
                                        if s.secondary_clicked() { sound.volume_playback = 1.0; vol_changed = true; }
                                        s.on_hover_text("RMB to reset to 100%");
                                    });
                                    ui.end_row();

                                    ui.strong("Volume (Mic):");
                                    ui.add_enabled_ui(!sound.use_global_volume, |ui| {
                                        let s = ui.add(egui::Slider::new(&mut sound.volume_out, 0.0..=2.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)).text("mic"));
                                        if s.changed() { vol_changed = true; }
                                        if s.secondary_clicked() { sound.volume_out = 1.0; vol_changed = true; }
                                        s.on_hover_text("RMB to reset to 100%");
                                    });
                                    ui.end_row();

                                    if vol_changed {
                                        changed = true;
                                        let v_local = if sound.use_global_volume { self.state.settings.global_volume_playback } else { sound.volume_playback };
                                        let v_mic = if sound.use_global_volume { self.state.settings.global_volume_out } else { sound.volume_out };
                                        self.audio.update_live_volume(sound_id, v_local, v_mic);
                                    }

                                    ui.strong("Local sound:");
                                    changed |= ui.checkbox(&mut sound.headphones_enabled, "enabled").changed();
                                    ui.end_row();

                                    ui.strong("Mic:");
                                    changed |= ui.checkbox(&mut sound.mic_enabled, "enabled").changed();
                                    ui.end_row();

                                    ui.strong("Channel routing:");
                                    match &sound.custom_channels {
                                        Some(ch) => {
                                            ui.vertical(|ui| {
                                                ui.label(format!("Mic: {}", ch.mic_sink));
                                                ui.label(format!("Out: {}", ch.output_device));
                                            });
                                        }
                                        None => {
                                            ui.label("Default (global settings)");
                                        }
                                    }
                                    ui.end_row();
                                });
                        }
                        1 => {
                            ui.horizontal(|ui| {
                                ui.strong("Name:");
                                changed |= ui.text_edit_singleline(&mut sound.name).changed();
                            });
                            ui.add_space(4.0);
                            ui.label("More editing options coming soon.");
                        }
                        _ => {}
                    }
                });

            if changed {
                self.state.sounds.insert(sound_id, sound);
                self.state.save();
            }
        }

        if !open {
            self.active_sound_popup = None;
        }
    }

    fn finalize_folder_import(&mut self, request: FolderImportRequest, create_new_folder: bool) {
        let target_folder = if create_new_folder {
            let folder_name = request
                .source_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Imported Sounds".to_string());

            let folder_id = Uuid::new_v4();
            self.state.folders.push(Folder {
                id: folder_id,
                name: folder_name,
                sound_ids: vec![],
            });
            self.state.active_folder = Some(folder_id);
            Some(folder_id)
        } else {
            self.state.active_folder
        };

        self.import_sound_paths(request.audio_paths, target_folder);
        self.pending_folder_import = None;
        self.state.save();
    }

    fn import_sound_paths(&mut self, paths: Vec<PathBuf>, target_folder: Option<Uuid>) {
        let mut skipped = 0;
        for path in paths {
            if !path.exists() || !Self::is_audio_file(&path) {
                continue;
            }

            let existing_id = self.state.sounds.iter()
                .find(|(_, s)| s.path == path)
                .map(|(id, _)| *id);

            match existing_id {
                Some(id) => {
                    if let Some(folder_id) = target_folder {
                        if let Some(folder) = self.state.folders.iter_mut().find(|f| f.id == folder_id) {
                            if !folder.sound_ids.contains(&id) {
                                folder.sound_ids.push(id);
                            } else {
                                skipped += 1;
                            }
                        }
                    } else {
                        skipped += 1;
                    }
                }
                None => {
                    let id = Uuid::new_v4();
                    let entry = Self::make_sound_entry(id, path);
                    self.state.sounds.insert(id, entry);

                    if let Some(folder_id) = target_folder {
                        if let Some(folder) = self.state.folders.iter_mut().find(|f| f.id == folder_id) {
                            if !folder.sound_ids.contains(&id) {
                                folder.sound_ids.push(id);
                            }
                        }
                    }
                }
            }
        }

        if skipped > 0 {
            self.error_message = Some(format!(
                "Skipped {} sound(s) that were already added to this scope.",
                skipped
            ));
        }

        self.state.save();
    }

    fn make_sound_entry(id: Uuid, path: PathBuf) -> SoundEntry {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        SoundEntry {
            id,
            name,
            path,
            hotkey: None,
            volume_out: 1.0,
            volume_playback: 1.0,
            use_global_volume: true,
            mic_enabled: true,
            headphones_enabled: true,
            custom_channels: None,
            time_added: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            exists: true,
        }
    }

    fn current_folder_label(&self) -> String {
        self.state
            .active_folder
            .and_then(|id| {
                self.state
                    .folders
                    .iter()
                    .find(|folder| folder.id == id)
                    .map(|folder| folder.name.clone())
            })
            .unwrap_or_else(|| "All Sounds".to_string())
    }

    fn collect_audio_files(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    Self::collect_audio_files(&path, out);
                } else if Self::is_audio_file(&path) {
                    out.push(path);
                }
            }
        }
    }

    fn is_audio_file(path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus" | "webm"
                )
            })
            .unwrap_or(false)
    }

    fn render_sound_widget(
        &mut self,
        ui: &mut Ui,
        sound: &mut SoundEntry,
        ctx: &Context,
        all_ids: &[Uuid],
    ) {
        ui.push_id(("sound_card", sound.id), |ui| {
            let is_selected = self.selected_sounds.contains(&sound.id);

            let frame_color = if !sound.exists {
                Color32::from_rgb(180, 82, 82)
            } else if is_selected {
                Color32::from_rgb(70, 90, 140)
            } else {
                Color32::from_rgb(45, 45, 56)
            };

            egui::Frame::none()
                .fill(frame_color)
                .inner_margin(4.0)
                .rounding(4.0)
                .show(ui, |ui| {
                    ui.set_width(310.0);
                    ui.set_height(54.0);

                    let widget_rect = ui.max_rect();
                    let bg_resp = ui.interact(
                        widget_rect,
                        ui.id().with("bg_click"),
                        egui::Sense::click_and_drag(),
                    );

                    if bg_resp.dragged() && is_selected {
                        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                        if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                            egui::Area::new(egui::Id::new("drag_tooltip"))
                                .fixed_pos(pos + egui::vec2(15.0, 15.0))
                                .order(egui::Order::Tooltip)
                                .show(ctx, |ui| {
                                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                                        ui.label(format!(
                                            "📦 Moving {} sound(s)",
                                            self.selected_sounds.len()
                                        ));
                                    });
                                });
                        }
                    }

                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let total_height = ui.available_height().max(54.0);

                        // ── PLAY SECTION ──────────────────────────────────────
                        let play_resp = ui
                            .push_id("play_col", |ui| {
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::Vec2::new(44.0, total_height),
                                    egui::Sense::click(),
                                );
                                ui.allocate_ui_at_rect(rect, |ui| {
                                    ui.with_layout(
                                        egui::Layout::top_down(egui::Align::Center),
                                        |ui| {
                                            ui.add_space((rect.height() - 28.0) / 2.0);
                                            let play_btn = ui.add_sized(
                                                [40.0, 28.0],
                                                egui::Button::new("▶"),
                                            );
                                            if play_btn.clicked() {
                                                let ctrl = ctx.input(|i| {
                                                    i.modifiers.command || i.modifiers.ctrl
                                                });
                                                let shift = ctx.input(|i| i.modifiers.shift);
                                                if !ctrl && !shift {
                                                    let (v_local, v_mic) = self.get_effective_volume(sound);
                                                    self.audio.play_sound(
                                                        sound.id,
                                                        &sound.name,
                                                        &sound.path,
                                                        v_local,
                                                        v_mic,
                                                        sound.headphones_enabled,
                                                        sound.mic_enabled,
                                                        &self.state.settings.default_output,
                                                        &self.state.settings.mic_sink,
                                                        self.state.settings.allow_overlap,
                                                    );
                                                }
                                            }
                                        },
                                    );
                                });
                                resp
                            })
                            .inner;

                        ui.separator();

                        // ── INFO SECTION ──────────────────────────────────────
                        let info_resp = ui
                            .push_id("info_col", |ui| {
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::Vec2::new(170.0, total_height),
                                    egui::Sense::click(),
                                );
                                ui.allocate_ui_at_rect(rect, |ui| {
                                    ui.with_layout(
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.style_mut().interaction.selectable_labels = false;
                                            ui.add_space((rect.height() - 32.0) / 2.0);

                                            let title = Self::truncate_name(&sound.name, 26);

                                            if sound.exists {
                                                ui.label(egui::RichText::new(title).strong());

                                                ui.horizontal(|ui| {
                                                    ui.label("⏱ --:--");

                                                    let is_capturing = self
                                                        .keybind_capture
                                                        .as_ref()
                                                        .map(|k| k.target == KeybindTarget::Sound(sound.id))
                                                        .unwrap_or(false);

                                                    if is_capturing {
                                                        let mods = ctx.input(|i| i.modifiers);
                                                        if let Some(cap) =
                                                            self.keybind_capture.as_mut()
                                                        {
                                                            cap.ctrl = mods.command || mods.ctrl;
                                                            cap.alt = mods.alt;
                                                            cap.shift = mods.shift;
                                                        }

                                                        let cap_ref =
                                                            self.keybind_capture.as_ref().unwrap();
                                                        let mut preview = String::new();
                                                        if cap_ref.ctrl {
                                                            preview.push_str("Ctrl+");
                                                        }
                                                        if cap_ref.alt {
                                                            preview.push_str("Alt+");
                                                        }
                                                        if cap_ref.shift {
                                                            preview.push_str("Shift+");
                                                        }
                                                        preview.push_str("_");

                                                        ui.colored_label(
                                                            Color32::YELLOW,
                                                            preview,
                                                        );

                                                        let events =
                                                            ctx.input(|i| i.events.clone());
                                                        for e in events {
                                                            if let egui::Event::Key {
                                                                key,
                                                                pressed: true,
                                                                ..
                                                            } = e
                                                            {
                                                                if key == egui::Key::Escape {
                                                                    self.keybind_capture = None;
                                                                } else {
                                                                    let cap = self
                                                                        .keybind_capture
                                                                        .take()
                                                                        .unwrap();
                                                                    let mut combo = String::new();
                                                                    if cap.ctrl {
                                                                        combo.push_str("Ctrl+");
                                                                    }
                                                                    if cap.alt {
                                                                        combo.push_str("Alt+");
                                                                    }
                                                                    if cap.shift {
                                                                        combo.push_str("Shift+");
                                                                    }
                                                                    combo.push_str(&format!(
                                                                        "{:?}",
                                                                        key
                                                                    ));
                                                                    sound.hotkey = Some(combo);
                                                                    self.state.save();
                                                                }
                                                            }
                                                        }
                                                    } else {
                                                        let btn_text = match &sound.hotkey {
                                                            Some(hk) => format!("⌨ {}", hk),
                                                            None => "➕".to_string(),
                                                        };
                                                        let btn_resp = ui.small_button(&btn_text);
                                                        if btn_resp.clicked() {
                                                            let mods =
                                                                ctx.input(|i| i.modifiers);
                                                            self.keybind_capture =
                                                                Some(KeybindCapture {
                                                                    target: KeybindTarget::Sound(sound.id),
                                                                    ctrl: mods.command
                                                                        || mods.ctrl,
                                                                    alt: mods.alt,
                                                                    shift: mods.shift,
                                                                });
                                                        } else if btn_resp.secondary_clicked() {
                                                            sound.hotkey = None;
                                                            self.state.save();
                                                        }
                                                        btn_resp.on_hover_text("LMB to assign hotkey, RMB to remove");
                                                    }
                                                });
                                            } else {
                                                ui.label(
                                                    egui::RichText::new("⚠️ Missing file")
                                                        .strong()
                                                        .color(Color32::WHITE),
                                                );
                                                ui.label(format!(
                                                    "Path: {}",
                                                    Self::truncate_name(
                                                        &sound.path.display().to_string(),
                                                        20
                                                    )
                                                ));
                                            }
                                        },
                                    );
                                });
                                resp
                            })
                            .inner;

                        ui.separator();

                        // ── CHANNELS SECTION ──────────────────────────────────
                        let channels_resp = ui
                            .push_id("channels_col", |ui| {
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::Vec2::new(60.0, total_height),
                                    egui::Sense::click(),
                                );
                                ui.allocate_ui_at_rect(rect, |ui| {
                                    ui.with_layout(
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.add_space((rect.height() - 44.0) / 2.0);

                                            if ui
                                                .add_sized(
                                                    [56.0, 20.0],
                                                    egui::Button::new("🎙 Mic"),
                                                )
                                                .clicked()
                                            {
                                                let ctrl = ctx.input(|i| {
                                                    i.modifiers.command || i.modifiers.ctrl
                                                });
                                                let shift = ctx.input(|i| i.modifiers.shift);
                                                if !ctrl && !shift {
                                                    let (v_local, v_mic) = self.get_effective_volume(sound);
                                                    self.audio.play_sound(
                                                        sound.id,
                                                        &sound.name,
                                                        &sound.path,
                                                        v_local,
                                                        v_mic,
                                                        false,
                                                        true,
                                                        &self.state.settings.default_output,
                                                        &self.state.settings.mic_sink,
                                                        self.state.settings.allow_overlap,
                                                    );
                                                }
                                            }

                                            if ui
                                                .add_sized(
                                                    [56.0, 20.0],
                                                    egui::Button::new("🎧 Local"),
                                                )
                                                .clicked()
                                            {
                                                let ctrl = ctx.input(|i| {
                                                    i.modifiers.command || i.modifiers.ctrl
                                                });
                                                let shift = ctx.input(|i| i.modifiers.shift);
                                                if !ctrl && !shift {
                                                    let (v_local, v_mic) = self.get_effective_volume(sound);
                                                    self.audio.play_sound(
                                                        sound.id,
                                                        &sound.name,
                                                        &sound.path,
                                                        v_local,
                                                        v_mic,
                                                        true,
                                                        false,
                                                        &self.state.settings.default_output,
                                                        &self.state.settings.mic_sink,
                                                        self.state.settings.allow_overlap,
                                                    );
                                                }
                                            }
                                        },
                                    );
                                });
                                resp
                            })
                            .inner;

                        let menu_content = |ui: &mut egui::Ui, app: &mut Self, s: &mut SoundEntry| {
                            ui.label("Shortcut:");
                            if s.hotkey.is_some() {
                                if ui.button("Clear Shortcut").clicked() {
                                    s.hotkey = None;
                                    app.state.save();
                                    ui.close_menu();
                                }
                            } else {
                                ui.weak("No shortcut assigned");
                            }
                            ui.separator();

                            if app.state.active_folder.is_some() {
                                if ui.button("Remove from folder").clicked() {
                                    if app.selected_sounds.contains(&s.id) {
                                        app.action_remove_sounds_from_folder =
                                            app.selected_sounds.iter().cloned().collect();
                                    } else {
                                        app.action_remove_sounds_from_folder = vec![s.id];
                                    }
                                    ui.close_menu();
                                }
                            }

                            if ui.button("Delete (from all sounds)").clicked() {
                                let ids = if app.selected_sounds.contains(&s.id) {
                                    app.selected_sounds.iter().cloned().collect()
                                } else {
                                    vec![s.id]
                                };
                                app.confirm_action = Some(PendingAction::DeleteSound(ids));
                                ui.close_menu();
                            }
                            ui.separator();

                            let mut vol_changed = ui.checkbox(&mut s.use_global_volume, "Default volume").changed();
                            
                            ui.add_enabled_ui(!s.use_global_volume, |ui| {
                                ui.label("Volume (Local sound):");
                                let hp_s = ui.add(egui::Slider::new(&mut s.volume_playback, 0.0..=2.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                                if hp_s.changed() { vol_changed = true; }
                                if hp_s.secondary_clicked() { s.volume_playback = 1.0; vol_changed = true; }
                                hp_s.on_hover_text("RMB to reset to 100%");

                                ui.label("Volume (Mic channel):");
                                let mic_s = ui.add(egui::Slider::new(&mut s.volume_out, 0.0..=2.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                                if mic_s.changed() { vol_changed = true; }
                                if mic_s.secondary_clicked() { s.volume_out = 1.0; vol_changed = true; }
                                mic_s.on_hover_text("RMB to reset to 100%");
                            });

                            if vol_changed {
                                app.state.save();
                                let v_local = if s.use_global_volume { app.state.settings.global_volume_playback } else { s.volume_playback };
                                let v_mic = if s.use_global_volume { app.state.settings.global_volume_out } else { s.volume_out };
                                app.audio.update_live_volume(s.id, v_local, v_mic);
                            }
                            ui.separator();

                            if ui.button("Settings").clicked() {
                                app.active_sound_popup = Some(s.id);
                                app.active_sound_popup_tab = 0;
                                ui.close_menu();
                            }

                            if ui.button("Edit").clicked() {
                                app.active_sound_popup = Some(s.id);
                                app.active_sound_popup_tab = 1;
                                ui.close_menu();
                            }
                        };

                        bg_resp.context_menu(|ui| menu_content(ui, self, sound));
                        play_resp.context_menu(|ui| menu_content(ui, self, sound));
                        info_resp.context_menu(|ui| menu_content(ui, self, sound));
                        channels_resp.context_menu(|ui| menu_content(ui, self, sound));

                        let ctrl = ctx.input(|i| i.modifiers.command || i.modifiers.ctrl);
                        let shift = ctx.input(|i| i.modifiers.shift);

                        let any_click = bg_resp.clicked()
                            || info_resp.clicked()
                            || channels_resp.clicked()
                            || (play_resp.clicked() && (ctrl || shift));

                        if any_click {
                            if shift {
                                if let Some(anchor) = self.last_selected_sound {
                                    let anchor_pos =
                                        all_ids.iter().position(|id| *id == anchor);
                                    let this_pos =
                                        all_ids.iter().position(|id| *id == sound.id);
                                    if let (Some(a), Some(b)) = (anchor_pos, this_pos) {
                                        let (lo, hi) =
                                            if a <= b { (a, b) } else { (b, a) };
                                        for id in &all_ids[lo..=hi] {
                                            self.selected_sounds.insert(*id);
                                        }
                                    }
                                } else {
                                    self.selected_sounds.clear();
                                    self.selected_sounds.insert(sound.id);
                                    self.last_selected_sound = Some(sound.id);
                                }
                            } else if ctrl {
                                if !self.selected_sounds.remove(&sound.id) {
                                    self.selected_sounds.insert(sound.id);
                                }
                                self.last_selected_sound = Some(sound.id);
                            } else {
                                self.selected_sounds.clear();
                                self.selected_sounds.insert(sound.id);
                                self.last_selected_sound = Some(sound.id);
                            }
                        }
                    });
                });
        });
    }

    fn truncate_name(name: &str, max_chars: usize) -> String {
        let count = name.chars().count();
        if count <= max_chars {
            name.to_string()
        } else {
            let mut truncated = name
                .chars()
                .take(max_chars.saturating_sub(3))
                .collect::<String>();
            truncated.push_str("...");
            truncated
        }
    }
}

impl eframe::App for SoundboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.hotkey_rx {
            while let Ok(crate::GlobalEvent::HotkeyTriggered(key_str)) = rx.try_recv() {
                let mut stopped_all = false;
                
                if let Some(hk) = &self.state.settings.stop_all_hotkey {
                    if hk == &key_str {
                        self.audio.stop_all();
                        stopped_all = true;
                    }
                }

                if stopped_all {
                    continue;
                }

                let sound_to_play = self.state.sounds.values().find(|s| {
                    s.hotkey.as_deref().map(|hk| {
                        hk == &key_str
                    }).unwrap_or(false)
                }).cloned();

                if let Some(sound) = sound_to_play {
                    let (v_local, v_mic) = self.get_effective_volume(&sound);
                    self.audio.play_sound(
                        sound.id,
                        &sound.name,
                        &sound.path,
                        v_local,
                        v_mic,
                        sound.headphones_enabled,
                        sound.mic_enabled,
                        &self.state.settings.default_output,
                        &self.state.settings.mic_sink,
                        self.state.settings.allow_overlap,
                    );
                }
            }
        }

        ctx.request_repaint();
        self.render_ui(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown();
    }
}
