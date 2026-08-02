use std::{
    collections::{BTreeSet, HashMap},
    fs::OpenOptions,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock, Weak},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use lru::LruCache;
use parking_lot::{Condvar, Mutex, RwLock};
use redb::{
    Database, Durability as RedbDurability, ReadableDatabase, ReadableTable, TableDefinition,
    WriteTransaction, backends::InMemoryBackend,
};
use revision::revisioned;

use crate::{BlobId, DocumentId, Error, Result, Revision};

pub(crate) const FORMAT_VERSION: u32 = 1;
pub(crate) const META: TableDefinition<u8, &[u8]> = TableDefinition::new("meta");
pub(crate) const COMMITS: TableDefinition<u64, &[u8]> = TableDefinition::new("commits");
pub(crate) const BLOBS: TableDefinition<BlobId, &[u8]> = TableDefinition::new("blobs");
const DURABILITY: TableDefinition<u8, u64> = TableDefinition::new("durability");

const FLUSH_IDLE: Duration = Duration::from_millis(250);
const MAX_FLUSH_DELAY: Duration = Duration::from_secs(2);

impl redb::Value for BlobId {
    type SelfType<'a>
        = Self
    where
        Self: 'a;
    type AsBytes<'a>
        = &'a [u8]
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        Some(32)
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        Self::from_bytes(
            data.try_into()
                .expect("redb enforces the fixed blob ID width"),
        )
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        value.as_bytes()
    }

    fn type_name() -> redb::TypeName {
        redb::TypeName::new("koharu-storage::BlobId")
    }
}

impl redb::Key for BlobId {
    fn compare(left: &[u8], right: &[u8]) -> std::cmp::Ordering {
        left.cmp(right)
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug)]
pub(crate) struct Metadata {
    pub(crate) format: u32,
    pub(crate) document: DocumentId,
    pub(crate) head: Revision,
    pub(crate) checkpoint_revision: Revision,
    pub(crate) checkpoint: Vec<u8>,
    pub(crate) commits_since_checkpoint: u64,
    pub(crate) bytes_since_checkpoint: u64,
}

pub(crate) struct Store {
    pub(crate) database: Arc<RwLock<Database>>,
    pub(crate) cache: Mutex<ByteCache>,
    pub(crate) leases: Mutex<Vec<Weak<BTreeSet<BlobId>>>>,
    durability: Option<Arc<Durability>>,
    flush_worker: Option<JoinHandle<()>>,
}

impl Store {
    pub(crate) fn create(
        path: &Path,
        database_cache_bytes: usize,
        blob_cache_bytes: usize,
    ) -> Result<Arc<Self>> {
        let mut stores = stores().lock();
        stores.retain(|_, store| store.strong_count() != 0);

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        let mut builder = Database::builder();
        builder.set_cache_size(database_cache_bytes);
        let database = builder.create_file(file)?;
        let key = std::fs::canonicalize(path)?;
        let store = Arc::new(Self::new(database, blob_cache_bytes, true));
        stores.insert(key, Arc::downgrade(&store));
        Ok(store)
    }

    pub(crate) fn open(
        path: &Path,
        database_cache_bytes: usize,
        blob_cache_bytes: usize,
    ) -> Result<Arc<Self>> {
        let key = std::fs::canonicalize(path)?;
        let mut stores = stores().lock();
        stores.retain(|_, store| store.strong_count() != 0);
        if let Some(store) = stores.get(&key).and_then(Weak::upgrade) {
            return Ok(store);
        }

        let mut builder = Database::builder();
        builder.set_cache_size(database_cache_bytes);
        let database = builder.open(&key)?;
        let store = Arc::new(Self::new(database, blob_cache_bytes, true));
        stores.insert(key, Arc::downgrade(&store));
        Ok(store)
    }

    pub(crate) fn memory(
        database_cache_bytes: usize,
        blob_cache_bytes: usize,
    ) -> Result<Arc<Self>> {
        let mut builder = Database::builder();
        builder.set_cache_size(database_cache_bytes);
        let database = builder.create_with_backend(InMemoryBackend::new())?;
        Ok(Arc::new(Self::new(database, blob_cache_bytes, false)))
    }

    fn new(database: Database, blob_cache_bytes: usize, deferred_durability: bool) -> Self {
        let database = Arc::new(RwLock::new(database));
        let durability = deferred_durability.then(|| Arc::new(Durability::default()));
        let flush_worker = durability.as_ref().map(|durability| {
            let database = database.clone();
            let durability = durability.clone();
            thread::Builder::new()
                .name("koharu-storage-flush".into())
                .spawn(move || flush_worker(&database, &durability))
                .expect("failed to start the storage durability worker")
        });
        Self {
            database,
            cache: Mutex::new(ByteCache::new(blob_cache_bytes)),
            leases: Mutex::new(Vec::new()),
            durability,
            flush_worker,
        }
    }

