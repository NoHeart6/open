use crate::policy::Policy;
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    time::Instant,
};

const MAX_FILE: u64 = 8 * 1024 * 1024;
pub fn hash(path: &Path) -> Result<String> {
    ensure!(
        fs::metadata(path)?.len() <= 64 * 1024 * 1024,
        "Hash file size limit exceeded"
    );
    let _guard = crate::policy::guard_parent(path)?;
    let mut f = crate::policy::open_file(path)?;
    let mut h = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}
fn bounded(path: &Path) -> Result<String> {
    let _guard = crate::policy::guard_parent(path)?;
    ensure!(
        fs::metadata(path)?.len() <= MAX_FILE,
        "File exceeds edit limit"
    );
    let mut file = crate::policy::open_file(path)?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}
pub fn atomic_write(path: &Path, text: &str, expected: Option<&str>) -> Result<()> {
    let _guard = crate::policy::guard_parent(path)?;
    ensure!(text.len() <= MAX_FILE as usize, "Write size limit exceeded");
    if path.exists() {
        ensure!(
            expected.is_some_and(|e| hash(path).is_ok_and(|h| h == e)),
            "Existing file requires matching expected_sha256"
        );
    } else {
        ensure!(expected.is_none(), "Expected existing file is missing");
    }
    let mut tmp = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("No parent directory"))?,
    )?;
    tmp.write_all(text.as_bytes())?;
    tmp.as_file().sync_all()?;
    if expected.is_some() {
        ensure!(
            expected.is_some_and(|e| hash(path).is_ok_and(|h| h == e)),
            "File changed while preparing write"
        );
        tmp.persist(path)?;
    } else {
        tmp.persist_noclobber(path)?;
    }
    Ok(())
}
fn string<'a>(a: &'a Value, key: &str) -> Result<&'a str> {
    a[key]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing {key}"))
}
pub fn execute(policy: &Policy, action: &str, a: &Value) -> Result<Value> {
    let path = policy.path(string(a, "path")?)?;
    let _guard = crate::policy::guard_parent(&path)?;
    match action {
        "file_read" => {
            let limit = a["limit"].as_u64().unwrap_or(16384);
            ensure!((1..=65536).contains(&limit), "Invalid chunk limit");
            let offset = a["offset"].as_u64().unwrap_or(0);
            let mut f = crate::policy::open_file(&path)?;
            let size = f.metadata()?.len();
            f.seek(SeekFrom::Start(offset))?;
            let mut buf = vec![];
            f.take(limit).read_to_end(&mut buf)?;
            let next = offset + buf.len() as u64;
            Ok(
                json!({"text":String::from_utf8_lossy(&buf),"offset":offset,"next_offset":next,"total_bytes":size,"eof":next>=size,"encoding":"UTF-8 lossy at chunk boundaries"}),
            )
        }
        "file_write" => {
            atomic_write(&path, string(a, "text")?, a["expected_sha256"].as_str())?;
            Ok(json!({"sha256":hash(&path)?,"bytes":fs::metadata(path)?.len()}))
        }
        "file_search" => {
            let query = string(a, "query")?;
            ensure!(!query.is_empty() && query.len() <= 256, "Invalid query");
            let content = a["content"].as_bool().unwrap_or(false);
            let offset = a["offset"].as_u64().unwrap_or(0) as usize;
            let limit = a["limit"].as_u64().unwrap_or(50) as usize;
            ensure!(
                offset <= 10000 && (1..=200).contains(&limit),
                "Invalid pagination"
            );
            let start = Instant::now();
            let mut results = vec![];
            let mut matched = 0;
            let mut visited = 0;
            let mut truncated = false;
            for entry in walkdir::WalkDir::new(&path)
                .follow_links(false)
                .max_depth(32)
                .into_iter()
                .filter_entry(|entry| policy.path(&entry.path().to_string_lossy()).is_ok())
            {
                visited += 1;
                if visited > 20000 || start.elapsed().as_secs() > 5 {
                    truncated = true;
                    break;
                }
                let Ok(entry) = entry else { continue };
                if policy.path(&entry.path().to_string_lossy()).is_err() {
                    continue;
                }
                let hit = if content && entry.file_type().is_file() {
                    fs::metadata(entry.path())?.len() <= 1024 * 1024
                        && bounded(entry.path()).is_ok_and(|s| s.contains(query))
                } else {
                    !content && entry.file_name().to_string_lossy().contains(query)
                };
                if hit {
                    if matched >= offset {
                        results.push(entry.path().to_string_lossy().to_string());
                    }
                    matched += 1;
                    if results.len() >= limit {
                        truncated = true;
                        break;
                    }
                }
            }
            Ok(
                json!({"matches":results,"next_offset":offset+results.len(),"truncated":truncated,"visited":visited}),
            )
        }
        "file_operation" => {
            let op = string(a, "op")?;
            if matches!(op, "list" | "metadata" | "hash" | "disk_usage") {
                return match op {
                    "list" => {
                        let entries=fs::read_dir(&path)?.filter_map(Result::ok).filter(|entry| policy.path(&entry.path().to_string_lossy()).is_ok()).take(200).map(|v|json!({"name":v.file_name().to_string_lossy(),"directory":v.file_type().is_ok_and(|t|t.is_dir())})).collect::<Vec<_>>();
                        Ok(json!({"entries":entries,"limit":200}))
                    }
                    "metadata" => {
                        let m = fs::metadata(&path)?;
                        Ok(
                            json!({"bytes":m.len(),"directory":m.is_dir(),"readonly":m.permissions().readonly()}),
                        )
                    }
                    "hash" => Ok(json!({"sha256":hash(&path)?})),
                    _ => {
                        let mut bytes = 0u64;
                        let mut count = 0;
                        for e in walkdir::WalkDir::new(&path)
                            .follow_links(false)
                            .max_depth(32)
                            .into_iter()
                            .filter_entry(|entry| {
                                policy.path(&entry.path().to_string_lossy()).is_ok()
                            })
                        {
                            let Ok(e) = e else { continue };
                            count += 1;
                            ensure!(count <= 20000, "Disk scan entry limit");
                            policy.path(&e.path().to_string_lossy())?;
                            if e.file_type().is_file()
                                && let Ok(metadata) = e.metadata()
                            {
                                bytes += metadata.len()
                            }
                        }
                        Ok(json!({"bytes":bytes,"entries":count}))
                    }
                };
            }
            let destination = a["destination"]
                .as_str()
                .map(|p| policy.path(p))
                .transpose()?;
            let _destination_guard = destination
                .as_ref()
                .map(|p| crate::policy::guard_parent(p))
                .transpose()?;
            let preview = a["preview"].as_bool().unwrap_or(true);
            if preview {
                return Ok(
                    json!({"preview":true,"operation":op,"path":path,"destination":destination,"requires_local_approval":op=="delete" && policy.needs_consent("SENSITIVE")}),
                );
            }
            match op {
                "mkdir" => fs::create_dir(&path)?,
                "delete" => {
                    ensure!(path.is_file(), "Only individual files can be deleted");
                    fs::remove_file(&path)?;
                }
                "copy" | "move" | "rename" => {
                    ensure!(path.is_file(), "Only files supported");
                    let dst = destination.ok_or_else(|| anyhow::anyhow!("Missing destination"))?;
                    ensure!(!dst.exists(), "Destination already exists");
                    ensure!(
                        fs::metadata(&path)?.len() <= 64 * 1024 * 1024,
                        "Copy size limit"
                    );
                    let mut source = if op == "copy" {
                        crate::policy::open_file(&path)?
                    } else {
                        crate::policy::open_file_delete(&path)?
                    };
                    let mut temp = tempfile::NamedTempFile::new_in(
                        dst.parent()
                            .ok_or_else(|| anyhow::anyhow!("Missing parent"))?,
                    )?;
                    std::io::copy(&mut source, &mut temp)?;
                    temp.as_file().sync_all()?;
                    temp.persist_noclobber(&dst)?;
                    if op != "copy" {
                        crate::policy::delete_open_file(&source)?;
                    }
                }
                "edit" => {
                    let text = bounded(&path)?;
                    let old = string(a, "search")?;
                    ensure!(
                        !old.is_empty() && text.matches(old).count() == 1,
                        "Edit requires exactly one literal match"
                    );
                    let new = text.replacen(old, string(a, "replacement")?, 1);
                    atomic_write(&path, &new, a["expected_sha256"].as_str())?;
                }
                "zip" => {
                    ensure!(path.is_file(), "Initial zip operation archives one file");
                    ensure!(fs::metadata(&path)?.len() <= MAX_FILE, "Archive size limit");
                    let dst = destination.ok_or_else(|| anyhow::anyhow!("Missing destination"))?;
                    let mut temp = tempfile::NamedTempFile::new_in(
                        dst.parent()
                            .ok_or_else(|| anyhow::anyhow!("Missing parent"))?,
                    )?;
                    {
                        let mut archive = zip::ZipWriter::new(&mut temp);
                        archive.start_file(
                            path.file_name()
                                .ok_or_else(|| anyhow::anyhow!("Missing name"))?
                                .to_string_lossy(),
                            zip::write::SimpleFileOptions::default(),
                        )?;
                        std::io::copy(&mut crate::policy::open_file(&path)?, &mut archive)?;
                        archive.finish()?;
                    }
                    temp.as_file().sync_all()?;
                    temp.persist_noclobber(dst)?;
                }
                "unzip" => {
                    let dst = destination.ok_or_else(|| anyhow::anyhow!("Missing destination"))?;
                    ensure!(!dst.exists(), "Extraction destination must be new");
                    let mut archive = zip::ZipArchive::new(crate::policy::open_file(&path)?)?;
                    ensure!(archive.len() <= 200, "Too many archive entries");
                    let mut total = 0;
                    // Validate the entire archive before touching the destination.
                    for i in 0..archive.len() {
                        let f = archive.by_index(i)?;
                        let name = f
                            .enclosed_name()
                            .ok_or_else(|| anyhow::anyhow!("ZIP traversal"))?;
                        policy.path(&dst.join(&name).to_string_lossy())?;
                        ensure!(
                            f.unix_mode().unwrap_or(0) & 0o170000 != 0o120000,
                            "ZIP symbolic link prohibited"
                        );
                        total += f.size();
                        ensure!(total <= MAX_FILE, "ZIP expanded size limit");
                    }
                    let temp = tempfile::tempdir_in(
                        dst.parent()
                            .ok_or_else(|| anyhow::anyhow!("Missing parent"))?,
                    )?;
                    for i in 0..archive.len() {
                        let mut f = archive.by_index(i)?;
                        let output = temp.path().join(
                            f.enclosed_name()
                                .ok_or_else(|| anyhow::anyhow!("ZIP traversal"))?,
                        );
                        if f.is_dir() {
                            fs::create_dir_all(output)?;
                        } else {
                            fs::create_dir_all(
                                output
                                    .parent()
                                    .ok_or_else(|| anyhow::anyhow!("Missing parent"))?,
                            )?;
                            let _target_guard = crate::policy::guard_parent(&output)?;
                            let mut target = fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(output)?;
                            std::io::copy(&mut f.by_ref().take(MAX_FILE + 1), &mut target)?;
                            target.sync_all()?;
                        }
                    }
                    ensure!(!dst.exists(), "Destination appeared during extraction");
                    fs::rename(temp.path(), dst)?;
                }
                _ => bail!("Unsupported file operation"),
            }
            Ok(json!({"operation":op,"completed":true}))
        }
        _ => bail!("Unsupported file action"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zip_traversal_is_rejected_before_destination_creation() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let archive = dir.path().join("malicious.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive)?);
        zip.start_file("../escape.txt", zip::write::SimpleFileOptions::default())?;
        zip.write_all(b"escape")?;
        zip.finish()?;
        let p = Policy {
            roots: vec![dir.path().to_path_buf()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: true,
            allow_screenshots: false,
            ask_before_sensitive: true,
        };
        let destination = dir.path().join("output");
        ensure!(
            execute(
                &p,
                "file_operation",
                &json!({"op":"unzip","path":archive,"destination":destination,"preview":false})
            )
            .is_err()
        );
        ensure!(!destination.exists() && !dir.path().join("escape.txt").exists());
        Ok(())
    }
    #[test]
    fn atomic_updates_search_edit_and_archive() -> Result<()> {
        let t = tempfile::tempdir()?;
        let p = Policy {
            roots: vec![t.path().to_path_buf()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: true,
            allow_screenshots: false,
            ask_before_sensitive: true,
        };
        let file = t.path().join("test.txt");
        atomic_write(&file, "one\ntwo", None)?;
        ensure!(atomic_write(&file, "lost", None).is_err());
        ensure!(atomic_write(&file, "lost", Some(&"0".repeat(64))).is_err());
        let h = hash(&file)?;
        atomic_write(&file, "two\nthree", Some(&h))?;
        ensure!(bounded(&file)? == "two\nthree");
        ensure!(
            execute(
                &p,
                "file_search",
                &json!({"path":t.path(),"query":"three","content":true})
            )?["matches"]
                .as_array()
                .unwrap()
                .len()
                == 1
        );
        execute(
            &p,
            "file_operation",
            &json!({"op":"edit","path":file,"search":"three","replacement":"four","expected_sha256":hash(&file)?,"preview":false}),
        )?;
        ensure!(bounded(&file)? == "two\nfour");
        let archive = t.path().join("out.zip");
        execute(
            &p,
            "file_operation",
            &json!({"op":"zip","path":file,"destination":archive,"preview":false}),
        )?;
        let extracted = t.path().join("extracted");
        execute(
            &p,
            "file_operation",
            &json!({"op":"unzip","path":archive,"destination":extracted,"preview":false}),
        )?;
        ensure!(bounded(&extracted.join("test.txt"))? == "two\nfour");
        Ok(())
    }
}
