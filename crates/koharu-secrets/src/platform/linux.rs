use std::fmt::Write as _;
use std::fs;
use std::io;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use atomicwrites::{AtomicFile, OverwriteBehavior};
use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use keyring::{Error, Result};

pub(crate) fn configure() {
    keyring::set_default_credential_builder(Box::new(Builder::new(secret_root())));
}

fn secret_root() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".koharu")
        .join("secrets")
}

#[derive(Debug)]
struct Builder {
    root: PathBuf,
}

impl Builder {
    fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl CredentialBuilderApi for Builder {
    fn build(&self, target: Option<&str>, service: &str, user: &str) -> Result<Box<Credential>> {
        Ok(Box::new(FileCredential {
            root: self.root.clone(),
            name: file_name(target, service, user),
        }))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
struct FileCredential {
    root: PathBuf,
    name: String,
}

impl FileCredential {
    fn path(&self) -> PathBuf {
        self.root.join(&self.name)
    }
}

impl CredentialApi for FileCredential {
    fn set_secret(&self, secret: &[u8]) -> Result<()> {
        fs::create_dir_all(&self.root).map_err(storage_error)?;
        set_mode(&self.root, 0o700)?;

        let path = self.path();
        AtomicFile::new(&path, OverwriteBehavior::AllowOverwrite)
            .write(|file| -> io::Result<()> {
                set_file_mode(file, 0o600)?;
                file.write_all(secret)
            })
            .map_err(atomic_write_error)?;
        set_mode(&path, 0o600)
    }

    fn get_secret(&self) -> Result<Vec<u8>> {
        fs::read(self.path()).map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => Error::NoEntry,
            _ => storage_error(err),
        })
    }

    fn delete_credential(&self) -> Result<()> {
        fs::remove_file(self.path()).map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => Error::NoEntry,
            _ => storage_error(err),
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(storage_error)
}

fn set_file_mode(file: &fs::File, mode: u32) -> io::Result<()> {
    file.set_permissions(fs::Permissions::from_mode(mode))
}

fn atomic_write_error(err: atomicwrites::Error<io::Error>) -> Error {
    match err {
        atomicwrites::Error::Internal(err) | atomicwrites::Error::User(err) => storage_error(err),
    }
}

fn storage_error(err: std::io::Error) -> Error {
    Error::NoStorageAccess(Box::new(err))
}

fn file_name(target: Option<&str>, service: &str, user: &str) -> String {
    let target = target
        .map(|value| format!("some-{}", encode(value)))
        .unwrap_or_else(|| "none".to_string());
    format!(
        "v1-target-{target}--service-{}--user-{}.secret",
        encode(service),
        encode(user)
    )
}

fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                encoded.push(byte as char);
            }
            _ => {
                let _ = write!(&mut encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_secret_across_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let builder = Builder::new(dir.path());

        let first = builder
            .build(None, "koharu", "llm_provider_api_key_openai")
            .unwrap();
        assert!(matches!(first.get_secret(), Err(Error::NoEntry)));
        first.set_secret(b"sk-test").unwrap();

        let second = builder
            .build(None, "koharu", "llm_provider_api_key_openai")
            .unwrap();
        assert_eq!(second.get_secret().unwrap(), b"sk-test");
        second.delete_credential().unwrap();
        assert!(matches!(second.get_secret(), Err(Error::NoEntry)));
    }

    #[test]
    fn file_names_escape_path_separators() {
        let name = file_name(Some("target/value"), "service\\name", "user name");

        assert!(name.contains("target%2Fvalue"));
        assert!(name.contains("service%5Cname"));
        assert!(name.contains("user%20name"));
        assert!(!name.contains('/'));
        assert!(!name.contains('\\'));
    }

    #[test]
    fn writes_private_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let builder = Builder::new(dir.path());
        let credential = builder
            .build(None, "koharu", "llm_provider_api_key_openai")
            .unwrap();

        credential.set_secret(b"sk-test").unwrap();

        let dir_mode = fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
        let file_path = dir
            .path()
            .join(file_name(None, "koharu", "llm_provider_api_key_openai"));
        let file_mode = fs::metadata(file_path).unwrap().permissions().mode() & 0o777;

        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }
}
