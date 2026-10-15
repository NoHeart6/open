//! Authenticated WordCraft MCP bridge for the local, running desktop app.
//! The WordCraft CLI owns its loopback control-key authentication. Never
//! expose or transmit the app's key through OpenCommander.
use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use serde_json::{Value, json};
use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command};

fn validate_tool(tool: &str, args: &Value, policy: &crate::policy::Policy) -> Result<()> {
    ensure!(
        args.is_object(),
        "WordCraft arguments must be a JSON object"
    );
    const TOOLS: [&str; 16] = [
        "list_commands",
        "execute",
        "batch",
        "new_document",
        "open_document",
        "save_document",
        "type_text",
        "select_text",
        "get_text",
        "inspect_document",
        "render_page",
        "parity",
        "screenshot",
        "click",
        "key",
        "ui_inspect",
    ];
    ensure!(TOOLS.contains(&tool), "Unknown WordCraft native MCP tool");
    ensure!(
        serde_json::to_vec(args)?.len() <= 65536,
        "WordCraft arguments exceed 64 KiB"
    );
    if !matches!(
        tool,
        "list_commands"
            | "get_text"
            | "inspect_document"
            | "render_page"
            | "parity"
            | "screenshot"
            | "ui_inspect"
    ) {
        ensure!(
            policy.allow_modify,
            "Local editor modification permission is disabled"
        );
    }
    if tool == "screenshot" {
        ensure!(
            policy.allow_screenshots,
            "Local screenshot permission is disabled"
        );
    }
    if matches!(tool, "open_document" | "save_document") {
        if let Some(path) = args.get("path").and_then(Value::as_str) {
            policy.path(path)?;
        } else if tool == "open_document" {
            bail!("open_document requires a file path");
        }
    }
    Ok(())
}

