use crate::{
    execution::{Output, describe_exit, job},
    terminal_input::Sessions,
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::File,
    os::windows::{ffi::OsStrExt, io::FromRawHandle},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::io::AsyncReadExt;
use windows_sys::Win32::{
    Foundation::*,
    System::{Console::*, Pipes::CreatePipe, Threading::*},
};

struct Handle(HANDLE);
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
impl Handle {
    fn file(self) -> File {
        let raw = self.0;
        std::mem::forget(self);
        unsafe { File::from_raw_handle(raw) }
    }
}
struct Process(Handle);
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            TerminateProcess(self.0.0, 1);
        }
    }
}

pub struct Control {
    console: Mutex<Option<HPCON>>,
    parser: Mutex<vt100::Parser>,
}
impl Control {
    pub fn resize(&self, (cols, rows): (u16, u16)) -> Result<()> {
        let console = self
            .console
            .lock()
            .map_err(|_| anyhow::anyhow!("Console lock failed"))?;
        let console = console.as_ref().context("ConPTY has closed")?;
        ensure!(
            unsafe {
                ResizePseudoConsole(
                    *console,
                    COORD {
                        X: cols as i16,
                        Y: rows as i16,
                    },
                )
            } >= 0,
            "ConPTY resize failed"
        );
        self.parser
            .lock()
            .map_err(|_| anyhow::anyhow!("Screen lock failed"))?
            .screen_mut()
            .set_size(rows, cols);
        Ok(())
    }
    pub fn application_cursor(&self) -> bool {
        self.parser
            .lock()
            .is_ok_and(|parser| parser.screen().application_cursor())
    }
    pub fn screen(&self) -> Result<Value> {
        let parser = self
            .parser
            .lock()
            .map_err(|_| anyhow::anyhow!("Screen lock failed"))?;
        let screen = parser.screen();
        let (rows, cols) = screen.size();
        let (row, col) = screen.cursor_position();
        let mut text = screen.contents();
        let truncated = text.len() > 16384;
        if truncated {
            let mut end = 16384;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        Ok(
            json!({"text":text,"rows":rows,"cols":cols,"cursor":{"row":row,"col":col,"visible":!screen.hide_cursor()},"alternate_screen":screen.alternate_screen(),"application_cursor":screen.application_cursor(),"truncated":truncated,"coordinates":"zero-based character cells","verification_state":"observed"}),
        )
    }
    fn process(&self, bytes: &[u8]) -> Result<()> {
        self.parser
            .lock()
            .map_err(|_| anyhow::anyhow!("Screen lock failed"))?
            .process(bytes);
        Ok(())
    }
    async fn close(&self) -> Result<()> {
        let handle = self
            .console
            .lock()
            .map_err(|_| anyhow::anyhow!("Console lock failed"))?
            .take();
        if let Some(handle) = handle {
            tokio::task::spawn_blocking(move || unsafe { ClosePseudoConsole(handle) }).await?;
        }
        Ok(())
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        if let Ok(handle) = self.console.get_mut()
            && let Some(handle) = handle.take()
        {
            // Closing may emit a final frame; the output reader remains independent.
            let _ = std::thread::Builder::new()
                .name("conpty-close".into())
                .spawn(move || unsafe { ClosePseudoConsole(handle) });
        }
    }
}
struct CloseOnDrop(Arc<Control>);
impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        if let Ok(mut console) = self.0.console.lock()
            && let Some(handle) = console.take()
        {
            let _ = std::thread::Builder::new()
                .name("conpty-close".into())
                .spawn(move || unsafe { ClosePseudoConsole(handle) });
        }
    }
}
fn wide(value: &OsStr) -> Result<Vec<u16>> {
    let mut value: Vec<u16> = value.encode_wide().collect();
    ensure!(!value.contains(&0), "NUL in process argument");
    value.push(0);
    Ok(value)
}
fn quote(value: &OsStr) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for c in value.to_string_lossy().chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat_n(
            '\\',
            if c == '"' { slashes * 2 + 1 } else { slashes },
        ));
        slashes = 0;
        result.push(c);
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}
fn pipe() -> Result<(Handle, Handle)> {
    let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
    ensure!(
        unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } != 0,
        "ConPTY pipe creation failed: {}",
        std::io::Error::last_os_error()
    );
    Ok((Handle(read), Handle(write)))
}
struct Attributes {
    memory: Vec<usize>,
}
impl Attributes {
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.memory.as_mut_ptr().cast()
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.pointer());
        }
    }
}
fn launch(
    command: &std::process::Command,
    args: &Value,
    size: (u16, u16),
) -> Result<(
    Arc<Control>,
    tokio::fs::File,
    tokio::fs::File,
    Process,
    Handle,
    u32,
)> {
    let (input_read, input_write) = pipe()?;
    let (output_read, output_write) = pipe()?;
    let mut console = 0;
    ensure!(
        unsafe {
            CreatePseudoConsole(
                COORD {
                    X: size.0 as i16,
                    Y: size.1 as i16,
                },
                input_read.0,
                output_write.0,
                0,
                &mut console,
            )
        } >= 0,
        "ConPTY creation failed; Windows 10 1809 or newer required"
    );
    let control = Arc::new(Control {
        console: Mutex::new(Some(console)),
        parser: Mutex::new(vt100::Parser::new(size.1, size.0, 200)),
    });
    let mut bytes = 0;
    unsafe {
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut bytes);
    }
    ensure!(bytes > 0, "ConPTY attribute size unavailable");
    let mut memory = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    ensure!(
        unsafe { InitializeProcThreadAttributeList(memory.as_mut_ptr().cast(), 1, 0, &mut bytes) }
            != 0,
        "ConPTY attributes failed"
    );
    let mut attributes = Attributes { memory };
    ensure!(
        unsafe {
            UpdateProcThreadAttribute(
                attributes.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console as *const _,
                std::mem::size_of::<HPCON>(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        } != 0,
        "ConPTY attribute assignment failed"
    );
    let program = if args["shell"] == "cmd" {
        std::path::PathBuf::from(std::env::var_os("WINDIR").context("WINDIR missing")?)
            .join("System32/cmd.exe")
    } else {
        command.get_program().into()
    };
    let executable = wide(program.as_os_str())?;
    let line = if args["shell"] == "cmd" {
        format!(
            "{} /D /S /C \"{}\"",
            quote(program.as_os_str()),
            args["command"].as_str().context("Missing command")?
        )
    } else {
        std::iter::once(quote(program.as_os_str()))
            .chain(command.get_args().map(quote))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut line = wide(OsStr::new(&line))?;
    let cwd = wide(
        command
            .get_current_dir()
            .context("Missing cwd")?
            .as_os_str(),
    )?;
    let mut environment: BTreeMap<String, (std::ffi::OsString, std::ffi::OsString)> =
        std::env::vars_os()
            .map(|(key, value)| (key.to_string_lossy().to_uppercase(), (key, value)))
            .collect();
    for (key, value) in command.get_envs() {
        let normalized = key.to_string_lossy().to_uppercase();
        if let Some(value) = value {
            environment.insert(normalized, (key.into(), value.into()));
        } else {
            environment.remove(&normalized);
        }
    }
    let mut env = Vec::new();
    for (_, (key, value)) in environment {
        let entry = format!("{}={}", key.to_string_lossy(), value.to_string_lossy());
        env.extend(wide(OsStr::new(&entry))?);
    }
    env.push(0);
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    // Null standard handles let ConPTY provide console handles instead of copying redirected parent pipes.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.lpAttributeList = attributes.pointer();
    let mut info = PROCESS_INFORMATION::default();
    ensure!(
        unsafe {
            CreateProcessW(
                executable.as_ptr(),
                line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
                env.as_ptr().cast(),
                cwd.as_ptr(),
                &startup.StartupInfo,
                &mut info,
            )
        } != 0,
        "ConPTY process launch failed: {}",
        std::io::Error::last_os_error()
    );
    drop(input_read);
    drop(output_write);
    Ok((
        control,
        tokio::fs::File::from_std(input_write.file()),
        tokio::fs::File::from_std(output_read.file()),
        Process(Handle(info.hProcess)),
        Handle(info.hThread),
        info.dwProcessId,
    ))
}
async fn read(
    mut file: tokio::fs::File,
    control: Arc<Control>,
    output: Option<Output>,
) -> Result<(Vec<u8>, bool)> {
    let mut collected = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 4096];
    let mut pending = Vec::new();
    loop {
        let n = match file.read(&mut buffer).await {
            Ok(n) => n,
            Err(error) if error.raw_os_error() == Some(109) => 0,
            Err(error) => return Err(error.into()),
        };
        if n == 0 {
            break;
        }
        control.process(&buffer[..n])?;
        let available = 65536usize.saturating_sub(collected.len());
        collected.extend_from_slice(&buffer[..n.min(available)]);
        truncated |= n > available;
        pending.extend_from_slice(&buffer[..n]);
        for text in decode(&mut pending) {
            truncated |= !crate::execution::publish_output(output.as_ref(), "stdout", text)?;
        }
    }
    if !pending.is_empty() {
        crate::execution::publish_output(
            output.as_ref(),
            "stdout",
            String::from_utf8_lossy(&pending).into_owned(),
        )?;
    }
    Ok((collected, truncated))
}
fn decode(pending: &mut Vec<u8>) -> Vec<String> {
    let mut decoded = Vec::new();
    while !pending.is_empty() {
        let (valid, invalid) = match std::str::from_utf8(pending) {
            Ok(_) => (pending.len(), None),
            Err(error) => (error.valid_up_to(), error.error_len()),
        };
        if valid > 0 {
            decoded.push(String::from_utf8_lossy(&pending[..valid]).into_owned());
            pending.drain(..valid);
        }
        if let Some(length) = invalid {
            decoded.push("\u{fffd}".into());
            pending.drain(..length);
        } else {
            break;
        }
    }
    decoded
}
pub async fn run(
    command: &std::process::Command,
    args: &Value,
    cancel: Arc<AtomicBool>,
    seconds: u64,
    output: Option<Output>,
    (sessions, owner, task_id): (&Sessions, &str, &str),
) -> Result<Value> {
    let cols = args["cols"].as_u64().unwrap_or(80);
    let rows = args["rows"].as_u64().unwrap_or(24);
    ensure!(
        (20..=240).contains(&cols) && (5..=100).contains(&rows),
        "ConPTY size out of bounds"
    );
    let (control, input, reader, process, thread, pid) =
        launch(command, args, (cols as u16, rows as u16))?;
    let _close = CloseOnDrop(control.clone());
    let reader = tokio::spawn(read(reader, control.clone(), output.clone()));
    let job = job(pid)?;
    let registration = sessions.attach_pty(owner, task_id, input, control.clone())?;
    ensure!(
        unsafe { ResumeThread(thread.0) } == 1,
        "Cannot resume ConPTY child"
    );
    drop(thread);
    if let Some(text) = args["stdin"].as_str() {
        sessions
            .send(owner, &json!({"target_task_id":task_id,"text":text}))
            .await?;
    }
    let end = tokio::time::Instant::now() + Duration::from_secs(seconds);
    let mut aborted = None;
    let mut code = 0;
    loop {
        if cancel.load(Ordering::SeqCst) || tokio::time::Instant::now() >= end {
            aborted = Some(if cancel.load(Ordering::SeqCst) {
                "Command cancelled"
            } else {
                "Command timed out"
            });
            break;
        }
        let state = unsafe { WaitForSingleObject(process.0.0, 0) };
        if state == WAIT_OBJECT_0 {
            ensure!(
                unsafe { GetExitCodeProcess(process.0.0, &mut code) } != 0,
                "Cannot read ConPTY exit code"
            );
            break;
        }
        ensure!(state == WAIT_TIMEOUT, "Cannot wait for ConPTY child");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    drop(job);
    drop(registration);
    drop(process);
    control.close().await?;
    let result = tokio::time::timeout(Duration::from_secs(5), reader)
        .await
        .context("ConPTY output drain timeout")???;
    if let Some(reason) = aborted {
        bail!(reason)
    }
    let mut result = json!({"exit_code":code,"stdout":String::from_utf8_lossy(&result.0),"stderr":"","output_truncated":result.1,"streaming":output.is_some(),"interactive":true,"pty":true,"stdin_mode":"conpty","output_format":"utf8_vt","stderr_merged":true,"screen":control.screen()?});
    describe_exit(&mut result, args)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{execution::terminal_with_output, policy::Policy};
    fn policy(root: &std::path::Path) -> Policy {
        Policy {
            roots: vec![root.to_path_buf()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: true,
            allow_screenshots: false,
            ask_before_sensitive: false,
        }
    }
    async fn wait_screen(sessions: &Sessions, marker: &str) -> Result<Value> {
        let mut last = Value::Null;
        for _ in 0..200 {
            if let Ok(screen) = sessions.screen("alice", &json!({"target_task_id":"tui-fixture"})) {
                last = screen.clone();
                if screen["text"]
                    .as_str()
                    .is_some_and(|text| text.contains(marker))
                {
                    return Ok(screen);
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        bail!("TUI did not display {marker}: {last}")
    }
    struct Cancel(Arc<AtomicBool>);
    impl Drop for Cancel {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    #[tokio::test]
    async fn conpty_renders_full_screen_keys_unicode_resize_and_nonzero_exit() -> Result<()> {
        let root = tempfile::tempdir()?;
        let p = policy(root.path());
        let sessions = Sessions::default();
        let registry = sessions.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let _cancel = Cancel(cancel.clone());
        let cwd = root.path().to_path_buf();
        let task = tokio::spawn(async move {
            terminal_with_output(&p,&json!({"cwd":cwd,"interactive":true,"pty":true,"command":"$e=[char]27;[Console]::Write(\"$e[2J$e[HREADY_TUI\");$k=[Console]::ReadKey($true);[Console]::Write(\"$e[2;1HKEY:$($k.Key)\");$line=[Console]::ReadLine();[Console]::Write(\"$e[3;1HTEXT:$line\");[Console]::ReadKey($true)|Out-Null;[Console]::Write(\"$e[4;1HSIZE:$([Console]::WindowWidth)x$([Console]::WindowHeight)\");[Console]::ReadKey($true)|Out-Null;[Console]::Write(\"$e[5;1HDONE_TUI\");exit 3"}),cancel,20,None,Some((&registry,"alice","tui-fixture"))).await
        });
        let first = wait_screen(&sessions, "READY_TUI").await?;
        ensure!(first["cols"] == 80 && first["rows"] == 24);
        ensure!(
            sessions
                .screen("mallory", &json!({"target_task_id":"tui-fixture"}))
                .is_err()
        );
        ensure!(
            sessions
                .send(
                    "mallory",
                    &json!({"target_task_id":"tui-fixture","keys":["UP"]})
                )
                .await
                .is_err()
        );
        ensure!(
            sessions
                .send("alice", &json!({"target_task_id":"tui-fixture","eof":true}))
                .await
                .is_err()
        );
        let sent = sessions
            .send(
                "alice",
                &json!({"target_task_id":"tui-fixture","keys":["UP"]}),
            )
            .await?;
        ensure!(sent["input_sent"] == true);
        wait_screen(&sessions, "KEY:UpArrow").await?;
        sessions
            .send(
                "alice",
                &json!({"target_task_id":"tui-fixture","text":"世界","keys":["ENTER"]}),
            )
            .await?;
        wait_screen(&sessions, "TEXT:世界").await?;
        sessions
            .send(
                "alice",
                &json!({"target_task_id":"tui-fixture","cols":120,"rows":40}),
            )
            .await?;
        let resized = sessions.screen("alice", &json!({"target_task_id":"tui-fixture"}))?;
        ensure!(resized["cols"] == 120 && resized["rows"] == 40);
        sessions
            .send(
                "alice",
                &json!({"target_task_id":"tui-fixture","keys":["ENTER"]}),
            )
            .await?;
        wait_screen(&sessions, "SIZE:120x40").await?;
        sessions
            .send(
                "alice",
                &json!({"target_task_id":"tui-fixture","keys":["ENTER"]}),
            )
            .await?;
        let result = task.await??;
        ensure!(
            result["exit_code"] == 3
                && result["execution_succeeded"] == true
                && result["program_succeeded"] == false
                && result["pty"] == true
        );
        ensure!(
            result["stdout"]
                .as_str()
                .is_some_and(|text| text.contains("DONE_TUI"))
        );
        ensure!(
            sessions
                .screen("alice", &json!({"target_task_id":"tui-fixture"}))
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn screen_tracks_vt_erase_cursor_alternate_buffer_and_split_utf8() -> Result<()> {
        let control = Control {
            console: Mutex::new(None),
            parser: Mutex::new(vt100::Parser::new(24, 80, 200)),
        };
        control.process(b"old\x1b[2J\x1b[Hnew\x1b[3;5H")?;
        control.process(&[0xe4, 0xb8])?;
        control.process(&[0x96])?;
        let screen = control.screen()?;
        ensure!(
            screen["text"].as_str().unwrap().contains("世")
                && !screen["text"].as_str().unwrap().contains("old")
        );
        control.process(b"\x1b[?1049h\x1b[?1h\x1b[?25lALT")?;
        let alternate = control.screen()?;
        ensure!(
            alternate["alternate_screen"] == true
                && alternate["application_cursor"] == true
                && alternate["cursor"]["visible"] == false
        );
        control.process(b"\x1b[?1049l")?;
        ensure!(control.screen()?["text"].as_str().unwrap().contains("new"));
        Ok(())
    }
    #[tokio::test]
    async fn conpty_timeout_kills_the_child_tree() -> Result<()> {
        let root = tempfile::tempdir()?;
        let sessions = Sessions::default();
        let pid = root.path().join("child.pid");
        let command = format!(
            "$p=Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList '-NoProfile -Command Start-Sleep -Seconds 30' -NoNewWindow -PassThru;[IO.File]::WriteAllText('{}',[string]$p.Id);Start-Sleep -Seconds 30",
            pid.to_string_lossy().replace('\'', "''")
        );
        let result = terminal_with_output(
            &policy(root.path()),
            &json!({"cwd":root.path(),"interactive":true,"pty":true,"command":command}),
            Arc::new(AtomicBool::new(false)),
            3,
            None,
            Some((&sessions, "alice", "timeout-fixture")),
        )
        .await;
        ensure!(result.is_err());
        let pid = std::fs::read_to_string(pid)?.parse()?;
        let child = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if !child.is_null() {
            let handle = Handle(child);
            ensure!(
                unsafe { WaitForSingleObject(handle.0, 2000) } == WAIT_OBJECT_0,
                "Child survived ConPTY timeout"
            );
        }
        ensure!(
            sessions
                .screen("alice", &json!({"target_task_id":"timeout-fixture"}))
                .is_err()
        );
        Ok(())
    }
}
