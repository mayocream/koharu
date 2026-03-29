mod hf_hub;

pub mod comic_text_detector;
pub mod facade;
pub mod font_detector;
pub mod lama;
pub mod loading;
pub mod manga_ocr;
pub mod manga_text_segmentation_2025;
pub mod mit48px_ocr;
pub mod paddleocr_vl;
pub mod pp_doclayout_v3;

use anyhow::Result;
use candle_core::utils::metal_is_available;

pub use candle_core::Device;
pub use koharu_http::download::HubAssetSpec;
pub use koharu_http::hf_hub::set_cache_dir;

pub fn device(cpu: bool) -> Result<Device> {
    if cpu {
        Ok(Device::Cpu)
    } else if cuda_is_available() {
        Ok(Device::new_cuda(0)?)
    } else if metal_is_available() {
        Ok(Device::new_metal(0)?)
    } else {
        println!("CUDA and Metal are not available. Using CPU device.");
        Ok(Device::Cpu)
    }
}

pub fn cuda_is_available() -> bool {
    (unsafe {
        libloading::Library::new(if cfg!(target_os = "windows") {
            "nvcuda.dll"
        } else {
            "libcuda.so"
        })
        .is_ok()
    }) && cfg!(feature = "cuda")
}

pub const BASE_PREFETCH_ASSETS: [HubAssetSpec; 8] = [
    HubAssetSpec {
        repo: "PaddlePaddle/PP-DocLayoutV3_safetensors",
        filename: "config.json",
    },
    HubAssetSpec {
        repo: "PaddlePaddle/PP-DocLayoutV3_safetensors",
        filename: "preprocessor_config.json",
    },
    HubAssetSpec {
        repo: "PaddlePaddle/PP-DocLayoutV3_safetensors",
        filename: "model.safetensors",
    },
    HubAssetSpec {
        repo: "mayocream/comic-text-detector",
        filename: "yolo-v5.safetensors",
    },
    HubAssetSpec {
        repo: "mayocream/comic-text-detector",
        filename: "unet.safetensors",
    },
    HubAssetSpec {
        repo: "mayocream/lama-manga",
        filename: "lama-manga.safetensors",
    },
    HubAssetSpec {
        repo: "fffonion/yuzumarker-font-detection",
        filename: "yuzumarker-font-detection.safetensors",
    },
    HubAssetSpec {
        repo: "fffonion/yuzumarker-font-detection",
        filename: "font-labels-ex.json",
    },
];
