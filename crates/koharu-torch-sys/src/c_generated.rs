#![allow(clippy::all, unused_imports)]

use crate::{scalar, tensor};

include!(concat!(env!("OUT_DIR"), "/torch_api_generated.rs"));
