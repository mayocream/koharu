use std::io;

use anyhow::Result;
use flate2::read::DeflateDecoder;

use crate::http::http_client;

// ZIP signatures and constants
const SIG_EOCD: [u8; 4] = [0x50, 0x4b, 0x05, 0x06]; // End Of Central Directory
const SIG_CFH: [u8; 4] = [0x50, 0x4b, 0x01, 0x02]; // Central Directory File Header
const SIG_LFH: [u8; 4] = [0x50, 0x4b, 0x03, 0x04]; // Local File Header
const EOCD_MIN_LEN: usize = 22; // Minimum EOCD size
const CFH_FIXED_LEN: usize = 46; // CFH fixed-length portion
const LFH_FIXED_LEN: u64 = 30; // LFH fixed-length portion

// EOCD field offsets (relative to EOCD start)
const EOCD_OFF_CD_SIZE: usize = 12; // u32: size of central directory
const EOCD_OFF_CD_OFFSET: usize = 16; // u32: offset of central directory

// CFH offsets (relative to CFH start)
const CFH_OFF_COMP_METHOD: usize = 10; // u16
const CFH_OFF_COMP_SIZE: usize = 20; // u32
const CFH_OFF_NAME_LEN: usize = 28; // u16
const CFH_OFF_EXTRA_LEN: usize = 30; // u16
const CFH_OFF_COMMENT_LEN: usize = 32; // u16
const CFH_OFF_LFH_OFFSET: usize = 42; // u32

// LFH offsets (relative to LFH start)
const LFH_OFF_NAME_LEN: usize = 26; // u16
const LFH_OFF_EXTRA_LEN: usize = 28; // u16

#[inline]
fn le_u16(bytes: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([bytes[off], bytes[off + 1]])
}

#[inline]
fn le_u32(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct RecordEntry {
    pub path: String,
    pub hash: Option<String>, // e.g., Some("sha256-<urlsafe_b64>") or None
    pub size: Option<u64>,
}

pub async fn fetch_record(url: &str) -> Result<Vec<RecordEntry>> {
    // 1) Locate EOCD and central directory
    let (cd_offset, cd_size) = http_zip_eocd_and_cd(url).await?;
    let cd_bytes = http_get_range(url, cd_offset, cd_offset + cd_size - 1).await?;
    let (lh_off, comp_size, comp_method) = parse_central_directory_for_record(&cd_bytes)?;

    // 2) Read local header to compute exact data offset
    let lh_fixed = http_get_range(url, lh_off, lh_off + LFH_FIXED_LEN - 1).await?;
    if lh_fixed.len() < LFH_FIXED_LEN as usize || lh_fixed[0..4] != SIG_LFH {
        anyhow::bail!("bad local file header");
    }
    let name_len = le_u16(&lh_fixed, LFH_OFF_NAME_LEN) as u64;
    let extra_len = le_u16(&lh_fixed, LFH_OFF_EXTRA_LEN) as u64;
    let data_off = lh_off + LFH_FIXED_LEN + name_len + extra_len;
    let data_end = data_off + comp_size as u64 - 1;

    // 3) Fetch and decode RECORD (deflate only, method 8)
    let comp = http_get_range(url, data_off, data_end).await?;
    if comp_method != 8 {
        anyhow::bail!("RECORD compression method unsupported: {comp_method}");
    }
    let mut dec = DeflateDecoder::new(std::io::Cursor::new(comp));
    let mut rec_csv = Vec::new();
    io::copy(&mut dec, &mut rec_csv)?;

    // 4) Parse CSV for all entries
    parse_record_csv(&rec_csv)
}

fn parse_central_directory_for_record(cd: &[u8]) -> Result<(u64, u32, u16)> {
    let mut i = 0usize;
    while i + CFH_FIXED_LEN <= cd.len() {
        if cd[i..i + 4] != SIG_CFH {
            i += 1;
            continue;
        }

        let comp_method = le_u16(cd, i + CFH_OFF_COMP_METHOD);
        let comp_size = le_u32(cd, i + CFH_OFF_COMP_SIZE);
        let name_len = le_u16(cd, i + CFH_OFF_NAME_LEN);
        let extra_len = le_u16(cd, i + CFH_OFF_EXTRA_LEN);
        let comment_len = le_u16(cd, i + CFH_OFF_COMMENT_LEN);
        let lh_off = le_u32(cd, i + CFH_OFF_LFH_OFFSET) as u64;

        let name_start = i + CFH_FIXED_LEN;
        let name_end = name_start + name_len as usize;
        if name_end > cd.len() {
            anyhow::bail!("bad central directory entry");
        }
        let name = &cd[name_start..name_end];
        let name_str = std::str::from_utf8(name).unwrap_or("");
        if name_str.ends_with("/RECORD") && name_str.contains(".dist-info/") {
            if comp_size == u32::MAX {
                anyhow::bail!("zip64 not supported for RECORD");
            }
            return Ok((lh_off, comp_size, comp_method));
        }

        i = name_end + extra_len as usize + comment_len as usize;
    }
    anyhow::bail!("RECORD not found")
}

async fn http_zip_eocd_and_cd(url: &str) -> Result<(u64, u64)> {
    // Fetch last ~70KiB to find EOCD (max comment is 64KiB; add slack)
    let tail = http_get_tail(url, 70 * 1024).await?;
    let mut found = None;
    for i in (0..=tail.len().saturating_sub(EOCD_MIN_LEN)).rev() {
        if tail[i..i + 4] == SIG_EOCD {
            found = Some(i);
            break;
        }
    }
    let pos = found.ok_or_else(|| anyhow::anyhow!("zip EOCD not found"))?;
    let eocd = &tail[pos..];
    if eocd.len() < EOCD_MIN_LEN {
        anyhow::bail!("truncated EOCD");
    }
    let cd_size = le_u32(eocd, EOCD_OFF_CD_SIZE) as u64;
    let cd_off = le_u32(eocd, EOCD_OFF_CD_OFFSET) as u64;
    Ok((cd_off, cd_size))
}

async fn http_get_tail(url: &str, nbytes: usize) -> Result<Vec<u8>> {
    // Use HEAD to get content length, then request [len-n .. len-1]
    let len = head_content_length(url).await?;
    let start = len.saturating_sub(nbytes as u64);
    http_get_range(url, start, len.saturating_sub(1)).await
}

async fn http_get_range(url: &str, start: u64, end_inclusive: u64) -> Result<Vec<u8>> {
    let resp = http_client()
        .get(url)
        .header(
            reqwest::header::RANGE,
            format!("bytes={}-{}", start, end_inclusive),
        )
        .send()
        .await?;
    if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        anyhow::bail!("server did not honor range: {}", resp.status());
    }
    Ok(resp.bytes().await?.to_vec())
}

