use crate::{
    files,
    policy::{self, Policy},
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    before: Option<String>,
    before_hash: Option<String>,
    after_hash: Option<String>,
    directory: bool,
}
#[derive(Serialize, Deserialize)]
struct Undo {
    id: String,
    entries: Vec<Entry>,
    created_at: u64,
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn record_path(state: &Path, id: &str) -> Result<PathBuf> {
    ensure!(
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid undo receipt"
    );
    Ok(state.join("undo").join(format!("{id}.dpapi")))
}
fn snapshot(p: &Policy, path: &Path, after_hash: Option<String>, directory: bool) -> Result<Entry> {
    p.path(&path.to_string_lossy())?;
    let _guard = policy::guard_parent(path)?;
    let (before, before_hash) = if path.exists() {
        ensure!(
            !directory && path.is_file(),
            "Batch path conflicts with an existing directory"
        );
        let mut file = policy::open_file(path)?;
        ensure!(
            file.metadata()?.len() <= 1024 * 1024,
            "Batch rollback file limit is 1 MiB"
        );
        let mut bytes = vec![];
        file.read_to_end(&mut bytes)?;
        (Some(STANDARD.encode(&bytes)), Some(digest(&bytes)))
    } else {
        (None, None)
    };
    Ok(Entry {
        path: path.to_owned(),
        before,
        before_hash,
        after_hash,
        directory,
    })
}
fn revert(p: &Policy, undo: &Undo) -> Result<()> {
    for entry in &undo.entries {
        let path = p.path(&entry.path.to_string_lossy())?;
        let _guard = policy::guard_parent(&path)?;
        if entry.directory {
            if path.exists() {
                policy::reject_links(&path)?;
                ensure!(
                    std::fs::read_dir(&path)?.next().is_none(),
                    "Undo directory contains new data"
                );
            }
        } else {
            let current = if path.exists() {
                Some(files::hash(&path)?)
            } else {
                None
            };
            ensure!(
                current == entry.before_hash || current == entry.after_hash,
                "Undo conflict: a file changed after the batch"
            );
        }
    }
    for entry in undo.entries.iter().rev() {
        let path = p.path(&entry.path.to_string_lossy())?;
        let _guard = policy::guard_parent(&path)?;
        if entry.directory {
            if path.exists() {
                policy::reject_links(&path)?;
                ensure!(
                    std::fs::read_dir(&path)?.next().is_none(),
                    "Undo directory contains new data"
                );
                std::fs::remove_dir(&path)?;
            }
            continue;
        }
        let current = if path.exists() {
            Some(files::hash(&path)?)
        } else {
            None
        };
        if current == entry.before_hash {
            continue;
        }
        ensure!(
            current == entry.after_hash,
            "Undo conflict: a file changed after the batch"
        );
        if let Some(bytes) = &entry.before {
            let bytes = STANDARD.decode(bytes)?;
            ensure!(
                Some(digest(&bytes)) == entry.before_hash,
                "Undo receipt integrity mismatch"
            );
            let mut tmp =
                tempfile::NamedTempFile::new_in(path.parent().context("Missing parent")?)?;
            tmp.write_all(&bytes)?;
            tmp.as_file().sync_all()?;
            if current.is_some() {
                ensure!(
                    Some(files::hash(&path)?) == current,
                    "File changed during undo"
                );
                tmp.persist(&path)?;
            } else {
                tmp.persist_noclobber(&path)?;
            }
        } else if path.exists() {
            let file = policy::open_file_delete(&path)?;
            policy::delete_open_file(&file)?;
        }
    }
    Ok(())
}
pub fn execute(p: &Policy, state: &Path, a: &Value) -> Result<Value> {
    ensure!(p.allow_modify, "Local policy denies batch modification");
    let items = a["items"].as_array().context("Missing batch items")?;
    ensure!(
        !items.is_empty() && items.len() <= 32,
        "Batch supports 1–32 operations"
    );
    let mut entries = vec![];
    let mut touched = HashSet::new();
    let mut total = 0;
    for item in items {
        let op = item["op"].as_str().context("Missing batch operation")?;
        let source = p.path(item["path"].as_str().context("Missing batch path")?)?;
        ensure!(
            touched.insert(source.clone()),
            "Batch operations must use distinct source and destination paths"
        );
        let entry = match op {
            "write" => {
                p.authorize("file_write", item)?;
                let text = item["text"].as_str().context("Missing batch text")?;
                ensure!(text.len() <= 65536, "Batch text exceeds budget");
                let entry = snapshot(p, &source, Some(digest(text.as_bytes())), false)?;
                ensure!(
                    entry.before_hash.as_deref() == item["expected_sha256"].as_str(),
                    "Batch overwrite requires the current SHA-256"
                );
                entry
            }
            "mkdir" => {
                ensure!(!source.exists(), "Directory already exists");
                snapshot(p, &source, None, true)?
            }
            "copy" | "move" | "rename" => {
                p.authorize("file_operation", item)?;
                ensure!(source.is_file(), "Batch source must be a file");
                let hash = files::hash(&source)?;
                let destination = p.path(
                    item["destination"]
                        .as_str()
                        .context("Missing batch destination")?,
                )?;
                ensure!(
                    touched.insert(destination.clone()) && !destination.exists(),
                    "Batch destination conflicts"
                );
                entries.push(snapshot(p, &destination, Some(hash.clone()), false)?);
                snapshot(
                    p,
                    &source,
                    if op == "copy" { Some(hash) } else { None },
                    false,
                )?
            }
            _ => anyhow::bail!(
                "Batch supports write, mkdir, copy, move and rename; trash is a separate consented operation"
            ),
        };
        total += entry.before.as_ref().map_or(0, String::len);
        ensure!(total <= 8 * 1024 * 1024, "Batch rollback budget exceeded");
        entries.push(entry);
    }
    if a["preview"].as_bool().unwrap_or(true) {
        return Ok(
            json!({"preview":true,"operations":items,"conflicts":[],"rollback_available":true,"rollback_bytes":total}),
        );
    }
    let folder = state.join("undo");
    policy::reject_links(&folder)?;
    std::fs::create_dir_all(&folder)?;
    ensure!(
        std::fs::read_dir(&folder)?.count() < 128,
        "Undo history is full"
    );
    let mut id = [0u8; 16];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut id);
    let id = hex::encode(id);
    let undo = Undo {
        id: id.clone(),
        entries,
        created_at: crate::now(),
    };
    let receipt = record_path(state, &id)?;
    crate::storage::save(&receipt, &serde_json::to_vec(&undo)?)?;
    let mut results = vec![];
    for item in items {
        let op = item["op"].as_str().unwrap();
        let mut item = item.clone();
        item["preview"] = json!(false);
        let action = if op == "write" {
            "file_write"
        } else {
            "file_operation"
        };
        match files::execute(p, action, &item) {
            Ok(result) => results.push(result),
            Err(error) => {
                let rollback = revert(p, &undo);
                return Ok(
                    json!({"completed":false,"error":error.to_string(),"rolled_back":rollback.is_ok(),"uncertain":rollback.is_err(),"rollback_error":rollback.err().map(|e|e.to_string()),"undo_id":id,"completed_steps":results.len()}),
                );
            }
        }
    }
    Ok(
        json!({"completed":true,"operations":results,"undo_id":id,"progress":{"completed":items.len(),"total":items.len()}}),
    )
}
pub fn undo(p: &Policy, state: &Path, a: &Value) -> Result<Value> {
    ensure!(p.allow_modify, "Local policy denies undo");
    let path = record_path(
        state,
        a["undo_id"].as_str().context("Missing undo receipt")?,
    )?;
    let undo: Undo = serde_json::from_slice(&crate::storage::load(&path)?)?;
    if a["preview"].as_bool().unwrap_or(true) {
        return Ok(
            json!({"preview":true,"paths":undo.entries.iter().map(|e|&e.path).collect::<Vec<_>>(),"created_at":undo.created_at}),
        );
    }
    if let Err(error) = revert(p, &undo) {
        return Ok(
            json!({"completed":false,"uncertain":true,"error":error.to_string(),"undo_id":a["undo_id"],"receipt_preserved":true}),
        );
    }
    std::fs::remove_file(path)?;
    Ok(json!({"completed":true,"operation":"undo"}))
}
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn batch_undo_refuses_new_data_and_restores_original() -> Result<()> {
        let root = tempfile::tempdir()?;
        let state = tempfile::tempdir()?;
        let p = Policy {
            roots: vec![root.path().into()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: true,
            allow_screenshots: false,
            ask_before_sensitive: true,
        };
        let old = root.path().join("old.txt");
        let new = root.path().join("new.txt");
        std::fs::write(&old, "original")?;
        let result = execute(
            &p,
            state.path(),
            &json!({"preview":false,"items":[{"op":"write","path":old,"text":"updated","expected_sha256":files::hash(&old)?},{"op":"write","path":new,"text":"created"}]}),
        )?;
        ensure!(result["completed"] == true);
        std::fs::write(&new, "user changed it")?;
        let args = json!({"undo_id":result["undo_id"],"preview":false});
        ensure!(undo(&p, state.path(), &args)?["uncertain"] == true);
        ensure!(std::fs::read_to_string(&new)? == "user changed it");
        std::fs::write(&new, "created")?;
        undo(&p, state.path(), &args)?;
        ensure!(std::fs::read_to_string(old)? == "original" && !new.exists());
        Ok(())
    }
}
