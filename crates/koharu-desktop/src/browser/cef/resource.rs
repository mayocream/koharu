use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
pub(super) struct ResourceRoot(Arc<PathBuf>);

pub(super) struct Resource {
    pub reader: File,
    pub mime_type: &'static str,
}

impl ResourceRoot {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|error| format!("failed to resolve UI resource root: {error}"))?;
        if !root.is_dir() {
            return Err(format!(
                "UI resource root is not a directory: {}",
                root.display()
            ));
        }
        Ok(Self(Arc::new(root)))
    }

    pub fn load(&self, request_path: &str) -> Option<Resource> {
        let decoded = percent_encoding::percent_decode_str(request_path)
            .decode_utf8()
            .ok()?;
        let relative = Path::new(decoded.trim_start_matches('/'));
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::Prefix(_) | Component::RootDir
                )
            })
        {
            return None;
        }
        let relative = if relative.as_os_str().is_empty() {
            Path::new("index.html")
        } else {
            relative
        };
        let path = self.0.join(relative);
        let path = path.canonicalize().ok()?;
        if !path.starts_with(self.0.as_path()) || !path.is_file() {
            return None;
        }
        let reader = File::open(&path).ok()?;
        Some(Resource {
            reader,
            mime_type: mime_type(&path),
        })
    }
}

fn mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html",
        Some("css") => "text/css",
        Some("js") | Some("mjs") => "application/javascript",
        Some("json") => "application/json",
        Some("wasm") => "application/wasm",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        _ => "application/octet-stream",
    }
}

pub(super) fn read(resource: &mut Resource, output: &mut [u8]) -> std::io::Result<usize> {
    resource.reader.read(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_paths_are_rejected_before_filesystem_access() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("index.html"), "ok").unwrap();
        let root = ResourceRoot::new(temp.path().to_path_buf()).unwrap();
        assert!(root.load("../secret").is_none());
        assert!(root.load("%2e%2e/secret").is_none());
        assert!(root.load("").is_some());
    }
}
