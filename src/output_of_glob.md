### (1) FILE: ui.rs
**SIZE:** 68K
**PATH:** /home/gridi/projects/nisound/src/ui.rs

```
use crate::audio::AudioEngine;
use crate::config::{AppState, Folder, SoundEntry};
use egui::{Color32, Context, Ui};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

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

// Стан прослуховування гарячої клавіші
#[derive(Clone, Debug)]
struct KeybindCapture {
    sound_id: Uuid,
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
    available_devices: Vec<String>,
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
        Self {
            state,
            audio: AudioEngine::init(),
            available_devices: AudioEngine::available_output_devices(),
            hotkey_rx: None,
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

    pub fn render_ui(&mut self, ctx: &Context) {
        self.audio.clean_dead_sinks();

        // ── TOP TOOLBAR ──────────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("➕ Add Sound").clicked() {
                    if let Some(paths) = rfd::FileDialog::new()
                        .set_title("Select sound files")
                        .add_filter("Audio", &["mp3", "wav", "ogg", "flac", "m4a", "aac"])
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
                ui.label("Out (Headphones):");
                let mut out_changed = false;
                egui::ComboBox::from_id_source("def_out")
                    .selected_text(&self.state.settings.default_output)
                    .show_ui(ui, |ui| {
                        for dev in &self.available_devices {
                            out_changed |= ui
                                .selectable_value(
                                    &mut self.state.settings.default_output,
                                    dev.clone(),
                                    dev,
                                )
                                .changed();
                        }
                    });
                if out_changed {
                    self.state.save();
                }

                ui.label("Out (Virtual Mic):");
                let mut in_changed = false;
                egui::ComboBox::from_id_source("def_in")
                    .selected_text(&self.state.settings.default_input)
                    .show_ui(ui, |ui| {
                        for dev in &self.available_devices {
                            in_changed |= ui
                                .selectable_value(
                                    &mut self.state.settings.default_input,
                                    dev.clone(),
                                    dev,
                                )
                                .changed();
                        }
                    });
                if in_changed {
                    self.state.save();
                }

                ui.separator();
                if ui.button("🎙 Record (Stub)").clicked() {}

                if ui.button("⚙ Settings").clicked() {
                    self.show_settings = !self.show_settings;
                }
            });
        });

        // ── RIGHT PANEL: Categories + Playback ──────────────────────────────
        egui::SidePanel::right("folders")
            .width_range(150.0..=250.0)
            .show(ctx, |ui| {
                // Отримуємо список активних звуків для Playback-панелі
                let active_snapshot: Vec<(Uuid, String)> = self
                    .audio
                    .active_sounds
                    .lock()
                    .map(|a| a.iter().map(|s| (s.play_id, s.name.clone())).collect())
                    .unwrap_or_default();

                // Playback секція — гнучка висота, але не більше половини екрану
                let total_height = ui.available_height();
                let playback_count = active_snapshot.len() as f32;
                // Висота: заголовок + рядки по ~24px + відступи
                let desired_playback_h = 32.0 + playback_count * 24.0 + 16.0;
                let max_playback_h = (total_height * 0.5).max(80.0);
                let playback_h = desired_playback_h.clamp(60.0, max_playback_h);

                // Малюємо Playback знизу
                egui::TopBottomPanel::bottom("playback_panel")
                    .resizable(true)
                    .min_height(48.0)
                    .max_height(max_playback_h)
                    .default_height(playback_h)
                    .show_inside(ui, |ui| {
                        ui.add_space(4.0);
                        ui.heading("▶ Playback");
                        ui.separator();

                        if active_snapshot.is_empty() {
                            ui.weak("Nothing playing");
                        } else {
                            let mut to_stop: Option<Uuid> = None;

                            egui::ScrollArea::vertical().show(ui, |ui| {
                                for (play_id, name) in &active_snapshot {
                                    ui.horizontal(|ui| {
                                        // Кнопка зупинки
                                        if ui
                                            .add_sized(
                                                [18.0, 18.0],
                                                egui::Button::new("✕")
                                                    .fill(Color32::from_rgb(160, 60, 60)),
                                            )
                                            .clicked()
                                        {
                                            to_stop = Some(*play_id);
                                        }

                                        let label = Self::truncate_name(name, 22);
                                        ui.label(label);
                                    });
                                }
                            });

                            if let Some(pid) = to_stop {
                                self.audio.stop_sound(pid);
                            }
                        }
                    });

                // Categories секція — решта простору
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.heading("categories");
                        ui.add_space(8.0);

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
                });
            });

        // ── CENTRAL PANEL: Sound Grid ────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let sound_ids_to_render: Vec<Uuid> = {
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
                            ui.label("Advanced Routing Configurations System");
                            ui.label(format!(
                                "Current output route: {}",
                                self.state.settings.default_output
                            ));
                            ui.label(format!(
                                "Current input route: {}",
                                self.state.settings.default_input
                            ));
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

    // ── SOUND SETTINGS POPUP ─────────────────────────────────────────────────
    fn render_sound_popup(&mut self, ctx: &Context) {
        let Some(sound_id) = self.active_sound_popup else {
            return;
        };

        let Some(sound_name) = self.state.sounds.get(&sound_id).map(|s| s.name.clone()) else {
            self.active_sound_popup = None;
            return;
        };

        let mut open = true;

        // Клонуємо щоб уникнути borrow conflict під час рендерингу
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
                            // ── Settings tab ──
                            egui::Grid::new("sound_settings_grid")
                                .num_columns(2)
                                .spacing([12.0, 8.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    // Шлях до файлу
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
                                    .on_hover_text(&path_str); // повний шлях у tooltip
                                    ui.end_row();

                                    // Статус файлу
                                    ui.strong("Status:");
                                    if sound.exists {
                                        ui.colored_label(Color32::from_rgb(100, 200, 100), "✔ File found");
                                    } else {
                                        ui.colored_label(Color32::from_rgb(220, 80, 80), "✘ File missing");
                                    }
                                    ui.end_row();

                                    // Гарячa клавіша
                                    ui.strong("Hotkey:");
                                    ui.horizontal(|ui| {
                                        let is_capturing = self.keybind_capture.as_ref()
                                            .map(|k| k.sound_id == sound_id)
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

                                            // Читаємо модифікатори і клавішу
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
                                                None => "＋ Assign hotkey".to_string(),
                                            };
                                            if ui.button(&btn_text).clicked() {
                                                let mods = ctx.input(|i| i.modifiers);
                                                self.keybind_capture = Some(KeybindCapture {
                                                    sound_id,
                                                    ctrl:  mods.command || mods.ctrl,
                                                    alt:   mods.alt,
                                                    shift: mods.shift,
                                                });
                                            }
                                            if sound.hotkey.is_some() {
                                                if ui.small_button("✕ Clear").clicked() {
                                                    sound.hotkey = None;
                                                    changed = true;
                                                }
                                            }
                                        }
                                    });
                                    ui.end_row();

                                    // Гучність (Headphones)
                                    ui.strong("Volume (HP):");
                                    changed |= ui
                                        .add(
                                            egui::Slider::new(&mut sound.volume_playback, 0.0..=1.0)
                                                .text("headphones"),
                                        )
                                        .changed();
                                    ui.end_row();

                                    // Гучність (Mic)
                                    ui.strong("Volume (Mic):");
                                    changed |= ui
                                        .add(
                                            egui::Slider::new(&mut sound.volume_out, 0.0..=1.0)
                                                .text("virtual mic"),
                                        )
                                        .changed();
                                    ui.end_row();

                                    // Увімкнення каналів
                                    ui.strong("Headphones:");
                                    changed |= ui.checkbox(&mut sound.headphones_enabled, "enabled").changed();
                                    ui.end_row();

                                    ui.strong("Mic:");
                                    changed |= ui.checkbox(&mut sound.mic_enabled, "enabled").changed();
                                    ui.end_row();

                                    // Custom channels (інформаційно)
                                    ui.strong("Channel routing:");
                                    match &sound.custom_channels {
                                        Some(ch) => {
                                            ui.vertical(|ui| {
                                                ui.label(format!("In: {}", ch.input_device));
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
                            // ── Edit tab ──
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

            // Зберігаємо назад якщо щось змінилось
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

            let is_duplicate = self.state.sounds.values().any(|s| s.path == path);
            if is_duplicate {
                skipped += 1;
                continue;
            }

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

        if skipped > 0 {
            self.error_message = Some(format!(
                "Skipped {} sound(s) that were already added.",
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
            mic_enabled: true,
            headphones_enabled: true,
            custom_channels: None,
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
                                                    self.audio.play_sound(
                                                        sound.id,
                                                        &sound.name,
                                                        &sound.path,
                                                        sound.volume_playback,
                                                        sound.volume_out,
                                                        sound.headphones_enabled,
                                                        sound.mic_enabled,
                                                        &self.state.settings.default_output,
                                                        &self.state.settings.default_input,
                                                    );
                                                }
                                            }
                                        },
                                    );
                                });
                                resp
                            })
                            .inner;

                        play_resp.context_menu(|ui| {
                            ui.label("Playback Vol (Headphones):");
                            if ui
                                .add(egui::Slider::new(&mut sound.volume_playback, 0.0..=1.0))
                                .changed()
                            {
                                self.state.save();
                            }
                            ui.label("Output Vol (Virtual Mic):");
                            if ui
                                .add(egui::Slider::new(&mut sound.volume_out, 0.0..=1.0))
                                .changed()
                            {
                                self.state.save();
                            }
                        });

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

                                                // Гарячa клавіша або кнопка "+"
                                                ui.horizontal(|ui| {
                                                    ui.label("⏱ --:--");

                                                    let is_capturing = self
                                                        .keybind_capture
                                                        .as_ref()
                                                        .map(|k| k.sound_id == sound.id)
                                                        .unwrap_or(false);

                                                    if is_capturing {
                                                        // Показуємо поточні модифікатори
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

                                                        // Обробляємо клавішу
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
                                                        // Кнопка призначення гарячої клавіші
                                                        let btn_text = match &sound.hotkey {
                                                            Some(hk) => format!("⌨ {}", hk),
                                                            None => "＋".to_string(),
                                                        };

                                                        let btn_resp = ui.small_button(&btn_text);
                                                        if btn_resp.clicked() {
                                                            let mods =
                                                                ctx.input(|i| i.modifiers);
                                                            self.keybind_capture =
                                                                Some(KeybindCapture {
                                                                    sound_id: sound.id,
                                                                    ctrl: mods.command
                                                                        || mods.ctrl,
                                                                    alt: mods.alt,
                                                                    shift: mods.shift,
                                                                });
                                                        }
                                                        btn_resp.on_hover_text(
                                                            "Click to assign hotkey",
                                                        );
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

                        info_resp.context_menu(|ui| {
                            ui.label("Shortcut Configuration");
                            if ui.button("Assign Global Hotkey").clicked() {
                                sound.hotkey = Some("F12".to_string());
                                self.state.save();
                                ui.close_menu();
                            }
                            ui.separator();
                            if self.state.active_folder.is_some() {
                                if ui.button("Remove from folder").clicked() {
                                    if self.selected_sounds.contains(&sound.id) {
                                        self.action_remove_sounds_from_folder =
                                            self.selected_sounds.iter().cloned().collect();
                                    } else {
                                        self.action_remove_sounds_from_folder = vec![sound.id];
                                    }
                                    ui.close_menu();
                                }
                            }
                            if ui.button("Delete (from all sounds)").clicked() {
                                let ids = if self.selected_sounds.contains(&sound.id) {
                                    self.selected_sounds.iter().cloned().collect()
                                } else {
                                    vec![sound.id]
                                };
                                self.confirm_action = Some(PendingAction::DeleteSound(ids));
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Settings").clicked() {
                                self.active_sound_popup = Some(sound.id);
                                self.active_sound_popup_tab = 0;
                                ui.close_menu();
                            }
                            if ui.button("Edit").clicked() {
                                self.active_sound_popup = Some(sound.id);
                                self.active_sound_popup_tab = 1;
                                ui.close_menu();
                            }
                        });

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
                                                    self.audio.play_sound(
                                                        sound.id,
                                                        &sound.name,
                                                        &sound.path,
                                                        sound.volume_playback,
                                                        sound.volume_out,
                                                        false,
                                                        true,
                                                        &self.state.settings.default_output,
                                                        &self.state.settings.default_input,
                                                    );
                                                }
                                            }

                                            if ui
                                                .add_sized(
                                                    [56.0, 20.0],
                                                    egui::Button::new("🎧 HP"),
                                                )
                                                .clicked()
                                            {
                                                let ctrl = ctx.input(|i| {
                                                    i.modifiers.command || i.modifiers.ctrl
                                                });
                                                let shift = ctx.input(|i| i.modifiers.shift);
                                                if !ctrl && !shift {
                                                    self.audio.play_sound(
                                                        sound.id,
                                                        &sound.name,
                                                        &sound.path,
                                                        sound.volume_playback,
                                                        sound.volume_out,
                                                        true,
                                                        false,
                                                        &self.state.settings.default_output,
                                                        &self.state.settings.default_input,
                                                    );
                                                }
                                            }
                                        },
                                    );
                                });
                                resp
                            })
                            .inner;

                        channels_resp.context_menu(|ui| {
                            ui.label("Channel routing");
                            ui.separator();
                            ui.label("Output channel:");
                            if ui.button("Default").clicked() {
                                sound.custom_channels = None;
                                self.state.save();
                                ui.close_menu();
                            }
                            ui.separator();
                            ui.label("Input channel:");
                            if ui.button("Default").clicked() {
                                sound.custom_channels = None;
                                self.state.save();
                                ui.close_menu();
                            }
                            ui.label("Custom routing is not implemented yet.");
                        });

                        // ── SELECTION LOGIC ───────────────────────────────────
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
        // Обробка глобальних гарячих клавіш з rdev
        if let Some(rx) = &self.hotkey_rx {
            while let Ok(crate::GlobalEvent::HotkeyTriggered(key_str)) = rx.try_recv() {
                // Шукаємо звук з відповідним хоткеєм
                // key_str прийде у форматі "F5", "KeyA" і т.д. — порівнюємо з хвостом combo
                let sound_to_play = self.state.sounds.values().find(|s| {
                    s.hotkey.as_deref().map(|hk| {
                        // Хоткей може бути "Ctrl+F5" або просто "F5"
                        // rdev повертає тільки клавішу без модифікаторів у HotkeyTriggered
                        // Тому для simple keys (без модифікаторів) порівнюємо напряму
                        hk == &key_str || hk.ends_with(&format!("+{}", key_str))
                    }).unwrap_or(false)
                }).cloned();

                if let Some(sound) = sound_to_play {
                    self.audio.play_sound(
                        sound.id,
                        &sound.name,
                        &sound.path,
                        sound.volume_playback,
                        sound.volume_out,
                        sound.headphones_enabled,
                        sound.mic_enabled,
                        &self.state.settings.default_output,
                        &self.state.settings.default_input,
                    );
                }
            }
        }

        ctx.request_repaint();
        self.render_ui(ctx);
    }
}
```

