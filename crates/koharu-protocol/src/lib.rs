//! Authoritative transport-neutral desktop protocol.
//!
//! The protocol deliberately contains no browser, window, IPC, or GPU types.
//! Desktop transports serialize these messages and generated frontend clients
//! consume the same Rust schema.

mod event;
mod message;
mod types;

pub use event::{AppEvent, ServerEvent};
pub use message::{Command, CommandResult, Request, RequestId, Response, ServerMessage};
pub use types::*;