    pub(crate) fn write(&self) -> Result<StoreWriteTransaction<'_>> {
        if let Some(durability) = &self.durability {
            durability.check_error()?;
        }
        let mut transaction = self.database.read().begin_write()?;
        if self.durability.is_some() {
            transaction.set_durability(RedbDurability::None)?;
        }
        Ok(StoreWriteTransaction {
            transaction: Some(transaction),
            durability: self.durability.as_deref(),
        })
    }

    pub(crate) fn initialize(&self, metadata: &Metadata) -> Result<()> {
        let encoded = revision::to_vec(metadata)?;
        let transaction = self.write()?;
        {
            let mut meta = transaction.open_table(META)?;
            transaction.open_table(COMMITS)?;
            transaction.open_table(BLOBS)?;
            transaction.open_table(DURABILITY)?;
            meta.insert(0, encoded.as_slice())?;
        }
        transaction.commit()?;
        self.flush()
    }

    pub(crate) fn flush(&self) -> Result<()> {
        if let Some(durability) = &self.durability {
            durability.flush()?;
        }
        Ok(())
    }

    pub(crate) fn metadata(&self) -> Result<Metadata> {
        let transaction = self.database.read().begin_read()?;
        let table = match transaction.open_table(META) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Err(Error::NotADocument),
            Err(error) => return Err(error.into()),
        };
        let encoded = table.get(0)?.ok_or(Error::NotADocument)?;
        let metadata: Metadata = revision::from_slice(encoded.value())?;
        if metadata.format != FORMAT_VERSION {
            return Err(Error::UnsupportedSchema(metadata.format));
        }
        Ok(metadata)
    }

    pub(crate) fn load_commit(&self, revision: Revision) -> Result<Option<Vec<u8>>> {
        let transaction = self.database.read().begin_read()?;
        let table = transaction.open_table(COMMITS)?;
        Ok(table
            .get(revision.get())?
            .map(|value| value.value().to_vec()))
    }

    pub(crate) fn load_commits(
        &self,
        after: Revision,
        through: Revision,
    ) -> Result<Vec<(Revision, Vec<u8>)>> {
        if after >= through {
            return Ok(Vec::new());
        }
        let transaction = self.database.read().begin_read()?;
        let table = transaction.open_table(COMMITS)?;
        let start = after
            .get()
            .checked_add(1)
            .ok_or_else(|| Error::invalid("document revision overflow"))?;
        let mut commits = Vec::new();
        for entry in table.range(start..=through.get())? {
            let (revision, payload) = entry?;
            commits.push((Revision::new(revision.value()), payload.value().to_vec()));
        }
        Ok(commits)
    }

    pub(crate) fn backup(&self, path: &Path, database_cache_bytes: usize) -> Result<()> {
        self.flush()?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        let mut builder = Database::builder();
        builder.set_cache_size(database_cache_bytes);
        let destination = builder.create_file(file)?;

        let source_transaction = self.database.read().begin_read()?;
        let source_meta = source_transaction.open_table(META)?;
        let source_commits = source_transaction.open_table(COMMITS)?;
        let source_blobs = source_transaction.open_table(BLOBS)?;
        let mut destination_transaction = destination.begin_write()?;
        destination_transaction.set_quick_repair(true);
        {
            let mut destination_meta = destination_transaction.open_table(META)?;
            let mut destination_commits = destination_transaction.open_table(COMMITS)?;
            let mut destination_blobs = destination_transaction.open_table(BLOBS)?;
            for entry in source_meta.iter()? {
                let (key, value) = entry?;
                destination_meta.insert(key.value(), value.value())?;
            }
            for entry in source_commits.iter()? {
                let (key, value) = entry?;
                destination_commits.insert(key.value(), value.value())?;
            }
            for entry in source_blobs.iter()? {
                let (key, value) = entry?;
                destination_blobs.insert(key.value(), value.value())?;
            }
        }
        destination_transaction.commit()?;
        Ok(())
    }

    pub(crate) fn compact(&self) -> Result<()> {
        self.flush()?;
        self.database.write().compact()?;
        Ok(())
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Some(durability) = &self.durability {
            durability.stop();
        }
        if let Some(worker) = self.flush_worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) struct StoreWriteTransaction<'a> {
    transaction: Option<WriteTransaction>,
    durability: Option<&'a Durability>,
}

impl Deref for StoreWriteTransaction<'_> {
    type Target = WriteTransaction;

    fn deref(&self) -> &Self::Target {
        self.transaction
            .as_ref()
            .expect("storage transaction is available until commit")
    }
}

impl StoreWriteTransaction<'_> {
    pub(crate) fn commit(mut self) -> Result<()> {
        self.transaction
            .take()
            .expect("storage transaction is committed once")
            .commit()?;
        if let Some(durability) = self.durability {
            durability.record_commit();
        }
        Ok(())
    }
}

#[derive(Default)]
struct Durability {
    state: Mutex<DurabilityState>,
    changed: Condvar,
}

