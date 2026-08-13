use std::{fs, path::Path};

use nvml_wrapper::Nvml;

use super::{Sample, Vendor};

pub(super) struct Monitor {
    nvml: Option<Nvml>,
}

impl Monitor {
    pub(super) fn new() -> Result<Self, String> {
        Ok(Self {
            nvml: Nvml::init().ok(),
        })
    }

    pub(super) fn sample(&mut self) -> Result<Vec<Sample>, String> {
        let mut samples = self
            .nvml
            .as_ref()
            .and_then(|nvml| sample_nvml(nvml).ok())
            .unwrap_or_default();
        let has_nvidia = samples.iter().any(|sample| sample.vendor == Vendor::Nvidia);
        samples.extend(sample_drm(has_nvidia));
        (!samples.is_empty())
            .then_some(samples)
            .ok_or_else(|| "no supported Linux accelerator memory provider was found".to_owned())
    }
}

fn sample_nvml(nvml: &Nvml) -> Result<Vec<Sample>, String> {
    let count = nvml
        .device_count()
        .map_err(|error| format!("NVML device enumeration failed: {error}"))?;
    let mut samples = Vec::with_capacity(count as usize);
    let mut first_error = None;
    for index in 0..count {
        match sample_nvml_device(nvml, index) {
            Ok(sample) => samples.push(sample),
            Err(error) => {
                first_error.get_or_insert(error);
            }
        };
    }
    if samples.is_empty()
        && let Some(error) = first_error
    {
        return Err(error);
    }
    Ok(samples)
}

fn sample_nvml_device(nvml: &Nvml, index: u32) -> Result<Sample, String> {
    let device = nvml
        .device_by_index(index)
        .map_err(|error| format!("NVML device {index} lookup failed: {error}"))?;
    let name = device
        .name()
        .map_err(|error| format!("NVML device {index} name query failed: {error}"))?;
    let memory = device
        .memory_info()
        .map_err(|error| format!("NVML device {index} memory query failed: {error}"))?;
    Ok(Sample {
        id: index as usize,
        name,
        vendor: Vendor::Nvidia,
        budget_bytes: memory.total,
        used_bytes: memory.used,
        utilization_percent: device
            .utilization_rates()
            .ok()
            .map(|rates| rates.gpu as f32),
    })
}

fn sample_drm(skip_nvidia: bool) -> Vec<Sample> {
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut samples = entries
        .flatten()
        .filter_map(|entry| {
            let card = entry.file_name().to_string_lossy().into_owned();
            let id = card.strip_prefix("card")?.parse::<usize>().ok()?;
            if card.contains('-') {
                return None;
            }
            let device = entry.path().join("device");
            let vendor = read_hex(device.join("vendor")).map_or(Vendor::Unknown, vendor);
            if skip_nvidia && vendor == Vendor::Nvidia {
                return None;
            }
            let (total, used) = read_memory_pair(&device)?;
            let name = match vendor {
                Vendor::Nvidia => format!("NVIDIA {card}"),
                Vendor::Amd => format!("AMD {card}"),
                Vendor::Intel => format!("Intel {card}"),
                Vendor::Apple => format!("Apple {card}"),
                Vendor::Unknown => card,
            };
            Some(Sample {
                id,
                name,
                vendor,
                budget_bytes: total,
                used_bytes: used,
                utilization_percent: read_number(device.join("gpu_busy_percent"))
                    .map(|value| value as f32),
            })
        })
        .collect::<Vec<_>>();
    samples.sort_by_key(|sample| sample.id);
    samples
}

fn read_memory_pair(device: &Path) -> Option<(u64, u64)> {
    let pairs = [
        ("mem_info_vram_total", "mem_info_vram_used"),
        ("tile0/vram0/total_bytes", "tile0/vram0/used_bytes"),
    ];
    pairs.into_iter().find_map(|(total, used)| {
        Some((
            read_number(device.join(total))?,
            read_number(device.join(used))?,
        ))
    })
}

fn read_number(path: impl AsRef<Path>) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn read_hex(path: impl AsRef<Path>) -> Option<u32> {
    let value = fs::read_to_string(path).ok()?;
    u32::from_str_radix(value.trim().trim_start_matches("0x"), 16).ok()
}

fn vendor(id: u32) -> Vendor {
    match id {
        0x10de => Vendor::Nvidia,
        0x1002 | 0x1022 => Vendor::Amd,
        0x8086 => Vendor::Intel,
        _ => Vendor::Unknown,
    }
}
