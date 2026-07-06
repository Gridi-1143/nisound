// pulse_devices.rs
// Enumerates PulseAudio/PipeWire sinks (outputs) and sources (inputs)
// with human-readable descriptions and Pavucontrol-style categories.

use libpulse_binding as pulse;
use pulse::context::{Context, FlagSet as ContextFlagSet};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::proplist::Proplist;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Hardware,
    Virtual,
    Monitor, // only relevant for inputs
}

#[derive(Clone, Debug)]
pub struct AudioDevice {
    /// Technical PulseAudio name, e.g. "alsa_output.pci-0000_00_1f.3.analog-stereo"
    pub name: String,
    /// Human-readable description, e.g. "Headphones — Realtek ALC1220 Analog Stereo"
    pub description: String,
    pub kind: DeviceKind,
    pub is_default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceFilter {
    All,
    HardwareOnly,
    AllExceptMonitors,
    VirtualOnly,
    MonitorsOnly,
}

impl DeviceFilter {
    pub fn label(&self) -> &'static str {
        match self {
            DeviceFilter::All => "All devices",
            DeviceFilter::HardwareOnly => "Only Hardware",
            DeviceFilter::AllExceptMonitors => "All, except monitors",
            DeviceFilter::VirtualOnly => "Virtual Only",
            DeviceFilter::MonitorsOnly => "Monitors only",
        }
    }

    pub fn matches(&self, dev: &AudioDevice) -> bool {
        match self {
            DeviceFilter::All => true,
            DeviceFilter::HardwareOnly => dev.kind == DeviceKind::Hardware,
            DeviceFilter::AllExceptMonitors => dev.kind != DeviceKind::Monitor,
            DeviceFilter::VirtualOnly => dev.kind == DeviceKind::Virtual,
            DeviceFilter::MonitorsOnly => dev.kind == DeviceKind::Monitor,
        }
    }

    pub const ALL_OUTPUT: [DeviceFilter; 3] = [
        DeviceFilter::All,
        DeviceFilter::HardwareOnly,
        DeviceFilter::VirtualOnly,
    ];

    pub const ALL_INPUT: [DeviceFilter; 5] = [
        DeviceFilter::All,
        DeviceFilter::HardwareOnly,
        DeviceFilter::AllExceptMonitors,
        DeviceFilter::VirtualOnly,
        DeviceFilter::MonitorsOnly,
    ];
}

fn classify_driver(driver: Option<&str>, is_monitor: bool) -> DeviceKind {
    if is_monitor {
        return DeviceKind::Monitor;
    }
    match driver {
        Some(d) if d.contains("alsa-card") || d.contains("alsa-sink") || d.contains("alsa-source") => {
            DeviceKind::Hardware
        }
        _ => DeviceKind::Virtual,
    }
}

/// Blocking helper: connects to the PulseAudio/PipeWire-pulse server, lists
/// sinks (outputs) and sources (inputs), then disconnects.
/// Returns (outputs, inputs).
pub fn list_devices() -> (Vec<AudioDevice>, Vec<AudioDevice>) {
    let outputs: Rc<RefCell<Vec<AudioDevice>>> = Rc::new(RefCell::new(Vec::new()));
    let inputs: Rc<RefCell<Vec<AudioDevice>>> = Rc::new(RefCell::new(Vec::new()));

    let mut proplist = Proplist::new().unwrap();
    proplist
        .set_str(pulse::proplist::properties::APPLICATION_NAME, "Nisound")
        .ok();

    let mut mainloop = Mainloop::new().expect("failed to create pulse mainloop");
    let context = Rc::new(RefCell::new(
        Context::new_with_proplist(&mainloop, "NisoundDeviceList", &proplist)
            .expect("failed to create pulse context"),
    ));

    context
        .borrow_mut()
        .connect(None, ContextFlagSet::NOFLAGS, None)
        .expect("failed to connect to pulse server");

    // Wait for context to be ready.
    loop {
        match mainloop.iterate(true) {
            IterateResult::Quit(_) | IterateResult::Err(_) => {
                panic!("pulse mainloop error while connecting");
            }
            IterateResult::Success(_) => {}
        }
        match context.borrow().get_state() {
            pulse::context::State::Ready => break,
            pulse::context::State::Failed | pulse::context::State::Terminated => {
                panic!("pulse context failed/terminated");
            }
            _ => {}
        }
    }

    let done_sinks = Rc::new(RefCell::new(false));
    let outputs_for_cb = outputs.clone();
    let done_sinks_for_cb = done_sinks.clone();
    let op = context.borrow().introspect().get_sink_info_list(move |res| {
        if let pulse::callbacks::ListResult::Item(info) = res {
            let name = info.name.as_ref().map(|c| c.to_string()).unwrap_or_default();
            let description = info
                .description
                .as_ref()
                .map(|c| c.to_string())
                .unwrap_or_else(|| name.clone());
            let driver = info.driver.as_ref().map(|c| c.to_string());
            outputs_for_cb.borrow_mut().push(AudioDevice {
                name,
                description,
                kind: classify_driver(driver.as_deref(), false),
                is_default: false,
            });
        } else if matches!(res, pulse::callbacks::ListResult::End) {
            *done_sinks_for_cb.borrow_mut() = true;
        }
    });
    while !*done_sinks.borrow() {
        mainloop.iterate(true);
    }
    drop(op);

    let done_sources = Rc::new(RefCell::new(false));
    let inputs_for_cb = inputs.clone();
    let done_sources_for_cb = done_sources.clone();
    let op = context
        .borrow()
        .introspect()
        .get_source_info_list(move |res| {
            if let pulse::callbacks::ListResult::Item(info) = res {
                let name = info.name.as_ref().map(|c| c.to_string()).unwrap_or_default();
                let description = info
                    .description
                    .as_ref()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| name.clone());
                let driver = info.driver.as_ref().map(|c| c.to_string());
                let is_monitor = info.monitor_of_sink.is_some();
                inputs_for_cb.borrow_mut().push(AudioDevice {
                    name,
                    description,
                    kind: classify_driver(driver.as_deref(), is_monitor),
                    is_default: false,
                });
            } else if matches!(res, pulse::callbacks::ListResult::End) {
                *done_sources_for_cb.borrow_mut() = true;
            }
        });
    while !*done_sources.borrow() {
        mainloop.iterate(true);
    }
    drop(op);

    context.borrow_mut().disconnect();

    (outputs.take(), inputs.take())
}
