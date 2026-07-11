// pulse_devices.rs
// Enumerates PulseAudio/PipeWire sinks (outputs) and sources (inputs) with
// human-readable descriptions and Pavucontrol-style categories, and manages
// the Nisound virtual "mic channel" (a null-sink + a non-monitor remap
// source pointed at that sink's monitor, so apps like Discord can actually
// select it — Discord filters out raw ".monitor" sources).

use libpulse_binding as pulse;
use pulse::context::{Context, FlagSet as ContextFlagSet};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::proplist::Proplist;
use std::cell::RefCell;
use std::rc::Rc;

/// Name of the virtual sink Nisound plays "mic channel" sounds into.
pub const MIC_SINK_NAME: &str = "nisound_mic_sink";
/// Name of the non-monitor source (visible to Discord/Zoom/OBS) that
/// mirrors `MIC_SINK_NAME`'s monitor.
pub const MIC_SOURCE_NAME: &str = "nisound_mic_source";

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

/// Opens a blocking connection to the Pulse/PipeWire-pulse server and waits
/// until it's ready. Returns None on any failure.
fn connect_blocking(app_name: &str) -> Option<(Mainloop, RefCell<Context>)> {
    let mut proplist = Proplist::new()?;
    proplist
        .set_str(pulse::proplist::properties::APPLICATION_NAME, app_name)
        .ok();

    let mut mainloop = Mainloop::new()?;
    let context = RefCell::new(Context::new_with_proplist(&mainloop, app_name, &proplist)?);

    context
        .borrow_mut()
        .connect(None, ContextFlagSet::NOFLAGS, None)
        .ok()?;

    loop {
        match mainloop.iterate(true) {
            IterateResult::Quit(_) | IterateResult::Err(_) => return None,
            IterateResult::Success(_) => {}
        }
        match context.borrow().get_state() {
            pulse::context::State::Ready => break,
            pulse::context::State::Failed | pulse::context::State::Terminated => return None,
            _ => {}
        }
    }

    Some((mainloop, context))
}

