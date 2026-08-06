use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock, Weak},
};

use parking_lot::Mutex;
use revision::revisioned;
use rocksdb::{
    BlockBasedOptions, BoundColumnFamily, Cache, ColumnFamilyDescriptor, DBCompressionType,
    DBWithThreadMode, IteratorMode, MultiThreaded, Options as RocksOptions, ReadOptions,
    WriteBatch, WriteOptions,
};

use crate::{
    BlobBatch, BlobId, DocumentId, Error, Result, Revision, blob::BlobCache, commit::StoredEntry,
    session::GcReport,
};

const SCHEMA_VERSION: u32 = 1;
const DOCUMENT_CF: &str = "document";
const HISTORY_CF: &str = "history";
const BLOB_INDEX_CF: &str = "blob-index";
const BLOBS_CF: &str = "blobs";
const HEAD_KEY: &[u8] = b"head";
const CHECKPOINT_KEY: &[u8] = b"checkpoint";

type Database = DBWithThreadMode<MultiThreaded>;

#[revisioned(revision = 1)]
#[derive(Clone, Debug)]
pub(crate) struct Head {
    pub(crate) schema: u32,
    pub(crate) document: DocumentId,
    pub(crate) revision: Revision,
    pub(crate) checkpoint_revision: Revision,
    pub(crate) commits_since_checkpoint: u64,
    pub(crate) bytes_since_checkpoint: u64,
}

impl Head {
    pub(crate) fn empty(document: DocumentId) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            document,
            revision: Revision::ZERO,
            checkpoint_revision: Revision::ZERO,
            commits_since_checkpoint: 0,
            bytes_since_checkpoint: 0,
        }
    }
}

pub(crate) struct Engine {
    database: Database,
    writes: Mutex<()>,
    cache: Mutex<BlobCache>,
    leases: Mutex<Vec<Weak<BTreeSet<BlobId>>>>,
    _block_cache: Cache,
    _temporary: Option<tempfile::TempDir>,
}

