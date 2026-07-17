use std::{path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use uuid::Uuid;

use crate::{Error, Result, Revision};

pub(crate) const SCHEMA_VERSION: u32 = 1;

pub(crate) struct ProjectRow {
    pub project_id: Uuid,
    pub head: Revision,
    pub checkpoint: Option<Revision>,
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
    if disk {
        connection.pragma_update(None, "journal_mode", "WAL")?;
    }
    Ok(())
}

pub(crate) fn create_schema(connection: &Connection, project_id: Uuid) -> Result<()> {
    connection.execute_batch(
        "
        CREATE TABLE project (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            schema_version INTEGER NOT NULL,
            project_id BLOB NOT NULL CHECK (length(project_id) = 16),
            head_revision INTEGER NOT NULL,
            checkpoint_revision INTEGER
        );

        CREATE TABLE commits (
            revision INTEGER PRIMARY KEY,
            parent_revision INTEGER NOT NULL,
            command_id BLOB NOT NULL UNIQUE CHECK (length(command_id) = 16),
            command_hash BLOB NOT NULL CHECK (length(command_hash) = 32),
            forward_batch BLOB NOT NULL,
            blob_refs BLOB NOT NULL,
            checkpoint BLOB
        );

        CREATE TABLE blobs (
            id BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 32),
            bytes BLOB NOT NULL
        ) WITHOUT ROWID;
        ",
    )?;
    connection.execute(
        "INSERT INTO project (
            singleton, schema_version, project_id, head_revision, checkpoint_revision
         ) VALUES (1, ?1, ?2, 0, NULL)",
        rusqlite::params![SCHEMA_VERSION, project_id.as_bytes().as_slice()],
    )?;
    Ok(())
}

pub(crate) fn project(connection: &Connection) -> Result<ProjectRow> {
    let row = connection
        .query_row(
            "SELECT schema_version, project_id, head_revision, checkpoint_revision
             FROM project WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(Error::NotAProject)?;
    if row.0 != SCHEMA_VERSION {
        return Err(Error::UnsupportedSchema(row.0));
    }
    let project_id = Uuid::from_slice(&row.1).map_err(|_| Error::NotAProject)?;
    Ok(ProjectRow {
        project_id,
        head: revision_from_sql(row.2)?,
        checkpoint: row.3.map(revision_from_sql).transpose()?,
    })
}

pub(crate) fn revision_to_sql(revision: Revision) -> Result<i64> {
    i64::try_from(revision.get()).map_err(|_| Error::invalid("revision exceeds SQLite INTEGER"))
}

pub(crate) fn revision_from_sql(revision: i64) -> Result<Revision> {
    let revision = u64::try_from(revision).map_err(|_| Error::NotAProject)?;
    Ok(Revision::new(revision))
}
