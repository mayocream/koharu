use std::{path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::{Error, ProjectId, Result, Revision};

pub(crate) const SCHEMA_VERSION: u32 = 1;

pub(crate) struct ProjectRow {
    pub id: ProjectId,
    pub head: Revision,
    pub checkpoint_revision: Revision,
    pub checkpoint: Vec<u8>,
}

pub(crate) fn create_disk(path: &Path, timeout: Duration) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure(&connection, timeout, true)?;
    Ok(connection)
}

pub(crate) fn open_disk(path: &Path, timeout: Duration) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure(&connection, timeout, true)?;
    Ok(connection)
}

pub(crate) fn open_memory(timeout: Duration) -> Result<Connection> {
    let connection = Connection::open_in_memory()?;
    configure(&connection, timeout, false)?;
    Ok(connection)
}

fn configure(connection: &Connection, timeout: Duration, disk: bool) -> Result<()> {
    connection.busy_timeout(timeout)?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.pragma_update(None, "foreign_keys", true)?;
    if disk {
        connection.pragma_update(None, "journal_mode", "WAL")?;
    }
    Ok(())
}

pub(crate) fn create_schema(
    connection: &Connection,
    id: ProjectId,
    checkpoint: &[u8],
) -> Result<()> {
    connection.execute_batch(
        "
        CREATE TABLE project (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            schema_version INTEGER NOT NULL,
            id BLOB NOT NULL CHECK (length(id) = 16),
            head_revision INTEGER NOT NULL,
            checkpoint_revision INTEGER NOT NULL,
            checkpoint BLOB NOT NULL
        );

        CREATE TABLE commits (
            revision INTEGER PRIMARY KEY,
            parent_revision INTEGER NOT NULL,
            changes BLOB NOT NULL
        );

        CREATE TABLE blobs (
            id BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 32),
            bytes BLOB NOT NULL
        ) WITHOUT ROWID;
        ",
    )?;
    connection.execute(
        "INSERT INTO project VALUES (1, ?1, ?2, 0, 0, ?3)",
        rusqlite::params![SCHEMA_VERSION, id.as_uuid().as_bytes(), checkpoint],
    )?;
    Ok(())
}

pub(crate) fn project(connection: &Connection) -> Result<ProjectRow> {
    let row = connection
        .query_row(
            "SELECT schema_version, id, head_revision, checkpoint_revision, checkpoint
             FROM project WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or(Error::NotAProject)?;
    if row.0 != SCHEMA_VERSION {
        return Err(Error::UnsupportedSchema(row.0));
    }
    let id = uuid::Uuid::from_slice(&row.1).map_err(|_| Error::NotAProject)?;
    Ok(ProjectRow {
        id: id.into(),
        head: revision_from_sql(row.2)?,
        checkpoint_revision: revision_from_sql(row.3)?,
        checkpoint: row.4,
    })
}

pub(crate) fn head(connection: &Connection) -> Result<Revision> {
    let head = connection
        .query_row(
            "SELECT head_revision FROM project WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or(Error::NotAProject)?;
    revision_from_sql(head)
}

pub(crate) fn revision_to_sql(revision: Revision) -> Result<i64> {
    i64::try_from(revision.get()).map_err(|_| Error::invalid("revision exceeds SQLite INTEGER"))
}

pub(crate) fn revision_from_sql(revision: i64) -> Result<Revision> {
    u64::try_from(revision)
        .map(Revision::new)
        .map_err(|_| Error::NotAProject)
}