/// Blocking helper: connects to the PulseAudio/PipeWire-pulse server, lists
/// sinks (outputs) and sources (inputs), then disconnects.
/// Returns (outputs, inputs).
pub fn list_devices() -> (Vec<AudioDevice>, Vec<AudioDevice>) {
    let outputs: Rc<RefCell<Vec<AudioDevice>>> = Rc::new(RefCell::new(Vec::new()));
    let inputs: Rc<RefCell<Vec<AudioDevice>>> = Rc::new(RefCell::new(Vec::new()));

    let Some((mut mainloop, context)) = connect_blocking("NisoundDeviceList") else {
        return (Vec::new(), Vec::new());
    };

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

/// Finds a loaded module whose name is `module_name` and whose argument
/// string contains `arg_substring`. Used to make module loading idempotent
/// (don't load the same virtual sink/source/loopback twice).
fn find_module(
    mainloop: &mut Mainloop,
    context: &RefCell<Context>,
    module_name: &str,
    arg_substring: &str,
) -> Option<u32> {
    let found: Rc<RefCell<Option<u32>>> = Rc::new(RefCell::new(None));
    let done: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    let found_cb = found.clone();
    let done_cb = done.clone();
    let module_name = module_name.to_string();
    let arg_substring = arg_substring.to_string();

    let op = context.borrow().introspect().get_module_info_list(move |res| {
        if let pulse::callbacks::ListResult::Item(info) = res {
            let name = info.name.as_ref().map(|c| c.to_string()).unwrap_or_default();
            let argument = info.argument.as_ref().map(|c| c.to_string()).unwrap_or_default();
            if name == module_name && argument.contains(&arg_substring) {
                *found_cb.borrow_mut() = Some(info.index);
            }
        } else if matches!(res, pulse::callbacks::ListResult::End) {
            *done_cb.borrow_mut() = true;
        }
    });
    while !*done.borrow() {
        mainloop.iterate(true);
    }
    drop(op);

    found.take()
}

fn load_module_blocking(
    mainloop: &mut Mainloop,
    context: &RefCell<Context>,
    module_name: &str,
    argument: &str,
) -> bool {
    let done: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let ok: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let done_cb = done.clone();
    let ok_cb = ok.clone();

    let op = {
        let mut introspector = context.borrow().introspect();
        introspector.load_module(module_name, argument, move |index| {
            *ok_cb.borrow_mut() = index != u32::MAX;
            *done_cb.borrow_mut() = true;
        })
    };
    while !*done.borrow() {
        mainloop.iterate(true);
    }
    drop(op);

    ok.take()
}

fn unload_module_blocking(mainloop: &mut Mainloop, context: &RefCell<Context>, index: u32) {
    let done: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let done_cb = done.clone();
    let op = {
        let mut introspector = context.borrow().introspect();
        introspector.unload_module(index, move |_success| {
            *done_cb.borrow_mut() = true;
        })
    };
    while !*done.borrow() {
        mainloop.iterate(true);
    }
    drop(op);
}

/// Whether the Nisound mic sink+source pair currently exists, and whether
/// a mic loopback (any source into MIC_SINK_NAME) is currently active.
pub fn mic_channel_status() -> (bool, bool) {
    let Some((mut mainloop, context)) = connect_blocking("NisoundStatus") else {
        return (false, false);
    };

    let sink_exists = find_module(
        &mut mainloop,
        &context,
        "module-null-sink",
        &format!("sink_name={}", MIC_SINK_NAME),
    )
    .is_some();

    let loopback_active = find_module(
        &mut mainloop,
        &context,
        "module-loopback",
        &format!("sink={}", MIC_SINK_NAME),
    )
    .is_some();

    context.borrow_mut().disconnect();
    (sink_exists, loopback_active)
}

/// Creates the Nisound virtual sink and its non-monitor mirror source, if
/// they don't already exist. Returns true on success (or if already present).
pub fn ensure_mic_channel() -> bool {
    let Some((mut mainloop, context)) = connect_blocking("NisoundSetup") else {
        return false;
    };

    let sink_arg = format!(
        "sink_name={} sink_properties=device.description=Nisound_Mic_Sink",
        MIC_SINK_NAME
    );
    let sink_ok = find_module(&mut mainloop, &context, "module-null-sink", &format!("sink_name={}", MIC_SINK_NAME))
        .is_some()
        || load_module_blocking(&mut mainloop, &context, "module-null-sink", &sink_arg);

    let source_arg = format!(
        "source_name={} master={}.monitor source_properties=device.description=Nisound_Mic",
        MIC_SOURCE_NAME, MIC_SINK_NAME
    );
    let source_ok = find_module(
        &mut mainloop,
        &context,
        "module-remap-source",
        &format!("source_name={}", MIC_SOURCE_NAME),
    )
    .is_some()
        || load_module_blocking(&mut mainloop, &context, "module-remap-source", &source_arg);

    context.borrow_mut().disconnect();
    sink_ok && source_ok
}

/// Unloads the Nisound virtual sink, its mirror source, and any loopback
/// feeding it.
pub fn remove_mic_channel() {
    let Some((mut mainloop, context)) = connect_blocking("NisoundTeardown") else {
        return;
    };

    if let Some(idx) = find_module(
        &mut mainloop,
        &context,
        "module-loopback",
        &format!("sink={}", MIC_SINK_NAME),
    ) {
        unload_module_blocking(&mut mainloop, &context, idx);
    }
    if let Some(idx) = find_module(
        &mut mainloop,
        &context,
        "module-remap-source",
        &format!("source_name={}", MIC_SOURCE_NAME),
    ) {
        unload_module_blocking(&mut mainloop, &context, idx);
    }
    if let Some(idx) = find_module(
        &mut mainloop,
        &context,
        "module-null-sink",
        &format!("sink_name={}", MIC_SINK_NAME),
    ) {
        unload_module_blocking(&mut mainloop, &context, idx);
    }

    context.borrow_mut().disconnect();
}

/// Enables or disables looping `source_name` (a real mic, e.g. "Starship")
/// into `MIC_SINK_NAME`, so the user's voice and the soundboard end up
/// mixed for anyone listening on `MIC_SOURCE_NAME`. Idempotent: if a
/// loopback from a different source is active, it's replaced.
pub fn set_mic_loopback(enabled: bool, source_name: &str) -> bool {
    let Some((mut mainloop, context)) = connect_blocking("NisoundLoopback") else {
        return false;
    };

    let existing = find_module(
        &mut mainloop,
        &context,
        "module-loopback",
        &format!("sink={}", MIC_SINK_NAME),
    );

    let mut ok = true;

    if let Some(idx) = existing {
        unload_module_blocking(&mut mainloop, &context, idx);
    }

    if enabled && !source_name.is_empty() && source_name != "Default" {
        let arg = format!("source={} sink={}", source_name, MIC_SINK_NAME);
        ok = load_module_blocking(&mut mainloop, &context, "module-loopback", &arg);
    }

    context.borrow_mut().disconnect();
    ok
}
