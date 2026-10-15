use crate::{desktop, files, policy::Policy};
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use sysinfo::{Disks, Networks, System, Users};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub struct Context<'a> {
    pub cancel: Arc<AtomicBool>,
    pub seconds: u64,
    pub output: Option<Output>,
    pub state: &'a std::path::Path,
    pub owner: &'a str,
    pub task_id: &'a str,
    pub sessions: &'a crate::terminal_input::Sessions,
}

pub async fn execute(p: &Policy, action: &str, a: Value, context: Context<'_>) -> Result<Value> {
    let Context {
        cancel,
        seconds,
        output,
        state,
        owner,
        task_id,
        sessions,
    } = context;
    match action {
        "file_batch" | "file_undo" => {
            let (p, state, undo) = (p.clone(), state.to_owned(), action == "file_undo");
            tokio::task::spawn_blocking(move || {
                if undo {
                    crate::batch::undo(&p, &state, &a)
                } else {
                    crate::batch::execute(&p, &state, &a)
                }
            })
            .await?
        }
        "clipboard_access" => {
            tokio::task::spawn_blocking(move || crate::clipboard::execute(&a)).await?
        }
        "system_inventory" => crate::system::execute(&a).await,
        "desktop_inspect" | "desktop_automation" => {
            let write = action == "desktop_automation";
            let p = p.clone();
            tokio::task::spawn_blocking(move || {
                let mut result = crate::automation::execute(&a, write)?;
                if write {
                    observe_desktop(&p, &a, &mut result);
                }
                Ok(result)
            })
            .await?
        }
        "file_recovery" => {
            let (p, state) = (p.clone(), state.to_owned());
            tokio::task::spawn_blocking(move || crate::recovery::execute(&p, &state, &a)).await?
        }
        "file_operation" if a["op"] == "delete" => {
            let (p, state) = (p.clone(), state.to_owned());
            tokio::task::spawn_blocking(move || crate::recovery::trash(&p, &state, &a)).await?
        }
        "terminal_execute" => {
            terminal_with_output(
                p,
                &a,
                cancel,
                seconds,
                output,
                Some((sessions, owner, task_id)),
            )
            .await
        }
        "terminal_input" => sessions.send(owner, &a).await,
        "terminal_screen" => sessions.screen(owner, &a),
        "image_place" => crate::image_place::run(p, &a, cancel, seconds).await,
        "asset_deliver" | "assets_deliver" => {
            crate::asset_bridge::run(p, &a, cancel, seconds).await
        }
        "media_transfer" => crate::media_transfer::run(p, &a, cancel, seconds).await,
        "wordcraft" => crate::wordcraft::run(p, &a, cancel, seconds).await,
        "wordcraft_compose" => crate::wordcraft_compose::run(p, &a, cancel, seconds).await,
        "application_launch" => {
            let path = p.application(
                a["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing executable path"))?,
            )?;
            let mut command = Command::new(path);
            if let Some(args) = a["args"].as_array() {
                ensure!(args.len() <= 32, "Too many arguments");
                for arg in args {
                    command.arg(
                        arg.as_str()
                            .ok_or_else(|| anyhow::anyhow!("Invalid application argument"))?,
                    );
                }
            }
            if let Some(cwd) = a["cwd"].as_str() {
                command.current_dir(p.path(cwd)?);
            }
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let mut child = command.spawn()?;
            let pid = child.id();
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
            Ok(json!({"pid":pid}))
        }
        "system_info" => tokio::task::spawn_blocking(system_info).await?,
        "process_manage" => tokio::task::spawn_blocking(move || processes(&a)).await?,
        "desktop_windows" => tokio::task::spawn_blocking(desktop::windows).await?,
        "desktop_screenshot" => tokio::task::spawn_blocking(desktop::screenshot).await?,
        "desktop_interact" => {
            let p = p.clone();
            tokio::task::spawn_blocking(move || {
                let mut result = desktop::interact(&a)?;
                observe_desktop(&p, &a, &mut result);
                Ok(result)
            })
            .await?
        }
        "file_export" => crate::file_export::run(p, &a, cancel),
        "workflow" => crate::workflow::run(p, &a, cancel, seconds).await,
        "visual_agent" => crate::visual_agent::run(p, &a),
        "file_read" | "file_write" | "file_search" | "file_operation" => {
            let p = p.clone();
            let action = action.to_owned();
            tokio::task::spawn_blocking(move || files::execute(&p, &action, &a)).await?
        }
        _ => bail!("Unsupported action"),
    }
}
fn observe_desktop(p: &Policy, args: &Value, result: &mut Value) {
    if !p.allow_screenshots || args["capture_after"].as_bool() == Some(false) {
        result["observation_status"] = json!("disabled");
        return;
    }
    std::thread::sleep(Duration::from_millis(150));
    match desktop::screenshot() {
        Ok(image) => {
            result["screenshot"] = image;
            result["observation_status"] = json!("captured");
        }
        Err(error) => {
            result["observation_status"] = json!("unavailable");
            result["observation_error"] = json!(error.to_string());
        }
    }
    if let Ok(focused) = crate::automation::focused() {
        result["focused_control"] = focused;
    }
}
fn system_info() -> Result<Value> {
    let mut s = System::new_all();
    s.refresh_all();
    std::thread::sleep(Duration::from_millis(200));
    s.refresh_cpu_usage();
    let disks:Vec<Value>=Disks::new_with_refreshed_list().iter().map(|d|json!({"name":d.name().to_string_lossy(),"mount":d.mount_point(),"total_bytes":d.total_space(),"available_bytes":d.available_space()})).collect();
    let networks:Vec<Value>=Networks::new_with_refreshed_list().iter().take(50).map(|(n,d)|json!({"name":n,"received_bytes":d.total_received(),"sent_bytes":d.total_transmitted()})).collect();
    Ok(
        json!({"os":System::name(),"os_version":System::os_version(),"host":System::host_name(),"uptime_seconds":System::uptime(),"cpu_count":s.cpus().len(),"cpu":s.cpus().first().map(|c|c.brand()),"cpu_usage_percent":s.global_cpu_usage(),"memory_total_bytes":s.total_memory(),"memory_used_bytes":s.used_memory(),"disks":disks,"networks":networks,"agent_version":env!("CARGO_PKG_VERSION")}),
    )
}
fn processes(a: &Value) -> Result<Value> {
    let s = System::new_all();
    let users = Users::new_with_refreshed_list();
    if a["op"] == "terminate" {
        let pid = a["pid"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing pid"))?;
        ensure!(
            pid > 4 && pid <= u32::MAX as u64 && pid != u64::from(std::process::id()),
            "Protected process"
        );
        let proc = s
            .process(sysinfo::Pid::from_u32(pid as u32))
            .ok_or_else(|| anyhow::anyhow!("Process not found"))?;
        ensure!(proc.kill(), "OS denied process termination");
        return Ok(json!({"pid":pid,"termination_requested":true}));
    }
    ensure!(a["op"] == "list", "Unsupported process operation");
    let procs:Vec<Value>=s.processes().iter().take(200).map(|(pid,p)|json!({"pid":pid.as_u32(),"name":p.name().to_string_lossy(),"memory_bytes":p.memory(),"cpu_percent":p.cpu_usage(),"owner":p.user_id().and_then(|id|users.get_user_by_id(id)).map(|u|u.name())})).collect();
    Ok(json!({"processes":procs,"limit":200}))
}
#[derive(Clone)]
pub struct Output {
    pub state: std::path::PathBuf,
    pub task_id: String,
    pub sender: tokio::sync::mpsc::Sender<Value>,
    pub sequence: Arc<std::sync::atomic::AtomicU64>,
}
pub fn output_path(state: &std::path::Path, id: &str, seq: u64) -> Result<std::path::PathBuf> {
    crate::journal_path(state, id)?;
    ensure!((1..=512).contains(&seq), "Invalid output sequence");
    Ok(state.join("output").join(format!("{id}.{seq}.dpapi")))
}
async fn drain<R: tokio::io::AsyncRead + Unpin>(
    mut r: R,
    stream: &'static str,
    output: Option<Output>,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut result = vec![];
    let mut buffer = [0; 4096];
    let mut truncated = false;
    loop {
        let n = r.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        let available = 65536usize.saturating_sub(result.len());
        result.extend_from_slice(&buffer[..n.min(available)]);
        truncated |= n > available;
        truncated |= !publish_output(
            output.as_ref(),
            stream,
            String::from_utf8_lossy(&buffer[..n]).into_owned(),
        )
        .map_err(std::io::Error::other)?;
    }
    Ok((result, truncated))
}
pub(crate) fn publish_output(output: Option<&Output>, stream: &str, text: String) -> Result<bool> {
    if let Some(output) = output {
        let seq = output.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        if seq > 512 {
            return Ok(false);
        }
        let chunk =
            json!({"type":"output","task_id":output.task_id,"seq":seq,"stream":stream,"text":text});
        crate::storage::save_bounded(
            &output_path(&output.state, &output.task_id, seq)?,
            &serde_json::to_vec(&chunk)?,
            4096,
            32 * 1024 * 1024,
        )?;
        let _ = output.sender.try_send(chunk);
    }
    Ok(true)
}
#[cfg(windows)]
pub(crate) struct Job(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for Job {}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(windows)]
pub(crate) fn job(pid: u32) -> Result<Job> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{JobObjects::*, Threading::*},
    };
    unsafe {
        let j = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        ensure!(!j.is_null(), "Cannot create process job");
        let guard = Job(j);
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        ensure!(
            SetInformationJobObject(
                j,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32
            ) != 0,
            "Job limit failed"
        );
        let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
        ensure!(!process.is_null(), "Cannot open child process");
        let ok = AssignProcessToJobObject(j, process);
        CloseHandle(process);
        ensure!(ok != 0, "Cannot assign child process to cancellation job");
        Ok(guard)
    }
}
#[cfg(windows)]
fn resume_child(pid: u32) -> Result<()> {
    use windows_sys::Win32::{
        Foundation::*,
        System::{Diagnostics::ToolHelp::*, Threading::*},
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        ensure!(
            snapshot != INVALID_HANDLE_VALUE,
            "Cannot enumerate suspended child thread"
        );
        let guard = Job(snapshot);
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut found = Vec::new();
        let mut valid = Thread32First(guard.0, &mut entry);
        while valid != 0 {
            if entry.th32OwnerProcessID == pid {
                found.push(entry.th32ThreadID);
            }
            valid = Thread32Next(guard.0, &mut entry);
        }
        ensure!(
            found.len() == 1,
            "Suspended child must have exactly one primary thread"
        );
        let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, found[0]);
        ensure!(!thread.is_null(), "Cannot open suspended child thread");
        let thread = Job(thread);
        ensure!(
            ResumeThread(thread.0) == 1,
            "Unexpected primary thread suspension state"
        );
    }
    Ok(())
}
#[cfg(test)]
pub async fn terminal(
    p: &Policy,
    a: &Value,
    cancel: Arc<AtomicBool>,
    seconds: u64,
) -> Result<Value> {
    terminal_with_output(p, a, cancel, seconds, None, None).await
}
pub async fn terminal_with_output(
    p: &Policy,
    a: &Value,
    cancel: Arc<AtomicBool>,
    seconds: u64,
    output: Option<Output>,
    session: Option<(&crate::terminal_input::Sessions, &str, &str)>,
) -> Result<Value> {
    validate_exit_codes(a)?;
    let interactive = a["interactive"].as_bool().unwrap_or(false);
    let initial = a["stdin"].as_str();
    ensure!(
        initial.is_none_or(|text| text.len() <= 16384),
        "Initial stdin size limit"
    );
    ensure!(
        !interactive || session.is_some(),
        "Interactive terminal context is required"
    );
    let cwd = p.path(
        a["cwd"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing cwd"))?,
    )?;
    let text = a["command"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing command"))?;
    ensure!(
        !text.is_empty() && text.len() <= 16384,
        "Invalid command size"
    );
    let mut command = match a["shell"].as_str().unwrap_or("powershell") {
        "powershell" => {
            #[cfg(windows)]
            let executable = std::path::PathBuf::from(
                std::env::var_os("WINDIR")
                    .ok_or_else(|| anyhow::anyhow!("WINDIR is unavailable"))?,
            )
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
            #[cfg(not(windows))]
            let executable = std::path::PathBuf::from("powershell.exe");
            let mut c = Command::new(executable);
            #[cfg(windows)]
            let input_encoding = if a["pty"].as_bool() == Some(true) {
                "[Text.UnicodeEncoding]::new($false,$false)"
            } else {
                "[Text.UTF8Encoding]::new($false)"
            };
            #[cfg(windows)]
            let script = format!(
                "[Console]::InputEncoding={input_encoding};[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);Import-Module -Name (Join-Path $PSHOME 'Modules\\Microsoft.PowerShell.Utility\\Microsoft.PowerShell.Utility.psd1') -ErrorAction Stop\n{text}"
            );
            #[cfg(not(windows))]
            let script = text.to_owned();
            c.args(["-NoLogo", "-NoProfile"]);
            if !interactive {
                c.arg("-NonInteractive");
            }
            c.args(["-Command", &script]);
            c
        }
        "cmd" => {
            let mut c = Command::new("cmd.exe");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.as_std_mut().raw_arg(format!("/D /S /C \"{text}\""));
            }
            #[cfg(not(windows))]
            c.args(["/D", "/S", "/C", text]);
            c
        }
        _ => bail!("Unsupported shell"),
    };
    if let Some(env) = a["env"].as_object() {
        ensure!(env.len() <= 32, "Too many environment variables");
        for (key, value) in env {
            ensure!(
                !key.is_empty() && key.len() <= 128 && !key.contains(['=', '\0']),
                "Invalid environment variable"
            );
            let v = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid environment value"))?;
            ensure!(
                v.len() <= 2048 && !v.contains('\0'),
                "Invalid environment value"
            );
            command.env(key, v);
        }
    }
    #[cfg(windows)]
    if a["shell"].as_str().unwrap_or("powershell") == "powershell" {
        let native_modules = std::path::PathBuf::from(
            std::env::var_os("WINDIR").ok_or_else(|| anyhow::anyhow!("WINDIR is unavailable"))?,
        )
        .join("System32/WindowsPowerShell/v1.0/Modules");
        let requested = a["env"]
            .as_object()
            .and_then(|env| {
                env.iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case("PSModulePath"))
            })
            .and_then(|(_, value)| value.as_str())
            .map(std::ffi::OsString::from)
            .or_else(|| std::env::var_os("PSModulePath"))
            .unwrap_or_default();
        let paths = std::iter::once(native_modules.clone()).chain(
            std::env::split_paths(&requested).filter(|path| {
                !path.as_os_str().is_empty()
                    && !path
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&native_modules.to_string_lossy())
            }),
        );
        command.env("PSModulePath", std::env::join_paths(paths)?);
    }
    command.current_dir(&cwd);
    if a["pty"].as_bool().unwrap_or(false) {
        ensure!(interactive, "pty=true requires interactive=true");
        #[cfg(windows)]
        return crate::conpty::run(
            command.as_std(),
            a,
            cancel,
            seconds,
            output,
            session.unwrap(),
        )
        .await;
        #[cfg(not(windows))]
        bail!("ConPTY requires Windows");
    }
    command
        .current_dir(cwd)
        .stdin(if interactive || initial.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000 | 0x00000004);
    let mut child = command.spawn()?;
    #[cfg(windows)]
    let _job = match job(child
        .id()
        .ok_or_else(|| anyhow::anyhow!("Missing child PID"))?)
    {
        Ok(j) => j,
        Err(e) => {
            child.kill().await?;
            return Err(e);
        }
    };
    #[cfg(windows)]
    if let Err(error) = resume_child(
        child
            .id()
            .ok_or_else(|| anyhow::anyhow!("Missing child PID"))?,
    ) {
        child.kill().await?;
        return Err(error);
    }
    let out = tokio::spawn(drain(
        child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Missing stdout"))?,
        "stdout",
        output.clone(),
    ));
    let err = tokio::spawn(drain(
        child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("Missing stderr"))?,
        "stderr",
        output.clone(),
    ));
    let mut stdin = child.stdin.take();
    if let Some(initial) = initial {
        let input = stdin
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Missing stdin"))?;
        tokio::time::timeout(Duration::from_secs(3), async {
            input.write_all(initial.as_bytes()).await?;
            input.flush().await
        })
        .await??;
    }
    let _input = if interactive {
        let (sessions, owner, task_id) = session.unwrap();
        Some(
            sessions.attach(
                owner,
                task_id,
                stdin
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("Missing interactive stdin"))?,
            )?,
        )
    } else {
        None
    };
    drop(stdin);
    let end = tokio::time::Instant::now() + Duration::from_secs(seconds);
    let status = loop {
        if cancel.load(Ordering::SeqCst) || tokio::time::Instant::now() >= end {
            #[cfg(windows)]
            drop(_job);
            let _ = child.kill().await;
            out.abort();
            err.abort();
            bail!(if cancel.load(Ordering::SeqCst) {
                "Command cancelled"
            } else {
                "Command timed out"
            });
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    #[cfg(windows)]
    drop(_job);
    let (stdout, out_truncated) = out.await??;
    let (stderr, err_truncated) = err.await??;
    let mut result = json!({"exit_code":status.code(),"stdout":String::from_utf8_lossy(&stdout),"stderr":String::from_utf8_lossy(&stderr),"output_truncated":out_truncated||err_truncated,"streaming":output.is_some(),"interactive":interactive,"pty":false,"stdin_mode":if interactive||initial.is_some(){"pipe"}else{"closed"},"output_last_sequence":output.map(|o|o.sequence.load(Ordering::SeqCst).min(512))});
    describe_exit(&mut result, a)?;
    Ok(result)
}
pub(crate) fn describe_exit(result: &mut Value, args: &Value) -> Result<()> {
    validate_exit_codes(args)?;
    let codes = args["success_exit_codes"].as_array();
    ensure!(
        codes.is_none_or(|codes| !codes.is_empty()
            && codes.len() <= 32
            && codes.iter().all(|code| code.as_i64().is_some())),
        "Invalid success exit codes"
    );
    let success = if let Some(codes) = codes {
        codes.iter().any(|code| code == &result["exit_code"])
    } else {
        result["exit_code"] == 0
    };
    result["execution_succeeded"] = json!(true);
    result["program_succeeded"] = json!(success);
    result["program_outcome"] = json!(if success {
        "expected_exit"
    } else {
        "unexpected_exit"
    });
    Ok(())
}
fn validate_exit_codes(args: &Value) -> Result<()> {
    if let Some(codes) = args.get("success_exit_codes") {
        ensure!(
            codes.as_array().is_some_and(|codes| !codes.is_empty()
                && codes.len() <= 32
                && codes.iter().all(|code| code
                    .as_i64()
                    .is_some_and(|code| (-2147483648..=4294967295).contains(&code)))),
            "Invalid success exit codes"
        );
    }
    Ok(())
}
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use anyhow::Context;
    #[tokio::test]
    async fn interactive_stdin_is_owned_streamed_and_closed_with_eof() -> Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::create_dir(root.path().join("output"))?;
        let policy = Policy {
            roots: vec![root.path().to_path_buf()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: true,
            allow_screenshots: false,
            ask_before_sensitive: false,
        };
        let sessions = crate::terminal_input::Sessions::default();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(16);
        let output = Output {
            state: root.path().to_owned(),
            task_id: "stdin-fixture".into(),
            sender,
            sequence: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        };
        let child_sessions = sessions.clone();
        let cwd = root.path().to_owned();
        let child = tokio::spawn(async move {
            terminal_with_output(&policy,&json!({"cwd":cwd,"interactive":true,"command":"[Console]::WriteLine('READY');while(($line=[Console]::ReadLine()) -ne $null){[Console]::WriteLine('ECHO:'+$line)};[Console]::WriteLine('EOF_DONE')"}),Arc::new(AtomicBool::new(false)),10,Some(output),Some((&child_sessions,"alice","stdin-fixture"))).await
        });
        let ready = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await?
            .context("Missing ready output")?;
        ensure!(ready["text"].as_str().unwrap_or_default().contains("READY"));
        let input = json!({"target_task_id":"stdin-fixture","text":"Halo 世界\n"});
        ensure!(sessions.send("mallory", &input).await.is_err());
        let receipt = sessions.send("alice", &input).await?;
        ensure!(receipt["input_sent"] == true);
        let echo = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await?
            .context("Missing echo")?;
        ensure!(
            echo["text"]
                .as_str()
                .unwrap_or_default()
                .contains("ECHO:Halo 世界")
        );
        ensure!(
            sessions
                .send(
                    "alice",
                    &json!({"target_task_id":"stdin-fixture","eof":true})
                )
                .await?["stdin_closed"]
                == true
        );
        let result = child.await??;
        ensure!(result["exit_code"] == 0 && result["interactive"] == true);
        ensure!(
            result["stdout"]
                .as_str()
                .unwrap_or_default()
                .contains("EOF_DONE")
        );
        ensure!(sessions.send("alice", &input).await.is_err());
        let initial = terminal(&Policy {roots:vec![root.path().to_path_buf()],protected_paths:vec![],applications:vec![],allow_all_applications:false,allow_modify:true,allow_screenshots:false,ask_before_sensitive:false}, &json!({"cwd":root.path(),"command":"[Console]::WriteLine([Console]::ReadLine());[Console]::WriteLine([Console]::ReadLine() -eq $null)","stdin":"Initial stdin\n"}), Arc::new(AtomicBool::new(false)),10).await?;
        ensure!(
            initial["stdout"]
                .as_str()
                .unwrap_or_default()
                .contains("Initial stdin")
        );
        ensure!(
            initial["stdout"]
                .as_str()
                .unwrap_or_default()
                .contains("True")
        );
        Ok(())
    }
    #[tokio::test]
    async fn suspended_child_cannot_run_before_job_assignment() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let marker = dir.path().join("spawned.txt");
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(format!(
                "Set-Content -LiteralPath '{}' -Value 'guarded'",
                marker.to_string_lossy().replace('\'', "''")
            ))
            .creation_flags(0x08000000 | 0x00000004)
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        tokio::time::sleep(Duration::from_millis(150)).await;
        ensure!(
            !marker.exists(),
            "Suspended child performed an effect before assignment"
        );
        let guard = job(child.id().context("Missing child")?)?;
        resume_child(child.id().context("Missing child")?)?;
        ensure!(child.wait().await?.success());
        ensure!(std::fs::read_to_string(marker)?.contains("guarded"));
        drop(guard);
        Ok(())
    }
    #[tokio::test]
    async fn execution_timeout_cancel_and_output_bound() -> Result<()> {
        let t = tempfile::tempdir()?;
        let p = Policy {
            roots: vec![t.path().to_path_buf()],
            protected_paths: vec![],
            applications: vec![],
            allow_all_applications: false,
            allow_modify: true,
            allow_screenshots: false,
            ask_before_sensitive: true,
        };
        let hash_file = t.path().join("hash-fixture.txt");
        std::fs::write(&hash_file, b"PowerShell native module fixture")?;
        let native_command = format!(
            "$ErrorActionPreference='Stop'; (Get-FileHash -LiteralPath '{}' -Algorithm SHA256).Hash",
            hash_file.to_string_lossy().replace('\'', "''")
        );
        let hash = terminal(&p, &json!({"cwd":t.path(),"command":native_command,"env":{"PSModulePath":"C:\\nonexistent-module-fixture"}}), Arc::new(AtomicBool::new(false)), 10).await?;
        ensure!(
            hash["exit_code"] == 0,
            "Get-FileHash could not load the native PowerShell module"
        );
        use sha2::{Digest, Sha256};
        ensure!(
            hash["stdout"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .eq_ignore_ascii_case(&hex::encode(Sha256::digest(
                    b"PowerShell native module fixture"
                ))),
            "Get-FileHash returned the wrong digest"
        );
        let a =
            json!({"cwd":t.path(),"shell":"powershell","command":"Write-Output 'verified-rust'"});
        let flag = Arc::new(AtomicBool::new(false));
        ensure!(
            terminal(&p, &a, flag.clone(), 10).await?["stdout"]
                .as_str()
                .unwrap()
                .contains("verified-rust")
        );
        let slow = json!({"cwd":t.path(),"command":"Start-Sleep -Seconds 10"});
        ensure!(terminal(&p, &slow, flag.clone(), 1).await.is_err());
        flag.store(true, Ordering::SeqCst);
        ensure!(terminal(&p, &slow, flag, 10).await.is_err());
        let loud = json!({"cwd":t.path(),"command":"Write-Output ('x' * 100000)"});
        let r = terminal(&p, &loud, Arc::new(AtomicBool::new(false)), 10).await?;
        ensure!(r["output_truncated"] == true);
        ensure!(r["stdout"].as_str().unwrap().len() <= 65536);
        Ok(())
    }
}
