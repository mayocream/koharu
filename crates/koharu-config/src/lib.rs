//! Live, typed configuration backed by Koharu's shared TOML file.
//!
//! `load(section)` returns a process-wide handle with `RwLock`-style access:
//!
//! ```no_run
//! # use serde::{Deserialize, Serialize};
//! #[derive(Default, Deserialize, Serialize)]
//! struct PipelineConfig { translator: String }
//! # fn main() -> anyhow::Result<()> {
//! let pipeline = koharu_config::load::<PipelineConfig>("pipeline")?;
//! pipeline.write()?.translator = "deepl".into();
//! pipeline.save()?;
//! # Ok(()) }
//! ```
//!
//! Keep the handle, not a copied value, and do not hold read or write guards
//! across `.await` points.

use std::any::Any;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

use anyhow::{Context, Result, anyhow};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::watch;

pub use koharu_secrets::{ExposeSecret, SecretStore, SecretString};

const CONFIG_DIRECTORY: &str = ".koharu";
const CONFIG_FILE: &str = "config.toml";
const SECRET_SERVICE: &str = "koharu";

static MANAGER: OnceLock<Result<Arc<Manager>, String>> = OnceLock::new();

/// Monotonically increasing version of a live configuration value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfigRevision(u64);

impl ConfigRevision {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A shared, live configuration value.
///
/// Clone this handle rather than retaining a value obtained from `read()`.
pub struct Config<T> {
    manager: Arc<Manager>,
    target: Target,
    state: Arc<State<T>>,
}

impl<T> Clone for Config<T> {
    fn clone(&self) -> Self {
        Self {
            manager: self.manager.clone(),
            target: self.target.clone(),
            state: self.state.clone(),
        }
    }
}

impl<T> std::fmt::Debug for Config<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Config")
            .field("target", &self.target)
            .field("revision", &self.revision())
            .finish_non_exhaustive()
    }
}

impl<T> Config<T> {
    /// Create a live configuration handle without file persistence.
    /// `save()` succeeds as a no-op, making this suitable for tests.
    #[must_use]
    pub fn memory(value: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        let manager = Manager::memory();
        let (changes, _) = watch::channel(ConfigRevision::ZERO);
        Self {
            manager,
            target: Target::Root,
            state: Arc::new(State {
                value: RwLock::new(value),
                revision: AtomicU64::new(0),
                changes,
            }),
        }
    }

    /// Borrow the latest configuration value for reading.
    pub fn read(&self) -> Result<ConfigRead<'_, T>> {
        let value = self
            .state
            .value
            .read()
            .map_err(|_| anyhow!("configuration read lock is poisoned"))?;
        let revision = self.revision();
        Ok(ConfigRead { revision, value })
    }

    /// Borrow the live configuration value for mutation.
    ///
    /// Mutations are immediately visible after the guard is dropped. Call
    /// `Config::save` or `ConfigWrite::save` when they must also be durable.
    pub fn write(&self) -> Result<ConfigWrite<'_, T>> {
        let value = self
            .state
            .value
            .write()
            .map_err(|_| anyhow!("configuration write lock is poisoned"))?;
        Ok(ConfigWrite {
            config: self,
            value: Some(value),
            dirty: false,
        })
    }

    #[must_use]
    pub fn revision(&self) -> ConfigRevision {
        ConfigRevision(self.state.revision.load(Ordering::Acquire))
    }

    /// Subscribe to published in-memory changes. The receiver always contains
    /// the latest revision, so lagging consumers can simply re-read the value.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<ConfigRevision> {
        self.state.changes.subscribe()
    }
}

impl<T> From<T> for Config<T>
where
    T: Send + Sync + 'static,
{
    fn from(value: T) -> Self {
        Self::memory(value)
    }
}

impl<T> Config<T>
where
    T: Serialize,
{
    /// Persist the latest complete value while preventing concurrent mutation.
    pub fn save(&self) -> Result<()> {
        let value = self.read()?;
        self.manager.save(&self.target, &*value)
    }
}

pub struct ConfigRead<'a, T> {
    revision: ConfigRevision,
    value: RwLockReadGuard<'a, T>,
}

impl<T> ConfigRead<'_, T> {
    #[must_use]
    pub const fn revision(&self) -> ConfigRevision {
        self.revision
    }
}

impl<T> Deref for ConfigRead<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

#[must_use = "configuration changes are not durable until `save()` succeeds"]
pub struct ConfigWrite<'a, T> {
    config: &'a Config<T>,
    value: Option<RwLockWriteGuard<'a, T>>,
    dirty: bool,
}

