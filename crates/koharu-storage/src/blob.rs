use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock, Weak},
    time::Duration,
};

use lru::LruCache;
use parking_lot::{Condvar, Mutex};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params_from_iter};

use crate::{BlobId, Error, Result};

#[derive(Clone, Debug)]
pub struct BlobAttachment {
    id: BlobId,
    bytes: Arc<[u8]>,
}

impl BlobAttachment {
    #[must_use]
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Self {
        let bytes = bytes.into();
        let id = BlobId::for_bytes(&bytes);
        Self { id, bytes }
    }

    pub(crate) fn from_parts(id: BlobId, bytes: Arc<[u8]>) -> Self {
        Self { id, bytes }
    }

    #[must_use]
    pub const fn id(&self) -> BlobId {
        self.id
    }

    #[must_use]
    pub fn bytes(&self) -> Arc<[u8]> {
        self.bytes.clone()
    }
}

#[derive(Clone, Debug, Default)]
pub struct BlobBatch {
    blobs: BTreeMap<BlobId, Arc<[u8]>>,
}

impl BlobBatch {
    #[must_use]
    pub fn get(&self, id: BlobId) -> Option<&Arc<[u8]>> {
        self.blobs.get(&id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (BlobId, &Arc<[u8]>)> {
        self.blobs.iter().map(|(id, bytes)| (*id, bytes))
    }

    pub(crate) fn insert(&mut self, id: BlobId, bytes: Arc<[u8]>) {
        self.blobs.insert(id, bytes);
    }
}

#[derive(Clone)]
enum Source {
    File(PathBuf),
    Uri(String),
}

struct ByteCache {
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

    fn insert(&mut self, id: BlobId, value: Arc<[u8]>) {
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
}

pub(crate) struct BlobStore {
    readers: ReaderPool,
    cache: Mutex<ByteCache>,
    leases: Mutex<Vec<Weak<BTreeSet<BlobId>>>>,
}

impl BlobStore {
    pub(crate) fn file(
        path: &Path,
        timeout: Duration,
        cache_bytes: usize,
        reader_limit: usize,
    ) -> Arc<Self> {
        static STORES: OnceLock<Mutex<HashMap<PathBuf, Weak<BlobStore>>>> = OnceLock::new();
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        let stores = STORES.get_or_init(|| Mutex::new(HashMap::new()));
        let mut stores = stores.lock();
        stores.retain(|_, store| store.strong_count() != 0);
        if let Some(store) = stores.get(&path).and_then(Weak::upgrade) {
            return store;
        }
        let store = Arc::new(Self {
            readers: ReaderPool::new(Source::File(path.to_owned()), timeout, reader_limit),
            cache: Mutex::new(ByteCache::new(cache_bytes)),
            leases: Mutex::new(Vec::new()),
        });
        stores.insert(path, Arc::downgrade(&store));
        store
    }

    pub(crate) fn uri(
        uri: String,
        timeout: Duration,
        cache_bytes: usize,
        reader_limit: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            readers: ReaderPool::new(Source::Uri(uri), timeout, reader_limit),
            cache: Mutex::new(ByteCache::new(cache_bytes)),
            leases: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn contains(&self, id: BlobId) -> Result<bool> {
        if self.cache.lock().entries.contains(&id) {
            return Ok(true);
        }
        self.readers
            .checkout()?
            .query_row(
                "SELECT 1 FROM blobs WHERE id = ?1",
                [id.as_bytes().as_slice()],
                |_| Ok(()),
            )
            .optional()
            .map(|value| value.is_some())
            .map_err(Into::into)
    }

    pub(crate) fn read(&self, id: BlobId) -> Result<Arc<[u8]>> {
        if let Some(bytes) = self.cache.lock().entries.get(&id).cloned() {
            return Ok(bytes);
        }
        let bytes = self
            .readers
            .checkout()?
            .query_row(
                "SELECT bytes FROM blobs WHERE id = ?1",
                [id.as_bytes().as_slice()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?
            .ok_or(Error::BlobNotFound(id))?;
        if BlobId::for_bytes(&bytes) != id {
            return Err(Error::invalid(format!("blob {id} has invalid content")));
        }
        let bytes: Arc<[u8]> = bytes.into();
        self.cache.lock().insert(id, bytes.clone());
        Ok(bytes)
    }

    pub(crate) fn read_many(&self, ids: impl IntoIterator<Item = BlobId>) -> Result<BlobBatch> {
        const QUERY_CHUNK: usize = 500;
        let ids = ids.into_iter().collect::<BTreeSet<_>>();
        let mut batch = BlobBatch::default();
        let mut missing = Vec::new();
        {
            let mut cache = self.cache.lock();
            for id in &ids {
                if let Some(bytes) = cache.entries.get(id).cloned() {
                    batch.blobs.insert(*id, bytes);
                } else {
                    missing.push(*id);
                }
            }
        }
        if missing.is_empty() {
            return Ok(batch);
        }

        let connection = self.readers.checkout()?;
        for chunk in missing.chunks(QUERY_CHUNK) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!("SELECT id, bytes FROM blobs WHERE id IN ({placeholders})");
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map(
                params_from_iter(chunk.iter().map(|id| id.as_bytes().as_slice())),
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )?;
            for row in rows {
                let (raw_id, bytes) = row?;
                let raw_id: [u8; 32] = raw_id.try_into().map_err(|_| Error::NotADocument)?;
                let id = BlobId::from_bytes(raw_id);
                if BlobId::for_bytes(&bytes) != id {
                    return Err(Error::invalid(format!("blob {id} has invalid content")));
                }
                let bytes: Arc<[u8]> = bytes.into();
                batch.blobs.insert(id, bytes.clone());
                self.cache.lock().insert(id, bytes);
            }
        }
        if let Some(id) = ids.iter().find(|id| !batch.blobs.contains_key(id)).copied() {
            return Err(Error::BlobNotFound(id));
        }
        Ok(batch)
    }

    pub(crate) fn lease(&self, blobs: BTreeSet<BlobId>) -> Arc<BTreeSet<BlobId>> {
        let lease = Arc::new(blobs);
        let mut leases = self.leases.lock();
        leases.retain(|lease| lease.strong_count() != 0);
        leases.push(Arc::downgrade(&lease));
        lease
    }

    pub(crate) fn live_blobs(&self) -> BTreeSet<BlobId> {
        let mut result = BTreeSet::new();
        let mut leases = self.leases.lock();
        leases.retain(|lease| {
            if let Some(lease) = lease.upgrade() {
                result.extend(lease.iter().copied());
                true
            } else {
                false
            }
        });
        result
    }

    pub(crate) fn invalidate(&self, ids: &BTreeSet<BlobId>) {
        let mut cache = self.cache.lock();
        for id in ids {
            if let Some(bytes) = cache.entries.pop(id) {
                cache.bytes = cache.bytes.saturating_sub(bytes.len());
            }
        }
    }
}

struct ReaderPool {
    source: Source,
    timeout: Duration,
    limit: usize,
    state: Mutex<ReaderPoolState>,
    available: Condvar,
}

#[derive(Default)]
struct ReaderPoolState {
    idle: Vec<Connection>,
    total: usize,
}

impl ReaderPool {
    fn new(source: Source, timeout: Duration, limit: usize) -> Self {
        Self {
            source,
            timeout,
            limit,
            state: Mutex::new(ReaderPoolState::default()),
            available: Condvar::new(),
        }
    }

    fn checkout(&self) -> Result<ReaderGuard<'_>> {
        loop {
            let mut state = self.state.lock();
            if let Some(connection) = state.idle.pop() {
                return Ok(ReaderGuard {
                    pool: self,
                    connection: Some(connection),
                });
            }
            if state.total < self.limit {
                state.total += 1;
                drop(state);
                match self.open() {
                    Ok(connection) => {
                        return Ok(ReaderGuard {
                            pool: self,
                            connection: Some(connection),
                        });
                    }
                    Err(error) => {
                        let mut state = self.state.lock();
                        state.total -= 1;
                        self.available.notify_one();
                        return Err(error);
                    }
                }
            }
            self.available.wait(&mut state);
        }
    }

    fn open(&self) -> Result<Connection> {
        let (location, flags) = match &self.source {
            Source::File(path) => (
                path.to_string_lossy().into_owned(),
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ),
            Source::Uri(uri) => (
                uri.clone(),
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ),
        };
        let connection = Connection::open_with_flags(location, flags)?;
        connection.busy_timeout(self.timeout)?;
        Ok(connection)
    }
}

struct ReaderGuard<'pool> {
    pool: &'pool ReaderPool,
    connection: Option<Connection>,
}

impl std::ops::Deref for ReaderGuard<'_> {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.connection
            .as_ref()
            .expect("reader guard owns a connection")
    }
}

impl Drop for ReaderGuard<'_> {
    fn drop(&mut self) {
        let connection = self
            .connection
            .take()
            .expect("reader guard owns a connection");
        self.pool.state.lock().idle.push(connection);
        self.pool.available.notify_one();
    }
}
