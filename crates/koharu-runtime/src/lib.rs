mod device;
mod hardware;
mod network;
mod runtime;
mod source;
mod store;

pub mod downloads;

pub use device::{Backend, Device, DeviceType};
pub use hardware::Hardware;
pub use runtime::{Feature, Package, Runtime, Torch};
pub use source::HuggingFaceFile;
pub use store::Store;

/// Builds the shared client used by remote APIs and long-running model requests.
///
/// Requests have no implicit timeout or retry policy. Callers own cancellation,
/// and non-idempotent generation requests are never replayed automatically.
pub fn http_client() -> anyhow::Result<reqwest::Client> {
    network::api_client()
}