impl<T> ConfigWrite<'_, T>
where
    T: Serialize,
{
    /// Persist this exact write-locked value and then release the guard.
    pub fn save(self) -> Result<()> {
        self.config.manager.save(
            &self.config.target,
            self.value
                .as_deref()
                .expect("configuration guard is present"),
        )
    }
}

impl<T> Deref for ConfigWrite<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
            .as_deref()
            .expect("configuration guard is present")
    }
}

impl<T> DerefMut for ConfigWrite<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        self.value
            .as_deref_mut()
            .expect("configuration guard is present")
    }
}

impl<T> Drop for ConfigWrite<'_, T> {
    fn drop(&mut self) {
        if !self.dirty {
            return;
        }

        let revision = self.state_revision();
        self.config.state.changes.send_replace(revision);
    }
}

impl<T> ConfigWrite<'_, T> {
    fn state_revision(&self) -> ConfigRevision {
        let previous = self.config.state.revision.fetch_add(1, Ordering::AcqRel);
        ConfigRevision(
            previous
                .checked_add(1)
                .expect("configuration revision overflow"),
        )
    }
}

struct State<T> {
    value: RwLock<T>,
    revision: AtomicU64,
    changes: watch::Sender<ConfigRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Target {
    Root,
    Section(String),
}

struct Registered {
    type_name: &'static str,
    state: Arc<dyn Any + Send + Sync>,
}

struct Manager {
    path: Option<PathBuf>,
    sections: Mutex<HashMap<Target, Registered>>,
    save: Mutex<()>,
}

impl Manager {
    fn new(path: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            path: Some(path),
            sections: Mutex::new(HashMap::new()),
            save: Mutex::new(()),
        })
    }

    fn memory() -> Arc<Self> {
        Arc::new(Self {
            path: None,
            sections: Mutex::new(HashMap::new()),
            save: Mutex::new(()),
        })
    }

    fn load<T>(self: &Arc<Self>, target: Target) -> Result<Config<T>>
    where
        T: Default + DeserializeOwned + Serialize + Send + Sync + 'static,
    {
        let mut sections = self
            .sections
            .lock()
            .map_err(|_| anyhow!("configuration registry lock is poisoned"))?;

        if let Some(registered) = sections.get(&target) {
            let state = registered
                .state
                .clone()
                .downcast::<State<T>>()
                .map_err(|_| {
                    anyhow!(
                        "configuration {target:?} was already loaded as `{}` instead of `{}`",
                        registered.type_name,
                        std::any::type_name::<T>()
                    )
                })?;
            return Ok(Config {
                manager: self.clone(),
                target,
                state,
            });
        }

        let path = self
            .path
            .as_deref()
            .context("in-memory configuration cannot load another section")?;
        let value = load_value(path, &target)?;
        let (changes, _) = watch::channel(ConfigRevision::ZERO);
        let state = Arc::new(State {
            value: RwLock::new(value),
            revision: AtomicU64::new(0),
            changes,
        });
        sections.insert(
            target.clone(),
            Registered {
                type_name: std::any::type_name::<T>(),
                state: state.clone(),
            },
        );

        Ok(Config {
            manager: self.clone(),
            target,
            state,
        })
    }

    fn save<T>(&self, target: &Target, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        let Some(path) = self.path.as_deref() else {
            return Ok(());
        };
        let _save = self
            .save
            .lock()
            .map_err(|_| anyhow!("configuration save lock is poisoned"))?;
        let mut document = load_document(path)?;
        let update = toml::Value::try_from(value).context("failed to serialize configuration")?;

        match target {
            Target::Root => {
                let update = update
                    .as_table()
                    .context("root configuration must serialize as a table")?;
                for (key, value) in update {
                    document.insert(key.clone(), value.clone());
                }
            }
            Target::Section(section) => {
                document.insert(section.clone(), update);
            }
        }

        write_document(path, &document)
    }
}

/// Returns the shared Koharu configuration path: `~/.koharu/config.toml`.
pub fn path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine the home directory")?;
    Ok(home.join(CONFIG_DIRECTORY).join(CONFIG_FILE))
}

/// Load or retrieve a live, process-wide top-level configuration section.
pub fn load<T>(section: &str) -> Result<Config<T>>
where
    T: Default + DeserializeOwned + Serialize + Send + Sync + 'static,
{
    validate_section(section)?;
    manager()?.load(Target::Section(section.to_owned()))
}

/// Load or retrieve the live root configuration value.
///
/// Prefer `load(section)` for independently owned configuration. This exists
/// for applications whose schema intentionally covers the complete file.
pub fn load_root<T>() -> Result<Config<T>>
where
    T: Default + DeserializeOwned + Serialize + Send + Sync + 'static,
{
    manager()?.load(Target::Root)
}

