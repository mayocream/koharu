//! Emit the OpenAPI spec for the current router to a JSON file.

use std::fs;

fn main() {
    let (_, spec) = koharu_rpc::api::api();
    let json = spec.to_pretty_json().unwrap();
    let path = std::env::args()
        .nth(1)
        .expect("Output path for OpenAPI spec JSON must be provided as the first argument");
    fs::write(path, json).unwrap();
}
