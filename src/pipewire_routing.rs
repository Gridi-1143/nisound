use std::collections::BTreeMap;
use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct PwPort {
    pub id: u32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PwTargetNode {
    pub display_name: String,
    pub ports: Vec<PwPort>,
}

/// Перераховує активні вхідні потоки з унікальними числовими ID
pub fn list_active_targets() -> Vec<PwTargetNode> {
    let output = match Command::new("pw-link").args(["-i", "-I"]).output() {
        Ok(out) => out,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut grouped: BTreeMap<String, Vec<PwPort>> = BTreeMap::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let id_str = parts.next().unwrap_or_default();
        let full_name = parts.collect::<Vec<&str>>().join(" ");

        if let Ok(id) = id_str.parse::<u32>() {
            if let Some((node, port)) = full_name.split_once(':') {
                if node.starts_with("nisound") 
                    || node.starts_with("alsa_output") 
                    || node.contains("Volume Control") 
                {
                    continue;
                }

                grouped
                    .entry(node.to_string())
                    .or_default()
                    .push(PwPort {
                        id,
                        name: port.to_string(),
                    });
            }
        }
    }

    grouped
        .into_iter()
        .map(|(node_name, ports)| {
            let primary_id = ports.first().map(|p| p.id).unwrap_or(0);
            PwTargetNode {
                display_name: format!("{} (#{})", node_name, primary_id),
                ports,
            }
        })
        .collect()
}

/// З'єднує вихід Nisound_Mic безпосередньо з обраним вхідним RecordStream
pub fn link_stream_to_targets(targets: &[PwTargetNode]) {
    let output = match Command::new("pw-link").args(["-o", "-I"]).output() {
        Ok(out) => out,
        Err(_) => return,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut mic_out_ports: Vec<u32> = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        let mut parts = line.split_whitespace();
        let id_str = parts.next().unwrap_or_default();
        let full_name = parts.collect::<Vec<&str>>().join(" ");

        if let Ok(id) = id_str.parse::<u32>() {
            if full_name.starts_with("nisound_mic_sink:monitor") 
                || full_name.starts_with("Nisound_Mic:capture") 
            {
                mic_out_ports.push(id);
            }
        }
    }

    if mic_out_ports.is_empty() {
        return;
    }

    for target in targets {
        for (idx, target_port) in target.ports.iter().enumerate() {
            let src_id = mic_out_ports.get(idx).unwrap_or(&mic_out_ports[0]);
            let _ = Command::new("pw-link")
                .arg(src_id.to_string())
                .arg(target_port.id.to_string())
                .output();
        }
    }
}

/// Агресивно відключає всі зв'язки Nisound від вказаної цілі
pub fn unlink_target(target_display_name: &str) {
    let targets = list_active_targets();
    if let Some(target) = targets.iter().find(|t| t.display_name == target_display_name) {
        let output = match Command::new("pw-link").args(["-o", "-I"]).output() {
            Ok(out) => out,
            Err(_) => return,
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut mic_out_ports: Vec<u32> = Vec::new();

        for line in stdout.lines() {
            let line = line.trim();
            let mut parts = line.split_whitespace();
            let id_str = parts.next().unwrap_or_default();
            let full_name = parts.collect::<Vec<&str>>().join(" ");

            if let Ok(id) = id_str.parse::<u32>() {
                if full_name.starts_with("nisound_mic_sink:monitor") 
                    || full_name.starts_with("Nisound_Mic:capture") 
                    || full_name.starts_with("input.nisound_mic_source")
                {
                    mic_out_ports.push(id);
                }
            }
        }

        for target_port in &target.ports {
            for src_id in &mic_out_ports {
                // Виконуємо розрив з'єднання (-d = disconnect)
                let _ = Command::new("pw-link")
                    .arg("-d")
                    .arg(src_id.to_string())
                    .arg(target_port.id.to_string())
                    .output();
            }
        }
    }
}
