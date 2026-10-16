use anyhow::Result;
use std::path::Path;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

pub const IDLE_SECONDS: u64 = 3600;
static ACTIVITY_LOCK: Mutex<()> = Mutex::new(());

fn last_activity(state: &Path) -> Result<u64> {
    let marker = state.join("mcp-activity.dpapi");
    if marker.exists() {
        return Ok(serde_json::from_slice(&crate::storage::load(&marker)?)?);
    }
    let mut latest = None;
    for folder in ["journal", "output"] {
        let path = state.join(folder);
        crate::policy::reject_links(&path)?;
        if !path.exists() {
            continue;
        }
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let modified = entry
                    .metadata()?
                    .modified()?
                    .duration_since(UNIX_EPOCH)?
                    .as_secs();
                latest = Some(latest.unwrap_or(0).max(modified));
            }
        }
    }
    let at = latest.unwrap_or_else(crate::now);
    crate::storage::save(&marker, &serde_json::to_vec(&at)?)?;
    Ok(at)
}

pub fn touch(state: &Path, at: u64) -> Result<()> {
    let _lock = ACTIVITY_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Activity lock failed"))?;
    if at > last_activity(state)? {
        crate::storage::save(&state.join("mcp-activity.dpapi"), &serde_json::to_vec(&at)?)?;
    }
    Ok(())
}

pub fn prune(state: &Path, at: u64, active: bool) -> Result<usize> {
    let _lock = ACTIVITY_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Activity lock failed"))?;
    if active || at.saturating_sub(last_activity(state)?) < IDLE_SECONDS {
        return Ok(0);
    }
    let mut removed = 0;
    for folder in ["journal", "output"] {
        let path = state.join(folder);
        crate::policy::reject_links(&path)?;
        if !path.exists() {
            continue;
        }
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && entry.path().extension().is_some_and(|s| s == "dpapi")
            {
                std::fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
    }
    for name in ["events.log", "events.1.log", "events.2.log"] {
        let path = state.join(name);
        crate::policy::reject_links(&path)?;
        if path.is_file() {
            std::fs::remove_file(path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn idle_cleanup_waits_for_one_hour_and_preserves_configuration() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let state = dir.path();
        for folder in ["journal", "output", "trash", "undo"] {
            std::fs::create_dir(state.join(folder))?;
        }
        let record = state.join("journal/finished.dpapi");
        std::fs::write(&record, b"old result")?;
        std::fs::write(state.join("output/finished.1.dpapi"), b"old output")?;
        std::fs::write(state.join("events.log"), b"old events")?;
        for name in [
            "credentials.dpapi",
            "config.json",
            "trash/receipt.dpapi",
            "undo/receipt.dpapi",
        ] {
            std::fs::write(state.join(name), b"preserve")?;
        }
        let start = crate::now() + 10;
        touch(state, start)?;
        assert_eq!(prune(state, start + 3599, false)?, 0);
        assert_eq!(prune(state, start + 3600, true)?, 0);
        touch(state, start + 3500)?;
        assert_eq!(prune(state, start + 3600, false)?, 0);
        assert_eq!(prune(state, start + 7100, false)?, 3);
        assert!(!record.exists());
        for name in [
            "credentials.dpapi",
            "config.json",
            "trash/receipt.dpapi",
            "undo/receipt.dpapi",
        ] {
            assert_eq!(std::fs::read(state.join(name))?, b"preserve");
        }
        assert_eq!(prune(state, start + 7200, false)?, 0);
        Ok(())
    }

    #[test]
    fn full_journal_can_be_pruned_before_startup_recovery() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let state = dir.path();
        std::fs::create_dir(state.join("journal"))?;
        std::fs::create_dir(state.join("output"))?;
        let record = crate::Record {
            task_id: "old-task".into(),
            status: "running".into(),
            result: serde_json::json!(null),
            error: String::new(),
            fingerprint: "test".into(),
            acknowledged_at: 0,
        };
        crate::save_record(state, &record)?;
        let full = crate::Record {
            task_id: "full-task".into(),
            status: "running".into(),
            result: serde_json::json!("x".repeat(64 * 1024 * 1024)),
            error: String::new(),
            fingerprint: "test".into(),
            acknowledged_at: 0,
        };
        crate::storage::save(
            &state.join("journal/full-task.dpapi"),
            &serde_json::to_vec(&full)?,
        )?;
        assert!(
            format!("{:#}", crate::recover(state).unwrap_err())
                .contains("Protected storage capacity exceeded")
        );
        let start = crate::now() + 10;
        touch(state, start)?;
        assert_eq!(prune(state, start + 3600, false)?, 2);
        crate::recover(state)?;
        crate::save_record(state, &record)?;
        Ok(())
    }
}
