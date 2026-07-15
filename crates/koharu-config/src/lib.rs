use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use config::{Config, File, FileFormat};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use koharu_secrets::SecretStore;

const CONFIG_DIRECTORY: &str = ".koharu";
const CONFIG_FILE: &str = "config.toml";
const SECRET_SERVICE: &str = "koharu";

/// Returns the shared Koharu configuration path: `~/.koharu/config.toml`.
pub fn path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine the home directory")?;
    Ok(home.join(CONFIG_DIRECTORY).join(CONFIG_FILE))
}

/// Loads the shared configuration into a caller-defined root type.
///
/// Values from `T::default()` are used as the lowest-priority source, so a
/// file may contain only the values that differ from those defaults. Fields
/// not represented by `T` remain available to other configuration consumers.
pub fn load<T>() -> Result<T>
where
    T: Default + DeserializeOwned + Serialize,
{
    load_from(&path()?)
}

/// Writes the top-level fields represented by a caller-defined root type.
///
/// Top-level fields already in the file but not represented by `T` are
/// preserved, allowing independent crates to extend the shared file.
pub fn save<T>(value: &T) -> Result<()>
where
    T: Serialize,
{
    save_to(&path()?, value)
}

/// Loads one caller-defined top-level section from the shared configuration.
///
/// Missing values are filled from `T::default()`. The section name is kept
/// outside this crate so consumers own both their schema and namespace.
pub fn load_section<T>(section: &str) -> Result<T>
where
    T: Default + DeserializeOwned + Serialize,
{
    load_section_from(&path()?, section)
}

/// Replaces one top-level section while preserving every other section.
pub fn save_section<T>(section: &str, value: &T) -> Result<()>
where
    T: Serialize,
{
    save_section_to(&path()?, section, value)
}

/// Returns Koharu's platform-backed secret store.
pub fn secrets() -> SecretStore {
    SecretStore::new(SECRET_SERVICE)
}

fn load_from<T>(path: &Path) -> Result<T>
where
    T: Default + DeserializeOwned + Serialize,
{
    Config::builder()
        .add_source(Config::try_from(&T::default()).context("failed to serialize defaults")?)
        .add_source(toml_file(path))
        .build()
        .with_context(|| format!("failed to load `{}`", path.display()))?
        .try_deserialize()
        .with_context(|| format!("failed to deserialize `{}`", path.display()))
}

fn save_to<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let mut document = load_document(path)?;
    let update = toml::Value::try_from(value).context("failed to serialize configuration")?;
    let update = update
        .as_table()
        .context("root configuration must serialize as a table")?;

    for (key, value) in update {
        document.insert(key.clone(), value.clone());
    }

    write_document(path, document)
}

fn load_section_from<T>(path: &Path, section: &str) -> Result<T>
where
    T: Default + DeserializeOwned + Serialize,
{
    validate_section(section)?;

    let mut defaults = toml::Table::new();
    defaults.insert(
        section.to_owned(),
        toml::Value::try_from(T::default()).context("failed to serialize section defaults")?,
    );

    Config::builder()
        .add_source(Config::try_from(&defaults).context("failed to serialize section defaults")?)
        .add_source(toml_file(path))
        .build()
        .with_context(|| format!("failed to load `{}`", path.display()))?
        .get(section)
        .with_context(|| {
            format!(
                "failed to deserialize section `{section}` from `{}`",
                path.display()
            )
        })
}

fn save_section_to<T>(path: &Path, section: &str, value: &T) -> Result<()>
where
    T: Serialize,
{
    validate_section(section)?;

    let mut document = load_document(path)?;
    document.insert(
        section.to_owned(),
        toml::Value::try_from(value)
            .with_context(|| format!("failed to serialize section `{section}`"))?,
    );
    write_document(path, document)
}

fn load_document(path: &Path) -> Result<toml::Table> {
    if !path.exists() {
        return Ok(toml::Table::new());
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))?;
    toml::from_str(&content).with_context(|| format!("failed to parse `{}`", path.display()))
}

fn write_document(path: &Path, document: toml::Table) -> Result<()> {
    let parent = path
        .parent()
        .context("configuration path has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create `{}`", parent.display()))?;

    let content = toml::to_string_pretty(&document).context("failed to encode configuration")?;
    fs::write(path, content).with_context(|| format!("failed to write `{}`", path.display()))
}

fn toml_file(path: &Path) -> File<config::FileSourceFile, FileFormat> {
    File::from(path).format(FileFormat::Toml).required(false)
}

fn validate_section(section: &str) -> Result<()> {
    anyhow::ensure!(!section.is_empty(), "configuration section cannot be empty");
    anyhow::ensure!(
        !section.contains(['.', '[', ']']),
        "configuration section must be a top-level key"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct RootConfig {
        http: HttpConfig,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct HttpConfig {
        connect_timeout: u64,
        read_timeout: u64,
    }

    impl Default for HttpConfig {
        fn default() -> Self {
            Self {
                connect_timeout: 20,
                read_timeout: 300,
            }
        }
    }

    #[test]
    fn path_uses_home_dot_koharu_layout() {
        let path = path().unwrap();
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(CONFIG_FILE)
        );
        assert_eq!(
            path.parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str()),
            Some(CONFIG_DIRECTORY)
        );
    }

    #[test]
    fn root_load_merges_caller_defaults_with_file_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE);
        fs::write(
            &path,
            r#"
                [http]
                connect_timeout = 45
            "#,
        )
        .unwrap();

        let config: RootConfig = load_from(&path).unwrap();

        assert_eq!(config.http.connect_timeout, 45);
        assert_eq!(config.http.read_timeout, 300);
    }

    #[test]
    fn section_load_uses_caller_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE);
        fs::write(
            &path,
            r#"
                [http]
                connect_timeout = 45
            "#,
        )
        .unwrap();

        let config: HttpConfig = load_section_from(&path, "http").unwrap();

        assert_eq!(config.connect_timeout, 45);
        assert_eq!(config.read_timeout, 300);
    }

    #[test]
    fn missing_section_uses_caller_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE);

        let config: HttpConfig = load_section_from(&path, "http").unwrap();

        assert_eq!(config, HttpConfig::default());
    }

    #[test]
    fn root_save_preserves_unrelated_sections() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE);
        fs::write(
            &path,
            r#"
                [plugin]
                enabled = true
            "#,
        )
        .unwrap();

        save_to(&path, &RootConfig::default()).unwrap();
        let document = fs::read_to_string(&path).unwrap();

        assert!(document.contains("[plugin]"));
        assert!(document.contains("enabled = true"));
        assert!(document.contains("[http]"));
    }

    #[test]
    fn section_save_preserves_other_sections() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE);
        fs::write(
            &path,
            r#"
                [plugin]
                enabled = true
            "#,
        )
        .unwrap();

        save_section_to(&path, "http", &HttpConfig::default()).unwrap();
        let document = fs::read_to_string(&path).unwrap();

        assert!(document.contains("[plugin]"));
        assert!(document.contains("enabled = true"));
        assert!(document.contains("[http]"));
    }

    #[test]
    fn section_save_creates_the_config_directory() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_DIRECTORY).join(CONFIG_FILE);

        save_section_to(&path, "http", &HttpConfig::default()).unwrap();

        assert!(path.is_file());
        let config: HttpConfig = load_section_from(&path, "http").unwrap();
        assert_eq!(config, HttpConfig::default());
    }
}
