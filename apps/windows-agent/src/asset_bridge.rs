use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const LIMIT: u64 = 512 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct AssetJournal {
    file_id: String,
}

fn clean_name(input: &str) -> String {
    let basename = input.rsplit(['/', '\\']).next().unwrap_or("asset.bin");
    let value: String = basename
        .chars()
        .filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c))
        .take(120)
        .collect();
    let value = value.trim().trim_matches('.');
    if value.is_empty() {
        "asset.bin".into()
    } else {
        value.to_string()
    }
}
fn paths(dest: &Path) -> (PathBuf, PathBuf) {
    let n = dest.file_name().unwrap().to_string_lossy();
    (
        dest.with_file_name(format!("{n}.asset.part")),
        dest.with_file_name(format!("{n}.asset.json")),
    )
}
fn auto_dest(policy: &crate::policy::Policy, a: &Value, name: &str) -> Result<PathBuf> {
    let dir = if let Some(s) = a["directory"].as_str() {
        PathBuf::from(s)
    } else {
        PathBuf::from(std::env::var("USERPROFILE").context("USERPROFILE not set")?)
            .join("Pictures")
            .join("OpenCommander")
    };
    let base = clean_name(name);
    let stem = Path::new(&base).file_stem().unwrap().to_string_lossy();
    let ext = Path::new(&base)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("");
    for n in 0..10000 {
        let candidate = if n == 0 {
            dir.join(&base)
        } else if ext.is_empty() {
            dir.join(format!("{stem}-{n}"))
        } else {
            dir.join(format!("{stem}-{n}.{ext}"))
        };
        let candidate = policy.path(candidate.to_str().context("Invalid path")?)?;
        let (part, meta) = paths(&candidate);
        if !candidate.exists() && (!part.exists() || meta.exists()) {
            return Ok(candidate);
        }
    }
    bail!("Cannot choose an available destination name")
}
fn hash(path: &Path) -> Result<String> {
    let mut h = Sha256::new();
    let mut f = File::open(path)?;
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
fn sniff_image(path: &Path) -> Result<bool> {
    let ext = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["png", "jpg", "jpeg"].contains(&ext.as_str()) {
        return Ok(false);
    }
    let mut f = File::open(path)?;
    let mut header = [0_u8; 8];
    let n = f.read(&mut header)?;
    Ok((ext == "png" && n >= 8 && header == *b"\x89PNG\r\n\x1a\n")
        || ((ext == "jpg" || ext == "jpeg") && n >= 3 && header[..3] == [255, 216, 255]))
}
async fn insert(
    policy: &crate::policy::Policy,
    a: &Value,
    dest: &Path,
    cancel: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    let detected =
        if a["wordcraft_cli_path"].as_str().is_none() && a["insert_into_wordcraft"] == true {
            let profile = std::env::var("USERPROFILE").context("USERPROFILE not set")?;
            let root = PathBuf::from(profile).join("Documents").join("WordCraft");
            let mut candidates = Vec::new();
            if root.is_dir() {
                for entry in walkdir::WalkDir::new(root)
                    .max_depth(3)
                    .into_iter()
                    .filter_map(Result::ok)
                {
                    if entry.file_type().is_file()
                        && entry
                            .file_name()
                            .to_string_lossy()
                            .eq_ignore_ascii_case("wordcraft-cli.exe")
                    {
                        candidates.push(entry.path().to_path_buf());
                    }
                }
            }
            candidates
                .into_iter()
                .next()
                .and_then(|v| v.to_str().map(str::to_owned))
        } else {
            None
        };
    let cli_override = detected.as_deref();
    let cli = a["wordcraft_cli_path"]
        .as_str()
        .or(cli_override)
        .context("WordCraft CLI path required")?;
    ensure!(
        sniff_image(dest)?,
        "Only valid PNG/JPEG images can be inserted into WordCraft"
    );
    let mut params = json!({"path":dest.to_str().context("Invalid destination")?});
    if let Some(w) = a["width_pt"].as_f64() {
        params["width"] = json!(w);
    }
    if let Some(alt) = a["alt_text"].as_str() {
        params["alt"] = json!(alt);
    }
    let request = json!({"cli_path":cli,"port":a["wordcraft_port"].as_u64().unwrap_or(7981),
      "tool":"execute","arguments":{"command":"insert.picture","params":params}});
    let initial = crate::wordcraft::run(policy, &request, cancel.clone(), seconds).await;
    if initial.is_ok() || a["insert_into_wordcraft"] != true {
        return initial;
    }
    let exe = Path::new(cli).with_file_name("wordcraft.exe");
    policy.application(exe.to_str().context("WordCraft executable path invalid")?)?;
    let _child = std::process::Command::new(exe)
        .arg("--control")
        .arg(a["wordcraft_port"].as_u64().unwrap_or(7981).to_string())
        .spawn()?;
    for _ in 0..12 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Ok(v) = crate::wordcraft::run(policy, &request, cancel.clone(), seconds).await {
            return Ok(v);
        }
    }
    initial
}
pub async fn run_one(
    policy: &crate::policy::Policy,
    a: &Value,
    cancel: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    ensure!(
        policy.allow_modify,
        "Asset import requires modify permission"
    );
    let file = &a["file"];
    let url = file["download_url"]
        .as_str()
        .context("File download_url is required")?;
    let file_id = file["file_id"]
        .as_str()
        .context("File file_id is required")?;
    ensure!(
        !file_id.is_empty() && file_id.len() <= 256,
        "Invalid file_id"
    );
    let url = reqwest::Url::parse(url)?;
    ensure!(url.scheme() == "https", "File download URL must use HTTPS");
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "URL credentials unsupported"
    );
    let name = file["file_name"]
        .as_str()
        .filter(|v| !v.is_empty())
        .unwrap_or("chatgpt-asset.bin");
    let dest = if let Some(s) = a["destination"].as_str() {
        policy.path(s)?
    } else {
        auto_dest(policy, a, name)?
    };
    ensure!(!dest.exists(), "Destination already exists");
    if let Some(parent) = dest.parent() {
        policy.path(parent.to_str().context("Invalid parent")?)?;
        fs::create_dir_all(parent)?;
    }
    let (part, meta) = paths(&dest);
    if meta.exists() {
        let old: AssetJournal = serde_json::from_slice(&fs::read(&meta)?)?;
        ensure!(
            old.file_id == file_id,
            "A different file has an unfinished transfer at this destination"
        );
        ensure!(part.is_file(), "Transfer journal has no partial file");
    } else {
        ensure!(!part.exists(), "Unrecognized partial asset file");
        let data = serde_json::to_vec(&AssetJournal {
            file_id: file_id.to_owned(),
        })?;
        let mut journal = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&meta)?;
        journal.write_all(&data)?;
        journal.sync_all()?;
        if let Err(err) = OpenOptions::new().create_new(true).write(true).open(&part) {
            let _ = fs::remove_file(&meta);
            return Err(err.into());
        }
    }
    let offset = fs::metadata(&part)?.len();
    ensure!(
        offset < LIMIT,
        "Incomplete asset already reached maximum size"
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(seconds.clamp(1, 600)))
        .build()?;
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity");
    if offset > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let response = request.send().await?.error_for_status()?;
    if offset > 0 {
        ensure!(
            response.status() == reqwest::StatusCode::PARTIAL_CONTENT,
            "Source does not support resumable ranges"
        );
        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .context("No Content-Range")?;
        ensure!(
            content_range.starts_with(&format!("bytes {offset}-")),
            "Invalid resume offset"
        );
    }
    if let Some(len) = response.content_length() {
        ensure!(offset + len <= LIMIT, "File exceeds 512 MiB");
    }
    let mut out = OpenOptions::new().append(true).open(&part)?;
    let mut received = offset;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        ensure!(
            !cancel.load(Ordering::SeqCst),
            "Transfer cancelled, partial bytes kept for resume"
        );
        let chunk = chunk?;
        ensure!(
            received + chunk.len() as u64 <= LIMIT,
            "File exceeds 512 MiB"
        );
        out.write_all(&chunk)?;
        received += chunk.len() as u64;
    }
    out.sync_all()?;
    drop(out);
    ensure!(received > 0, "Empty asset");
    if let Some(expected) = a["expected_sha256"].as_str() {
        ensure!(
            expected.eq_ignore_ascii_case(&hash(&part)?),
            "Asset SHA-256 mismatch"
        );
    }
    let digest = hash(&part)?;
    ensure!(!dest.exists(), "Destination created during transfer");
    fs::rename(&part, &dest)?;
    fs::remove_file(&meta)?;
    let mut result = json!({"saved":true,"destination":dest,"bytes":received,"sha256":digest,
        "file_id":file_id,"resumed_from_bytes":offset,"inserted_into_wordcraft":false});
    if a["wordcraft_cli_path"].as_str().is_some() || a["insert_into_wordcraft"] == true {
        match insert(policy, a, &dest, cancel, seconds).await {
            Ok(v) => {
                result["inserted_into_wordcraft"] = json!(true);
                result["wordcraft_result"] = v;
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
    if let Some(files) = a["files"].as_array() {
        ensure!(
            !files.is_empty() && files.len() <= 10,
            "Batch must contain 1..10 files"
        );
        ensure!(
            a["destination"].is_null(),
            "Use directory, not destination, for batches"
        );
        let mut results = Vec::new();
        for file in files {
            let mut item = a.clone();
            item.as_object_mut().unwrap().remove("files");
            item["file"] = file.clone();
            match run_one(policy, &item, cancel.clone(), seconds).await {
                Ok(v) => results.push(v),
                Err(e) => results.push(
                    json!({"saved":false,"file_id":file["file_id"],"error":format!("{e:#}")}),
                ),
            }
        }
        let saved = results.iter().filter(|v| v["saved"] == true).count();
        Ok(json!({"saved_count":saved,"total":files.len(),"results":results}))
    } else {
        run_one(policy, a, cancel, seconds).await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_names_do_not_escape() {
        assert_eq!(clean_name("../../<bad>:naruto.png"), "badnaruto.png");
        assert_eq!(clean_name(""), "asset.bin");
        assert_eq!(clean_name(r"C:\temp\demo.jpg"), "demo.jpg");
    }
    #[test]
    fn metadata_journal_paths_distinct() {
        let (part, meta) = paths(Path::new(r"C:\Pictures\anime.png"));
        assert_ne!(part, meta);
    }
}
