mod cuda;
mod hip;
mod vulkan;

/// Accelerator capabilities discovered once while assembling a runtime plan.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Hardware {
    pub(crate) cuda_driver: Option<i32>,
    pub(crate) rocm_target: Option<String>,
    pub(crate) vulkan: bool,
}

impl Hardware {
    #[must_use]
    pub fn discover() -> Self {
        Self {
            cuda_driver: cuda::probe(),
            rocm_target: hip::probe(),
            vulkan: vulkan::probe(),
        }
    }

    #[must_use]
    pub fn supports_cuda(&self) -> bool {
        self.cuda_driver.is_some_and(|version| version >= 13000)
    }

    #[must_use]
    pub fn supports_rocm(&self) -> bool {
        self.rocm_target.is_some()
    }

    #[must_use]
    pub fn rocm_target(&self) -> Option<&str> {
        self.rocm_target.as_deref()
    }

    #[must_use]
    pub fn supports_vulkan(&self) -> bool {
        self.vulkan
    }

    #[must_use]
    pub fn supports_metal(&self) -> bool {
        cfg!(all(target_os = "macos", target_arch = "aarch64"))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn discovery_is_safe_without_accelerators() {
        let _ = super::Hardware::discover();
    }
}
