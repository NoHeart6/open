use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};

#[derive(Default)]
pub struct Control {
    pub mode: AtomicU8,
    pub reconnect: AtomicU64,
    pub pair: AtomicBool,
    pub shutdown: AtomicBool,
}
impl Control {
    pub fn cancel_all(&self, cancels: &crate::Cancellations) {
        if let Ok(map) = cancels.lock() {
            for (id, flag) in map.iter() {
                flag.store(true, Ordering::SeqCst);
                let _ = crate::desktop::close_consent(id);
            }
        }
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use anyhow::{Result, ensure};
    use std::{
        path::PathBuf,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::*,
        UI::{Shell::*, WindowsAndMessaging::*},
    };
    const CALLBACK: u32 = WM_APP + 42;
    const STATUS: i32 = 100;
    const ROOTS: i32 = 101;
    const APPS: i32 = 102;
    const MODIFY: i32 = 103;
    const SCREENSHOTS: i32 = 104;
    const CONFIRM_SENSITIVE: i32 = 105;
    const ALL_APPS: i32 = 106;
    const SAVE: usize = 200;
    const PAUSE: usize = 201;
    const DISCONNECT: usize = 202;
    const RECONNECT: usize = 203;
    const PAIR: usize = 204;
    const ACTIVITY: usize = 205;
    const APPROVALS: usize = 206;
    const DIAGNOSTICS: usize = 207;
    const UPDATE: usize = 208;
    const EXIT: usize = 209;
    static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    struct Context {
        state: PathBuf,
        control: Arc<Control>,
        cancels: crate::Cancellations,
        icon: NOTIFYICONDATAW,
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    unsafe fn text(hwnd: HWND, id: i32, value: &str) {
        unsafe {
            SetWindowTextW(GetDlgItem(hwnd, id), wide(value).as_ptr());
        }
    }
    unsafe fn value(hwnd: HWND, id: i32) -> String {
        unsafe {
            let handle = GetDlgItem(hwnd, id);
            let mut buf = vec![0; 16385];
            let len = GetWindowTextW(handle, buf.as_mut_ptr(), buf.len() as i32);
            String::from_utf16_lossy(&buf[..len.max(0) as usize])
        }
    }
    unsafe fn item(hwnd: HWND, class: &str, label: &str, id: i32, rect: [i32; 4], style: u32) {
        unsafe {
            let child = CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(label).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                rect[0],
                rect[1],
                rect[2],
                rect[3],
                hwnd,
                id as usize as HMENU,
                GetModuleHandleW(null()),
                null(),
            );
            SendMessageW(
                child,
                WM_SETFONT,
                GetStockObject(DEFAULT_GUI_FONT) as usize,
                1,
            );
            if class == "EDIT" {
                SendMessageW(child, 0x00C5, 16384, 0);
            }
        }
    }
    unsafe fn message(hwnd: HWND, caption: &str, body: &str) {
        unsafe {
            MessageBoxW(
                hwnd,
                wide(body).as_ptr(),
                wide(caption).as_ptr(),
                MB_OK | MB_ICONINFORMATION,
            );
        }
    }
    fn save_permissions(hwnd: HWND, context: &Context) -> Result<()> {
        let path = context.state.join("config.json");
        let mut cfg: crate::Config = serde_json::from_slice(&std::fs::read(&path)?)?;
        unsafe {
            cfg.policy.roots = value(hwnd, ROOTS)
                .lines()
                .filter(|s| !s.trim().is_empty())
                .map(|s| PathBuf::from(s.trim()))
                .collect();
            cfg.policy.applications = value(hwnd, APPS)
                .lines()
                .filter(|s| !s.trim().is_empty())
                .map(|s| PathBuf::from(s.trim()))
                .collect();
            cfg.policy.allow_modify = SendMessageW(GetDlgItem(hwnd, MODIFY), 0x00F0, 0, 0) == 1;
            cfg.policy.allow_screenshots =
                SendMessageW(GetDlgItem(hwnd, SCREENSHOTS), 0x00F0, 0, 0) == 1;
            cfg.policy.ask_before_sensitive =
                SendMessageW(GetDlgItem(hwnd, CONFIRM_SENSITIVE), 0x00F0, 0, 0) == 1;
            cfg.policy.allow_all_applications =
                SendMessageW(GetDlgItem(hwnd, ALL_APPS), 0x00F0, 0, 0) == 1;
        }
        ensure!(
            cfg.policy.roots.len() <= 32 && cfg.policy.applications.len() <= 32,
            "Maximum 32 roots and applications"
        );
        cfg.policy.validate()?;
        cfg.policy.protect_internal(&context.state)?;
        for app in &cfg.policy.applications {
            cfg.policy.application(&app.to_string_lossy())?;
        }
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new_in(&context.state)?;
        tmp.write_all(&serde_json::to_vec_pretty(&cfg)?)?;
        tmp.as_file().sync_all()?;
        tmp.persist(path)?;
        context.control.cancel_all(&context.cancels);
        crate::log_event(
            &context.state,
            serde_json::json!({"event":"local_permissions_changed"}),
        )?;
        Ok(())
    }
    unsafe fn refresh(hwnd: HWND, ctx: &mut Context) {
        unsafe {
            let mode = ctx.control.mode.load(Ordering::SeqCst);
            let connection: serde_json::Value =
                crate::storage::load(&ctx.state.join("connection.dpapi"))
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default();
            let pairing: serde_json::Value =
                crate::storage::load(&ctx.state.join("pairing-view.dpapi"))
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default();
            let mut state = match mode {
                1 => "Remote access paused".to_owned(),
                2 => "Disconnected by you".to_owned(),
                _ if connection["connected"] == true
                    && crate::now()
                        .saturating_sub(connection["updated_at"].as_u64().unwrap_or(0))
                        < 45 =>
                {
                    "Connected".to_owned()
                }
                _ => "Connecting / offline".to_owned(),
            };
            if let Some(code) = pairing["code"].as_str() {
                state = format!(
                    "Pairing code: {code}\r\nIn ChatGPT, ask OpenCommander to pair this code. Expires in {} seconds.",
                    pairing["expires"]
                        .as_u64()
                        .unwrap_or(0)
                        .saturating_sub(crate::now())
                );
            } else if !ctx.state.join("credentials.dpapi").exists() {
                state = "Not paired. Click Pair device to get a code.".into();
            }
            text(
                hwnd,
                STATUS,
                &format!(
                    "{} • OpenCommander {}\r\n{state}",
                    sysinfo::System::host_name().unwrap_or_default(),
                    env!("CARGO_PKG_VERSION")
                ),
            );
            let update: serde_json::Value =
                crate::storage::load(&ctx.state.join("update-status.dpapi"))
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default();
            text(
                hwnd,
                114,
                update["message"].as_str().unwrap_or(
                    "Updates require signed release metadata and a trusted package hash.",
                ),
            );
            let tooltip = wide(&format!(
                "OpenCommander — {}",
                state.lines().next().unwrap_or_default()
            ));
            ctx.icon.szTip.fill(0);
            let n = tooltip.len().min(ctx.icon.szTip.len() - 1);
            ctx.icon.szTip[..n].copy_from_slice(&tooltip[..n]);
            Shell_NotifyIconW(NIM_MODIFY, &ctx.icon);
            if ctx.control.shutdown.load(Ordering::SeqCst) {
                DestroyWindow(hwnd);
            }
        }
    }
    fn describe(ctx: &Context, kind: usize) -> String {
        if kind == ACTIVITY {
            return std::fs::read_to_string(ctx.state.join("events.log"))
                .unwrap_or_default()
                .lines()
                .rev()
                .take(20)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\r\n");
        }
        if kind == APPROVALS {
            let mut rows = vec![];
            if let Ok(entries) = std::fs::read_dir(ctx.state.join("journal")) {
                for entry in entries.flatten().take(2048) {
                    if let Ok(bytes) = crate::storage::load(&entry.path())
                        && let Ok(record) = serde_json::from_slice::<crate::Record>(&bytes)
                        && record.status == "approval_pending"
                    {
                        rows.push(format!(
                            "{}: waiting for the separate local consent dialog",
                            record.task_id
                        ));
                    }
                }
            }
            return if rows.is_empty() {
                "No pending approvals.".into()
            } else {
                rows.join("\r\n")
            };
        }
        format!(
            "Version: {}\r\nComputer: {}\r\nState directory: {}\r\nRemote access mode: {}\r\nPaired: {}\r\nNo credentials are included in diagnostics.\r\nActivity logs rotate at 2 MiB; task history is cleared after one hour without MCP use when no tasks are active.",
            env!("CARGO_PKG_VERSION"),
            sysinfo::System::host_name().unwrap_or_default(),
            ctx.state.display(),
            ctx.control.mode.load(Ordering::SeqCst),
            ctx.state.join("credentials.dpapi").exists()
        )
    }
    unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe {
            if msg == WM_NCCREATE {
                let create = &*(lp as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            }
            let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Context;
            if pointer.is_null() {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            let ctx = &mut *pointer;
            if msg == TASKBAR_CREATED.load(Ordering::SeqCst) && msg != 0 {
                Shell_NotifyIconW(NIM_ADD, &ctx.icon);
                return 0;
            }
            match msg {
                WM_CREATE => {
                    item(hwnd, "STATIC", "", 114, [18, 500, 660, 40], 0);
                    item(hwnd, "STATIC", "", STATUS, [18, 14, 680, 70], 0);
                    item(
                        hwnd,
                        "BUTTON",
                        "Pair device",
                        PAIR as i32,
                        [18, 86, 110, 30],
                        BS_PUSHBUTTON as u32,
                    );
                    item(
                        hwnd,
                        "BUTTON",
                        "Reconnect",
                        RECONNECT as i32,
                        [136, 86, 110, 30],
                        BS_PUSHBUTTON as u32,
                    );
                    item(
                        hwnd,
                        "BUTTON",
                        "Pause / resume",
                        PAUSE as i32,
                        [254, 86, 130, 30],
                        BS_PUSHBUTTON as u32,
                    );
                    item(
                        hwnd,
                        "BUTTON",
                        "Disconnect",
                        DISCONNECT as i32,
                        [392, 86, 110, 30],
                        BS_PUSHBUTTON as u32,
                    );
                    item(
                        hwnd,
                        "STATIC",
                        "Approved folders — one existing absolute folder per line",
                        110,
                        [18, 135, 660, 22],
                        0,
                    );
                    item(
                        hwnd,
                        "EDIT",
                        "",
                        ROOTS,
                        [18, 158, 660, 70],
                        WS_BORDER | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | WS_VSCROLL,
                    );
                    item(
                        hwnd,
                        "STATIC",
                        "Allowed executables (when all-apps is off)",
                        111,
                        [18, 239, 390, 22],
                        0,
                    );
                    item(
                        hwnd,
                        "BUTTON",
                        "Allow all applications",
                        ALL_APPS,
                        [420, 237, 258, 26],
                        BS_AUTOCHECKBOX as u32,
                    );
                    item(
                        hwnd,
                        "EDIT",
                        "",
                        APPS,
                        [18, 262, 660, 70],
                        WS_BORDER | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | WS_VSCROLL,
                    );
                    item(
                        hwnd,
                        "BUTTON",
                        "Allow file changes in approved folders",
                        MODIFY,
                        [18, 344, 370, 26],
                        BS_AUTOCHECKBOX as u32,
                    );
                    item(
                        hwnd,
                        "BUTTON",
                        "Allow screenshots",
                        SCREENSHOTS,
                        [398, 344, 270, 26],
                        BS_AUTOCHECKBOX as u32,
                    );
                    item(
                        hwnd,
                        "BUTTON",
                        "Confirm sensitive actions (uncheck to trust the connected owner)",
                        CONFIRM_SENSITIVE,
                        [18, 377, 660, 24],
                        BS_AUTOCHECKBOX as u32,
                    );
                    item(
                        hwnd,
                        "BUTTON",
                        "Save permissions",
                        SAVE as i32,
                        [18, 408, 150, 30],
                        BS_PUSHBUTTON as u32,
                    );
                    for (id, label, x) in [
                        (ACTIVITY, "Activity", 18),
                        (APPROVALS, "Pending approvals", 130),
                        (DIAGNOSTICS, "Diagnostics", 292),
                        (UPDATE, "Check updates", 422),
                        (EXIT, "Exit", 560),
                    ] {
                        item(
                            hwnd,
                            "BUTTON",
                            label,
                            id as i32,
                            [x, 460, if id == APPROVALS { 154 } else { 104 }, 30],
                            BS_PUSHBUTTON as u32,
                        );
                    }
                    if let Ok(bytes) = std::fs::read(ctx.state.join("config.json"))
                        && let Ok(cfg) = serde_json::from_slice::<crate::Config>(&bytes)
                    {
                        text(
                            hwnd,
                            ROOTS,
                            &cfg.policy
                                .roots
                                .iter()
                                .map(|p| p.to_string_lossy())
                                .collect::<Vec<_>>()
                                .join("\r\n"),
                        );
                        text(
                            hwnd,
                            APPS,
                            &cfg.policy
                                .applications
                                .iter()
                                .map(|p| p.to_string_lossy())
                                .collect::<Vec<_>>()
                                .join("\r\n"),
                        );
                        SendMessageW(
                            GetDlgItem(hwnd, MODIFY),
                            0x00F1,
                            usize::from(cfg.policy.allow_modify),
                            0,
                        );
                        SendMessageW(
                            GetDlgItem(hwnd, SCREENSHOTS),
                            0x00F1,
                            usize::from(cfg.policy.allow_screenshots),
                            0,
                        );
                        SendMessageW(
                            GetDlgItem(hwnd, CONFIRM_SENSITIVE),
                            0x00F1,
                            usize::from(cfg.policy.ask_before_sensitive),
                            0,
                        );
                        SendMessageW(
                            GetDlgItem(hwnd, ALL_APPS),
                            0x00F1,
                            usize::from(cfg.policy.allow_all_applications),
                            0,
                        );
                    }
                    ctx.icon.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                    ctx.icon.hWnd = hwnd;
                    ctx.icon.uID = 1;
                    ctx.icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
                    ctx.icon.uCallbackMessage = CALLBACK;
                    ctx.icon.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
                    Shell_NotifyIconW(NIM_ADD, &ctx.icon);
                    SetTimer(hwnd, 1, 1000, None);
                    refresh(hwnd, ctx);
                    0
                }
                WM_TIMER => {
                    refresh(hwnd, ctx);
                    0
                }
                CALLBACK => {
                    if matches!(lp as u32, WM_LBUTTONUP | WM_LBUTTONDBLCLK) {
                        ShowWindow(hwnd, SW_SHOW);
                        SetForegroundWindow(hwnd);
                    }
                    if lp as u32 == WM_RBUTTONUP {
                        let menu = CreatePopupMenu();
                        for (id, label) in [
                            (210, "Open OpenCommander"),
                            (PAUSE, "Pause / resume remote access"),
                            (RECONNECT, "Reconnect"),
                            (DISCONNECT, "Disconnect"),
                            (EXIT, "Exit"),
                        ] {
                            AppendMenuW(menu, MF_STRING, id, wide(label).as_ptr());
                        }
                        let mut point = POINT::default();
                        GetCursorPos(&mut point);
                        SetForegroundWindow(hwnd);
                        let chosen = TrackPopupMenu(
                            menu,
                            TPM_RETURNCMD | TPM_RIGHTBUTTON,
                            point.x,
                            point.y,
                            0,
                            hwnd,
                            null(),
                        );
                        DestroyMenu(menu);
                        if chosen > 0 {
                            SendMessageW(hwnd, WM_COMMAND, chosen as usize, 0);
                        }
                    }
                    0
                }
                WM_COMMAND => {
                    match wp & 0xFFFF {
                        210 => {
                            ShowWindow(hwnd, SW_SHOW);
                            SetForegroundWindow(hwnd);
                        }
                        SAVE => match save_permissions(hwnd, ctx) {
                            Ok(()) => message(
                                hwnd,
                                "Permissions saved",
                                "New permissions apply to subsequent tasks. Current operations were cancelled.",
                            ),
                            Err(e) => message(hwnd, "Cannot save permissions", &e.to_string()),
                        },
                        PAIR => {
                            if ctx.state.join("credentials.dpapi").exists() {
                                message(
                                    hwnd,
                                    "Device already paired",
                                    "Existing pairing is preserved. Reconnect to resume access.",
                                );
                            } else {
                                ctx.control.pair.store(true, Ordering::SeqCst);
                            }
                        }
                        PAUSE | DISCONNECT | RECONNECT => {
                            let mode = match wp & 0xFFFF {
                                DISCONNECT => 2,
                                RECONNECT => 0,
                                _ => u8::from(ctx.control.mode.load(Ordering::SeqCst) == 0),
                            };
                            ctx.control.mode.store(mode, Ordering::SeqCst);
                            ctx.control.cancel_all(&ctx.cancels);
                            if mode == 2 || wp & 0xFFFF == RECONNECT {
                                ctx.control.reconnect.fetch_add(1, Ordering::SeqCst);
                            }
                            let _ = crate::storage::save(&ctx.state.join("access.dpapi"), &[mode]);
                            let _ = crate::log_event(
                                &ctx.state,
                                serde_json::json!({"event":"local_access_changed","mode":mode}),
                            );
                            refresh(hwnd, ctx);
                        }
                        ACTIVITY | APPROVALS | DIAGNOSTICS => {
                            message(hwnd, "OpenCommander", &describe(ctx, wp & 0xFFFF))
                        }
                        UPDATE => crate::updater::launch_check(ctx.state.clone()),
                        EXIT => {
                            ctx.control.shutdown.store(true, Ordering::SeqCst);
                            ctx.control.cancel_all(&ctx.cancels);
                            DestroyWindow(hwnd);
                        }
                        _ => (),
                    }
                    0
                }
                WM_CLOSE => {
                    ShowWindow(hwnd, SW_HIDE);
                    0
                }
                WM_QUERYENDSESSION => {
                    ctx.control.shutdown.store(true, Ordering::SeqCst);
                    ctx.control.cancel_all(&ctx.cancels);
                    1
                }
                WM_DESTROY => {
                    KillTimer(hwnd, 1);
                    Shell_NotifyIconW(NIM_DELETE, &ctx.icon);
                    PostQuitMessage(0);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }
    }
    pub fn start(
        state: PathBuf,
        control: Arc<Control>,
        cancels: crate::Cancellations,
        visible: bool,
    ) {
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                unsafe {
                    let name = wide("OpenCommanderNativeWindow");
                    let instance = GetModuleHandleW(null());
                    let class = WNDCLASSW {
                        lpfnWndProc: Some(procedure),
                        hInstance: instance,
                        lpszClassName: name.as_ptr(),
                        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                        hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
                        ..Default::default()
                    };
                    ensure!(RegisterClassW(&class) != 0, "Cannot register native window");
                    TASKBAR_CREATED.store(
                        RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
                        Ordering::SeqCst,
                    );
                    let mut context = Box::new(Context {
                        state: state.clone(),
                        control: control.clone(),
                        cancels,
                        icon: Default::default(),
                    });
                    let hwnd = CreateWindowExW(
                        0,
                        name.as_ptr(),
                        wide("OpenCommander").as_ptr(),
                        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        720,
                        580,
                        null_mut(),
                        null_mut(),
                        instance,
                        (&mut *context as *mut Context).cast(),
                    );
                    ensure!(!hwnd.is_null(), "Cannot create native window");
                    if visible {
                        ShowWindow(hwnd, SW_SHOW);
                    }
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                        if IsDialogMessageW(hwnd, &msg) == 0 {
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                let _ = crate::log_event(
                    &state,
                    serde_json::json!({"event":"native_ui_failed","error":e.to_string()}),
                );
                control.shutdown.store(true, Ordering::SeqCst);
            }
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn native_window_loads_policy_cancels_access_and_exits() -> Result<()> {
            let state = tempfile::tempdir()?;
            let root = tempfile::tempdir()?;
            let cfg = crate::Config {
                gateway: "https://gateway-production-82d7.up.railway.app".into(),
                policy: crate::policy::Policy {
                    roots: vec![root.path().into()],
                    protected_paths: vec![],
                    applications: vec![],
                    allow_all_applications: false,
                    allow_modify: true,
                    allow_screenshots: false,
                    ask_before_sensitive: true,
                },
                dev: false,
            };
            std::fs::write(state.path().join("config.json"), serde_json::to_vec(&cfg)?)?;
            let control = Arc::new(Control::default());
            let flag = Arc::new(AtomicBool::new(false));
            let cancels = Arc::new(std::sync::Mutex::new(std::collections::HashMap::from([(
                "native-fixture-task".into(),
                flag.clone(),
            )])));
            start(state.path().into(), control.clone(), cancels, false);
            let mut hwnd = null_mut();
            for _ in 0..200 {
                unsafe {
                    let mut candidate = null_mut();
                    loop {
                        candidate = FindWindowExW(
                            null_mut(),
                            candidate,
                            wide("OpenCommanderNativeWindow").as_ptr(),
                            wide("OpenCommander").as_ptr(),
                        );
                        if candidate.is_null() {
                            break;
                        }
                        let mut pid = 0;
                        GetWindowThreadProcessId(candidate, &mut pid);
                        if pid == std::process::id() {
                            hwnd = candidate;
                            break;
                        }
                    }
                }
                if !hwnd.is_null() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            ensure!(!hwnd.is_null(), "Native fixture window was not created");
            unsafe {
                let mut pid = 0;
                GetWindowThreadProcessId(hwnd, &mut pid);
                ensure!(
                    pid == std::process::id(),
                    "Refusing to operate another process window"
                );
                let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Context;
                ensure!(
                    !ctx.is_null() && (*ctx).state == state.path(),
                    "Refusing to operate a different agent state"
                );
                struct Cleanup(HWND);
                impl Drop for Cleanup {
                    fn drop(&mut self) {
                        unsafe {
                            PostMessageW(self.0, WM_COMMAND, EXIT, 0);
                        }
                    }
                }
                let _cleanup = Cleanup(hwnd);
                let expected_root = root.path().to_string_lossy().into_owned();
                for _ in 0..100 {
                    if value(hwnd, ROOTS) == expected_root {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                ensure!(
                    value(hwnd, ROOTS) == expected_root,
                    "Initial roots did not load"
                );
                ensure!(
                    SendMessageW(GetDlgItem(hwnd, MODIFY), 0x00F0, 0, 0) == 1,
                    "Initial modification permission did not load"
                );
                ensure!(
                    SendMessageW(GetDlgItem(hwnd, CONFIRM_SENSITIVE), 0x00F0, 0, 0) == 1,
                    "Confirmation default did not load"
                );
                ensure!(
                    SendMessageW(GetDlgItem(hwnd, ALL_APPS), 0x00F0, 0, 0) == 0,
                    "All-apps default did not load"
                );
                SendMessageW(GetDlgItem(hwnd, ALL_APPS), 0x00F1, 1, 0);
                SendMessageW(GetDlgItem(hwnd, CONFIRM_SENSITIVE), 0x00F1, 0, 0);
                save_permissions(hwnd, &*ctx)?;
                let changed: crate::Config =
                    serde_json::from_slice(&std::fs::read(state.path().join("config.json"))?)?;
                ensure!(
                    changed.policy.allow_all_applications
                        && !changed.policy.ask_before_sensitive
                        && !changed.policy.needs_consent("SENSITIVE"),
                    "Trusted owner setting was not saved"
                );
                SendMessageW(GetDlgItem(hwnd, ALL_APPS), 0x00F1, 0, 0);
                SendMessageW(GetDlgItem(hwnd, CONFIRM_SENSITIVE), 0x00F1, 1, 0);
                save_permissions(hwnd, &*ctx)?;
                let changed: crate::Config =
                    serde_json::from_slice(&std::fs::read(state.path().join("config.json"))?)?;
                ensure!(
                    !changed.policy.allow_all_applications
                        && changed.policy.ask_before_sensitive
                        && changed.policy.needs_consent("SENSITIVE"),
                    "Confirmation could not be restored"
                );
                flag.store(false, Ordering::SeqCst);
                SendMessageW(hwnd, WM_COMMAND, PAUSE, 0);
                ensure!(
                    control.mode.load(Ordering::SeqCst) == 1 && flag.load(Ordering::SeqCst),
                    "Pause did not cancel remote operations"
                );
                ensure!(
                    crate::storage::load(&state.path().join("access.dpapi"))? == [1],
                    "Pause did not persist"
                );
                SendMessageW(hwnd, WM_COMMAND, DISCONNECT, 0);
                ensure!(
                    control.mode.load(Ordering::SeqCst) == 2,
                    "Disconnect did not apply"
                );
                SendMessageW(hwnd, WM_COMMAND, RECONNECT, 0);
                ensure!(
                    control.mode.load(Ordering::SeqCst) == 0
                        && control.reconnect.load(Ordering::SeqCst) == 2,
                    "Reconnect did not apply"
                );
                SendMessageW(hwnd, WM_COMMAND, EXIT, 0);
                ensure!(
                    control.shutdown.load(Ordering::SeqCst) && IsWindow(hwnd) == 0,
                    "Exit left the native window running"
                );
            }
            Ok(())
        }
    }
}
#[cfg(windows)]
pub use native::start;
#[cfg(not(windows))]
pub fn start(_: std::path::PathBuf, _: Arc<Control>, _: crate::Cancellations, _: bool) {}
