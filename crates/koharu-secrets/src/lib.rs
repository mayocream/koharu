mod platform;
mod store;

pub use store::{SecretStore, delete_secret, get_secret, set_secret};