/// Returns Koharu's platform-backed secret store.
#[must_use]
pub fn secrets() -> SecretStore {
    SecretStore::new(SECRET_SERVICE)
}

fn manager() -> Result<Arc<Manager>> {
    match MANAGER.get_or_init(|| {
        path()
            .map(Manager::new)
            .map_err(|error| format!("{error:#}"))
    }) {
        Ok(manager) => Ok(manager.clone()),
        Err(error) => Err(anyhow!(error.clone())),
    }
}

fn load_value<T>(path: &Path, target: &Target) -> Result<T>
where
    T: Default + DeserializeOwned + Serialize,
{
    let mut value = toml::Value::try_from(T::default())
        .context("failed to serialize configuration defaults")?;
    let document = load_document(path)?;

    let update = match target {
        Target::Root => Some(toml::Value::Table(document)),
        Target::Section(section) => document.get(section).cloned(),
    };
    if let Some(update) = update {
        merge(&mut value, update);
    }

    value.try_into().with_context(|| match target {
        Target::Root => format!("failed to deserialize `{}`", path.display()),
        Target::Section(section) => format!(
            "failed to deserialize section `{section}` from `{}`",
            path.display()
        ),
    })
}

fn merge(base: &mut toml::Value, update: toml::Value) {
    match (base, update) {
        (toml::Value::Table(base), toml::Value::Table(update)) => {
            let changes_tag = matches!(
                (base.get("model"), update.get("model")),
                (Some(base), Some(update)) if base != update
            );
            if changes_tag {
                *base = update;
                return;
            }
            for (key, value) in update {
                match base.get_mut(&key) {
                    Some(base) => merge(base, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, update) => *base = update,
    }
}

fn load_document(path: &Path) -> Result<toml::Table> {
    if !path.exists() {
        return Ok(toml::Table::new());
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))?;
    toml::from_str(&content).with_context(|| format!("failed to parse `{}`", path.display()))
}

fn write_document(path: &Path, document: &toml::Table) -> Result<()> {
    let parent = path
        .parent()
        .context("configuration path has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create `{}`", parent.display()))?;

    let content = toml::to_string_pretty(document).context("failed to encode configuration")?;
    AtomicFile::new(path, OverwriteBehavior::AllowOverwrite)
        .write(|file| file.write_all(content.as_bytes()))
        .map_err(atomic_write_error)
        .with_context(|| format!("failed to write `{}`", path.display()))
}

fn atomic_write_error(error: atomicwrites::Error<io::Error>) -> io::Error {
    match error {
        atomicwrites::Error::Internal(error) | atomicwrites::Error::User(error) => error,
    }
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

    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct RootConfig {
        http: HttpConfig,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct PluginConfig {
        enabled: bool,
    }

    fn test_manager() -> (tempfile::TempDir, Arc<Manager>) {
        let directory = tempfile::tempdir().unwrap();
        let manager = Manager::new(directory.path().join(CONFIG_FILE));
        (directory, manager)
    }

    #[test]
    fn handles_for_the_same_section_share_live_state() {
        let (_directory, manager) = test_manager();
        let first = manager
            .load::<HttpConfig>(Target::Section("http".into()))
            .unwrap();
        let second = manager
            .load::<HttpConfig>(Target::Section("http".into()))
            .unwrap();

        first.write().unwrap().connect_timeout = 45;

        assert_eq!(second.read().unwrap().connect_timeout, 45);
        assert_eq!(second.revision(), ConfigRevision(1));
    }

    #[test]
    fn save_persists_the_latest_live_value() {
        let (directory, manager) = test_manager();
        let config = manager
            .load::<HttpConfig>(Target::Section("http".into()))
            .unwrap();
        config.write().unwrap().connect_timeout = 45;

        assert!(!directory.path().join(CONFIG_FILE).exists());
        config.save().unwrap();

        let reloaded = Manager::new(directory.path().join(CONFIG_FILE))
            .load::<HttpConfig>(Target::Section("http".into()))
            .unwrap();
        assert_eq!(reloaded.read().unwrap().connect_timeout, 45);
    }

    #[test]
    fn write_guard_can_save_while_holding_the_exact_value() {
        let (directory, manager) = test_manager();
        let config = manager
            .load::<HttpConfig>(Target::Section("http".into()))
            .unwrap();
        let mut value = config.write().unwrap();
        value.read_timeout = 600;
        value.save().unwrap();

        let document = fs::read_to_string(directory.path().join(CONFIG_FILE)).unwrap();
        assert!(document.contains("read_timeout = 600"));
    }

    #[test]
    fn subscribers_receive_the_latest_revision() {
        let (_directory, manager) = test_manager();
        let config = manager
            .load::<HttpConfig>(Target::Section("http".into()))
            .unwrap();
        let changes = config.subscribe();

        config.write().unwrap().connect_timeout = 45;

        assert_eq!(*changes.borrow(), ConfigRevision(1));
    }

    #[test]
    fn concurrent_writers_mutate_one_live_value_without_lost_updates() {
        let (_directory, manager) = test_manager();
        let config = manager
            .load::<HttpConfig>(Target::Section("http".into()))
            .unwrap();

        std::thread::scope(|scope| {
            for _ in 0..4 {
                let config = config.clone();
                scope.spawn(move || {
                    for _ in 0..100 {
                        config.write().unwrap().connect_timeout += 1;
                    }
                });
            }
        });

        assert_eq!(config.read().unwrap().connect_timeout, 420);
        assert_eq!(config.revision().get(), 400);
    }

    #[test]
    fn concurrent_section_saves_preserve_both_sections() {
        let (directory, manager) = test_manager();
        let http = manager
            .load::<HttpConfig>(Target::Section("http".into()))
            .unwrap();
        let plugin = manager
            .load::<PluginConfig>(Target::Section("plugin".into()))
            .unwrap();
        plugin.write().unwrap().enabled = true;

        std::thread::scope(|scope| {
            scope.spawn(|| http.save().unwrap());
            scope.spawn(|| plugin.save().unwrap());
        });

        let document = fs::read_to_string(directory.path().join(CONFIG_FILE)).unwrap();
        assert!(document.contains("[http]"));
        assert!(document.contains("[plugin]"));
        assert!(document.contains("enabled = true"));
    }

    #[test]
    fn defaults_are_deeply_merged_with_file_values() {
        let (directory, manager) = test_manager();
        fs::write(
            directory.path().join(CONFIG_FILE),
            "[http]\nconnect_timeout = 45\n",
        )
        .unwrap();

        let config = manager
            .load::<HttpConfig>(Target::Section("http".into()))
            .unwrap();

        assert_eq!(
            *config.read().unwrap(),
            HttpConfig {
                connect_timeout: 45,
                read_timeout: 300,
            }
        );
    }

    #[test]
    fn changing_a_model_tag_replaces_its_configuration() {
        let mut base: toml::Value = toml::from_str(
            r#"
                model = "local"
                local_model = "lfm2.5-1.2b-instruct"
            "#,
        )
        .unwrap();
        let update = toml::from_str(
            r#"
                model = "openai"
                remote_model = "gpt-4.1-mini"
            "#,
        )
        .unwrap();

        merge(&mut base, update);

        assert_eq!(base["model"].as_str(), Some("openai"));
        assert_eq!(base["remote_model"].as_str(), Some("gpt-4.1-mini"));
        assert!(base.get("local_model").is_none());
    }

    #[test]
    fn section_save_preserves_unrelated_sections() {
        let (directory, manager) = test_manager();
        fs::write(
            directory.path().join(CONFIG_FILE),
            "[plugin]\nenabled = true\n",
        )
        .unwrap();
        let config = manager
            .load::<HttpConfig>(Target::Section("http".into()))
            .unwrap();

        config.save().unwrap();

        let document = fs::read_to_string(directory.path().join(CONFIG_FILE)).unwrap();
        assert!(document.contains("[plugin]"));
        assert!(document.contains("enabled = true"));
        assert!(document.contains("[http]"));
    }

    #[test]
    fn root_save_preserves_unknown_top_level_sections() {
        let (directory, manager) = test_manager();
        fs::write(
            directory.path().join(CONFIG_FILE),
            "[plugin]\nenabled = true\n",
        )
        .unwrap();
        let config = manager.load::<RootConfig>(Target::Root).unwrap();

        config.save().unwrap();

        let document = fs::read_to_string(directory.path().join(CONFIG_FILE)).unwrap();
        assert!(document.contains("[plugin]"));
        assert!(document.contains("[http]"));
    }

    #[test]
    fn loading_a_section_with_two_types_is_rejected() {
        let (_directory, manager) = test_manager();
        manager
            .load::<HttpConfig>(Target::Section("http".into()))
            .unwrap();

        let error = manager
            .load::<RootConfig>(Target::Section("http".into()))
            .unwrap_err();

        assert!(error.to_string().contains("already loaded"));
    }
}
