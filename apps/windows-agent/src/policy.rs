use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub roots: Vec<PathBuf>,
    #[serde(skip)]
    pub protected_paths: Vec<PathBuf>,
    #[serde(default)]
    pub applications: Vec<PathBuf>,
    #[serde(default)]
    pub allow_all_applications: bool,
    pub allow_modify: bool,
    pub allow_screenshots: bool,
    #[serde(default = "default_confirmation")]
    pub ask_before_sensitive: bool,
}
fn default_confirmation() -> bool {
    true
}
impl Policy {
    pub fn needs_consent(&self, category: &str) -> bool {
        category == "SENSITIVE" && (self.ask_before_sensitive || !self.allow_modify)
    }
    pub fn protect_internal(&mut self, state: &Path) -> Result<()> {
        self.protected_paths = vec![ordinary_path(state.canonicalize()?)];
        let executable = ordinary_path(std::env::current_exe()?.canonicalize()?);
        self.protected_paths.push(
            executable
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Missing executable directory"))?
                .to_path_buf(),
        );
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let installed_state = PathBuf::from(local).join("OpenCommander");
            if installed_state.is_dir() {
                self.protected_paths
                    .push(ordinary_path(installed_state.canonicalize()?));
            }
        }
        for root in &self.roots {
            let approved = ordinary_path(root.canonicalize()?);
            ensure!(
                !self.protected_paths.iter().any(|p| approved.starts_with(p)),
                "Approved root is inside OpenCommander internal storage"
            );
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.roots.is_empty(),
            "At least one local root is required"
        );
        for root in &self.roots {
            ensure!(
                root.is_absolute() && root.is_dir(),
                "Root must be an existing absolute directory"
            );
            reject_links(root)?;
            let canonical = ordinary_path(root.canonicalize()?);
            ensure!(
                canonical.is_absolute() && !canonical.to_string_lossy().starts_with("\\\\"),
                "Only local filesystem roots are supported"
            );
        }
        Ok(())
    }
    pub fn application(&self, input: &str) -> Result<PathBuf> {
        let path = Path::new(input);
        ensure!(
            path.is_absolute() && !input.contains('\0'),
            "Application must be an existing absolute executable"
        );
        #[cfg(windows)]
        ensure!(
            !input.starts_with("\\\\") && !input[2..].contains(':'),
            "Network executables, device namespaces and alternate data streams are prohibited"
        );
        ensure!(
            path.is_file(),
            "Application must be an existing absolute executable"
        );
        reject_links(path)?;
        let resolved = ordinary_path(path.canonicalize()?);
        ensure!(
            resolved.is_absolute() && !resolved.to_string_lossy().starts_with("\\\\"),
            "Network executables are prohibited"
        );
        ensure!(
            self.allow_all_applications
                || self
                    .applications
                    .iter()
                    .any(|p| p.canonicalize().is_ok_and(|v| ordinary_path(v) == resolved)),
            "Application is not in the locally configured allowlist"
        );
        Ok(resolved)
    }
    pub fn path(&self, input: &str) -> Result<PathBuf> {
        let path = Path::new(input);
        ensure!(path.is_absolute(), "Absolute paths are required");
        ensure!(!input.contains('\0'), "Invalid path");
        ensure!(
            !path.components().any(|c| {
                let n = c.as_os_str().to_string_lossy().to_lowercase();
                matches!(
                    n.as_str(),
                    ".ssh" | ".aws" | ".azure" | ".codex" | "credentials.dpapi" | "pending.dpapi"
                ) || n == ".env"
                    || n.starts_with(".env.")
            }),
            "Credential locations are excluded from remote file access"
        );
        #[cfg(windows)]
        ensure!(
            !input.starts_with("\\\\") && !input[2..].contains(':'),
            "UNC, device namespaces and alternate data streams are prohibited"
        );
        ensure!(
            !path.components().any(|c| matches!(c, Component::ParentDir)),
            "Parent traversal prohibited"
        );
        reject_links(path)?;
        let mut existing = path.to_path_buf();
        let mut suffix = vec![];
        while !existing.exists() {
            suffix.push(
                existing
                    .file_name()
                    .ok_or_else(|| anyhow::anyhow!("Invalid path"))?
                    .to_owned(),
            );
            ensure!(existing.pop(), "Cannot resolve path");
        }
        let mut resolved = ordinary_path(existing.canonicalize()?);
        ensure!(
            resolved.is_absolute() && !resolved.to_string_lossy().starts_with("\\\\"),
            "Mapped network paths are prohibited"
        );
        for name in suffix.into_iter().rev() {
            resolved.push(name);
        }
        ensure!(
            !self.protected_paths.iter().any(|p| resolved.starts_with(p)),
            "OpenCommander internal storage is excluded from remote file access"
        );
        for root in &self.roots {
            let approved = ordinary_path(root.canonicalize()?);
            if resolved.starts_with(&approved) {
                return Ok(resolved);
            }
        }
        bail!("Path outside approved roots")
    }
    pub fn authorize(&self, action: &str, args: &Value) -> Result<&'static str> {
        let category = category(action, args)?;
        if category == "MODIFY" {
            ensure!(self.allow_modify, "Local policy denies modification");
        }
        if matches!(action, "desktop_screenshot" | "desktop_inspect") {
            ensure!(self.allow_screenshots, "Local policy denies screenshots");
        }
        for key in ["path", "destination", "cwd"] {
            if let Some(p) = args.get(key).and_then(Value::as_str) {
                if key == "path" && action == "application_launch" {
                    self.application(p)?;
                } else {
                    self.path(p)?;
                }
            }
        }
        if category == "SENSITIVE" {
            let limit = if self.needs_consent(category) {
                1024
            } else if action == "media_transfer" && args["op"] == "chunk" {
                1_150_000
            } else {
                64 * 1024
            };
            ensure!(
                serde_json::to_vec(args)?.len() <= limit,
                "Sensitive request exceeds local execution or consent preview limit"
            );
        }
        Ok(category)
    }
}
fn ordinary_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(drive) = text.strip_prefix("\\\\?\\")
            && !drive.starts_with("UNC\\")
        {
            return PathBuf::from(drive);
        }
    }
    path
}
pub fn category(action: &str, args: &Value) -> Result<&'static str> {
    match action {
        "system_info" | "system_inventory" | "file_export" | "file_read" | "file_search"
        | "desktop_windows" | "desktop_screenshot" | "desktop_inspect" | "terminal_screen" => {
            Ok("READ")
        }
        "file_write" | "file_batch" | "file_undo" => Ok("MODIFY"),
        "file_recovery" => match args["op"].as_str() {
            Some("list") => Ok("READ"),
            Some("restore") => Ok("MODIFY"),
            _ => bail!("Unsupported recovery operation"),
        },
        "visual_agent" | "workflow" | "wordcraft_compose" | "image_place" | "asset_deliver"
        | "assets_deliver" | "media_transfer" | "wordcraft" | "mangamotion" | "terminal_execute"
        | "terminal_input" | "application_launch" | "desktop_interact" | "desktop_automation"
        | "clipboard_access" => Ok("SENSITIVE"),
        "process_manage" => match args["op"].as_str() {
            Some("list") => Ok("READ"),
            Some("terminate") => Ok("SENSITIVE"),
            _ => bail!("Unsupported process operation"),
        },
        "file_operation" => match args["op"].as_str() {
            Some("list" | "metadata" | "hash" | "disk_usage") => Ok("READ"),
            Some("mkdir" | "copy" | "move" | "rename" | "edit" | "zip" | "unzip") => Ok("MODIFY"),
            Some("delete") => Ok("SENSITIVE"),
            _ => bail!("Unsupported file operation"),
        },
        _ => bail!("Unsupported action"),
    }
}
pub fn reject_links(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(m) => {
                ensure!(!m.file_type().is_symlink(), "Symbolic links prohibited");
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(
                        m.file_attributes() & 0x400 == 0,
                        "Windows reparse points/junctions prohibited"
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
pub struct PathGuard {
    _directories: Vec<std::fs::File>,
}
pub fn guard_parent(path: &Path) -> Result<PathGuard> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Path has no parent"))?;
    let mut handles = vec![];
    let mut prefix = PathBuf::new();
    for component in parent.components() {
        prefix.push(component);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            if matches!(component, Component::Prefix(_)) {
                continue;
            }
            let file = std::fs::OpenOptions::new()
                .access_mode(0x81)
                .share_mode(3)
                .custom_flags(0x02000000 | 0x00200000)
                .open(&prefix)?;
            validate_handle(&file)?;
            ensure!(file.metadata()?.is_dir(), "Ancestor is not a directory");
            handles.push(file);
        }
        #[cfg(not(windows))]
        {
            reject_links(&prefix)?;
            handles.push(std::fs::File::open(&prefix)?);
        }
    }
    Ok(PathGuard {
        _directories: handles,
    })
}
#[cfg(windows)]
fn validate_handle(file: &std::fs::File) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::*;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe {
        ensure!(
            GetFileInformationByHandle(file.as_raw_handle(), &mut info) != 0,
            "Cannot validate file handle"
        );
    }
    ensure!(
        info.dwFileAttributes & 0x400 == 0,
        "Reparse point handle prohibited"
    );
    ensure!(
        info.dwFileAttributes & 0x10 != 0 || info.nNumberOfLinks == 1,
        "Hard-linked files are prohibited"
    );
    Ok(())
}
pub fn open_file(path: &Path) -> Result<std::fs::File> {
    open_file_access(path, false)
}
pub fn open_file_delete(path: &Path) -> Result<std::fs::File> {
    open_file_access(path, true)
}
fn open_file_access(path: &Path, delete: bool) -> Result<std::fs::File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .access_mode(0x80000000 | if delete { 0x00010000 } else { 0 })
            .share_mode(1)
            .custom_flags(0x00200000)
            .open(path)?;
        validate_handle(&file)?;
        ensure!(file.metadata()?.is_file(), "Expected an ordinary file");
        Ok(file)
    }
    #[cfg(not(windows))]
    {
        let _ = delete;
        reject_links(path)?;
        Ok(std::fs::File::open(path)?)
    }
}
pub fn delete_open_file(file: &std::fs::File) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::*;
        let info = FILE_DISPOSITION_INFO { DeleteFile: true };
        unsafe {
            ensure!(
                SetFileInformationByHandle(
                    file.as_raw_handle(),
                    FileDispositionInfo,
                    (&info as *const FILE_DISPOSITION_INFO).cast(),
                    std::mem::size_of_val(&info) as u32
                ) != 0,
                "OS refused deletion of the checked file handle"
            );
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = file;
        bail!("Handle deletion requires Windows")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_applications_requires_local_setting_and_preserves_path_validation() -> Result<()> {
        let root = tempfile::tempdir()?;
        let executable = std::env::current_exe()?;
        let mut p: Policy = serde_json::from_value(
            serde_json::json!({"roots":[root.path()],"applications":[],"allow_modify":true,"allow_screenshots":false}),
        )?;
        assert!(!p.allow_all_applications);
        let args = serde_json::json!({"path":executable,"allow_all_applications":true});
        assert!(p.authorize("application_launch", &args).is_err());
        p.allow_all_applications = true;
        assert_eq!(p.authorize("application_launch", &args)?, "SENSITIVE");
        let restored: Policy = serde_json::from_slice(&serde_json::to_vec(&p)?)?;
        assert!(restored.allow_all_applications);
        assert!(
            restored
                .application(&root.path().to_string_lossy())
                .is_err()
        );
        assert!(restored.application("relative.exe").is_err());
        assert!(
            restored
                .application(&root.path().join("missing.exe").to_string_lossy())
                .is_err()
        );
        assert!(
            restored
                .application("\\\\server\\share\\application.exe")
                .is_err()
        );
        p.allow_all_applications = false;
        assert!(p.application(&executable.to_string_lossy()).is_err());
        p.applications.push(executable.clone());
        assert!(p.application(&executable.to_string_lossy()).is_ok());
        Ok(())
    }
    #[test]
    fn confirmation_is_default_and_only_local_policy_can_disable_it() -> Result<()> {
        let root = tempfile::tempdir()?;
        let mut p: Policy = serde_json::from_value(
            serde_json::json!({"roots":[root.path()],"applications":[],"allow_modify":true,"allow_screenshots":false}),
        )?;
        let args = serde_json::json!({"cwd":root.path(),"command":"echo approved","ask_before_sensitive":false,"category":"READ"});
        let category = p.authorize("terminal_execute", &args)?;
        ensure!(
            category == "SENSITIVE" && p.needs_consent(category),
            "Remote arguments changed confirmation policy"
        );
        p.ask_before_sensitive = false;
        ensure!(!p.needs_consent(p.authorize("terminal_execute", &args)?));
        ensure!(
            p.path(&root.path().join("../outside.txt").to_string_lossy())
                .is_err()
        );
        let mut restored: Policy = serde_json::from_slice(&serde_json::to_vec(&p)?)?;
        ensure!(
            !restored.needs_consent("SENSITIVE"),
            "Trusted setting did not persist"
        );
        restored.allow_modify = false;
        ensure!(
            restored.needs_consent("SENSITIVE"),
            "Read-only policy silently allowed sensitive operations"
        );
        restored.allow_modify = true;
        restored.ask_before_sensitive = true;
        ensure!(
            restored.needs_consent("SENSITIVE"),
            "Confirmation mode could not be restored"
        );
        Ok(())
    }
    #[test]
    fn profile_root_allows_work_but_protects_internal_state() -> Result<()> {
        let profile = tempfile::tempdir()?;
        let state = profile.path().join("AppData/Local/OpenCommander");
        let documents = profile.path().join("Documents");
        let desktop = profile.path().join("Desktop");
        let sibling = profile.path().join("AppData/Local/OpenCommander-public");
        for folder in [&state, &documents, &desktop, &sibling] {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(state.join("config.json"), "private config")?;
        std::fs::write(state.join("internal-fixture.txt"), "private journal")?;
        std::fs::write(documents.join("profile-fixture.txt"), "ordinary document")?;
        let mut p = Policy {
            roots: vec![profile.path().into()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: true,
            allow_screenshots: false,
            ask_before_sensitive: true,
        };
        p.validate()?;
        p.protect_internal(&state)?;
        for folder in [&documents, &desktop, &sibling] {
            p.authorize(
                "file_write",
                &serde_json::json!({"path":folder.join("new.txt")}),
            )?;
        }
        for input in [
            state.clone(),
            state.join("config.json"),
            state.join("new.txt"),
        ] {
            ensure!(
                p.path(&input.to_string_lossy()).is_err(),
                "Internal storage was exposed"
            );
        }
        let listed = crate::files::execute(
            &p,
            "file_operation",
            &serde_json::json!({"op":"list","path":state.parent()}),
        )?;
        ensure!(
            !listed["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["name"] == "OpenCommander")
        );
        let searched = crate::files::execute(
            &p,
            "file_search",
            &serde_json::json!({"path":profile.path(),"query":"fixture"}),
        )?;
        let matches = searched["matches"].as_array().unwrap();
        ensure!(
            matches.len() == 1
                && matches[0]
                    .as_str()
                    .unwrap()
                    .ends_with("profile-fixture.txt")
        );
        crate::files::execute(
            &p,
            "file_operation",
            &serde_json::json!({"op":"disk_usage","path":profile.path()}),
        )?;
        let encoded = serde_json::to_value(&p)?;
        ensure!(
            encoded.get("protected_paths").is_none(),
            "Runtime exclusions must not be editable configuration"
        );
        let mut reloaded: Policy = serde_json::from_value(encoded)?;
        reloaded.protect_internal(&state)?;
        ensure!(
            reloaded
                .path(&state.join("config.json").to_string_lossy())
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn denies_escape_and_reclassification() -> Result<()> {
        let t = tempfile::tempdir()?;
        let p = Policy {
            roots: vec![t.path().to_path_buf()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: false,
            allow_screenshots: false,
            ask_before_sensitive: true,
        };
        ensure!(p.path(&t.path().join("ok.txt").to_string_lossy()).is_ok());
        ensure!(
            p.path(&t.path().join("..").join("secret.txt").to_string_lossy())
                .is_err()
        );
        ensure!(p.path("relative.txt").is_err());
        ensure!(p.authorize("file_write", &serde_json::json!({})).is_err());
        ensure!(
            category("terminal_execute", &serde_json::json!({"category":"READ"}))? == "SENSITIVE"
        );
        ensure!(
            p.authorize("desktop_screenshot", &serde_json::json!({}))
                .is_err()
        );
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    fn rejects_junction_and_alternate_streams() -> Result<()> {
        ensure!(
            ordinary_path(PathBuf::from("\\\\?\\UNC\\server\\share\\folder")).is_absolute(),
            "UNC must never become a relative path"
        );
        let root = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let link = root.path().join("junction");
        let output = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()?;
        ensure!(output.status.success(), "Cannot create fixture junction");
        let p = Policy {
            roots: vec![root.path().to_path_buf()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: true,
            allow_screenshots: false,
            ask_before_sensitive: true,
        };
        ensure!(p.path(&link.join("private.txt").to_string_lossy()).is_err());
        ensure!(
            p.path(&root.path().join("file.txt:secret").to_string_lossy())
                .is_err()
        );
        std::fs::remove_dir(link)?;
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    fn handle_guards_deny_hardlinks_and_ancestor_rename() -> Result<()> {
        let root = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let secret = outside.path().join("private.txt");
        std::fs::write(&secret, "private")?;
        let linked = root.path().join("alias.txt");
        std::fs::hard_link(&secret, &linked)?;
        ensure!(
            open_file(&linked).is_err(),
            "Hardlink must not disclose outside file content"
        );
        let parent = root.path().join("folder");
        std::fs::create_dir(&parent)?;
        let guard = guard_parent(&parent.join("new.txt"))?;
        ensure!(
            std::fs::rename(&parent, root.path().join("renamed")).is_err(),
            "Locked ancestor was renamed"
        );
        drop(guard);
        std::fs::rename(&parent, root.path().join("renamed"))?;
        ensure!(std::fs::read_to_string(secret)? == "private");
        Ok(())
    }
}
