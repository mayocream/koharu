//! koharu-ml: Machine learning inference engine
//!
//! Provides hardware-accelerated inference for vision and language models.
//! Supports multiple backends: CPU, CUDA, Metal, and Vulkan.

pub mod backend;

// Re-export common types
pub use backend::{Backend, BackendInfo};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_detection() {
        let detected = backend::detect_backend();
        println!("Detected backend: {}", detected);
    }
}
