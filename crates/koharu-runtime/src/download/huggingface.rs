/// ref: https://huggingface.co/docs/huggingface_hub/v1.21.0.rc0/en/package_reference/file_download#huggingface_hub.hf_hub_url.example
pub fn huggingface(repo: &str, filename: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/main/{filename}")
}
