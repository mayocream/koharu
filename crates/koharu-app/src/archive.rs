//! ZIP helpers for multi-file transport exports.

use std::io::{Cursor, Write};

use anyhow::Result;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const BEST_DEFLATE_LEVEL: i64 = 264;

/// Pack `(filename, bytes)` pairs into a maximally-compressed ZIP in memory.
pub fn zip_files_to_bytes(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(BEST_DEFLATE_LEVEL));
        for (name, bytes) in files {
            zip.start_file(name, options)?;
            zip.write_all(bytes)?;
        }
        zip.finish()?;
    }
    Ok(cursor.into_inner())
}