async fn head_content_length(url: &str) -> Result<u64> {
    let resp = http_client().head(url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("HEAD failed: {} with url {}", resp.status(), url);
    }
    let len = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| anyhow::anyhow!("missing Content-Length"))?;
    Ok(len)
}

fn parse_record_csv(csv_bytes: &[u8]) -> Result<Vec<RecordEntry>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(csv_bytes);
    let mut out = Vec::new();
    for rec in rdr.records() {
        let r = rec?;
        if r.is_empty() {
            continue;
        }
        let path = r.get(0).unwrap_or("").to_string();
        let hash = r.get(1).and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        });
        let size = r.get(2).and_then(|s| s.parse::<u64>().ok());
        out.push(RecordEntry { path, hash, size });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_pip_wheels() -> Result<()> {
        async fn pick_wheel_url(pkg: &str, tag: &str) -> Result<String> {
            let meta_url = format!("https://pypi.org/pypi/{pkg}/json");
            let resp = http_client().get(&meta_url).send().await?;
            let json: serde_json::Value = resp.json().await?;
            let urls = json
                .get("urls")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("bad json: urls"))?;

            urls.iter()
                .filter_map(|f| {
                    let filename = f.get("filename").and_then(|v| v.as_str())?;
                    let file_url = f.get("url").and_then(|v| v.as_str())?;
                    if !filename.ends_with(".whl") {
                        return None;
                    }
                    let matches = if tag == "manylinux" {
                        filename.contains("manylinux") && filename.contains("x86_64")
                    } else {
                        filename.contains(tag)
                    };
                    if matches {
                        Some(file_url.to_string())
                    } else {
                        None
                    }
                })
                .next()
                .ok_or_else(|| anyhow::anyhow!("no suitable wheel for tag {tag}"))
        }

        for pkg in crate::dylib::PACKAGES {
            for tag in ["win_amd64", "manylinux"] {
                let url = pick_wheel_url(pkg, tag).await?;
                let entries = fetch_record(&url).await?;
                assert!(
                    !entries.is_empty(),
                    "{} {}: record entries should not be empty",
                    pkg,
                    tag
                );
                assert!(
                    entries.iter().any(|e| e.path.ends_with("/RECORD")),
                    "{} {}: RECORD file should be present",
                    pkg,
                    tag
                );
                assert!(
                    entries.iter().any(|e| e.size.is_some()),
                    "{} {}: some RECORD entries should have sizes",
                    pkg,
                    tag
                );
            }
        }
        Ok(())
    }
}