#[derive(Default)]
struct DurabilityState {
    committed: u64,
    persisted: u64,
    first_unpersisted: Option<Instant>,
    last_commit: Option<Instant>,
    force: bool,
    stopping: bool,
    error: Option<String>,
}

impl Durability {
    fn record_commit(&self) {
        let mut state = self.state.lock();
        let now = Instant::now();
        if state.committed == state.persisted {
            state.first_unpersisted = Some(now);
        }
        state.last_commit = Some(now);
        state.committed = state.committed.wrapping_add(1);
        self.changed.notify_one();
    }

    fn check_error(&self) -> Result<()> {
        let state = self.state.lock();
        match &state.error {
            Some(error) => Err(Error::Durability(error.clone())),
            None => Ok(()),
        }
    }

    fn flush(&self) -> Result<()> {
        let mut state = self.state.lock();
        let target = state.committed;
        if state.persisted >= target {
            return match &state.error {
                Some(error) => Err(Error::Durability(error.clone())),
                None => Ok(()),
            };
        }
        state.error = None;
        state.force = true;
        self.changed.notify_one();
        while state.persisted < target && state.error.is_none() {
            self.changed.wait(&mut state);
        }
        match &state.error {
            Some(error) => Err(Error::Durability(error.clone())),
            None => Ok(()),
        }
    }

    fn stop(&self) {
        let mut state = self.state.lock();
        state.stopping = true;
        state.force = true;
        self.changed.notify_one();
    }
}

fn flush_worker(database: &RwLock<Database>, durability: &Durability) {
    loop {
        let (target, stopping) = {
            let mut state = durability.state.lock();
            while state.persisted >= state.committed && !state.stopping {
                durability.changed.wait(&mut state);
            }
            if state.persisted >= state.committed && state.stopping {
                return;
            }
            if !state.force && !state.stopping {
                while !state.force && !state.stopping {
                    let idle_deadline = state
                        .last_commit
                        .expect("a dirty store records its last commit")
                        + FLUSH_IDLE;
                    let maximum_deadline = state
                        .first_unpersisted
                        .expect("a dirty store records when it became dirty")
                        + MAX_FLUSH_DELAY;
                    let deadline = idle_deadline.min(maximum_deadline);
                    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                        break;
                    };
                    durability.changed.wait_for(&mut state, remaining);
                }
            }
            state.force = false;
            (state.committed, state.stopping)
        };

        let result = persist(database, target);
        let mut state = durability.state.lock();
        match result {
            Ok(()) => {
                state.persisted = state.persisted.max(target);
                state.error = None;
                if state.persisted >= state.committed {
                    state.first_unpersisted = None;
                    state.last_commit = None;
                } else {
                    state.first_unpersisted = state.last_commit;
                }
            }
            Err(error) => {
                let retry_at = Instant::now();
                state.first_unpersisted = Some(retry_at);
                state.last_commit = Some(retry_at);
                state.error = Some(error.to_string());
            }
        }
        durability.changed.notify_all();
        if stopping {
            return;
        }
    }
}

fn persist(database: &RwLock<Database>, generation: u64) -> Result<()> {
    let mut transaction = database.read().begin_write()?;
    transaction.set_durability(RedbDurability::Immediate)?;
    transaction.set_quick_repair(true);
    {
        transaction.open_table(DURABILITY)?.insert(0, generation)?;
    }
    transaction.commit()?;
    Ok(())
}

pub(crate) struct ByteCache {
    entries: LruCache<BlobId, Arc<[u8]>>,
    bytes: usize,
    limit: usize,
}

impl ByteCache {
    fn new(limit: usize) -> Self {
        Self {
            entries: LruCache::unbounded(),
            bytes: 0,
            limit,
        }
    }

    pub(crate) fn contains(&mut self, id: BlobId) -> bool {
        self.entries.contains(&id)
    }

    pub(crate) fn get(&mut self, id: BlobId) -> Option<Arc<[u8]>> {
        self.entries.get(&id).cloned()
    }

    pub(crate) fn insert(&mut self, id: BlobId, value: Arc<[u8]>) {
        if self.limit == 0 || value.len() > self.limit {
            return;
        }
        if let Some(old) = self.entries.put(id, value.clone()) {
            self.bytes = self.bytes.saturating_sub(old.len());
        }
        self.bytes = self.bytes.saturating_add(value.len());
        while self.bytes > self.limit {
            let Some((_, old)) = self.entries.pop_lru() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(old.len());
        }
    }

    pub(crate) fn remove(&mut self, id: BlobId) {
        if let Some(value) = self.entries.pop(&id) {
            self.bytes = self.bytes.saturating_sub(value.len());
        }
    }
}

fn stores() -> &'static Mutex<HashMap<PathBuf, Weak<Store>>> {
    static STORES: OnceLock<Mutex<HashMap<PathBuf, Weak<Store>>>> = OnceLock::new();
    STORES.get_or_init(|| Mutex::new(HashMap::new()))
}
