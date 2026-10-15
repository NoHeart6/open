use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use futures_util::StreamExt;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CHUNK: usize = 768 * 1024;

#[derive(Serialize, Deserialize)]
struct Transfer {
    id: String,
    total_bytes: u64,
    sha256: Option<String>,
    finished: bool,
}

fn paths(path: &Path) -> (PathBuf, PathBuf) {
    let name = path.file_name().unwrap().to_string_lossy();
    (
        path.with_file_name(format!("{name}.oc-transfer.part")),
        path.with_file_name(format!("{name}.oc-transfer.json")),
    )
}
fn load(path: &Path, id: &str) -> Result<(Transfer, PathBuf, PathBuf)> {
    let (part, meta) = paths(path);
    let t: Transfer = serde_json::from_slice(&fs::read(&meta).context("No active transfer")?)?;
    ensure!(t.id == id, "Transfer ID does not match");
    Ok((t, part, meta))
}
fn read_id(a: &Value) -> Result<&str> {
    let id = a["transfer_id"].as_str().context("transfer_id required")?;
    ensure!(
        id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid transfer ID"
    );
    Ok(id)
}
fn state(t: &Transfer, part: &Path, dest: &Path) -> Value {
    let received = if t.finished {
        fs::metadata(dest).map(|m| m.len()).unwrap_or(t.total_bytes)
    } else {
        fs::metadata(part).map(|m| m.len()).unwrap_or(0)
    };
    json!({"transfer_id":t.id,"received_bytes":received,"total_bytes":t.total_bytes,
      "remaining_bytes":t.total_bytes.saturating_sub(received),"percent":(received as f64 * 100.0 / t.total_bytes as f64),
      "finished":t.finished,"destination":dest})
}
fn checksum(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut b = [0_u8; 131072];
    loop {
        let n = f.read(&mut b)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(hex::encode(h.finalize()))
}
fn begin(p: &Path, a: &Value) -> Result<Value> {
    let size = a["total_bytes"]
        .as_u64()
        .context("total_bytes required for begin")?;
    ensure!(
        (1..=MAX_BYTES).contains(&size),
        "Total bytes must be 1..512 MiB"
    );
    ensure!(!p.exists(), "Destination already exists");
    let sha = a["sha256"].as_str().map(str::to_ascii_lowercase);
    if let Some(ref sha) = sha {
        ensure!(
            sha.len() == 64 && sha.bytes().all(|c| c.is_ascii_hexdigit()),
            "Invalid SHA-256"
        );
    }
    let (part, meta) = paths(p);
    ensure!(!part.exists(), "Partial transfer already exists");
    let mut random = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut random);
    let t = Transfer {
        id: hex::encode(random),
        total_bytes: size,
        sha256: sha,
        finished: false,
    };
    let mut m = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&meta)?;
    if let Err(e) = m.write_all(&serde_json::to_vec(&t)?) {
        drop(m);
        let _ = fs::remove_file(&meta);
        return Err(e.into());
    }
    m.sync_all()?;
    if let Err(e) = OpenOptions::new().write(true).create_new(true).open(&part) {
        let _ = fs::remove_file(meta);
        return Err(e.into());
    }
    Ok(state(&t, &part, p))
}
fn chunk(p: &Path, a: &Value) -> Result<Value> {
    let (t, part, _) = load(p, read_id(a)?)?;
    ensure!(!t.finished, "Transfer already complete");
    let offset = a["offset"].as_u64().context("offset required")?;
    let encoded = a["chunk_base64"]
        .as_str()
        .context("chunk_base64 required")?;
    ensure!(encoded.len() <= 1048576, "Base64 chunk too large");
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_CHUNK,
        "Chunk must be 1..768 KiB"
    );
    ensure!(
        offset.saturating_add(bytes.len() as u64) <= t.total_bytes,
        "Chunk exceeds transfer size"
    );
    let mut f = OpenOptions::new().read(true).write(true).open(&part)?;
    let current = f.metadata()?.len();
    if offset < current && offset + bytes.len() as u64 <= current {
        f.seek(SeekFrom::Start(offset))?;
        let mut previous = vec![0; bytes.len()];
        f.read_exact(&mut previous)?;
        ensure!(
            previous == bytes,
            "Retry chunk differs from already transferred bytes"
        );
        return Ok(state(&t, &part, p));
    }
    ensure!(
        offset == current,
        "Chunk offset does not match current offset (resume with status)"
    );
    f.seek(SeekFrom::Start(current))?;
    f.write_all(&bytes)?;
    f.sync_data()?;
    Ok(state(&t, &part, p))
}
async fn fetch(p: &Path, a: &Value, cancel: &Arc<AtomicBool>, seconds: u64) -> Result<Value> {
    let (t, part, _) = load(p, read_id(a)?)?;
    ensure!(!t.finished, "Transfer already complete");
    let url = a["source_url"].as_str().context("source_url required")?;
    let parsed = reqwest::Url::parse(url)?;
    ensure!(parsed.scheme() == "https", "Transfer source must use HTTPS");
    let mut f = OpenOptions::new().append(true).open(&part)?;
    let start = f.metadata()?.len();
    ensure!(
        start <= t.total_bytes,
        "Partial transfer exceeds declared size"
    );
    if start == t.total_bytes {
        return Ok(state(&t, &part, p));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(seconds.clamp(1, 600)))
        .build()?;
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity");
    if start > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={start}-"));
    }
    let response = request.send().await?.error_for_status()?;
    if start > 0 {
        ensure!(
            response.status() == reqwest::StatusCode::PARTIAL_CONTENT,
            "Remote server does not support safe resume"
        );
        let range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .context("Missing Content-Range")?;
        ensure!(
            range.starts_with(&format!("bytes {start}-")),
            "Wrong Content-Range offset"
        );
    } else {
        ensure!(
            response.status() == reqwest::StatusCode::OK
                || response.status() == reqwest::StatusCode::PARTIAL_CONTENT,
            "Unexpected download status"
        );
    }
    if let Some(len) = response.content_length() {
        ensure!(
            start + len <= t.total_bytes,
            "Server content exceeds expected size"
        );
    }
    let mut stream = response.bytes_stream();
    let mut amount = start;
    while let Some(part_bytes) = stream.next().await {
        ensure!(
            !cancel.load(Ordering::SeqCst),
            "Download cancelled; partial data retained for resume"
        );
        let b = part_bytes?;
        ensure!(
            amount + b.len() as u64 <= t.total_bytes,
            "Download exceeded expected length"
        );
        f.write_all(&b)?;
        amount += b.len() as u64;
    }
    f.sync_all()?;
    Ok(state(&t, &part, p))
}
async fn finish(
    policy: &crate::policy::Policy,
    p: &Path,
    a: &Value,
    cancel: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    let (mut t, part, meta) = load(p, read_id(a)?)?;
    if !t.finished {
        ensure!(!p.exists(), "Destination already exists");
        ensure!(
            fs::metadata(&part)?.len() == t.total_bytes,
            "Transfer incomplete"
        );
        let hash = checksum(&part)?;
        if let Some(ref expected) = t.sha256 {
            ensure!(&hash == expected, "Whole-file SHA-256 mismatch");
        }
        fs::rename(&part, p)?;
        t.sha256 = Some(hash);
        t.finished = true;
        fs::write(&meta, serde_json::to_vec(&t)?)?;
    }
    let mut result = state(&t, &part, p);
    result["sha256"] = json!(t.sha256);
    result["saved"] = json!(true);
    result["inserted_into_wordcraft"] = json!(false);
    if let Some(cli) = a["wordcraft_cli_path"].as_str() {
        let mut params = json!({"path":p.to_str().context("Non-UTF8 destination")?});
        if let Some(w) = a["width_pt"].as_f64() {
            params["width"] = json!(w);
        }
        if let Some(alt) = a["alt_text"].as_str() {
            params["alt"] = json!(alt);
        }
        let wc = json!({"cli_path":cli,"port":a["wordcraft_port"].as_u64().unwrap_or(7981),
          "tool":"execute","arguments":{"command":"insert.picture","params":params}});
        match crate::wordcraft::run(policy, &wc, cancel, seconds).await {
            Ok(v) => {
                result["wordcraft_result"] = v;
                result["inserted_into_wordcraft"] = json!(true);
            }
            Err(e) => {
                result["wordcraft_error"] = json!(format!("{e:#}"));
            }
        }
    }
    Ok(result)
}
pub async fn run(
    policy: &crate::policy::Policy,
    a: &Value,
    cancel: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    ensure!(policy.allow_modify, "Transfer needs modify permission");
    let dest = a["destination"].as_str().context("destination required")?;
    let path = policy.path(dest)?;
    match a["op"].as_str().context("op required")? {
        "begin" => begin(&path, a),
        "status" => {
            let (t, part, _) = load(&path, read_id(a)?)?;
            Ok(state(&t, &part, &path))
        }
        "chunk" => chunk(&path, a),
        "fetch" => fetch(&path, a, &cancel, seconds).await,
        "finish" => finish(policy, &path, a, cancel, seconds).await,
        "cancel" => {
            let (t, part, meta) = load(&path, read_id(a)?)?;
            ensure!(!t.finished, "Cannot cancel completed transfer");
            if part.exists() {
                fs::remove_file(part)?;
            }
            fs::remove_file(meta)?;
            Ok(json!({"cancelled":true,"transfer_id":t.id}))
        }
        other => bail!("Unsupported transfer op: {other}"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn chunks_resume_idempotence_and_checksum() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("large.bin");
        let policy = crate::policy::Policy {
            roots: vec![dir.path().to_path_buf()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: true,
            allow_screenshots: false,
            ask_before_sensitive: false,
        };
        let bytes: Vec<u8> = (0..2_098_576_usize).map(|i| (i % 251) as u8).collect();
        let hash = hex::encode(Sha256::digest(&bytes));
        let params = json!({"op":"begin","destination":p,"total_bytes":bytes.len(),"sha256":hash});
        let c = Arc::new(AtomicBool::new(false));
        let begin = run(&policy, &params, c.clone(), 30).await?;
        let id = begin["transfer_id"].as_str().unwrap();
        for (i, part) in bytes.chunks(500_000).enumerate() {
            let offset = i * 500_000;
            let args = json!({"op":"chunk","destination":p,"transfer_id":id,"offset":offset,"chunk_base64":base64::engine::general_purpose::STANDARD.encode(part)});
            run(&policy, &args, c.clone(), 30).await?;
            if i == 0 {
                run(&policy, &args, c.clone(), 30).await?;
            }
        }
        let status = run(
            &policy,
            &json!({"op":"status","destination":p,"transfer_id":id}),
            c.clone(),
            30,
        )
        .await?;
        assert_eq!(status["received_bytes"], bytes.len());
        let fin = run(
            &policy,
            &json!({"op":"finish","destination":p,"transfer_id":id}),
            c.clone(),
            30,
        )
        .await?;
        assert_eq!(fin["saved"], true);
        assert_eq!(fs::read(p)?, bytes);
        Ok(())
    }
    #[tokio::test]
    #[ignore = "manual large-transfer stress test"]
    async fn transfers_one_hundred_twelve_megabytes() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("large-112mb.dat");
        let policy = crate::policy::Policy {
            roots: vec![dir.path().to_path_buf()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: true,
            allow_screenshots: false,
            ask_before_sensitive: false,
        };
        let c = Arc::new(AtomicBool::new(false));
        let part: Vec<u8> = (0..524288_usize).map(|i| (i % 241) as u8).collect();
        let pieces = 224_usize;
        let mut h = Sha256::new();
        for _ in 0..pieces {
            h.update(&part);
        }
        let digest = hex::encode(h.finalize());
        let begin=run(&policy,&json!({"op":"begin","destination":path,"total_bytes":part.len()*pieces,"sha256":digest}),c.clone(),600).await?;
        let id = begin["transfer_id"].as_str().unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&part);
        for index in 0..pieces {
            let args = json!({"op":"chunk","destination":path,"transfer_id":id,"offset":index*part.len(),"chunk_base64":encoded});
            run(&policy, &args, c.clone(), 600).await?;
        }
        let finished = run(
            &policy,
            &json!({"op":"finish","destination":path,"transfer_id":id}),
            c.clone(),
            600,
        )
        .await?;
        assert_eq!(finished["saved"], true);
        assert_eq!(finished["sha256"], digest);
        assert_eq!(fs::metadata(&path)?.len(), (part.len() * pieces) as u64);
        Ok(())
    }
}
