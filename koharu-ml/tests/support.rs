use koharu_runtime::{ComputePolicy, RuntimeManager, Settings};

pub fn cpu_runtime() -> RuntimeManager {
    RuntimeManager::new(Settings::default(), ComputePolicy::CpuOnly)
        .expect("failed to build CPU runtime manager for tests")
}