impl Engine {
    pub(crate) fn create(path: &Path, block_cache: usize, blob_cache: usize) -> Result<Arc<Self>> {
        if path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{} already exists", path.display()),
            )
            .into());
        }
        std::fs::create_dir(path)?;
        match open_database(path, block_cache, true) {
            Ok((database, cache)) => {
                let path = dunce::canonicalize(path)?;
                let engine = Arc::new(Self::new(database, cache, blob_cache, None));
                engines().lock().insert(path, Arc::downgrade(&engine));
                Ok(engine)
            }
            Err(error) => {
                let _ = std::fs::remove_dir_all(path);
                Err(error)
            }
        }
    }

    pub(crate) fn open(path: &Path, block_cache: usize, blob_cache: usize) -> Result<Arc<Self>> {
        let path = dunce::canonicalize(path)?;
        if !path.is_dir() {
            return Err(Error::NotAProject);
        }
        let mut engines = engines().lock();
        engines.retain(|_, engine| engine.strong_count() != 0);
        if let Some(engine) = engines.get(&path).and_then(Weak::upgrade) {
            return Ok(engine);
        }
        let (database, cache) = open_database(&path, block_cache, false)?;
        let engine = Arc::new(Self::new(database, cache, blob_cache, None));
        engines.insert(path, Arc::downgrade(&engine));
        Ok(engine)
    }

    pub(crate) fn memory(block_cache: usize, blob_cache: usize) -> Result<Arc<Self>> {
        let directory = tempfile::Builder::new()
            .prefix("koharu-storage-")
            .tempdir()?;
        let (database, cache) = open_database(directory.path(), block_cache, true)?;
        Ok(Arc::new(Self::new(
            database,
            cache,
            blob_cache,
            Some(directory),
        )))
    }

    fn new(
        database: Database,
        block_cache: Cache,
        blob_cache: usize,
        temporary: Option<tempfile::TempDir>,
    ) -> Self {
        Self {
            database,
            writes: Mutex::new(()),
            cache: Mutex::new(BlobCache::new(blob_cache)),
            leases: Mutex::new(Vec::new()),
            _block_cache: block_cache,
            _temporary: temporary,
        }
    }

    pub(crate) fn initialize(&self, head: &Head, checkpoint: &[u8]) -> Result<()> {
        let _write = self.writes.lock();
        let document = self.cf(DOCUMENT_CF)?;
        if self.database.get_cf(&document, HEAD_KEY)?.is_some() {
            return Err(Error::NotAProject);
        }
        let mut batch = WriteBatch::default();
        batch.put_cf(&document, HEAD_KEY, revision::to_vec(head)?);
        batch.put_cf(&document, CHECKPOINT_KEY, checkpoint);
        let mut options = WriteOptions::default();
        options.set_sync(true);
        self.database.write_opt(batch, &options)?;
        Ok(())
    }

    pub(crate) fn head(&self) -> Result<Head> {
        let document = self.cf(DOCUMENT_CF)?;
        let bytes = self
            .database
            .get_cf(&document, HEAD_KEY)?
            .ok_or(Error::NotAProject)?;
        let head: Head = revision::from_slice(&bytes)?;
        if head.schema != SCHEMA_VERSION {
            return Err(Error::UnsupportedSchema(head.schema));
        }
        Ok(head)
    }

    pub(crate) fn recovery(&self) -> Result<(Head, Vec<u8>, Vec<(Revision, Vec<u8>)>)> {
        let head = self.head()?;
        let document = self.cf(DOCUMENT_CF)?;
        let checkpoint = self
            .database
            .get_cf(&document, CHECKPOINT_KEY)?
            .ok_or(Error::NotAProject)?;
        let history = self.history_range(head.checkpoint_revision, head.revision)?;
        Ok((head, checkpoint, history))
    }

    pub(crate) fn history(&self, revision: Revision) -> Result<Option<Vec<u8>>> {
        let history = self.cf(HISTORY_CF)?;
        Ok(self.database.get_cf(&history, history_key(revision))?)
    }

    pub(crate) fn history_range(
        &self,
        after: Revision,
        through: Revision,
    ) -> Result<Vec<(Revision, Vec<u8>)>> {
        if after >= through {
            return Ok(Vec::new());
        }
        let start = after
            .next()
            .ok_or_else(|| Error::invalid("document revision overflow"))?;
        let history = self.cf(HISTORY_CF)?;
        let mut entries = Vec::new();
        for item in self.database.iterator_cf(
            &history,
            IteratorMode::From(&history_key(start), rocksdb::Direction::Forward),
        ) {
            let (key, value) = item?;
            let revision = decode_revision(&key)?;
            if revision > through {
                break;
            }
            entries.push((revision, value.into_vec()));
        }
        Ok(entries)
    }

    pub(crate) fn missing_blobs(&self, ids: &BTreeSet<BlobId>) -> Result<Vec<BlobId>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let index = self.cf(BLOB_INDEX_CF)?;
        let values = self
            .database
            .multi_get_cf(ids.iter().map(|id| (&index, id.as_bytes())));
        ids.iter()
            .zip(values)
            .filter_map(|(id, value)| match value {
                Ok(Some(_)) => None,
                Ok(None) => Some(Ok(*id)),
                Err(error) => Some(Err(error.into())),
            })
            .collect()
    }

    pub(crate) fn read_blob(&self, id: BlobId) -> Result<Arc<[u8]>> {
        if let Some(bytes) = self.cache.lock().get(id) {
            return Ok(bytes);
        }
        let blobs = self.cf(BLOBS_CF)?;
        let bytes = self
            .database
            .get_cf(&blobs, id.as_bytes())?
            .ok_or(Error::BlobNotFound(id))?;
        let bytes = checked_blob(id, &bytes)?;
        self.cache.lock().insert(id, bytes.clone());
        Ok(bytes)
    }

    pub(crate) fn read_blobs(&self, ids: &BTreeSet<BlobId>) -> Result<BlobBatch> {
        let mut batch = BlobBatch::default();
        let mut missing = Vec::new();
        {
            let mut cache = self.cache.lock();
            for id in ids {
                if let Some(bytes) = cache.get(*id) {
                    batch.blobs.insert(*id, bytes);
                } else {
                    missing.push(*id);
                }
            }
        }
        if !missing.is_empty() {
            let blobs = self.cf(BLOBS_CF)?;
            let values = self
                .database
                .multi_get_cf(missing.iter().map(|id| (&blobs, id.as_bytes())));
            let mut loaded = Vec::with_capacity(missing.len());
            for (id, value) in missing.into_iter().zip(values) {
                let bytes = value?.ok_or(Error::BlobNotFound(id))?;
                let bytes = checked_blob(id, &bytes)?;
                batch.blobs.insert(id, bytes.clone());
                loaded.push((id, bytes));
            }
            let mut cache = self.cache.lock();
            for (id, bytes) in loaded {
                cache.insert(id, bytes);
            }
        }
        Ok(batch)
    }

    pub(crate) fn commit(
        &self,
        expected: &Head,
        next: &Head,
        revision: Revision,
        history_value: &[u8],
        blob_values: &BTreeMap<BlobId, Arc<[u8]>>,
        checkpoint: Option<&[u8]>,
    ) -> Result<()> {
        let _write = self.writes.lock();
        self.ensure_head(expected)?;
        let document = self.cf(DOCUMENT_CF)?;
        let history = self.cf(HISTORY_CF)?;
        let index = self.cf(BLOB_INDEX_CF)?;
        let blobs = self.cf(BLOBS_CF)?;
        let mut batch = WriteBatch::default();
        batch.put_cf(&history, history_key(revision), history_value);
        for (id, bytes) in blob_values {
            batch.put_cf(&index, id.as_bytes(), (bytes.len() as u64).to_be_bytes());
            batch.put_cf(&blobs, id.as_bytes(), bytes.as_ref());
        }
        if let Some(checkpoint) = checkpoint {
            batch.put_cf(&document, CHECKPOINT_KEY, checkpoint);
        }
        batch.put_cf(&document, HEAD_KEY, revision::to_vec(next)?);
        self.database.write(batch)?;
        Ok(())
    }

    pub(crate) fn checkpoint(&self, expected: &Head, next: &Head, checkpoint: &[u8]) -> Result<()> {
        let _write = self.writes.lock();
        self.ensure_head(expected)?;
        let document = self.cf(DOCUMENT_CF)?;
        let mut batch = WriteBatch::default();
        batch.put_cf(&document, CHECKPOINT_KEY, checkpoint);
        batch.put_cf(&document, HEAD_KEY, revision::to_vec(next)?);
        self.database.write(batch)?;
        Ok(())
    }

    pub(crate) fn collect(
        &self,
        expected: &Head,
        checkpoint: Option<(&Head, &[u8], Revision)>,
        mut marked: BTreeSet<BlobId>,
    ) -> Result<(GcReport, BTreeSet<BlobId>)> {
        let _write = self.writes.lock();
        self.ensure_head(expected)?;
        marked.extend(self.live_blobs());
        let history = self.cf(HISTORY_CF)?;
        let index = self.cf(BLOB_INDEX_CF)?;
        let blobs = self.cf(BLOBS_CF)?;
        let mut batch = WriteBatch::default();
        let keep_from = checkpoint.as_ref().map(|(_, _, revision)| *revision);
        let mut reads = ReadOptions::default();
        reads.fill_cache(false);
        for item in self
            .database
            .iterator_cf_opt(&history, reads, IteratorMode::Start)
        {
            let (key, value) = item?;
            let revision = decode_revision(&key)?;
            if keep_from.is_some_and(|keep| revision < keep) {
                batch.delete_cf(&history, key);
            } else {
                let entry: StoredEntry = revision::from_slice(&value)?;
                marked.extend(entry.blobs);
            }
        }
        let mut removed = BTreeSet::new();
        let mut removed_bytes = 0_u64;
        let mut reads = ReadOptions::default();
        reads.fill_cache(false);
        for item in self
            .database
            .iterator_cf_opt(&index, reads, IteratorMode::Start)
        {
            let (key, value) = item?;
            let id = decode_blob_id(&key)?;
            if !marked.contains(&id) {
                removed.insert(id);
                removed_bytes = removed_bytes.saturating_add(decode_blob_size(&value)?);
                batch.delete_cf(&index, &key);
                batch.delete_cf(&blobs, key);
            }
        }
        if let Some((next, checkpoint, _)) = checkpoint {
            let document = self.cf(DOCUMENT_CF)?;
            batch.put_cf(&document, CHECKPOINT_KEY, checkpoint);
            batch.put_cf(&document, HEAD_KEY, revision::to_vec(next)?);
        }
        self.database.write(batch)?;
        for id in &removed {
            self.cache.lock().remove(*id);
        }
        Ok((
            GcReport {
                blobs: removed.len(),
                bytes: removed_bytes,
            },
            removed,
        ))
    }

    pub(crate) fn lease(&self, ids: BTreeSet<BlobId>) -> Arc<BTreeSet<BlobId>> {
        let lease = Arc::new(ids);
        let mut leases = self.leases.lock();
        leases.retain(|lease| lease.strong_count() != 0);
        leases.push(Arc::downgrade(&lease));
        lease
    }

    fn live_blobs(&self) -> BTreeSet<BlobId> {
        let mut live = BTreeSet::new();
        let mut leases = self.leases.lock();
        leases.retain(|weak| {
            if let Some(lease) = weak.upgrade() {
                live.extend(lease.iter().copied());
                true
            } else {
                false
            }
        });
        live
    }

    pub(crate) fn compact(&self) -> Result<()> {
        self.flush()?;
        let history = self.cf(HISTORY_CF)?;
        let blobs = self.cf(BLOBS_CF)?;
        self.database
            .compact_range_cf::<&[u8], &[u8]>(&history, None, None);
        self.database
            .compact_range_cf::<&[u8], &[u8]>(&blobs, None, None);
        Ok(())
    }

    pub(crate) fn flush(&self) -> Result<()> {
        self.database.flush_wal(true)?;
        Ok(())
    }

    fn ensure_head(&self, expected: &Head) -> Result<()> {
        let actual = self.head()?;
        if actual.document != expected.document || actual.revision != expected.revision {
            return Err(Error::RevisionConflict {
                expected: expected.revision,
                actual: actual.revision,
            });
        }
        Ok(())
    }

    fn cf(&self, name: &str) -> Result<Arc<BoundColumnFamily<'_>>> {
        self.database.cf_handle(name).ok_or(Error::NotAProject)
    }
}

