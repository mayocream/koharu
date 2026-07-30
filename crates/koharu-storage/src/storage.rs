use std::{path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::{DocumentId, Error, Result, Revision};

pub(crate) const SCHEMA_VERSION: u32 = 1;
const APPLICATION_ID: i64 = 0x4b485354; // KHST

pub(crate) struct MetaRow {
    pub(crate) document: DocumentId,
    pub(crate) head: Revision,
    pub(crate) checkpoint_revision: Revision,
    pub(crate) checkpoint: Vec<u8>,
    pub(crate) commits_since_checkpoint: u64,
    pub(crate) bytes_since_checkpoint: u64,
}

pub(crate) fn create_disk(
    path: &Path,
    timeout: Duration,
    synchronous_full: bool,
) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure(&connection, timeout, true, synchronous_full)?;
    Ok(connection)
}

pub(crate) fn open_disk(
    path: &Path,
    timeout: Duration,
    synchronous_full: bool,
) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure(&connection, timeout, true, synchronous_full)?;
    Ok(connection)
}

pub(crate) fn open_memory(
    timeout: Duration,
    synchronous_full: bool,
) -> Result<(Connection, String)> {
    let uri = format!(
        "file:koharu-storage-{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4()
    );
    let connection = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure(&connection, timeout, false, synchronous_full)?;
    Ok((connection, uri))
}

fn configure(
    connection: &Connection,
    timeout: Duration,
    disk: bool,
    synchronous_full: bool,
) -> Result<()> {
    connection.busy_timeout(timeout)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(
        None,
        "synchronous",
        if synchronous_full { "FULL" } else { "NORMAL" },
    )?;
    if disk {
        connection.pragma_update(None, "journal_mode", "WAL")?;
    }
    Ok(())
}

pub(crate) fn create_schema(
    connection: &Connection,
    document: DocumentId,
    checkpoint: &[u8],
) -> Result<()> {
    connection.pragma_update(None, "application_id", APPLICATION_ID)?;
    connection.execute_batch(
        "
        CREATE TABLE meta (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            schema_version INTEGER NOT NULL,
            document_id BLOB NOT NULL CHECK (length(document_id) = 16),
            head_revision INTEGER NOT NULL CHECK (head_revision >= 0),
            checkpoint_revision INTEGER NOT NULL CHECK (checkpoint_revision >= 0),
            checkpoint BLOB NOT NULL,
            commits_since_checkpoint INTEGER NOT NULL CHECK (commits_since_checkpoint >= 0),
            bytes_since_checkpoint INTEGER NOT NULL CHECK (bytes_since_checkpoint >= 0)
        );

        CREATE TABLE commits (
            revision INTEGER PRIMARY KEY CHECK (revision > 0),
            parent_revision INTEGER NOT NULL CHECK (parent_revision >= 0),
            label TEXT,
            payload BLOB NOT NULL
        );

        CREATE TABLE blobs (
            id BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 32),
            byte_len INTEGER NOT NULL CHECK (byte_len >= 0),
            bytes BLOB NOT NULL,
            CHECK (length(bytes) = byte_len)
        ) WITHOUT ROWID;

        CREATE TABLE commit_blobs (
            revision INTEGER NOT NULL,
            blob_id BLOB NOT NULL,
            PRIMARY KEY (revision, blob_id),
            FOREIGN KEY (revision) REFERENCES commits(revision) ON DELETE CASCADE,
            FOREIGN KEY (blob_id) REFERENCES blobs(id)
        ) WITHOUT ROWID;
        CREATE INDEX commit_blobs_blob ON commit_blobs(blob_id);
        ",
    )?;
    connection.execute(
        "INSERT INTO meta VALUES (1, ?1, ?2, 0, 0, ?3, 0, 0)",
        rusqlite::params![SCHEMA_VERSION, document.as_uuid().as_bytes(), checkpoint],
    )?;
    Ok(())
}

pub(crate) fn meta(connection: &Connection) -> Result<MetaRow> {
    let application_id: i64 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if application_id != APPLICATION_ID {
        return Err(Error::NotADocument);
    }
    let row = connection
        .query_row(
            "SELECT schema_version, document_id, head_revision, checkpoint_revision,
                    checkpoint, commits_since_checkpoint, bytes_since_checkpoint
             FROM meta WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )
        .optional()?
        .ok_or(Error::NotADocument)?;
    if row.0 != SCHEMA_VERSION {
        return Err(Error::UnsupportedSchema(row.0));
    }
    let document = uuid::Uuid::from_slice(&row.1).map_err(|_| Error::NotADocument)?;
    Ok(MetaRow {
        document: document.into(),
        head: revision_from_sql(row.2)?,
        checkpoint_revision: revision_from_sql(row.3)?,
        checkpoint: row.4,
        commits_since_checkpoint: nonnegative(row.5)?,
        bytes_since_checkpoint: nonnegative(row.6)?,
    })
}

pub(crate) fn head(connection: &Connection) -> Result<Revision> {
    connection
        .query_row(
            "SELECT head_revision FROM meta WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or(Error::NotADocument)
        .and_then(revision_from_sql)
}

pub(crate) fn revision_to_sql(revision: Revision) -> Result<i64> {
    i64::try_from(revision.get()).map_err(|_| Error::invalid("revision exceeds SQLite INTEGER"))
}

pub(crate) fn revision_from_sql(revision: i64) -> Result<Revision> {
    u64::try_from(revision)
        .map(Revision::new)
        .map_err(|_| Error::NotADocument)
}

fn nonnegative(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::NotADocument)
}
