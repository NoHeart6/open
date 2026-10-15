use anyhow::{Context, Result, ensure};
use base64::Engine;
use image::GenericImageView;
use serde_json::{Value, json};
use sha2::Digest;
use std::{
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub async fn run(
    policy: &crate::policy::Policy,
    args: &Value,
    cancel: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    ensure!(
        policy.allow_modify,
        "Image import needs local modify permission"
    );
    let destination = args["destination"]
        .as_str()
        .context("Missing destination")?;
    let path = policy.path(destination)?;
    ensure!(!path.exists(), "Destination already exists (no overwrite)");
    let url = args["source_url"].as_str();
    let encoded = args["image_base64"].as_str();
    ensure!(
        url.is_some() != encoded.is_some(),
        "Supply one image source: source_url or image_base64"
    );
    let bytes = if let Some(source) = url {
        let parsed = reqwest::Url::parse(source)?;
        ensure!(parsed.scheme() == "https", "Image URL must be HTTPS");
        ensure!(
            parsed.username().is_empty() && parsed.password().is_none(),
            "Image URL credentials are unsupported"
        );
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(seconds.clamp(1, 45)))
            .build()?;
        let response = client.get(source).send().await?.error_for_status()?;
        ensure!(
            response.content_length().unwrap_or(0) <= 8_000_000,
            "Image exceeds 8MB"
        );
        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;
        let mut data = Vec::new();
        while let Some(chunk) = stream.next().await {
            ensure!(!cancel.load(Ordering::SeqCst), "Image download cancelled");
            let chunk = chunk?;
            ensure!(data.len() + chunk.len() <= 8_000_000, "Image exceeds 8MB");
            data.extend_from_slice(&chunk);
        }
        data
    } else {
        base64::engine::general_purpose::STANDARD.decode(encoded.unwrap())?
    };
    ensure!(!cancel.load(Ordering::SeqCst), "Image import cancelled");
    let png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
    let jpg = bytes.starts_with(&[0xff, 0xd8, 0xff]);
    ensure!(png || jpg, "Only PNG and JPEG images are supported");
    let suffix = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    ensure!(
        if png {
            suffix == "png"
        } else {
            suffix == "jpg" || suffix == "jpeg"
        },
        "Destination extension differs from image format"
    );
    let dimensions = image::load_from_memory(&bytes)
        .context("Invalid image content")?
        .dimensions();
    ensure!(
        dimensions.0 > 0
            && dimensions.1 > 0
            && u64::from(dimensions.0) * u64::from(dimensions.1) <= 80_000_000,
        "Image dimensions exceed limit"
    );
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        if let Err(e) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(e.into());
        }
    }
    let digest = sha2::Sha256::digest(&bytes);
    let mut result = json!({"saved":true,"path":path,"bytes":bytes.len(),"sha256":hex::encode(digest),"format":if png {"png"} else {"jpeg"},"width_px":dimensions.0,"height_px":dimensions.1,"inserted_into_wordcraft":false});
    if let Some(cli_path) = args["wordcraft_cli_path"].as_str() {
        let mut params = json!({"path":destination});
        if let Some(width) = args["width_pt"].as_f64() {
            params["width"] = json!(width);
        }
        if let Some(alt) = args["alt_text"].as_str() {
            params["alt"] = json!(alt);
        }
        let wc = json!({"cli_path":cli_path,"port":args["wordcraft_port"].as_u64().unwrap_or(7981),"tool":"execute","arguments":{"command":"insert.picture","params":params}});
        match crate::wordcraft::run(policy, &wc, cancel, seconds).await {
            Ok(value) => {
                result["inserted_into_wordcraft"] = json!(true);
                result["wordcraft_result"] = value;
            }
            Err(e) => {
                result["wordcraft_error"] = json!(format!("{e:#}"));
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn imports_png_without_overwrite() -> Result<()> {
        let root = tempfile::tempdir()?;
        let mut bytes = Vec::new();
        let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        image::DynamicImage::ImageRgba8(img).write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )?;
        let policy = crate::policy::Policy {
            roots: vec![root.path().to_path_buf()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: true,
            allow_screenshots: false,
            ask_before_sensitive: false,
        };
        let destination = root.path().join("picture.png");
        let args = json!({"destination":destination,"image_base64":base64::engine::general_purpose::STANDARD.encode(&bytes)});
        let first = run(&policy, &args, Arc::new(AtomicBool::new(false)), 25).await?;
        assert_eq!(first["saved"], true);
        assert_eq!(first["width_px"], 3);
        assert_eq!(first["height_px"], 2);
        assert_eq!(std::fs::read(&destination)?, bytes);
        assert!(
            run(&policy, &args, Arc::new(AtomicBool::new(false)), 25)
                .await
                .is_err()
        );
        let wrong = json!({"destination":root.path().join("bad.jpg"),"image_base64":base64::engine::general_purpose::STANDARD.encode(&bytes)});
        assert!(
            run(&policy, &wrong, Arc::new(AtomicBool::new(false)), 25)
                .await
                .is_err()
        );
        Ok(())
    }
}