fn open_database(path: &Path, cache_size: usize, create: bool) -> Result<(Database, Cache)> {
    let cache = Cache::new_lru_cache(cache_size);
    let mut database = RocksOptions::default();
    database.create_if_missing(create);
    database.create_missing_column_families(create);
    database.set_atomic_flush(true);
    database.set_avoid_unnecessary_blocking_io(true);
    database.set_db_write_buffer_size(64 * 1024 * 1024);
    database.set_bytes_per_sync(1024 * 1024);
    database.set_wal_bytes_per_sync(1024 * 1024);
    database.set_keep_log_file_num(2);
    let parallelism = std::thread::available_parallelism()
        .map_or(2, std::num::NonZeroUsize::get)
        .min(i32::MAX as usize) as i32;
    database.increase_parallelism(parallelism);
    database.set_max_background_jobs(parallelism.max(2));

    let column_families = [
        ColumnFamilyDescriptor::new(DOCUMENT_CF, column_options(&cache, 4 * 1024 * 1024)),
        ColumnFamilyDescriptor::new(HISTORY_CF, column_options(&cache, 16 * 1024 * 1024)),
        ColumnFamilyDescriptor::new(BLOB_INDEX_CF, column_options(&cache, 8 * 1024 * 1024)),
        ColumnFamilyDescriptor::new(BLOBS_CF, blob_options(&cache)),
    ];
    Ok((
        Database::open_cf_descriptors(&database, path, column_families)?,
        cache,
    ))
}

