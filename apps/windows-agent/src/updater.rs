use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

const CHANNEL: &str = "https://mcp-production-394c.up.railway.app/releases/windows/stable.json";
static CHECKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload: String,
    signature: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    version: String,
    platform: String,
    url: String,
    sha256: String,
    size: u64,
    issued_at: u64,
    expires_at: u64,
}
fn version(raw: &str) -> Result<[u64; 3]> {
    let parts = raw
        .split('.')
        .map(|p| p.parse::<u64>())
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        parts.len() == 3
            && parts
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(".")
                == raw,
        "Release version must be canonical major.minor.patch"
    );
    Ok([parts[0], parts[1], parts[2]])
}
fn verify(bytes: &[u8], key: &[u8; 32], current: &str, highest: &str, at: u64) -> Result<Release> {
    ensure!(bytes.len() <= 8192, "Release metadata exceeds budget");
    let envelope: Envelope = serde_json::from_slice(bytes)?;
    let payload = STANDARD.decode(envelope.payload)?;
    ensure!(payload.len() <= 4096, "Release payload exceeds budget");
    VerifyingKey::from_bytes(key)?.verify_strict(
        &payload,
        &Signature::from_slice(&STANDARD.decode(envelope.signature)?)?,
    )?;
    let release: Release = serde_json::from_slice(&payload)?;
    ensure!(
        release.platform == "windows-x86_64",
        "Release targets a different platform"
    );
    ensure!(
        release.issued_at <= at + 300
            && release.expires_at > at
            && release.expires_at.saturating_sub(release.issued_at) <= 180 * 86400,
        "Expired or invalid release dates"
    );
    ensure!(
        version(&release.version)? >= version(current)?
            && version(&release.version)? >= version(highest)?,
        "Release downgrade refused"
    );
    let url = url::Url::parse(&release.url)?;
    let channel = url::Url::parse(CHANNEL)?;
    ensure!(
        url.origin() == channel.origin()
            && url.scheme() == "https"
            && url.path().starts_with("/releases/windows/")
            && url.path().ends_with(".exe")
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Release URL is outside the trusted channel"
    );
    ensure!(
        release.size > 0
            && release.size <= 100 * 1024 * 1024
            && release.sha256.len() == 64
            && release
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "Invalid package size or checksum"
    );
    Ok(release)
}
fn trusted_key() -> Result<[u8; 32]> {
    let encoded = option_env!("OPENCOMMANDER_RELEASE_PUBLIC_KEY")
        .context("No release verification key was compiled into this build")?;
    hex::decode(encoded)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid compiled release key"))
}
async fn check(state: &Path) -> Result<Release> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut response = client.get(CHANNEL).send().await?.error_for_status()?;
    let mut bytes = vec![];
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 8192,
            "Release metadata too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    let highest = crate::storage::load(&state.join("installed-version.dpapi"))
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());
    verify(
        &bytes,
        &trusted_key()?,
        env!("CARGO_PKG_VERSION"),
        &highest,
        crate::now(),
    )
}
async fn download(state: &Path, release: &Release) -> Result<PathBuf> {
    let folder = state.join("updates");
    crate::policy::reject_links(&folder)?;
    std::fs::create_dir_all(&folder)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut response = client.get(&release.url).send().await?.error_for_status()?;
    ensure!(
        response.content_length().is_none_or(|n| n == release.size),
        "Package content length differs from signed metadata"
    );
    let mut tmp = tempfile::NamedTempFile::new_in(&folder)?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    while let Some(chunk) = response.chunk().await? {
        size += chunk.len() as u64;
        ensure!(size <= release.size, "Download exceeds signed size");
        hash.update(&chunk);
        tmp.write_all(&chunk)?;
    }
    ensure!(
        size == release.size && hex::encode(hash.finalize()) == release.sha256,
        "Downloaded package checksum mismatch"
    );
    tmp.as_file().sync_all()?;
    let target = folder.join(format!("OpenCommanderSetup-{}.exe", release.version));
    crate::policy::reject_links(&target)?;
    tmp.persist(&target)?;
    Ok(target)
}
fn status(state: &Path, message: &str) -> Result<()> {
    crate::storage::save(
        &state.join("update-status.dpapi"),
        &serde_json::to_vec(&serde_json::json!({"at":crate::now(),"message":message}))?,
    )
}
pub fn launch_check(state: PathBuf) {
    if CHECKING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        struct CheckGuard;
        impl Drop for CheckGuard {
            fn drop(&mut self) {
                CHECKING.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _guard = CheckGuard;
        let result = (|| -> Result<()> {
            status(&state, "Checking signed release metadata…")?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let release = runtime.block_on(check(&state))?;
            if version(&release.version)? == version(env!("CARGO_PKG_VERSION"))? {
                return status(
                    &state,
                    "This version is current on the signed release channel.",
                );
            }
            #[cfg(windows)]
            {
                use windows_sys::Win32::UI::WindowsAndMessaging::*;
                let body:Vec<u16>=format!("OpenCommander {} is available. Download and install this verified update? Pairing and permissions will be preserved.",release.version).encode_utf16().chain(Some(0)).collect();
                let title: Vec<u16> = "OpenCommander update"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                if unsafe {
                    MessageBoxW(
                        std::ptr::null_mut(),
                        body.as_ptr(),
                        title.as_ptr(),
                        MB_YESNO | MB_DEFBUTTON2 | MB_ICONINFORMATION,
                    )
                } != IDYES
                {
                    return status(&state, "Update available; installation was not requested.");
                }
            }
            status(&state, "Downloading and verifying update…")?;
            let package = runtime.block_on(download(&state, &release))?;
            let mut command = std::process::Command::new(package);
            command.arg("--upgrade");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            command.spawn()?;
            status(
                &state,
                "Verified updater started; installation performs a startup check and rollback on failure.",
            )
        })();
        if let Err(error) = result {
            let _ = status(&state, &format!("Update refused: {error:#}"));
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    #[test]
    fn signed_metadata_rejects_tampering_downgrades_expiry_and_external_packages() -> Result<()> {
        let key = SigningKey::from_bytes(&[29; 32]);
        let mut release=Release {version:"0.2.0".into(),platform:"windows-x86_64".into(),url:"https://mcp-production-394c.up.railway.app/releases/windows/OpenCommanderSetup-0.2.0.exe".into(),sha256:"a".repeat(64),size:1024,issued_at:100,expires_at:200};
        let sign = |r: &Release| -> Result<Vec<u8>> {
            let p = serde_json::to_vec(r)?;
            Ok(serde_json::to_vec(
                &serde_json::json!({"payload":STANDARD.encode(&p),"signature":STANDARD.encode(key.sign(&p).to_bytes())}),
            )?)
        };
        ensure!(
            verify(
                &sign(&release)?,
                key.verifying_key().as_bytes(),
                "0.1.0",
                "0.1.0",
                150
            )
            .is_ok()
        );
        ensure!(
            verify(
                &sign(&release)?,
                key.verifying_key().as_bytes(),
                "0.3.0",
                "0.1.0",
                150
            )
            .is_err()
        );
        ensure!(
            verify(
                &sign(&release)?,
                key.verifying_key().as_bytes(),
                "0.1.0",
                "0.3.0",
                150
            )
            .is_err()
        );
        ensure!(
            verify(
                &sign(&release)?,
                key.verifying_key().as_bytes(),
                "0.1.0",
                "0.1.0",
                201
            )
            .is_err()
        );
        ensure!(
            verify(
                &sign(&release)?,
                SigningKey::from_bytes(&[30; 32]).verifying_key().as_bytes(),
                "0.1.0",
                "0.1.0",
                150
            )
            .is_err()
        );
        release.url = "https://evil.example/package.exe".into();
        ensure!(
            verify(
                &sign(&release)?,
                key.verifying_key().as_bytes(),
                "0.1.0",
                "0.1.0",
                150
            )
            .is_err()
        );
        Ok(())
    }
}
