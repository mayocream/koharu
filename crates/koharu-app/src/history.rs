//! Linear undo/redo history + append-only durable op log.
//!
//! Two concerns, deliberately separated:
//!   1. **Durability log** — `history.log`: each applied op fsynced before ack
//!      so a crash loses at most the op currently being written.
//!   2. **Undo/redo stacks** — in-memory only; Cmd+Z within a session.
//!
//! Undo/redo are themselves logged ops: when the user undoes, we apply the
//! inverse and append it to the log as a normal op. Replay on open always
//! produces the post-undo state. No special entry type.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use koharu_core::{Op, Scene};
use serde::{Deserialize, Serialize};

/// Default cap for the in-memory undo stack. The log on disk is not capped —
/// it's compacted on snapshot.
const DEFAULT_UNDO_LIMIT: usize = 500;

// ---------------------------------------------------------------------------
// Log frames
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct LogFrame {
    epoch: u64,
    op: Op,
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

pub struct History {
    log_path: PathBuf,
    log: BufWriter<File>,
    epoch: u64,
    undo_stack: VecDeque<Op>,
    redo_stack: Vec<Op>,
    limit: usize,
}

impl History {
    /// Open the log at `path`, creating it if missing. Caller is expected to
    /// have already replayed any existing frames (see `Self::replay`).
    pub fn open(path: impl Into<PathBuf>, epoch: u64) -> Result<Self> {
        let log_path = path.into();
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&log_path)
            .with_context(|| format!("open history log {}", log_path.display()))?;
        Ok(Self {
            log_path,
            log: BufWriter::new(file),
            epoch,
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            limit: DEFAULT_UNDO_LIMIT,
        })
    }

    /// Override the in-memory undo-stack cap.
    pub fn with_undo_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Apply an op to the scene, fsync a frame to disk, push to the undo stack.
    pub fn apply(&mut self, scene: &mut Scene, mut op: Op) -> Result<u64> {
        op.apply(scene).context("apply op to scene")?;
        self.epoch += 1;
        self.write_frame(&op)?;
        self.push_undo(op);
        self.redo_stack.clear();
        Ok(self.epoch)
    }

    /// Undo the most recent op. Applies its inverse, records the inverse in
    /// the log, and moves the original onto the redo stack. Returns the new
    /// epoch + the inverse op that was just applied (so the RPC layer can
    /// broadcast it for clients to patch their mirrors without refetching).
    pub fn undo(&mut self, scene: &mut Scene) -> Result<Option<(u64, Op)>> {
        let Some(original) = self.undo_stack.pop_back() else {
            return Ok(None);
        };
        let mut inverse = original.inverse();
        inverse.apply(scene).context("apply inverse op")?;
        self.epoch += 1;
        self.write_frame(&inverse)?;
        let inverse_out = inverse.clone();
        self.redo_stack.push(original);
        Ok(Some((self.epoch, inverse_out)))
    }

    /// Re-apply the most recent undo. Symmetric with `undo`. Returns the new
    /// epoch + the op that was just re-applied.
    pub fn redo(&mut self, scene: &mut Scene) -> Result<Option<(u64, Op)>> {
        let Some(mut op) = self.redo_stack.pop() else {
            return Ok(None);
        };
        op.apply(scene).context("re-apply op")?;
        self.epoch += 1;
        self.write_frame(&op)?;
        let applied = op.clone();
        self.push_undo(op);
        Ok(Some((self.epoch, applied)))
    }

    /// Truncate the log after a snapshot has been committed.
    /// Caller must have already fsynced the snapshot file.
    pub fn truncate_log(&mut self) -> Result<()> {
        self.log.flush()?;
        self.log.get_ref().sync_all()?;
        // Reopen to truncate; BufWriter's underlying file handle is append-only.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(true)
            .open(&self.log_path)
            .with_context(|| format!("truncate history log {}", self.log_path.display()))?;
        file.sync_all()?;
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .open(&self.log_path)?;
        self.log = BufWriter::new(file);
        Ok(())
    }

    // --- internals ---------------------------------------------------------

    fn write_frame(&mut self, op: &Op) -> Result<()> {
        let frame = LogFrame {
            epoch: self.epoch,
            op: op.clone(),
        };
        let bytes = postcard::to_allocvec(&frame).context("encode log frame")?;
        let len = u32::try_from(bytes.len()).context("log frame too large")?;
        self.log.write_all(&len.to_le_bytes())?;
        self.log.write_all(&bytes)?;
        self.log.flush()?;
        self.log.get_ref().sync_data()?;
        Ok(())
    }

    fn push_undo(&mut self, op: Op) {
        self.undo_stack.push_back(op);
        while self.undo_stack.len() > self.limit {
            self.undo_stack.pop_front();
        }
    }
}

// ---------------------------------------------------------------------------
// Replay — called once on project open, before a `History` is constructed.
// ---------------------------------------------------------------------------

/// Replay each frame in `log_path` with epoch greater than `start_epoch`
/// against `scene`. Returns the final epoch seen.
pub fn replay(log_path: &Path, start_epoch: u64, scene: &mut Scene) -> Result<u64> {
    if !log_path.exists() {
        return Ok(start_epoch);
    }
    let file =
        File::open(log_path).with_context(|| format!("open history log {}", log_path.display()))?;
    let mut reader = BufReader::new(file);
    let mut epoch = start_epoch;
    loop {
        let mut len_buf = [0u8; 4];
        match reader.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(anyhow::Error::new(e).context("read log frame length")),
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        match reader.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // Truncated frame (likely crash mid-write) — stop cleanly.
                tracing::warn!(
                    path = %log_path.display(),
                    expected_len = len,
                    "truncated trailing frame in history log; discarding"
                );
                break;
            }
            Err(e) => return Err(anyhow::Error::new(e).context("read log frame body")),
        }
        let frame: LogFrame = match postcard::from_bytes(&buf) {
            Ok(frame) => frame,
            Err(err) => {
                tracing::warn!(
                    path = %log_path.display(),
                    error = %err,
                    "undecodable frame in history log; stopping replay"
                );
                break;
            }
        };
        if frame.epoch > epoch {
            let mut op = frame.op;
            op.apply(scene).context("replay op")?;
            epoch = frame.epoch;
        }
    }
    // Seek to end so subsequent appends go after the last valid frame.
    let _ = reader.seek(SeekFrom::End(0));
    Ok(epoch)
}
