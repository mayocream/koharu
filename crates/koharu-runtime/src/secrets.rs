use std::sync::Once;

use keyring::Entry;

static INIT_CREDENTIAL_STORE: Once = Once::new();

/// Service-scoped access to Koharu's platform-backed string secret storage.
#[derive(Debug, Clone)]
pub struct SecretStore {
    service: String,
}

impl SecretStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// Load a secret by key, returning `None` when no credential exists.
    pub fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        get_secret(&self.service, key)
    }

    /// Store a secret by key. Use `delete` to clear an existing credential.
    pub fn set(&self, key: &str, secret: &str) -> anyhow::Result<()> {
        set_secret(&self.service, key, secret)
    }

    /// Clear a secret by key. Missing credentials are treated as success.
    pub fn delete(&self, key: &str) -> anyhow::Result<()> {
        delete_secret(&self.service, key)
    }
}

pub fn get_secret(service: &str, key: &str) -> anyhow::Result<Option<String>> {
    let entry = secret_entry(service, key)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub fn set_secret(service: &str, key: &str, secret: &str) -> anyhow::Result<()> {
    secret_entry(service, key)?.set_password(secret)?;
    Ok(())
}

pub fn delete_secret(service: &str, key: &str) -> anyhow::Result<()> {
    match secret_entry(service, key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn secret_entry(service: &str, key: &str) -> anyhow::Result<Entry> {
    INIT_CREDENTIAL_STORE.call_once(configure_platform_store);
    Ok(Entry::new(service, key)?)
}

#[cfg(target_os = "linux")]
fn configure_platform_store() {
    let root = crate::runtime::default_app_data_root()
        .as_std_path()
        .join("secrets")
        .join("keyring");
    keyring::set_default_credential_builder(Box::new(filesystem::Builder::new(root)));
}

#[cfg(not(target_os = "linux"))]
fn configure_platform_store() {}

#[cfg(any(target_os = "linux", test))]
mod filesystem {
    use std::fmt::Write as _;
    use std::fs;
    use std::io;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};

    use atomicwrites::{AtomicFile, OverwriteBehavior};
    use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
    use keyring::{Error, Result};

    #[derive(Debug)]
    pub(super) struct Builder {
        root: PathBuf,
    }

    impl Builder {
        pub(super) fn new(root: impl Into<PathBuf>) -> Self {
            Self { root: root.into() }
        }
    }

    impl CredentialBuilderApi for Builder {
        fn build(
            &self,
            target: Option<&str>,
            service: &str,
            user: &str,
        ) -> Result<Box<Credential>> {
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

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(storage_error)
    }

    #[cfg(not(unix))]
    fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
        Ok(())
    }

    #[cfg(unix)]
    fn set_file_mode(file: &fs::File, mode: u32) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;

        file.set_permissions(fs::Permissions::from_mode(mode))
    }

    #[cfg(not(unix))]
    fn set_file_mode(_file: &fs::File, _mode: u32) -> io::Result<()> {
        Ok(())
    }

    fn atomic_write_error(err: atomicwrites::Error<io::Error>) -> Error {
        match err {
            atomicwrites::Error::Internal(err) | atomicwrites::Error::User(err) => {
                storage_error(err)
            }
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

        #[cfg(unix)]
        #[test]
        fn writes_private_permissions() {
            use std::os::unix::fs::PermissionsExt;

            let dir = tempfile::tempdir().unwrap();
            let builder = Builder::new(dir.path());
            let credential = builder
                .build(None, "koharu", "llm_provider_api_key_openai")
                .unwrap();

            credential.set_secret(b"sk-test").unwrap();

            let dir_mode = fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
            let file_path =
                dir.path()
                    .join(file_name(None, "koharu", "llm_provider_api_key_openai"));
            let file_mode = fs::metadata(file_path).unwrap().permissions().mode() & 0o777;

            assert_eq!(dir_mode, 0o700);
            assert_eq!(file_mode, 0o600);
        }
    }
}
