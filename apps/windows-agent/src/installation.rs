use anyhow::{Result, ensure};
#[cfg(windows)]
mod native {
    use super::*;
    use std::{
        path::PathBuf,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::System::Registry::*;
    const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const UNINSTALL: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\OpenCommander";
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    fn target() -> Result<PathBuf> {
        let base = PathBuf::from(std::env::var("LOCALAPPDATA")?);
        ensure!(base.is_absolute(), "LOCALAPPDATA must be absolute");
        Ok(base.join("Programs").join("OpenCommander"))
    }
    fn set(key: &str, name: &str, value: &str) -> Result<()> {
        let mut handle = null_mut();
        unsafe {
            ensure!(
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    wide(key).as_ptr(),
                    0,
                    null(),
                    0,
                    KEY_SET_VALUE,
                    null(),
                    &mut handle,
                    null_mut()
                ) == 0,
                "Registry creation failed"
            );
            let text = wide(value);
            let result = RegSetValueExW(
                handle,
                wide(name).as_ptr(),
                0,
                REG_SZ,
                text.as_ptr().cast(),
                (text.len() * 2) as u32,
            );
            RegCloseKey(handle);
            ensure!(result == 0, "Registry value failed");
        }
        Ok(())
    }
    fn stop_installed(exe: &std::path::Path, state: &std::path::Path, force: bool) -> Result<()> {
        let expected = exe.canonicalize()?;
        let matches = |system: &sysinfo::System| -> Vec<u32> {
            system
                .processes()
                .iter()
                .filter(|(pid, p)| {
                    pid.as_u32() != std::process::id()
                        && p.exe().and_then(|p| p.canonicalize().ok()).as_ref() == Some(&expected)
                })
                .map(|(pid, _)| pid.as_u32())
                .collect()
        };
        let running = matches(&sysinfo::System::new_all());
        ensure!(
            running.len() <= 1,
            "More than one installed agent is running"
        );
        for pid in &running {
            crate::storage::save(
                &state.join("local-control.dpapi"),
                &serde_json::to_vec(
                    &serde_json::json!({"pid":pid,"at":crate::now(),"shutdown":true}),
                )?,
            )?;
        }
        for _ in 0..100 {
            if matches(&sysinfo::System::new_all()).is_empty() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        ensure!(
            force,
            "Agent did not stop gracefully; upgrade cancelled without replacing it"
        );
        let system = sysinfo::System::new_all();
        for pid in matches(&system) {
            ensure!(
                system
                    .process(sysinfo::Pid::from_u32(pid))
                    .is_some_and(|p| p.kill()),
                "Cannot stop installed agent"
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        ensure!(
            matches(&sysinfo::System::new_all()).is_empty(),
            "Installed agent is still running"
        );
        Ok(())
    }
    pub fn install(upgrade: bool, launch: bool) -> Result<()> {
        let folder = target()?;
        let state = PathBuf::from(std::env::var("LOCALAPPDATA")?).join("OpenCommander");
        install_at(
            upgrade,
            launch,
            &std::env::current_exe()?,
            &folder,
            &state,
            RUN,
            UNINSTALL,
        )
    }
    fn install_at(
        upgrade: bool,
        launch: bool,
        source: &std::path::Path,
        folder: &std::path::Path,
        state: &std::path::Path,
        run_key: &str,
        uninstall_key: &str,
    ) -> Result<()> {
        ensure!(
            std::env::consts::ARCH == "x86_64",
            "Windows x64 is required"
        );
        crate::policy::reject_links(folder)?;
        std::fs::create_dir_all(folder)?;
        let exe = folder.join("OpenCommander.exe");
        let legacy = folder.join("opencommander-agent.exe");
        crate::policy::reject_links(&legacy)?;
        ensure!(
            !(exe.exists() && legacy.exists()),
            "Two installation layouts found; preserve both and resolve before upgrading"
        );
        let previous = if exe.exists() { &exe } else { &legacy };
        ensure!(
            source.canonicalize()? != exe.canonicalize().unwrap_or_default(),
            "Already running from the installation directory"
        );
        ensure!(
            !previous.exists() || upgrade,
            "Existing installation requires --upgrade; pairing will be preserved"
        );
        let mut staged = tempfile::NamedTempFile::new_in(folder)?;
        std::io::copy(&mut std::fs::File::open(source)?, &mut staged)?;
        staged.as_file().sync_all()?;
        let backup = folder.join("OpenCommander.previous.exe");
        if previous.exists() {
            stop_installed(previous, state, false)?;
            crate::policy::reject_links(&backup)?;
            if backup.exists() {
                std::fs::remove_file(&backup)?;
            }
            std::fs::rename(previous, &backup)?;
        }
        if let Err(error) = staged.persist_noclobber(&exe) {
            if backup.exists() {
                std::fs::rename(&backup, previous)?;
            }
            return Err(error.into());
        }
        let verified = std::process::Command::new(&exe).arg("--version").output();
        if !verified.is_ok_and(|v| {
            v.status.success()
                && String::from_utf8_lossy(&v.stdout).contains(env!("CARGO_PKG_VERSION"))
        }) {
            std::fs::remove_file(&exe)?;
            if backup.exists() {
                std::fs::rename(&backup, previous)?;
            }
            anyhow::bail!(
                "New executable did not pass startup validation; previous executable restored"
            );
        }
        let path = exe.to_string_lossy();
        set(
            run_key,
            "OpenCommander",
            &format!("\"{path}\" --background"),
        )?;
        set(uninstall_key, "DisplayName", "OpenCommander")?;
        set(uninstall_key, "DisplayVersion", env!("CARGO_PKG_VERSION"))?;
        std::fs::create_dir_all(state)?;
        crate::storage::save(
            &state.join("installed-version.dpapi"),
            env!("CARGO_PKG_VERSION").as_bytes(),
        )?;
        set(uninstall_key, "InstallLocation", &folder.to_string_lossy())?;
        set(
            uninstall_key,
            "UninstallString",
            &format!("\"{path}\" --uninstall"),
        )?;
        println!("Installed per-user at {}", folder.display());
        if launch {
            use std::os::windows::process::CommandExt;
            std::process::Command::new(&exe)
                .arg("--background")
                .creation_flags(0x08000000)
                .spawn()?;
        }
        println!(
            "Startup registered for this user. Pairing and permissions are preserved. The native window handles first-run pairing."
        );
        Ok(())
    }
    pub fn uninstall() -> Result<()> {
        let folder = target()?;
        let state = PathBuf::from(std::env::var("LOCALAPPDATA")?).join("OpenCommander");
        uninstall_at(&folder, &state, RUN, UNINSTALL)
    }
    fn uninstall_at(
        folder: &std::path::Path,
        state: &std::path::Path,
        run_key: &str,
        uninstall_key: &str,
    ) -> Result<()> {
        crate::policy::reject_links(folder)?;
        let exe = folder.join("OpenCommander.exe");
        let current = std::process::id();
        if exe.exists() && std::env::current_exe()?.canonicalize()? != exe.canonicalize()? {
            stop_installed(&exe, state, true)?;
        }
        let system = sysinfo::System::new_all();
        for (pid, proc) in system.processes() {
            if pid.as_u32() != current
                && proc
                    .exe()
                    .is_some_and(|p| p.canonicalize().ok() == exe.canonicalize().ok())
                && exe.exists()
            {
                ensure!(proc.kill(), "Cannot stop installed agent");
            }
        }
        unsafe {
            let mut key = null_mut();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(run_key).as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut key,
            ) == 0
            {
                RegDeleteValueW(key, wide("OpenCommander").as_ptr());
                RegCloseKey(key);
            }
            RegDeleteTreeW(HKEY_CURRENT_USER, wide(uninstall_key).as_ptr());
        }
        if std::env::current_exe()?.canonicalize()? != exe.canonicalize().unwrap_or_default() {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if exe.exists() {
                std::fs::remove_file(&exe)?;
            }
            let backup = folder.join("OpenCommander.previous.exe");
            if backup.exists() {
                crate::policy::reject_links(&backup)?;
                std::fs::remove_file(backup)?;
            }
            if folder.exists() {
                std::fs::remove_dir(folder)?;
            }
        } else {
            // One PowerShell process owns the delayed cleanup, with fixed checked absolute targets.
            let file = exe.to_string_lossy().replace('\'', "''");
            let dir = folder.to_string_lossy().replace('\'', "''");
            let script = format!(
                "Start-Sleep -Seconds 2; Remove-Item -LiteralPath '{file}' -Force; $previous=Join-Path '{dir}' 'OpenCommander.previous.exe'; if(Test-Path -LiteralPath $previous){{Remove-Item -LiteralPath $previous -Force}}; Remove-Item -LiteralPath '{dir}'"
            );
            let mut command = std::process::Command::new("powershell.exe");
            command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
            command.spawn()?;
        }
        println!(
            "Startup and uninstall registration removed. Local encrypted state is preserved; revoke the device at the gateway."
        );
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn isolated_install_upgrade_rollback_and_uninstall_preserve_pairing() -> Result<()> {
            let sandbox = tempfile::tempdir()?;
            let folder = sandbox.path().join("application");
            let state = sandbox.path().join("state");
            std::fs::create_dir_all(&state)?;
            let fixture_credentials = crate::Credentials {
                device_id: "isolated-device".into(),
                owner: "isolated-owner".into(),
                seed: [9; 32],
                gateway_public_key: base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    [8; 32],
                ),
            };
            crate::storage::save(
                &state.join("credentials.dpapi"),
                &serde_json::to_vec(&fixture_credentials)?,
            )?;
            let root = sandbox.path().join("files");
            std::fs::create_dir(&root)?;
            let cfg = crate::Config {
                gateway: "https://127.0.0.1:1".into(),
                policy: crate::policy::Policy {
                    roots: vec![root],
                    protected_paths: vec![],
                    applications: vec![],
                    allow_all_applications: false,
                    allow_modify: false,
                    allow_screenshots: false,
                    ask_before_sensitive: true,
                },
                dev: false,
            };
            std::fs::write(state.join("config.json"), serde_json::to_vec(&cfg)?)?;
            crate::storage::save(&state.join("access.dpapi"), &[2])?;
            let original = std::fs::read(state.join("credentials.dpapi"))?;
            let source =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/OpenCommander.exe");
            ensure!(
                source.exists(),
                "Build the native artifact before installer acceptance"
            );
            let key = format!(
                "Software\\OpenCommanderTest\\{}",
                sandbox.path().file_name().unwrap().to_string_lossy()
            );
            let run = format!("{key}\\Run");
            let uninstall = format!("{key}\\Uninstall");
            struct Cleanup(String);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    unsafe {
                        RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.0).as_ptr());
                    }
                }
            }
            let _cleanup = Cleanup(key);
            install_at(false, false, &source, &folder, &state, &run, &uninstall)?;
            let exe = folder.join("OpenCommander.exe");
            ensure!(
                std::fs::read(&exe)? == std::fs::read(&source)?,
                "Installer changed candidate bytes"
            );
            ensure!(
                install_at(false, false, &source, &folder, &state, &run, &uninstall).is_err(),
                "Replacement without upgrade was accepted"
            );
            let legacy = folder.join("opencommander-agent.exe");
            std::fs::rename(&exe, &legacy)?;
            install_at(true, false, &source, &folder, &state, &run, &uninstall)?;
            ensure!(
                exe.exists() && !legacy.exists(),
                "Legacy executable layout was not migrated"
            );
            ensure!(
                std::fs::read(state.join("credentials.dpapi"))? == original,
                "Legacy layout migration changed pairing"
            );
            struct Running(std::process::Child);
            impl Drop for Running {
                fn drop(&mut self) {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
            let mut running = Running(
                std::process::Command::new(&exe)
                    .arg("--state")
                    .arg(&state)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()?,
            );
            let mut ready = false;
            for _ in 0..100 {
                ensure!(
                    running.0.try_wait()?.is_none(),
                    "Fixture agent exited before upgrade"
                );
                if let Ok(lock) = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(state.join("agent.lock"))
                    && lock.try_lock().is_err()
                {
                    ready = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            ensure!(ready, "Fixture agent did not acquire its state lock");
            install_at(true, false, &source, &folder, &state, &run, &uninstall)?;
            ensure!(
                running.0.try_wait()?.is_some_and(|s| s.success()),
                "Upgrade did not gracefully stop the fixture agent"
            );
            ensure!(
                folder.join("OpenCommander.previous.exe").exists(),
                "Upgrade lost the rollback binary"
            );
            let bad = sandbox.path().join("invalid.exe");
            std::fs::write(&bad, b"invalid executable fixture")?;
            ensure!(
                install_at(true, false, &bad, &folder, &state, &run, &uninstall).is_err(),
                "Invalid candidate was accepted"
            );
            ensure!(
                std::fs::read(&exe)? == std::fs::read(&source)?,
                "Invalid upgrade did not restore the old executable"
            );
            ensure!(
                std::fs::read(state.join("credentials.dpapi"))? == original,
                "Upgrade changed encrypted pairing"
            );
            uninstall_at(&folder, &state, &run, &uninstall)?;
            ensure!(!folder.exists(), "Uninstall left application files");
            ensure!(
                std::fs::read(state.join("credentials.dpapi"))? == original,
                "Uninstall removed private pairing state"
            );
            unsafe {
                let mut handle = null_mut();
                ensure!(
                    RegOpenKeyExW(
                        HKEY_CURRENT_USER,
                        wide(&uninstall).as_ptr(),
                        0,
                        KEY_READ,
                        &mut handle
                    ) != 0,
                    "Uninstall registration remains"
                );
            }
            Ok(())
        }
    }
}
#[cfg(windows)]
pub use native::*;
#[cfg(not(windows))]
pub fn install(_: bool, _: bool) -> Result<()> {
    anyhow::bail!("Installer requires Windows")
}
#[cfg(not(windows))]
pub fn uninstall() -> Result<()> {
    anyhow::bail!("Installer requires Windows")
}
