use nvml_wrapper::Nvml;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
    DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, IDXGIAdapter1,
    IDXGIAdapter3, IDXGIFactory6,
};
use windows::core::Interface as _;

use super::{Sample, Vendor};

pub(super) struct Monitor {
    factory: IDXGIFactory6,
    nvml: Option<Nvml>,
}

impl Monitor {
    pub(super) fn new() -> Result<Self, String> {
        let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory6>() }
            .map_err(|error| format!("failed to create DXGI factory: {error}"))?;
        Ok(Self {
            factory,
            nvml: Nvml::init().ok(),
        })
    }

    pub(super) fn sample(&mut self) -> Result<Vec<Sample>, String> {
        // DXGI exposes the OS-assigned budget and this process's usage, which
        // better describes current pressure than physical capacity alone.
        let mut samples = Vec::new();
        for index in 0.. {
            let adapter = match unsafe {
                self.factory.EnumAdapterByGpuPreference::<IDXGIAdapter1>(
                    index,
                    DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
                )
            } {
                Ok(adapter) => adapter,
                Err(_) => break,
            };
            let description = unsafe { adapter.GetDesc1() }
                .map_err(|error| format!("failed to inspect DXGI adapter {index}: {error}"))?;
            if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let adapter3: IDXGIAdapter3 = adapter.cast().map_err(|error| {
                format!("DXGI adapter {index} has no memory budget API: {error}")
            })?;
            let segment = if description.DedicatedVideoMemory > 0 {
                DXGI_MEMORY_SEGMENT_GROUP_LOCAL
            } else {
                DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL
            };
            let mut memory = Default::default();
            unsafe { adapter3.QueryVideoMemoryInfo(0, segment, &mut memory) }
                .map_err(|error| format!("failed to query DXGI adapter {index} memory: {error}"))?;
            samples.push(Sample {
                id: index as usize,
                name: utf16_name(&description.Description),
                vendor: vendor(description.VendorId),
                budget_bytes: memory.Budget,
                used_bytes: memory.CurrentUsage,
                utilization_percent: None,
            });
        }
        if let Some(nvml) = &self.nvml {
            apply_nvml_utilization(&mut samples, nvml);
        }
        (!samples.is_empty())
            .then_some(samples)
            .ok_or_else(|| "DXGI found no hardware adapters".to_owned())
    }
}

fn apply_nvml_utilization(samples: &mut [Sample], nvml: &Nvml) {
    let Ok(count) = nvml.device_count() else {
        return;
    };
    let devices = (0..count)
        .filter_map(|index| {
            let device = nvml.device_by_index(index).ok()?;
            Some((
                device.name().ok()?,
                device.utilization_rates().ok()?.gpu as f32,
            ))
        })
        .collect::<Vec<_>>();

    let mut unmatched = devices.iter().collect::<Vec<_>>();
    for sample in samples
        .iter_mut()
        .filter(|sample| sample.vendor == Vendor::Nvidia)
    {
        let position = unmatched
            .iter()
            .position(|(name, _)| names_match(&sample.name, name))
            .or_else(|| (unmatched.len() == 1).then_some(0));
        if let Some(position) = position {
            let (_, utilization) = unmatched.remove(position);
            sample.utilization_percent = Some(*utilization);
        }
    }
}

fn names_match(left: &str, right: &str) -> bool {
    let left = left.trim().to_ascii_lowercase();
    let right = right.trim().to_ascii_lowercase();
    !left.is_empty()
        && !right.is_empty()
        && (left == right || left.contains(&right) || right.contains(&left))
}

fn utf16_name(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

fn vendor(id: u32) -> Vendor {
    match id {
        0x10de => Vendor::Nvidia,
        0x1002 | 0x1022 => Vendor::Amd,
        0x8086 => Vendor::Intel,
        _ => Vendor::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::names_match;

    #[test]
    fn nvml_and_dxgi_names_match_without_case_sensitivity() {
        assert!(names_match(
            "NVIDIA GeForce RTX 5090",
            "nvidia geforce rtx 5090"
        ));
        assert!(names_match("GeForce RTX 5090", "NVIDIA GeForce RTX 5090"));
        assert!(!names_match("NVIDIA RTX 5090", "AMD Radeon"));
        assert!(!names_match("", ""));
    }
}
