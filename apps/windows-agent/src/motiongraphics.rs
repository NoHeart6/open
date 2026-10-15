//! Structured Motion Graphics CLI bridge. The caller supplies declarative scene edits,
//! never arbitrary PowerShell or executable commands. Run via existing MCP task queue.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command};

const OPERATIONS: &[&str] = &[
    "list",
    "inspect",
    "patch",
    "render_still",
    "render_video",
    "compile_motion",
    "import_image",
    "set_text",
];

pub async fn run(
    policy: &crate::policy::Policy,
    args: &Value,
    cancelled: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    let op = args
        .get("op")
        .and_then(Value::as_str)
        .context("Missing Motion Graphics operation")?;
    ensure!(
        OPERATIONS.contains(&op),
        "Unsupported Motion Graphics operation"
    );
    let writes = matches!(
        op,
        "patch" | "render_still" | "render_video" | "compile_motion" | "import_image" | "set_text"
    );
    if writes {
        ensure!(policy.allow_modify, "Modification permission is disabled");
    }
    let raw_root = args
        .get("project_root")
        .and_then(Value::as_str)
        .context("Missing Motion Graphics project_root")?;
    let root = policy.path(raw_root)?;
    ensure!(
        root.is_dir(),
        "Motion Graphics project root must be a directory"
    );
    let cli: PathBuf = root.join("scripts").join("mcp-control.mjs");
    let cli = policy.path(cli.to_str().context("Invalid CLI path")?)?;
    ensure!(
        cli.is_file(),
        "Motion Graphics CLI not found in selected project root"
    );
    ensure!(
        std::fs::metadata(&cli)?.len() < 256 * 1024,
        "Unexpected CLI file size"
    );
    let mut request = args
        .as_object()
        .context("Motion Graphics arguments must be an object")?
        .clone();
    request.remove("project_root");
    let payload = serde_json::to_vec(&Value::Object(request))?;
    ensure!(
        payload.len() <= 65536,
        "Motion Graphics request exceeds 64KiB"
    );
    ensure!(!cancelled.load(Ordering::Relaxed), "Operation cancelled");
    let mut child = Command::new("node.exe")
        .arg("--experimental-strip-types")
        .arg(&cli)
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("Start Motion Graphics control CLI")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&payload).await?;
    }
    let output = tokio::time::timeout(
        Duration::from_secs(seconds.max(1)),
        child.wait_with_output(),
    )
    .await
    .context("Motion Graphics CLI timed out")??;
    ensure!(!cancelled.load(Ordering::Relaxed), "Operation cancelled");
    ensure!(
        output.stdout.len() <= 256 * 1024 && output.stderr.len() <= 256 * 1024,
        "Motion Graphics output too large"
    );
    let result: Value = serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "Motion Graphics invalid response: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    if !output.status.success() || result["ok"] != true {
        bail!(
            "Motion Graphics: {}",
            result
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("CLI failure")
        );
    }
    Ok(json!({"operation":op,"result":result}))
}