fn column_options(cache: &Cache, write_buffer: usize) -> RocksOptions {
    let mut blocks = BlockBasedOptions::default();
    blocks.set_block_cache(cache);
    blocks.set_cache_index_and_filter_blocks(true);
    let mut options = RocksOptions::default();
    options.set_block_based_table_factory(&blocks);
    options.set_compression_type(DBCompressionType::Lz4);
    options.set_write_buffer_size(write_buffer);
    options.set_max_write_buffer_number(3);
    options
}

fn blob_options(cache: &Cache) -> RocksOptions {
    let mut options = column_options(cache, 32 * 1024 * 1024);
    options.set_enable_blob_files(true);
    options.set_min_blob_size(0);
    options.set_blob_file_size(256 * 1024 * 1024);
    options.set_blob_compression_type(DBCompressionType::None);
    options.set_enable_blob_gc(true);
    options.set_blob_gc_age_cutoff(0.25);
    options.set_blob_gc_force_threshold(0.75);
    options
}

fn history_key(revision: Revision) -> [u8; 8] {
    revision.get().to_be_bytes()
}

fn decode_revision(bytes: &[u8]) -> Result<Revision> {
    let bytes: [u8; 8] = bytes
        .try_into()
        .map_err(|_| Error::invalid("history key has an invalid width"))?;
    Ok(Revision::new(u64::from_be_bytes(bytes)))
}

fn decode_blob_id(bytes: &[u8]) -> Result<BlobId> {
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| Error::invalid("blob key has an invalid width"))?;
    Ok(BlobId::from_bytes(bytes))
}

fn decode_blob_size(bytes: &[u8]) -> Result<u64> {
    let bytes: [u8; 8] = bytes
        .try_into()
        .map_err(|_| Error::invalid("blob index value has an invalid width"))?;
    Ok(u64::from_be_bytes(bytes))
}

fn checked_blob(id: BlobId, bytes: &[u8]) -> Result<Arc<[u8]>> {
    if BlobId::for_bytes(bytes) != id {
        return Err(Error::invalid(format!("blob {id} has invalid content")));
    }
    Ok(Arc::from(bytes))
}

fn engines() -> &'static Mutex<HashMap<PathBuf, Weak<Engine>>> {
    static ENGINES: OnceLock<Mutex<HashMap<PathBuf, Weak<Engine>>>> = OnceLock::new();
    ENGINES.get_or_init(|| Mutex::new(HashMap::new()))
}
