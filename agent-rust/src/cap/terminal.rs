use anyhow::Result;
use serde_json::{json, Value};
use std::time::Duration;

pub async fn run(shell: &str, command: &str, cwd: Option<&str>, timeout_ms: u64) -> Result<Value> {
    if command.len() > 8000 {
        anyhow::bail!("command too long");
    }
    let mut cmd = if shell == "cmd" {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(command);
        c
    } else {
        let mut c = tokio::process::Command::new("powershell");
        c.arg("-NoProfile").arg("-NonInteractive").arg("-Command").arg(command);
        c
    };
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    cmd.kill_on_drop(true);
    let child = cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn()?;
    let id = child.id();
    let out = tokio::time::timeout(Duration::from_millis(timeout_ms.max(1000)), child.wait_with_output()).await;
    match out {
        Ok(Ok(o)) => Ok(json!({
            "pid": id, "exit_code": o.status.code().unwrap_or(-1),
            "stdout": String::from_utf8_lossy(&o.stdout).chars().take(20000).collect::<String>(),
            "stderr": String::from_utf8_lossy(&o.stderr).chars().take(8000).collect::<String>(),
        })),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => Ok(json!({"timed_out": true, "pid": id})),
    }
}
