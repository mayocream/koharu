//! Tests for backend selection and fallback logic
//!
//! These tests verify that the backend module correctly selects and initializes
//! the appropriate compute backend (CPU, CUDA, Metal, or Vulkan).

#[cfg(test)]
mod backend_selection_tests {
    use koharu_ml::backend;

    #[test]
    fn test_backend_detection() {
        let detected = backend::detect_backend();
        println!("Detected backend: {}", detected);
        // Should return some backend (CPU as fallback)
    }

    #[test]
    fn test_backend_display() {
        let backends = vec![
            backend::Backend::Cpu,
            #[cfg(feature = "vulkan")]
            backend::Backend::Vulkan,
            #[cfg(feature = "cuda")]
            backend::Backend::Cuda,
            #[cfg(feature = "metal")]
            backend::Backend::Metal,
        ];

        for backend_variant in backends {
            let display_str = format!("{}", backend_variant);
            assert!(!display_str.is_empty());
            println!("Backend: {}", display_str);
        }
    }

    #[test]
    fn test_backend_initialization() {
        let backend = backend::Backend::Cpu;
        let result = backend::BackendInfo::init(backend);
        assert!(result.is_ok());

        let info = result.unwrap();
        assert_eq!(info.backend, backend::Backend::Cpu);
    }
}
