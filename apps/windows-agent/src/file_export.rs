use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_FILE: u64 = 512 * 1024 * 1024;
const CHUNK: u64 = 48 * 1024;
fn sha(mut f: impl Read, cancel: &AtomicBool) -> Result<String> {
    let mut digest = Sha256::new();
    let mut buf = [0_u8; 131072];
    loop {
        ensure!(!cancel.load(Ordering::SeqCst), "Read cancelled");
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
    }
    Ok(hex::encode(digest.finalize()))
}
pub fn run(policy: &crate::policy::Policy, a: &Value, cancel: Arc<AtomicBool>) -> Result<Value> {
    let target = a["path"].as_str().context("path required")?;
    let path = policy.path(target)?;
    let _guard = crate::policy::guard_parent(&path)?;
    let mut f = crate::policy::open_file(&path)?;
    let total = f.metadata()?.len();
    ensure!(total <= MAX_FILE, "File exceeds 512 MiB");
    let name = path
        .file_name()
        .context("Invalid filename")?
        .to_string_lossy()
        .to_string();
    match a["op"].as_str().unwrap_or("info") {
        "info" => Ok(
            json!({"path":path,"name":name,"total_bytes":total,"sha256":sha(&mut f,&cancel)?,"chunk_bytes":CHUNK}),
        ),
        "chunk" => {
            let offset = a["offset"].as_u64().unwrap_or(0);
            let limit = a["limit"].as_u64().unwrap_or(CHUNK);
            ensure!((1..=CHUNK).contains(&limit), "Chunk size must be 1..48 KiB");
            ensure!(offset <= total, "Offset out of range");
            f.seek(SeekFrom::Start(offset))?;
            let mut buf = vec![0_u8; limit as usize];
            let n = f.read(&mut buf)?;
            buf.truncate(n);
            let next = offset + n as u64;
            Ok(
                json!({"name":name,"total_bytes":total,"offset":offset,"next_offset":next,"eof":next>=total,"bytes":n,"base64":base64::engine::general_purpose::STANDARD.encode(buf)}),
            )
        }
        other => bail!("Unsupported file_export op {other}"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_exact_binary_chunks_and_hash() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("image.bin");
        let bytes: Vec<u8> = (0..113_000).map(|i| ((i * 13) % 256) as u8).collect();
        std::fs::write(&path, &bytes)?;
        let p = crate::policy::Policy {
            roots: vec![dir.path().to_path_buf()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: false,
            allow_screenshots: false,
            ask_before_sensitive: true,
        };
        let c = Arc::new(AtomicBool::new(false));
        let info = run(&p, &json!({"op":"info","path":path}), c.clone())?;
        assert_eq!(info["total_bytes"], bytes.len());
        assert_eq!(info["sha256"], hex::encode(Sha256::digest(&bytes)));
        let mut got = vec![];
        let mut offset = 0;
        loop {
            let v = run(
                &p,
                &json!({"op":"chunk","path":path,"offset":offset,"limit":40000}),
                c.clone(),
            )?;
            got.extend(
                base64::engine::general_purpose::STANDARD.decode(v["base64"].as_str().unwrap())?,
            );
            offset = v["next_offset"].as_u64().unwrap();
            if v["eof"] == true {
                break;
            }
        }
        assert_eq!(got, bytes);
        Ok(())
    }
}
