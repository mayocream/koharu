use std::path::Path;

use super::*;

const FIXTURE: &str = "src/gguf/ggml-vocab-bert-bge.gguf";

#[test]
#[ignore = "requires initialized koharu-llm runtime libraries"]
fn opens_valid_file() {
    assert!(GgufContext::from_file(Path::new(FIXTURE)).is_some());
}

#[test]
#[ignore = "requires initialized koharu-llm runtime libraries"]
fn returns_none_for_missing_file() {
    assert!(GgufContext::from_file(Path::new("nonexistent.gguf")).is_none());
}

#[test]
#[ignore = "requires initialized koharu-llm runtime libraries"]
fn kv_count() {
    let ctx = GgufContext::from_file(Path::new(FIXTURE)).unwrap();
    assert_eq!(ctx.n_kv(), 20);
}

#[test]
#[ignore = "requires initialized koharu-llm runtime libraries"]
fn no_tensors_in_vocab_file() {
    let ctx = GgufContext::from_file(Path::new(FIXTURE)).unwrap();
    assert_eq!(ctx.n_tensors(), 0);
}

#[test]
#[ignore = "requires initialized koharu-llm runtime libraries"]
fn find_known_key() {
    let ctx = GgufContext::from_file(Path::new(FIXTURE)).unwrap();
    let idx = ctx.find_key("general.architecture");
    assert!(idx >= 0);
    assert_eq!(ctx.val_str(idx), Some("bert"));
}

#[test]
#[ignore = "requires initialized koharu-llm runtime libraries"]
fn find_missing_key() {
    let ctx = GgufContext::from_file(Path::new(FIXTURE)).unwrap();
    assert_eq!(ctx.find_key("does.not.exist"), -1);
}

#[test]
#[ignore = "requires initialized koharu-llm runtime libraries"]
fn key_at_first() {
    let ctx = GgufContext::from_file(Path::new(FIXTURE)).unwrap();
    assert_eq!(ctx.key_at(0), Some("general.architecture"));
}

#[test]
#[ignore = "requires initialized koharu-llm runtime libraries"]
fn read_u32_value() {
    let ctx = GgufContext::from_file(Path::new(FIXTURE)).unwrap();
    let idx = ctx.find_key("bert.block_count");
    assert!(idx >= 0);
    assert_eq!(ctx.val_u32(idx), 12);
}

#[test]
#[ignore = "requires initialized koharu-llm runtime libraries"]
fn kv_type_string() {
    let ctx = GgufContext::from_file(Path::new(FIXTURE)).unwrap();
    let idx = ctx.find_key("general.architecture");
    assert_eq!(ctx.kv_type(idx), crate::sys::GGUF_TYPE_STRING);
}

#[test]
#[ignore = "requires initialized koharu-llm runtime libraries"]
fn kv_type_uint32() {
    let ctx = GgufContext::from_file(Path::new(FIXTURE)).unwrap();
    let idx = ctx.find_key("bert.block_count");
    assert_eq!(ctx.kv_type(idx), crate::sys::GGUF_TYPE_UINT32);
}
