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

    let modifiers: Arc<Mutex<(bool, bool, bool)>> = Arc::new(Mutex::new((false, false, false)));
    let modifiers_clone = modifiers.clone();

    std::thread::spawn(move || {
        if let Err(error) = rdev::listen(move |event| {
            match event.event_type {
                rdev::EventType::KeyPress(key) => {
                    {
                        let mut mods = modifiers_clone.lock().unwrap();
                        match key {
                            rdev::Key::ControlLeft | rdev::Key::ControlRight => mods.0 = true,
                            rdev::Key::ShiftLeft   | rdev::Key::ShiftRight   => mods.1 = true,
                            rdev::Key::Alt         | rdev::Key::AltGr        => mods.2 = true,
                            _ => {}
                        }
                    }

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
        "Nisound",
        options,
        Box::new(|_cc| {
            let mut app = SoundboardApp::new(state);
            app.hotkey_rx = Some(rx);
            Box::new(app)
        }),
    )
}

fn rdev_key_to_egui_name(key: &rdev::Key) -> String {
    match key {
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

        other => format!("{:?}", other),
    }
}