pub async fn run(
    policy: &crate::policy::Policy,
    args: &Value,
    cancelled: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    let tool = args["tool"]
        .as_str()
        .context("Missing WordCraft MCP tool name")?;
    let params = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
    validate_tool(tool, &params, policy)?;
    let output_path = args
        .get("output_path")
        .and_then(Value::as_str)
        .map(Path::new);
    ensure!(
        output_path.is_none() || matches!(tool, "render_page" | "screenshot"),
        "output_path only applies to image tools"
    );
    if let Some(path) = output_path {
        ensure!(
            policy.allow_modify,
            "Image export requires local modify permission"
        );
        policy.path(
            path.to_str()
                .context("Image export path is not valid UTF-8")?,
        )?;
    }
    let executable = args["cli_path"]
        .as_str()
        .context("Missing absolute path to wordcraft-cli.exe")?;
    let cli = policy.application(executable)?;
    ensure!(
        cli.file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case("wordcraft-cli.exe")),
        "Only the WordCraft CLI is permitted for this tool"
    );
    let port = args["port"].as_u64().unwrap_or(7981);
    ensure!(
        (1024..=65535).contains(&port),
        "Invalid WordCraft local control port"
    );
    ensure!(
        !cancelled.load(Ordering::SeqCst),
        "WordCraft task cancelled"
    );

    let mut command = Command::new(&cli);
    command
        .arg("mcp")
        .arg("--connect")
        .arg(format!("127.0.0.1:{port}"))
        .current_dir(cli.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().context("Cannot start WordCraft MCP CLI")?;
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"OpenCommander","version":"0.3.1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":tool,"arguments":params}}),
    ];
    let mut bytes = Vec::new();
    for req in requests {
        bytes.extend(serde_json::to_vec(&req)?);
        bytes.push(b'\n');
    }
    {
        let mut input = child.stdin.take().context("WordCraft stdin unavailable")?;
        input
            .write_all(&bytes)
            .await
            .context("Cannot submit WordCraft MCP request")?;
        input
            .shutdown()
            .await
            .context("Cannot close WordCraft MCP stdin")?;
    }

    let output = tokio::time::timeout(
        Duration::from_secs(seconds.min(90)),
        child.wait_with_output(),
    )
    .await
    .context("WordCraft MCP request timed out")?
    .context("WordCraft MCP process failed")?;
    ensure!(
        !cancelled.load(Ordering::SeqCst),
        "WordCraft task cancelled"
    );
    ensure!(
        output.status.success(),
        "WordCraft MCP CLI exited unsuccessfully"
    );
    ensure!(
        output.stdout.len() <= 34 * 1024 * 1024,
        "WordCraft MCP reply too large"
    );
    let output_text =
        String::from_utf8(output.stdout).context("Invalid WordCraft MCP UTF-8 reply")?;
    let reply = output_text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|m| m["id"] == 3)
        .context("WordCraft MCP tool response was not returned")?;
    if !reply["error"].is_null() {
        bail!(
            "WordCraft MCP error: {}",
            reply["error"]
                .to_string()
                .chars()
                .take(1000)
                .collect::<String>()
        );
    }
    let result = &reply["result"];
    let parts = result["content"]
        .as_array()
        .context("WordCraft MCP response has no content")?;
    let returned = parts
        .iter()
        .find(|part| part["type"] == "text")
        .and_then(|part| part["text"].as_str())
        .unwrap_or("");
    ensure!(
        !result["isError"].as_bool().unwrap_or(false),
        "WordCraft MCP tool error: {}",
        returned.chars().take(1000).collect::<String>()
    );
    let parsed = serde_json::from_str::<Value>(returned).unwrap_or_else(|_| json!(returned));
    let mut answer =
        json!({"tool":tool,"result":parsed,"via":"wordcraft-native-mcp","control_port":port});
    if let Some(part) = parts.iter().find(|part| part["type"] == "image") {
        let data = part["data"]
            .as_str()
            .context("WordCraft image data is absent")?;
        let media = part["mimeType"].as_str().unwrap_or("image/png");
        let original = base64::engine::general_purpose::STANDARD
            .decode(data)
            .context("WordCraft image encoding invalid")?;
        ensure!(
            original.len() <= 24 * 1024 * 1024,
            "WordCraft image is too large"
        );
        if let Some(path) = output_path {
            std::fs::write(path, &original).context("Cannot write WordCraft image")?;
            answer["image_saved_to"] = json!(path.to_string_lossy().to_string());
        }
        let mut preview = original;
        let mut preview_mime = media.to_owned();
        if preview.len() > 450_000 {
            let picture =
                image::load_from_memory(&preview).context("Cannot decode WordCraft preview")?;
            let mut max_width = 1500;
            let mut max_height = 1800;
            for _ in 0..4 {
                let downsized =
                    picture.resize(max_width, max_height, image::imageops::FilterType::Lanczos3);
                let mut jpeg = Vec::new();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 76)
                    .encode_image(&downsized)
                    .context("Cannot encode WordCraft preview")?;
                preview = jpeg;
                if preview.len() <= 450_000 {
                    break;
                }
                max_width = (max_width * 3 / 4).max(320);
                max_height = (max_height * 3 / 4).max(400);
            }
            preview_mime = "image/jpeg".into();
        }
        ensure!(
            preview.len() <= 1_000_000,
            "WordCraft preview exceeds transport capacity"
        );
        answer["image_data"] = json!({
            "mime_type": preview_mime,
            "base64": base64::engine::general_purpose::STANDARD.encode(preview),
        });
    } else {
        ensure!(
            returned.len() <= 1024 * 1024,
            "WordCraft response text too large"
        );
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Requires WordCraft desktop --control 7981 and WORDCRAFT_CLI_PATH/WORDCRAFT_TEST_ROOT"]
    async fn live_bridge_reads_open_document_via_native_mcp() -> Result<()> {
        let executable = std::env::var("WORDCRAFT_CLI_PATH")?;
        let root = std::env::var("WORDCRAFT_TEST_ROOT")?;
        let policy = crate::policy::Policy {
            roots: vec![PathBuf::from(root)],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: true,
            allow_modify: true,
            allow_screenshots: true,
            ask_before_sensitive: false,
        };
        let answer = run(
            &policy,
            &json!({"cli_path": executable, "port":7981, "tool":"get_text", "arguments":{}}),
            Arc::new(AtomicBool::new(false)),
            30,
        )
        .await?;
        ensure!(answer["via"] == "wordcraft-native-mcp");
        ensure!(
            answer["result"]["text"]
                .as_str()
                .is_some_and(|s| s.contains("WordCraft"))
        );
        Ok(())
    }

    #[tokio::test]
    #[ignore = "Requires WordCraft app listening on 127.0.0.1:7981"]
    async fn live_render_page_image_is_returned() -> Result<()> {
        let executable = std::env::var("WORDCRAFT_CLI_PATH")?;
        let root = std::env::var("WORDCRAFT_TEST_ROOT")?;
        let policy = crate::policy::Policy {
            roots: vec![PathBuf::from(root)],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: true,
            allow_modify: true,
            allow_screenshots: true,
            ask_before_sensitive: false,
        };
        let answer = run(
            &policy,
            &json!({"cli_path":executable, "port":7981,
            "tool":"render_page", "arguments":{"page":1,"scale":0.75}}),
            Arc::new(AtomicBool::new(false)),
            45,
        )
        .await?;
        ensure!(answer["image_data"]["mime_type"].as_str().is_some());
        ensure!(
            answer["image_data"]["base64"]
                .as_str()
                .is_some_and(|x| x.len() > 1000)
        );
        Ok(())
    }
    use std::path::PathBuf;
    #[test]
    fn all_native_tools_and_dynamic_commands_are_supported() {
        let policy = crate::policy::Policy {
            roots: vec![PathBuf::from(r"C:\Users\Test")],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: true,
            allow_modify: true,
            allow_screenshots: true,
            ask_before_sensitive: false,
        };
        assert!(validate_tool("execute", &json!({"command":"format.bold"}), &policy).is_ok());
        assert!(
            validate_tool(
                "execute",
                &json!({"command":"file.open","params":{"path":"C:\\secrets"}}),
                &policy
            )
            .is_ok()
        );
        assert!(validate_tool("batch", &json!({"commands":[]}), &policy).is_ok());
        assert!(validate_tool("type_text", &json!({"text":"Hello WordCraft"}), &policy).is_ok());
    }
}
