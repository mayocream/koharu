use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    #[serde(default = "default_runtime_setting")]
    pub runtime: DirectorySetting,
    #[serde(default = "default_models_setting")]
    pub models: DirectorySetting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectorySetting {
    pub path: Utf8PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputePolicy {
    PreferGpu,
    CpuOnly,
}

impl Default for Settings {
    fn default() -> Self {
        let app_data_root = default_app_data_root();
        Self {
            runtime: DirectorySetting {
                path: app_data_root.join("runtime"),
            },
            models: DirectorySetting {
                path: app_data_root.join("models"),
            },
        }
    }
}

impl ComputePolicy {
    pub fn wants_gpu(self) -> bool {
        matches!(self, Self::PreferGpu)
    }
}

pub fn default_app_data_root() -> Utf8PathBuf {
    let root = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("Koharu");
    Utf8PathBuf::from_path_buf(root)
        .unwrap_or_else(|path| Utf8PathBuf::from(path.to_string_lossy().into_owned()))
}

fn default_runtime_setting() -> DirectorySetting {
    DirectorySetting {
        path: default_app_data_root().join("runtime"),
    }
}

fn default_models_setting() -> DirectorySetting {
    DirectorySetting {
        path: default_app_data_root().join("models"),
    }
}
