//! Windowless CEF browser boundary.
//!
//! The browser process runs on the winit thread. Platform shared textures are
//! copied into WGPU-owned frames before accelerated callbacks return, with CEF
//! software paint as fallback. One bounded mailbox feeds the sole presenter.

mod accelerated;
mod cef;
mod frame;
mod input;
mod message;

pub(crate) use accelerated::{AcceleratedFrameImporter, BrowserGpu};
pub use cef::{
    BrowserCursor, CefBrowser, CefConfig, CefEvent, CefEventSender, dispatch_cef_process,
};
pub(crate) use frame::{AcceleratedFrame, BrowserFrame, BrowserFrameMailbox};
pub use frame::{DirtyRect, FrameError, SoftwareFrame};
pub use input::{
    ButtonState, InputEvent, InputModifiers, KeyLocation, KeyState, MouseButton, ScrollDelta,
};
pub use message::{AttachmentError, BinaryAttachment, WebMessage};

/// Stable JavaScript object installed by the concrete CEF adapter.
pub const JAVASCRIPT_BRIDGE_OBJECT: &str = "koharu";
pub const JAVASCRIPT_POST_MESSAGE: &str = "postMessage";
pub const JAVASCRIPT_RECEIVE_MESSAGE: &str = "receiveServerMessage";
