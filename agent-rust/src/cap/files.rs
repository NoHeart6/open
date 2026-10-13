use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use walkdir::WalkDir;

fn guard(path: &str) -> Result<std::path::PathBuf> {
    let p = Path::new(path);
    let canon = p.canonicalize().or_else(|_| Ok::<_, anyhow::Error>(p.to_path_buf()))?;
    Ok(canon)
}

pub fn list(path: String) -> Result<Value> {
    let dir = guard(&path)?;
    let mut entries = vec![];
    for e in std::fs::read_dir(&dir).with_context(|| format!("read_dir {path}"))? {
        let e = e?;
        let md = e.metadata()?;
        entries.push(json!({"name": e.file_name().to_string_lossy(),"is_dir": md.is_dir(),"size": md.len()}));
    }
    Ok(json!({"path": dir.to_string_lossy(), "entries": entries}))
}

pub fn read(path: String, max_bytes: u64) -> Result<Value> {
    let p = guard(&path)?;
    let data = std::fs::read(&p).with_context(|| format!("read {path}"))?;
    let truncated = data.len() as u64 > max_bytes;
    let n = (max_bytes as usize).min(data.len());
    Ok(json!({"path": path, "size": data.len(), "truncated": truncated,
        "content": String::from_utf8_lossy(&data[..n])}))
}

pub fn write_atomic(path: String, content: &[u8], overwrite: bool) -> Result<Value> {
    let p = Path::new(&path);
    if p.exists() && !overwrite {
        anyhow::bail!("exists and overwrite=false");
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = p.with_extension("tmp-oc-write");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, p)?;
    Ok(json!({"path": path, "bytes": content.len()}))
}

pub fn manage(action: &str, src: Option<&str>, dst: Option<&str>) -> Result<Value> {
    match action {
        "mkdir" => {
            let d = dst.or(src).ok_or_else(|| anyhow::anyhow!("missing dst"))?;
            std::fs::create_dir_all(d)?;
            Ok(json!({"ok": true}))
        }
        "copy" => {
            let (s, d) = (src_some(src)?, dst_some(dst)?);
            std::fs::copy(s, d)?;
            Ok(json!({"ok": true}))
        }
        "move" | "rename" => {
            let (s, d) = (src_some(src)?, dst_some(dst)?);
            std::fs::rename(s, d)?;
            Ok(json!({"ok": true}))
        }
        "hash" => {
            let s = src_some(src)?;
            let data = std::fs::read(s)?;
            let mut h = Sha256::new();
            h.update(&data);
            Ok(json!({"sha256": hex::encode(h.finalize())}))
        }
        "delete" => {
            let s = src_some(src)?;
            if Path::new(s).is_dir() {
                std::fs::remove_dir_all(s)?;
            } else {
                std::fs::remove_file(s)?;
            }
            Ok(json!({"ok": true}))
        }
        "zip" => {
            let (s, d) = (src_some(src)?, dst_some(dst)?);
            zip_dir(s, d)?;
            Ok(json!({"ok": true}))
        }
        "unzip" => {
            let (s, d) = (src_some(src)?, dst_some(dst)?);
            unzip_file(s, d)?;
            Ok(json!({"ok": true}))
        }
        _ => anyhow::bail!("unknown file_manage action"),
    }
}

fn src_some(s: Option<&str>) -> Result<&str> {
    s.ok_or_else(|| anyhow::anyhow!("missing src"))
}
fn dst_some(s: Option<&str>) -> Result<&str> {
    s.ok_or_else(|| anyhow::anyhow!("missing dst"))
}

fn zip_dir(src: &str, dst: &str) -> Result<()> {
    let f = std::fs::File::create(dst)?;
    let mut z = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default();
    for e in WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let p = e.path();
        if p.is_file() {
            let name = p.strip_prefix(src)?.to_string_lossy().replace('\\', "/");
            z.start_file(name, opts)?;
            z.write_all(&std::fs::read(p)?)?;
        }
    }
    z.finish()?;
    Ok(())
}

fn unzip_file(src: &str, dst: &str) -> Result<()> {
    let f = std::fs::File::open(src)?;
    let mut z = zip::ZipArchive::new(f)?;
    z.extract(dst)?;
    Ok(())
}

pub fn search(root: String, pattern: String, max: usize) -> Result<Value> {
    let mut out = vec![];
    for e in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()).take(max * 10) {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.contains(&pattern) {
            out.push(e.path().to_string_lossy().into_owned());
            if out.len() >= max {
                break;
            }
        }
    }
    Ok(json!({"root": root, "pattern": pattern, "results": out}))
}

use std::io::Write as _;

#[path = "files_tests.rs"]
#[cfg(test)]
mod files_tests;
