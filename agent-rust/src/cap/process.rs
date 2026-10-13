use anyhow::Result;
use serde_json::{json, Value};
use sysinfo::System;

pub fn list(filter: Option<&str>) -> Value {
    let mut s = System::new_all();
    s.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut out = vec![];
    for (pid, p) in s.processes() {
        let name = p.name().to_string_lossy().into_owned();
        if let Some(f) = filter {
            if !name.to_lowercase().contains(&f.to_lowercase()) {
                continue;
            }
        }
        out.push(json!({"pid": pid.as_u32(), "name": name, "cpu": p.cpu_usage(), "mem_kb": p.memory()}));
        if out.len() >= 300 {
            break;
        }
    }
    json!({"processes": out})
}

pub fn manage(action: &str, pid: u32) -> Result<Value> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        match action {
            "kill" => {
                let mut c = std::process::Command::new("taskkill");
                c.creation_flags(0x08000000);
                let st = c.arg("/PID").arg(pid.to_string()).arg("/F").status()?;
                return Ok(json!({"ok": st.success()}));
            }
            "suspend" | "resume" => anyhow::bail!("{action} needs PsSuspend; not bundled in v0.1"),
            _ => anyhow::bail!("unknown action"),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (action, pid);
        anyhow::bail!("process_manage only on Windows")
    }
}
