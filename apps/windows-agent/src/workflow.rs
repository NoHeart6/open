use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

#[derive(Clone, Serialize, Deserialize)]
struct Journal {
    plan_sha256: String,
    finished: Vec<String>,
    active: Option<String>,
    failed: Option<String>,
    complete: bool,
}
struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn digest(v: &Value) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(v)?)))
}
fn persist(path: &Path, journal: &Journal) -> Result<()> {
    let folder = path.parent().context("Checkpoint parent missing")?;
    let mut tmp = tempfile::NamedTempFile::new_in(folder)?;
    serde_json::to_writer_pretty(&mut tmp, journal)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    Ok(())
}
fn validate(p: &crate::policy::Policy, steps: &[Value]) -> Result<()> {
    ensure!(
        !steps.is_empty() && steps.len() <= 20,
        "Workflow supports 1..20 steps"
    );
    let mut seen = std::collections::HashSet::new();
    for s in steps {
        let id = s["id"].as_str().context("Step id required")?;
        ensure!(
            !id.is_empty() && id.len() <= 64 && seen.insert(id),
            "Duplicate or invalid step id"
        );
        let action = s["action"].as_str().context("Step action required")?;
        ensure!(
            matches!(
                action,
                "file_read"
                    | "file_write"
                    | "file_operation"
                    | "file_export"
                    | "wordcraft"
                    | "desktop_inspect"
                    | "desktop_automation"
            ),
            "Unsupported workflow action"
        );
        let args = s["args"].as_object().context("Step args required")?;
        ensure!(
            serde_json::to_vec(args)?.len() <= 12_000,
            "Workflow step args too large"
        );
        if action == "file_operation" {
            ensure!(
                s["args"]["op"] != "delete",
                "Destructive delete excluded from workflows"
            );
        }
        p.authorize(action, &s["args"])?;
        let v = &s["verify"];
        if !v.is_null() {
            let k = v["type"].as_str().context("Invalid verify.type")?;
            ensure!(
                matches!(k, "exists" | "sha256" | "contains" | "wordcraft_image"),
                "Invalid verifier"
            );
            if k != "wordcraft_image" {
                p.path(v["path"].as_str().context("Verifier path required")?)?;
            }
            if k == "sha256" {
                ensure!(
                    v["value"].as_str().is_some_and(|x| x.len() == 64),
                    "Verifier SHA256 required"
                );
            }
            if k == "contains" {
                ensure!(
                    v["value"].as_str().is_some_and(|x| x.len() <= 2000),
                    "Verifier text required"
                );
            }
        }
    }
    Ok(())
}
async fn verify(
    p: &crate::policy::Policy,
    step: &Value,
    cancel: Arc<AtomicBool>,
    seconds: u64,
) -> Result<bool> {
    let v = &step["verify"];
    if v.is_null() {
        return Ok(true);
    }
    let kind = v["type"].as_str().unwrap_or("");
    if kind == "wordcraft_image" {
        let a = &step["args"];
        let cli = v["cli_path"]
            .as_str()
            .or(a["cli_path"].as_str())
            .context("WordCraft CLI required for image verification")?;
        let port = v["port"].as_u64().or(a["port"].as_u64()).unwrap_or(7981);
        let r = crate::wordcraft::run(
            p,
            &json!({"cli_path":cli,"port":port,"tool":"inspect_document","arguments":{}}),
            cancel,
            seconds,
        )
        .await?;
        let text = serde_json::to_string(&r)?;
        return Ok(text.contains(r#""type":"image""#));
    }
    let path = p.path(v["path"].as_str().context("Verifier path required")?)?;
    if kind == "exists" {
        return Ok(path.is_file());
    }
    if !path.is_file() {
        return Ok(false);
    }
    let _guard = crate::policy::guard_parent(&path)?;
    if kind == "sha256" {
        let mut f = crate::policy::open_file(&path)?;
        use std::io::Read;
        let mut h = Sha256::new();
        let mut buf = [0_u8; 65536];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        return Ok(hex::encode(h.finalize()) == v["value"].as_str().unwrap());
    }
    if kind == "contains" {
        ensure!(
            path.metadata()?.len() <= 8 * 1024 * 1024,
            "Text verifier file too large"
        );
        let s = fs::read_to_string(path)?;
        return Ok(s.contains(v["value"].as_str().unwrap_or("")));
    }
    Ok(false)
}
async fn step(
    p: &crate::policy::Policy,
    s: &Value,
    c: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    let action = s["action"].as_str().context("Missing step.action")?;
    let args = &s["args"];
    match action {
        "file_read" | "file_write" | "file_operation" => crate::files::execute(p, action, args),
        "file_export" => crate::file_export::run(p, args, c),
        "wordcraft" => crate::wordcraft::run(p, args, c, seconds).await,
        "desktop_inspect" | "desktop_automation" => {
            let mut r = crate::automation::execute(args, action == "desktop_automation")?;
            if action == "desktop_automation"
                && p.allow_screenshots
                && args["capture_after"] != false
                && let Ok(s) = crate::desktop::screenshot()
            {
                r["screenshot"] = s;
            }
            Ok(r)
        }
        _ => bail!("Unsupported workflow step"),
    }
}
pub async fn run(
    p: &crate::policy::Policy,
    a: &Value,
    cancel: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    let op = a["op"].as_str().unwrap_or("run");
    ensure!(
        matches!(op, "run" | "preview" | "status"),
        "Unknown workflow op"
    );
    let target = p.path(
        a["checkpoint_path"]
            .as_str()
            .context("checkpoint_path required")?,
    )?;
    let _guard = crate::policy::guard_parent(&target)?;
    if op == "status" {
        let j: Journal =
            serde_json::from_slice(&fs::read(&target).context("Checkpoint not found")?)?;
        return Ok(
            json!({"plan_sha256":j.plan_sha256,"finished":j.finished,"active":j.active,"failed":j.failed,"complete":j.complete}),
        );
    }
    let steps = a["steps"].as_array().context("steps required")?;
    validate(p, steps)?;
    let fingerprint = digest(&json!({"steps":steps}))?;
    if op == "preview" {
        return Ok(
            json!({"preview":true,"steps":steps.iter().map(|x|json!({"id":x["id"],"action":x["action"],"verify":x["verify"]})).collect::<Vec<_>>(),"plan_sha256":fingerprint}),
        );
    }
    ensure!(
        p.allow_modify,
        "Workflow execution requires modify permission"
    );
    let lockpath = target.with_extension("workflow.lock");
    let _lockhandle = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lockpath)
        .context("Workflow checkpoint is already running")?;
    let _lock = Lock(lockpath);
    let mut j: Journal = if target.exists() {
        serde_json::from_slice(&fs::read(&target)?)?
    } else {
        Journal {
            plan_sha256: fingerprint.clone(),
            finished: vec![],
            active: None,
            failed: None,
            complete: false,
        }
    };
    ensure!(
        j.plan_sha256 == fingerprint,
        "Checkpoint belongs to a different workflow plan"
    );
    if j.active.is_some() {
        ensure!(
            a["retry_uncertain_step"] == true,
            "Previous step outcome uncertain. Inspect then retry_uncertain_step=true explicitly"
        );
        j.active = None;
    }
    if j.failed.is_some() {
        ensure!(
            a["retry_failed_step"] == true,
            "Previous step failed. Set retry_failed_step=true only after resolving the cause"
        );
        j.failed = None;
    }
    let started = Instant::now();
    let mut results = Vec::new();
    for s in steps {
        let id = s["id"].as_str().unwrap();
        if j.finished.iter().any(|x| x == id) {
            continue;
        }
        ensure!(!cancel.load(Ordering::SeqCst), "Workflow cancelled");
        ensure!(
            started.elapsed().as_secs() < seconds,
            "Workflow exceeded task deadline"
        );
        j.active = Some(id.to_owned());
        persist(&target, &j)?;
        let output = step(
            p,
            s,
            cancel.clone(),
            seconds.saturating_sub(started.elapsed().as_secs()),
        )
        .await;
        match output {
            Ok(r) => {
                match verify(
                    p,
                    s,
                    cancel.clone(),
                    seconds.saturating_sub(started.elapsed().as_secs()),
                )
                .await
                {
                    Ok(true) => {
                        j.active = None;
                        j.finished.push(id.to_owned());
                        persist(&target, &j)?;
                        results.push(json!({"id":id,"status":"succeeded","result":r}));
                    }
                    Ok(false) => {
                        j.active = None;
                        j.failed = Some(id.to_owned());
                        persist(&target, &j)?;
                        return Ok(
                            json!({"status":"failed","step":id,"reason":"Postcondition not met","finished":j.finished,"checkpoint_path":target}),
                        );
                    }
                    Err(err) => {
                        j.active = None;
                        j.failed = Some(id.to_owned());
                        persist(&target, &j)?;
                        return Ok(
                            json!({"status":"failed","step":id,"reason":format!("Postcondition: {err:#}"),"finished":j.finished,"checkpoint_path":target}),
                        );
                    }
                }
            }
            Err(err) => {
                j.active = None;
                j.failed = Some(id.to_owned());
                persist(&target, &j)?;
                return Ok(
                    json!({"status":"failed","step":id,"reason":format!("{err:#}"),"finished":j.finished,"checkpoint_path":target}),
                );
            }
        }
    }
    j.complete = true;
    j.active = None;
    j.failed = None;
    persist(&target, &j)?;
    Ok(
        json!({"status":"completed","finished":j.finished,"checkpoint_path":target,"results":results,"plan_sha256":fingerprint}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn checkpoint_resume_skips_completed_steps() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let policy = crate::policy::Policy {
            roots: vec![dir.path().to_path_buf()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: true,
            allow_screenshots: false,
            ask_before_sensitive: false,
        };
        let cp = dir.path().join("workflow.json");
        let one = dir.path().join("one.txt");
        let two = dir.path().join("two.txt");
        let actions = json!([
            {"id":"write-first","action":"file_write","args":{"path":one,"text":"test-1"},"verify":{"type":"contains","path":one,"value":"test-1"}},
            {"id":"write-second","action":"file_write","args":{"path":two,"text":"test-2"},"verify":{"type":"exists","path":two}}
        ]);
        let cancel = Arc::new(AtomicBool::new(false));
        let args = json!({"op":"run","checkpoint_path":cp,"steps":actions});
        let v = run(&policy, &args, cancel.clone(), 30).await?;
        assert_eq!(v["status"], "completed");
        let repeat = run(&policy, &args, cancel.clone(), 30).await?;
        assert_eq!(repeat["status"], "completed");
        assert_eq!(repeat["results"].as_array().unwrap().len(), 0);
        assert_eq!(fs::read_to_string(one)?, "test-1");
        Ok(())
    }
}
