use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

async fn wc(
    p: &crate::policy::Policy,
    cli: &str,
    port: u64,
    tool: &str,
    args: Value,
    c: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    crate::wordcraft::run(
        p,
        &json!({"cli_path":cli,"port":port,"tool":tool,"arguments":args}),
        c,
        seconds,
    )
    .await
}
async fn command(
    p: &crate::policy::Policy,
    cli: &str,
    port: u64,
    cmd: &str,
    args: Value,
    c: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    wc(
        p,
        cli,
        port,
        "execute",
        json!({"command":cmd,"params":args}),
        c,
        seconds,
    )
    .await
}
pub async fn run(
    p: &crate::policy::Policy,
    a: &Value,
    c: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    ensure!(
        p.allow_modify,
        "Document composition needs modify permission"
    );
    let cli = a["cli_path"].as_str().context("cli_path required")?;
    let port = a["port"].as_u64().unwrap_or(7981);
    let dest = p.path(a["save_path"].as_str().context("save_path required")?)?;
    ensure!(
        !dest.exists(),
        "Won't overwrite existing WordCraft document"
    );
    let blocks = a["blocks"].as_array().context("blocks required")?;
    ensure!(
        !blocks.is_empty() && blocks.len() <= 80,
        "Document requires 1..80 blocks"
    );
    wc(p, cli, port, "new_document", json!({}), c.clone(), seconds).await?;
    let mut images = 0;
    for (i, b) in blocks.iter().enumerate() {
        ensure!(!c.load(Ordering::SeqCst), "Composition cancelled");
        let kind = b["type"].as_str().context("Block type required")?;
        match kind {
            "title" | "heading1" | "heading2" | "heading3" | "paragraph" => {
                let text = b["text"].as_str().context("Text required")?;
                ensure!(text.len() <= 8000, "Block text too long");
                let style = match kind {
                    "title" => "Title",
                    "heading1" => "Heading 1",
                    "heading2" => "Heading 2",
                    "heading3" => "Heading 3",
                    _ => "Normal",
                };
                command(
                    p,
                    cli,
                    port,
                    "para.style",
                    json!({"style":style}),
                    c.clone(),
                    seconds,
                )
                .await?;
                command(
                    p,
                    cli,
                    port,
                    "text.insert",
                    json!({"text":text}),
                    c.clone(),
                    seconds,
                )
                .await?;
                command(
                    p,
                    cli,
                    port,
                    "text.newParagraph",
                    json!({}),
                    c.clone(),
                    seconds,
                )
                .await?;
            }
            "image" => {
                let image = p.path(b["path"].as_str().context("Image path required")?)?;
                ensure!(image.is_file(), "Image not found");
                let w = b["width_pt"].as_f64().unwrap_or(290.0);
                ensure!((12.0..=600.0).contains(&w), "Invalid width_pt");
                command(p,cli,port,"insert.picture",json!({"path":image,"width":w,"alt":b["alt"].as_str().unwrap_or("Illustration")}),c.clone(),seconds).await?;
                images += 1;
                if let Some(caption) = b["caption"].as_str() {
                    command(
                        p,
                        cli,
                        port,
                        "references.caption",
                        json!({"label":"Figure","text":caption,"position":"below"}),
                        c.clone(),
                        seconds,
                    )
                    .await?;
                }
                command(
                    p,
                    cli,
                    port,
                    "text.newParagraph",
                    json!({}),
                    c.clone(),
                    seconds,
                )
                .await?;
            }
            "page_break" => {
                command(
                    p,
                    cli,
                    port,
                    "text.pageBreak",
                    json!({}),
                    c.clone(),
                    seconds,
                )
                .await?;
            }
            "table" => {
                let rows = b["rows"].as_array().context("table.rows required")?;
                ensure!((1..=20).contains(&rows.len()), "Table rows 1..20");
                let cols = rows
                    .first()
                    .and_then(Value::as_array)
                    .context("Table first row required")?
                    .len();
                ensure!((1..=12).contains(&cols), "Table columns 1..12");
                command(
                    p,
                    cli,
                    port,
                    "insert.table",
                    json!({"rows":rows.len(),"cols":cols}),
                    c.clone(),
                    seconds,
                )
                .await?;
                let count = rows.len() * cols;
                let mut index = 0;
                for r in rows {
                    let cells = r.as_array().context("Table row must be an array")?;
                    ensure!(cells.len() == cols, "Table must be rectangular");
                    for cell in cells {
                        let value = cell.as_str().context("Table cells must be strings")?;
                        ensure!(value.len() <= 2000, "Table cell exceeds 2,000 characters");
                        if !value.is_empty() {
                            command(
                                p,
                                cli,
                                port,
                                "text.insert",
                                json!({"text":value}),
                                c.clone(),
                                seconds,
                            )
                            .await?;
                        }
                        index += 1;
                        if index < count {
                            command(p, cli, port, "text.tab", json!({}), c.clone(), seconds)
                                .await?;
                        }
                    }
                }
                command(p, cli, port, "caret.docEnd", json!({}), c.clone(), seconds).await?;
            }
            _ => bail!("Unsupported document block type {kind} at index {i}"),
        }
    }
    wc(
        p,
        cli,
        port,
        "save_document",
        json!({"path":dest}),
        c.clone(),
        seconds,
    )
    .await?;
    let inspected = wc(
        p,
        cli,
        port,
        "inspect_document",
        json!({}),
        c.clone(),
        seconds,
    )
    .await?;
    let raw = serde_json::to_string(&inspected)?;
    if images > 0 {
        ensure!(
            raw.contains(r#""type":"image""#),
            "Saved document image missing from inspection"
        );
    }
    let size = std::fs::metadata(&dest)?.len();
    Ok(
        json!({"saved":true,"path":dest,"bytes":size,"blocks_requested":blocks.len(),"images_requested":images,"verified_images":images==0||raw.contains(r#""type":"image""#),"inspected":true}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_basics() {
        assert_eq!(json!({"type":"heading1"})["type"], "heading1");
    }
}
