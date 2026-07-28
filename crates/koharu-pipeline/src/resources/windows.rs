use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
    DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, IDXGIAdapter1,
    IDXGIAdapter3, IDXGIFactory6,
};
use windows::core::Interface as _;

use super::{Sample, Vendor};

pub(super) struct Monitor {
    factory: IDXGIFactory6,
}

impl Monitor {
    pub(super) fn new() -> Result<Self, String> {
        let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory6>() }
            .map_err(|error| format!("failed to create DXGI factory: {error}"))?;
        Ok(Self { factory })
    }

    pub(super) fn sample(&mut self) -> Result<Vec<Sample>, String> {
        // DXGI exposes the OS-assigned budget and this process's usage. That is
        // more useful for admission than physical capacity because the budget
        // already reacts to pressure from other applications.
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
                available_bytes: memory.Budget.saturating_sub(memory.CurrentUsage),
                utilization_percent: None,
            });
        }
        (!samples.is_empty())
            .then_some(samples)
            .ok_or_else(|| "DXGI found no hardware adapters".to_owned())
    }
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
