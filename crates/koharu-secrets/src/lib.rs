mod platform;
mod store;

pub use secrecy::{ExposeSecret, SecretString};
pub use store::{SecretStore, delete_secret, get_secret, set_secret};
