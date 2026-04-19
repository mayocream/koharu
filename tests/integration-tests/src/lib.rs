//! End-to-end integration tests for the Koharu backend.
//!
//! Each test under `tests/` spawns a fresh [`TestApp`]: a temporary app-data
//! root, an `App`, an axum server on an ephemeral port, and a preconfigured
//! [`koharu_client::apis::configuration::Configuration`]. Tear-down drops
//! the tempdir; cleanup is best-effort (file locks on Windows may linger).

pub mod harness;

pub use harness::TestApp;
