mod cap;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize)]
struct Hello {
    #[serde(rename = "type")]
    kind: String,
    device_id: String,
    agent_version: String,
    capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Assign {
    #[serde(rename = "type")]
    kind: String,
    task_id: String,
    op: String,
    params: serde_json::Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--diagnostic") {
        let info = cap::system::system_info();
        println!("{}", serde_json::to_string_pretty(&info)?);
        return Ok(());
    }
    let gateway = std::env::var("OC_GATEWAY").unwrap_or_else(|_| "ws://localhost:8080".into());
    let device_id = cap::identity::device_id();
    tracing::info!(%device_id, %gateway, "connecting");
    let url = format!("{gateway}/agent/v1/connect?device_id={device_id}");
    let (ws, _) = tokio_tungstenite::connect_async(url).await?;
    let (tx, mut rx) = ws.split();
    let tx = std::sync::Arc::new(tokio::sync::Mutex::new(tx));
    let hello = Hello {
        kind: "hello".into(),
        device_id: device_id.clone(),
        agent_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            "system_info".into(), "file_list".into(), "file_read".into(), "file_write".into(),
            "file_manage".into(), "file_search".into(), "terminal_execute".into(),
            "process_list".into(), "process_manage".into(), "application_launch".into(),
            "desktop_screenshot".into(), "desktop_windows".into(), "desktop_interact".into(),
        ],
    };
    tx.lock().await.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&hello)?.into())).await?;
    let mut hb = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = hb.tick() => {
                let msg = serde_json::json!({"type":"heartbeat","device_id":device_id,"ts":chrono::Utc::now().timestamp(),"paused":cap::state::paused()});
                let _ = tx.lock().await.send(tokio_tungstenite::tungstenite::Message::Text(msg.to_string().into())).await;
            }
            m = rx.next() => {
                let Some(Ok(msg)) = m else { break };
                if let tokio_tungstenite::tungstenite::Message::Text(t) = msg {
                    if let Ok(a) = serde_json::from_str::<Assign>(&t) {
                        if a.kind == "task_assign" {
                            let device = device_id.clone();
                            let txc = tx.clone();
                            tokio::spawn(async move {
                                let ack = serde_json::json!({"type":"task_ack","task_id":a.task_id,"device_id":device,"state":"running"});
                                let _ = txc.lock().await.send(tokio_tungstenite::tungstenite::Message::Text(ack.to_string().into())).await;
                                let (ok, result, error) = execute(&a.op, &a.params).await;
                                let done = serde_json::json!({"type":"task_result","task_id":a.task_id,"device_id":device,"ok":ok,"result":result,"error":error});
                                let _ = txc.lock().await.send(tokio_tungstenite::tungstenite::Message::Text(done.to_string().into())).await;
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

async fn execute(op: &str, params: &serde_json::Value) -> (bool, serde_json::Value, Option<String>) {
    let run = async {
        let p: HashMap<String, serde_json::Value> =
            serde_json::from_value(params.clone()).unwrap_or_default();
        match op {
            "system_info" => Ok(cap::system::system_info()),
            "file_list" => cap::files::list(req(&p, "path")?),
            "file_read" => cap::files::read(req(&p, "path")?, opt_u64(&p, "max_bytes", 262144)),
            "file_write" => {
                let content = req(&p, "content")?;
                cap::files::write_atomic(req(&p, "path")?, content.as_bytes(), opt_bool(&p, "overwrite", false))
            }
            "file_manage" => cap::files::manage(
                &req(&p, "action")?,
                p.get("src").and_then(|v| v.as_str()),
                p.get("dst").and_then(|v| v.as_str()),
            ),
            "file_search" => cap::files::search(req(&p, "root")?, req(&p, "pattern")?, opt_u64(&p, "max_results", 50) as usize),
            "terminal_execute" => {
                cap::terminal::run(
                    p.get("shell").and_then(|v| v.as_str()).unwrap_or("powershell"),
                    &req(&p, "command")?,
                    p.get("cwd").and_then(|v| v.as_str()),
                    opt_u64(&p, "timeout_ms", 60000),
                ).await
            }
            "process_list" => Ok(cap::process::list(p.get("filter").and_then(|v| v.as_str()))),
            "process_manage" => cap::process::manage(&req(&p, "action")?, opt_u64(&p, "pid", 0) as u32),
            "application_launch" => cap::desktop::launch(req(&p, "app")?),
            "desktop_screenshot" => cap::desktop::screenshot(opt_u64(&p, "monitor", 0) as usize),
            "desktop_windows" => Ok(cap::desktop::windows()),
            "desktop_interact" => cap::desktop::interact(params.clone()),
            _ => Err(anyhow::anyhow!("unknown op {op}")),
        }
    };
    match run.await {
        Ok(v) => (true, v, None),
        Err(e) => (false, serde_json::json!({}), Some(e.to_string())),
    }
}

fn req(p: &HashMap<String, serde_json::Value>, k: &str) -> Result<String> {
    p.get(k).and_then(|v| v.as_str()).map(|s| s.to_owned()).ok_or_else(|| anyhow::anyhow!("missing {k}"))
}
fn opt_bool(p: &HashMap<String, serde_json::Value>, k: &str, d: bool) -> bool {
    p.get(k).and_then(|v| v.as_bool()).unwrap_or(d)
}
fn opt_u64(p: &HashMap<String, serde_json::Value>, k: &str, d: u64) -> u64 {
    p.get(k).and_then(|v| v.as_u64()).unwrap_or(d)
}
