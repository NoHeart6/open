use crate::policy::{self, Policy};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Receipt {
    id: String,
    original: PathBuf,
    sha256: String,
    bytes: String,
    created_at: u64,
}
fn receipt_path(state: &Path, id: &str) -> Result<PathBuf> {
    ensure!(
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid trash receipt"
    );
    Ok(state.join("trash").join(format!("{id}.dpapi")))
}
pub fn trash(p: &Policy, state: &Path, a: &Value) -> Result<Value> {
    let path = p.path(a["path"].as_str().context("Missing trash path")?)?;
    let _guard = policy::guard_parent(&path)?;
    let mut source = policy::open_file_delete(&path)?;
    ensure!(
        source.metadata()?.len() <= 8 * 1024 * 1024,
        "Recoverable trash supports individual files up to 8 MiB"
    );
    let mut bytes = vec![];
    source.read_to_end(&mut bytes)?;
    let sha = hex::encode(Sha256::digest(&bytes));
    if let Some(expected) = a["expected_sha256"].as_str() {
        ensure!(
            sha == expected,
            "File differs from the expected deletion preview"
        );
    }
    if a["preview"].as_bool().unwrap_or(true) {
        return Ok(
            json!({"preview":true,"path":path,"sha256":sha,"bytes":bytes.len(),"recoverable":true}),
        );
    }
    let folder = state.join("trash");
    policy::reject_links(&folder)?;
    std::fs::create_dir_all(&folder)?;
    ensure!(
        std::fs::read_dir(&folder)?.count() < 128,
        "Trash is full; restore or manage existing receipts locally"
    );
    let mut id = [0u8; 16];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut id);
    let id = hex::encode(id);
    let receipt = Receipt {
        id: id.clone(),
        original: path.clone(),
        sha256: sha.clone(),
        bytes: STANDARD.encode(bytes),
        created_at: crate::now(),
    };
    crate::storage::save(&receipt_path(state, &id)?, &serde_json::to_vec(&receipt)?)?;
    policy::delete_open_file(&source)?;
    drop(source);
    ensure!(!path.exists(), "File deletion did not complete");
    Ok(
        json!({"operation":"trash","completed":true,"receipt_id":id,"sha256":sha,"recoverable":true,"storage":"Encrypted OpenCommander trash; preserved across upgrade/uninstall"}),
    )
}
pub fn execute(p: &Policy, state: &Path, a: &Value) -> Result<Value> {
    let op = a["op"].as_str().context("Missing recovery operation")?;
    if op == "list" {
        let folder = state.join("trash");
        if !folder.exists() {
            return Ok(json!({"receipts":[]}));
        }
        let mut items = vec![];
        for entry in std::fs::read_dir(folder)?.take(128) {
            let r: Receipt = serde_json::from_slice(&crate::storage::load(&entry?.path())?)?;
            if p.path(&r.original.to_string_lossy()).is_ok() {
                items.push(json!({"receipt_id":r.id,"original":r.original,"sha256":r.sha256,"created_at":r.created_at}));
            }
        }
        return Ok(json!({"receipts":items}));
    }
    ensure!(
        op == "restore" && p.allow_modify,
        "Local policy denies restoration"
    );
    let receipt_file = receipt_path(state, a["receipt_id"].as_str().context("Missing receipt")?)?;
    let r: Receipt = serde_json::from_slice(&crate::storage::load(&receipt_file)?)?;
    let target = p.path(&r.original.to_string_lossy())?;
    let _guard = policy::guard_parent(&target)?;
    ensure!(
        !target.exists(),
        "Restore conflict: original path already exists"
    );
    let bytes = STANDARD.decode(r.bytes)?;
    ensure!(
        hex::encode(Sha256::digest(&bytes)) == r.sha256,
        "Trash receipt integrity mismatch"
    );
    if a["preview"].as_bool().unwrap_or(true) {
        return Ok(json!({"preview":true,"path":target,"bytes":bytes.len(),"sha256":r.sha256}));
    }
    let mut tmp = tempfile::NamedTempFile::new_in(target.parent().context("Missing parent")?)?;
    tmp.write_all(&bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist_noclobber(&target)?;
    std::fs::remove_file(receipt_file)?;
    Ok(json!({"operation":"restore","completed":true,"path":target,"sha256":r.sha256}))
}
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn trash_is_recoverable_and_restore_does_not_clobber() -> Result<()> {
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
        let original = root.path().join("fixture.txt");
        std::fs::write(&original, b"recover me")?;
        let removed = trash(&p, state.path(), &json!({"path":original,"preview":false}))?;
        ensure!(!original.exists());
        std::fs::write(&original, "new data")?;
        let restore = json!({"op":"restore","receipt_id":removed["receipt_id"],"preview":false});
        ensure!(execute(&p, state.path(), &restore).is_err());
        ensure!(std::fs::read_to_string(&original)? == "new data");
        std::fs::remove_file(&original)?;
        execute(&p, state.path(), &restore)?;
        ensure!(std::fs::read_to_string(&original)? == "recover me");
        ensure!(
            execute(&p, state.path(), &json!({"op":"list"}))?["receipts"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        Ok(())
    }
}
