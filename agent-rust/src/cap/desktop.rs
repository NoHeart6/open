use anyhow::Result;
use base64::Engine;
use serde_json::{json, Value};

pub fn launch(app: String) -> Result<Value> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("cmd");
        c.creation_flags(0x00000008);
        c.arg("/C").arg("start").arg("").arg(&app);
        let st = c.spawn()?;
        return Ok(json!({"launched": app, "pid": st.id()}));
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        anyhow::bail!("application_launch only on Windows")
    }
}

pub fn screenshot(_monitor: usize) -> Result<Value> {
    #[cfg(windows)]
    {
        // v0.1: use PowerShell + System.Drawing for a real screenshot without extra native deps.
        let ps = r#"Add-Type -AssemblyName System.Drawing; Add-Type -AssemblyName System.Windows.Forms; $b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds; $bmp=New-Object System.Drawing.Bitmap($b.Width,$b.Height); $g=[System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Location,[System.Drawing.Point]::Empty,$b.Size); $ms=New-Object System.IO.MemoryStream; $bmp.Save($ms,[System.Drawing.Imaging.ImageFormat]::Png); [Convert]::ToBase64String($ms.ToArray())"#;
        let out = std::process::Command::new("powershell")
            .arg("-NoProfile").arg("-NonInteractive").arg("-Command").arg(ps)
            .output()?;
        if !out.status.success() {
            anyhow::bail!("screenshot failed");
        }
        let b64 = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if b64.is_empty() {
            anyhow::bail!("empty screenshot");
        }
        let _ = base64::engine::general_purpose::STANDARD.decode(&b64)?;
        return Ok(json!({"mime": "image/png", "base64": b64, "note": "pass through as MCP image content"}));
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("screenshot only on Windows")
    }
}

pub fn windows() -> Value {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("powershell")
            .arg("-NoProfile").arg("-NonInteractive").arg("-Command")
            .arg(r#"Get-Process | Where-Object {$_.MainWindowTitle} | Select-Object Id,ProcessName,@{n='Title';e={$_.MainWindowTitle}} | ConvertTo-Json -Depth 2"#)
            .output();
        if let Ok(o) = out {
            if let Ok(v) = serde_json::from_slice::<Value>(&o.stdout) {
                return json!({"windows": v});
            }
        }
        return json!({"windows": []});
    }
    #[cfg(not(windows))]
    {
        json!({"windows": []})
    }
}

pub fn interact(params: Value) -> Result<Value> {
    let action = params.get("action").and_then(|v| v.as_str()).unwrap_or("");
    match action {
        "type" | "key" | "move" | "click" | "focus" | "scroll" | "clipboard_get" | "clipboard_set" => {
            Ok(json!({"ok": false, "deferred": true,
                "note": "v0.1 defers raw input to native consent build; use screenshot+windows+launch first"}))
        }
        _ => anyhow::bail!("unknown interact action"),
    }
}
