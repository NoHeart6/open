use anyhow::{Result, ensure};
use std::io::Write;
use std::path::Path;

#[cfg(windows)]
fn protect(input: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};
    let source = CRYPT_INTEGER_BLOB {
        cbData: input.len().try_into()?,
        pbData: input.as_ptr().cast_mut(),
    };
    let mut target = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &source,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut target,
            )
        } else {
            CryptUnprotectData(
                &source,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut target,
            )
        }
    };
    ensure!(ok != 0, "DPAPI failed: {}", std::io::Error::last_os_error());
    let result =
        unsafe { std::slice::from_raw_parts(target.pbData, target.cbData as usize).to_vec() };
    unsafe {
        LocalFree(target.pbData.cast());
    }
    Ok(result)
}
#[cfg(not(windows))]
fn protect(_input: &[u8], _encrypt: bool) -> Result<Vec<u8>> {
    anyhow::bail!("Protected agent storage requires Windows DPAPI")
}
pub fn save(path: &Path, bytes: &[u8]) -> Result<()> {
    let encrypted = protect(bytes, true)?;
    save_encrypted(path, &encrypted)
}
fn save_encrypted(path: &Path, encrypted: &[u8]) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("Missing storage parent"))?,
    )?;
    temp.write_all(encrypted)?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(())
}
static BUDGET_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub fn save_bounded(path: &Path, bytes: &[u8], max_files: usize, max_bytes: u64) -> Result<()> {
    let encrypted = protect(bytes, true)?;
    let _lock = BUDGET_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Storage budget lock failed"))?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Missing storage parent"))?;
    let mut count = 1usize;
    let mut size = encrypted.len() as u64;
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        if entry.path() == path {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        count += 1;
        size = size.saturating_add(meta.len());
        ensure!(
            count <= max_files && size <= max_bytes,
            "Protected storage capacity exceeded; existing outcomes are preserved"
        );
    }
    ensure!(
        count <= max_files && size <= max_bytes,
        "Protected storage capacity exceeded; existing outcomes are preserved"
    );
    save_encrypted(path, &encrypted)
}
pub fn load(path: &Path) -> Result<Vec<u8>> {
    protect(&std::fs::read(path)?, false)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn dpapi_is_protected_and_atomic() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("secret.dpapi");
        save(&path, b"first secret")?;
        ensure!(std::fs::read(&path)? != b"first secret");
        ensure!(load(&path)? == b"first secret");
        save(&path, b"second secret")?;
        ensure!(load(&path)? == b"second secret");
        Ok(())
    }
    #[test]
    fn storage_limits_refuse_growth_without_erasing_outcomes() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let first = dir.path().join("first.dpapi");
        let second = dir.path().join("second.dpapi");
        save_bounded(&first, b"old outcome", 2, 65536)?;
        save_bounded(&second, b"another outcome", 2, 65536)?;
        ensure!(save_bounded(&dir.path().join("third.dpapi"), b"new", 2, 65536).is_err());
        ensure!(load(&first)? == b"old outcome" && load(&second)? == b"another outcome");
        ensure!(save_bounded(&first, &[0; 8192], 2, 1024).is_err());
        ensure!(load(&first)? == b"old outcome");
        save_bounded(&first, b"acknowledged", 2, 65536)?;
        ensure!(load(&first)? == b"acknowledged");
        Ok(())
    }
}
