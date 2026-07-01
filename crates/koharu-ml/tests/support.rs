use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};

pub fn cpu_runtime() -> RuntimeManager {
    RuntimeManager::new(default_app_data_root(), ComputePolicy::CpuOnly)
        .expect("failed to build CPU runtime manager for tests")
}
