use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowAction {
    Minimize,
    ToggleMaximize,
    BeginDrag,
    Close,
}

/// Window facts sampled from winit. This is the authoritative source
/// for application-facing window state; browser UI must not infer these values
/// from an action it requested.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub focused: bool,
    pub minimized: bool,
    pub maximized: bool,
    pub fullscreen: bool,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum WindowActionError {
    #[error("window action was rejected: {0}")]
    Rejected(String),
}

/// Desktop-only operating-system services. Application state does not
/// implement or depend on this trait.
pub trait PlatformServices {
    fn open_external(&mut self, url: &Url) -> Result<(), PlatformError>;
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PlatformError {
    #[error("platform operation is unavailable: {0}")]
    Unavailable(String),
    #[error("platform operation failed: {0}")]
    Failed(String),
}