### (2) FILE: config.rs
**SIZE:** 4.0K
**PATH:** /home/gridi/projects/nisound/src/config.rs

```
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
```

### (3) FILE: audio.rs
**SIZE:** 4.0K
**PATH:** /home/gridi/projects/nisound/src/audio.rs

```
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
```

### (4) FILE: main.rs
**SIZE:** 8.0K
**PATH:** /home/gridi/projects/nisound/src/main.rs

```
mod config;
mod audio;
mod ui;

use eframe::egui;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use config::AppState;
use ui::SoundboardApp;

pub enum GlobalEvent {
    HotkeyTriggered(String),
}

fn main() -> eframe::Result<()> {
    let state = AppState::load_or_create();
    let (tx, rx): (Sender<GlobalEvent>, Receiver<GlobalEvent>) = std::sync::mpsc::channel();

    // Відстежуємо натиснуті модифікатори між подіями rdev
    let modifiers: Arc<Mutex<(bool, bool, bool)>> = Arc::new(Mutex::new((false, false, false)));
    let modifiers_clone = modifiers.clone();

    std::thread::spawn(move || {
        if let Err(error) = rdev::listen(move |event| {
            match event.event_type {
                rdev::EventType::KeyPress(key) => {
                    // Оновлюємо стан модифікаторів
                    {
                        let mut mods = modifiers_clone.lock().unwrap();
                        match key {
                            rdev::Key::ControlLeft | rdev::Key::ControlRight => mods.0 = true,
                            rdev::Key::ShiftLeft   | rdev::Key::ShiftRight   => mods.1 = true,
                            rdev::Key::Alt         | rdev::Key::AltGr        => mods.2 = true,
                            _ => {}
                        }
                    }

                    // Не відправляємо самі модифікатори як хоткей
                    let is_modifier = matches!(
                        key,
                        rdev::Key::ControlLeft | rdev::Key::ControlRight
                        | rdev::Key::ShiftLeft | rdev::Key::ShiftRight
                        | rdev::Key::Alt | rdev::Key::AltGr
                        | rdev::Key::MetaLeft | rdev::Key::MetaRight
                    );

                    if !is_modifier {
                        let mods = modifiers_clone.lock().unwrap();
                        let (ctrl, shift, alt) = *mods;

                        // Формуємо рядок комбінації так само як в egui-capture
                        let key_str = rdev_key_to_egui_name(&key);
                        let mut combo = String::new();
                        if ctrl  { combo.push_str("Ctrl+"); }
                        if alt   { combo.push_str("Alt+"); }
                        if shift { combo.push_str("Shift+"); }
                        combo.push_str(&key_str);

                        let _ = tx.send(GlobalEvent::HotkeyTriggered(combo));
                    }
                }
                rdev::EventType::KeyRelease(key) => {
                    let mut mods = modifiers_clone.lock().unwrap();
                    match key {
                        rdev::Key::ControlLeft | rdev::Key::ControlRight => mods.0 = false,
                        rdev::Key::ShiftLeft   | rdev::Key::ShiftRight   => mods.1 = false,
                        rdev::Key::Alt         | rdev::Key::AltGr        => mods.2 = false,
                        _ => {}
                    }
                }
                _ => {}
            }
        }) {
            eprintln!("Error starting rdev global listener: {:?}", error);
        }
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Nisound")
            .with_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "nisound",
        options,
        Box::new(|_cc| {
            let mut app = SoundboardApp::new(state);
            app.hotkey_rx = Some(rx);
            Box::new(app)
        }),
    )
}

/// Перетворює rdev::Key у рядок, аналогічний до того, що дає egui format!("{:?}", key)
/// щоб хоткеї призначені через egui збігались з тим що приходить від rdev
fn rdev_key_to_egui_name(key: &rdev::Key) -> String {
    match key {
        // Функціональні
        rdev::Key::F1  => "F1".into(),
        rdev::Key::F2  => "F2".into(),
        rdev::Key::F3  => "F3".into(),
        rdev::Key::F4  => "F4".into(),
        rdev::Key::F5  => "F5".into(),
        rdev::Key::F6  => "F6".into(),
        rdev::Key::F7  => "F7".into(),
        rdev::Key::F8  => "F8".into(),
        rdev::Key::F9  => "F9".into(),
        rdev::Key::F10 => "F10".into(),
        rdev::Key::F11 => "F11".into(),
        rdev::Key::F12 => "F12".into(),

        // Цифри
        rdev::Key::Num0 => "Num0".into(),
        rdev::Key::Num1 => "Num1".into(),
        rdev::Key::Num2 => "Num2".into(),
        rdev::Key::Num3 => "Num3".into(),
        rdev::Key::Num4 => "Num4".into(),
        rdev::Key::Num5 => "Num5".into(),
        rdev::Key::Num6 => "Num6".into(),
        rdev::Key::Num7 => "Num7".into(),
        rdev::Key::Num8 => "Num8".into(),
        rdev::Key::Num9 => "Num9".into(),

        // Букви — egui використовує "A", "B" і т.д.
        rdev::Key::KeyA => "A".into(),
        rdev::Key::KeyB => "B".into(),
        rdev::Key::KeyC => "C".into(),
        rdev::Key::KeyD => "D".into(),
        rdev::Key::KeyE => "E".into(),
        rdev::Key::KeyF => "F".into(),
        rdev::Key::KeyG => "G".into(),
        rdev::Key::KeyH => "H".into(),
        rdev::Key::KeyI => "I".into(),
        rdev::Key::KeyJ => "J".into(),
        rdev::Key::KeyK => "K".into(),
        rdev::Key::KeyL => "L".into(),
        rdev::Key::KeyM => "M".into(),
        rdev::Key::KeyN => "N".into(),
        rdev::Key::KeyO => "O".into(),
        rdev::Key::KeyP => "P".into(),
        rdev::Key::KeyQ => "Q".into(),
        rdev::Key::KeyR => "R".into(),
        rdev::Key::KeyS => "S".into(),
        rdev::Key::KeyT => "T".into(),
        rdev::Key::KeyU => "U".into(),
        rdev::Key::KeyV => "V".into(),
        rdev::Key::KeyW => "W".into(),
        rdev::Key::KeyX => "X".into(),
        rdev::Key::KeyY => "Y".into(),
        rdev::Key::KeyZ => "Z".into(),

        // Спеціальні
        rdev::Key::Escape    => "Escape".into(),
        rdev::Key::Space     => "Space".into(),
        rdev::Key::Return    => "Enter".into(),
        rdev::Key::Tab       => "Tab".into(),
        rdev::Key::Backspace => "Backspace".into(),
        rdev::Key::Delete    => "Delete".into(),
        rdev::Key::Insert    => "Insert".into(),
        rdev::Key::Home      => "Home".into(),
        rdev::Key::End       => "End".into(),
        rdev::Key::PageUp    => "PageUp".into(),
        rdev::Key::PageDown  => "PageDown".into(),
        rdev::Key::UpArrow   => "ArrowUp".into(),
        rdev::Key::DownArrow => "ArrowDown".into(),
        rdev::Key::LeftArrow => "ArrowLeft".into(),
        rdev::Key::RightArrow => "ArrowRight".into(),

        // Решта — fallback через Debug
        other => format!("{:?}", other),
    }
}
```


