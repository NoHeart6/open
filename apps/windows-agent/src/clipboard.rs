use anyhow::{Result, ensure};
use serde_json::{Value, json};
#[cfg(windows)]
pub fn execute(a: &Value) -> Result<Value> {
    use windows_sys::Win32::{
        Foundation::GlobalFree,
        System::{DataExchange::*, Memory::*},
        UI::WindowsAndMessaging::*,
    };
    crate::desktop::available()?;
    struct Clipboard;
    impl Drop for Clipboard {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    unsafe {
        let mut owner = 0;
        GetWindowThreadProcessId(GetForegroundWindow(), &mut owner);
        ensure!(
            owner != std::process::id(),
            "Clipboard operations cannot target OpenCommander controls"
        );
        ensure!(
            OpenClipboard(std::ptr::null_mut()) != 0,
            "Clipboard is currently owned by another application"
        );
        let _guard = Clipboard;
        let op = a["op"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing clipboard operation"))?;
        if op == "read" {
            ensure!(
                IsClipboardFormatAvailable(13) != 0,
                "Clipboard does not contain Unicode text"
            );
            let handle = GetClipboardData(13);
            ensure!(!handle.is_null(), "Clipboard text unavailable");
            let size = GlobalSize(handle);
            ensure!(
                size <= 32770,
                "Clipboard text exceeds the permitted read budget"
            );
            let pointer = GlobalLock(handle).cast::<u16>();
            ensure!(!pointer.is_null(), "Cannot lock clipboard text");
            let all = std::slice::from_raw_parts(pointer, size / 2);
            let end = all.iter().position(|&c| c == 0).unwrap_or(all.len());
            let text = String::from_utf16_lossy(&all[..end]);
            GlobalUnlock(handle);
            return Ok(json!({"text":text,"sequence":GetClipboardSequenceNumber()}));
        }
        ensure!(op == "write", "Unsupported clipboard operation");
        let text = a["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing clipboard text"))?;
        ensure!(
            text.len() <= 512 && !text.contains('\0'),
            "Clipboard text exceeds consent budget"
        );
        let wide = text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let memory = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
        ensure!(!memory.is_null(), "Cannot allocate clipboard text");
        let pointer = GlobalLock(memory).cast::<u16>();
        if pointer.is_null() {
            GlobalFree(memory);
            anyhow::bail!("Cannot lock clipboard allocation");
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), pointer, wide.len());
        GlobalUnlock(memory);
        if EmptyClipboard() == 0 {
            GlobalFree(memory);
            anyhow::bail!("Cannot replace clipboard");
        }
        if SetClipboardData(13, memory).is_null() {
            GlobalFree(memory);
            anyhow::bail!("Cannot set clipboard text");
        }
        let observed = GetClipboardData(13);
        let pointer = GlobalLock(observed).cast::<u16>();
        ensure!(!pointer.is_null(), "Cannot verify clipboard write");
        let verified = std::slice::from_raw_parts(pointer, wide.len()) == wide;
        GlobalUnlock(observed);
        ensure!(verified, "Clipboard readback differs from requested text");
        Ok(json!({"written":true,"verified":true,"sequence":GetClipboardSequenceNumber()}))
    }
}
#[cfg(not(windows))]
pub fn execute(_: &Value) -> Result<Value> {
    anyhow::bail!("Clipboard requires Windows")
}
