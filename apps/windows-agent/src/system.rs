use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::time::Duration;
pub async fn execute(a: &Value) -> Result<Value> {
    match a["op"].as_str().context("Missing inventory operation")? {
        "services" => tokio::task::spawn_blocking(services).await?,
        "applications" => tokio::task::spawn_blocking(applications).await?,
        "network" => {
            let interfaces:Vec<Value>=sysinfo::Networks::new_with_refreshed_list().iter().take(64).map(|(name,n)|json!({"name":name,"addresses":n.ip_networks().iter().map(ToString::to_string).collect::<Vec<_>>(),"received_bytes":n.total_received(),"sent_bytes":n.total_transmitted()})).collect();
            let mut result = json!({"interfaces":interfaces});
            if let Some(host) = a["host"].as_str() {
                ensure!(
                    !host.is_empty()
                        && host.len() <= 253
                        && host
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b)),
                    "Invalid diagnostic hostname or IP"
                );
                let port = a["port"].as_u64().unwrap_or(443);
                ensure!((1..=65535).contains(&port), "Invalid diagnostic port");
                let addresses = tokio::time::timeout(
                    Duration::from_secs(3),
                    tokio::net::lookup_host((host, port as u16)),
                )
                .await??
                .take(8)
                .collect::<Vec<_>>();
                let mut probes = vec![];
                for addr in addresses {
                    let start = std::time::Instant::now();
                    let open = matches!(
                        tokio::time::timeout(
                            Duration::from_millis(750),
                            tokio::net::TcpStream::connect(addr)
                        )
                        .await,
                        Ok(Ok(_))
                    );
                    probes.push(json!({"address":addr.to_string(),"connected":open,"elapsed_ms":start.elapsed().as_millis()}));
                }
                result["tcp_probes"] = json!(probes);
            }
            Ok(result)
        }
        _ => bail!("Unsupported system inventory operation"),
    }
}
#[cfg(windows)]
fn services() -> Result<Value> {
    use windows_sys::Win32::{Foundation::*, System::Services::*};
    struct Manager(SC_HANDLE);
    impl Drop for Manager {
        fn drop(&mut self) {
            unsafe {
                CloseServiceHandle(self.0);
            }
        }
    }
    unsafe fn string(pointer: *const u16) -> String {
        unsafe {
            let mut length = 0;
            while length < 4096 && *pointer.add(length) != 0 {
                length += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length))
        }
    }
    unsafe {
        let manager = Manager(OpenSCManagerW(
            std::ptr::null(),
            std::ptr::null(),
            SC_MANAGER_ENUMERATE_SERVICE,
        ));
        ensure!(!manager.0.is_null(), "Windows denied service inventory");
        let mut buffer = vec![0u64; 32768];
        let mut needed = 0;
        let mut count = 0;
        let mut resume = 0;
        let ok = EnumServicesStatusExW(
            manager.0,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_STATE_ALL,
            buffer.as_mut_ptr().cast(),
            262144,
            &mut needed,
            &mut count,
            &mut resume,
            std::ptr::null(),
        );
        ensure!(
            ok != 0 || GetLastError() == ERROR_MORE_DATA,
            "Service enumeration failed"
        );
        ensure!(
            count as usize * std::mem::size_of::<ENUM_SERVICE_STATUS_PROCESSW>() <= 262144,
            "Invalid service result size"
        );
        let rows = std::slice::from_raw_parts(
            buffer.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(),
            count as usize,
        );
        let list:Vec<Value>=rows.iter().take(1000).map(|s|json!({"name":string(s.lpServiceName),"display_name":string(s.lpDisplayName),"state":s.ServiceStatusProcess.dwCurrentState,"pid":s.ServiceStatusProcess.dwProcessId})).collect();
        Ok(
            json!({"services":list,"truncated":resume!=0 || count>1000,"visibility":"Services visible with this user's SCM permissions"}),
        )
    }
}
#[cfg(windows)]
fn applications() -> Result<Value> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{Foundation::*, System::Registry::*};
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
    unsafe fn get(key: HKEY, name: &str) -> Option<String> {
        unsafe {
            let mut buffer = vec![0u16; 2048];
            let mut bytes = 4096;
            let mut kind = 0;
            if RegGetValueW(
                key,
                null(),
                wide(name).as_ptr(),
                RRF_RT_REG_SZ,
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            ) != 0
            {
                return None;
            }
            let len = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
            Some(String::from_utf16_lossy(&buffer[..len]))
        }
    }
    let mut list = vec![];
    unsafe {
        for (hive, flags, scope) in [
            (HKEY_CURRENT_USER, KEY_READ, "user"),
            (
                HKEY_LOCAL_MACHINE,
                KEY_READ | KEY_WOW64_64KEY,
                "machine-x64",
            ),
            (
                HKEY_LOCAL_MACHINE,
                KEY_READ | KEY_WOW64_32KEY,
                "machine-x86",
            ),
        ] {
            if list.len() >= 1000 {
                break;
            }
            let mut handle = null_mut();
            if RegOpenKeyExW(
                hive,
                wide("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall").as_ptr(),
                0,
                flags,
                &mut handle,
            ) != 0
            {
                continue;
            }
            let root = Key(handle);
            for index in 0..1000 {
                let mut name = [0u16; 512];
                let mut size = 512;
                let code = RegEnumKeyExW(
                    root.0,
                    index,
                    name.as_mut_ptr(),
                    &mut size,
                    null(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                );
                if code == ERROR_NO_MORE_ITEMS {
                    break;
                }
                if code != 0 {
                    continue;
                }
                let mut child = null_mut();
                if RegOpenKeyExW(root.0, name.as_ptr(), 0, KEY_READ, &mut child) != 0 {
                    continue;
                }
                let child = Key(child);
                if let Some(display) = get(child.0, "DisplayName") {
                    list.push(json!({"name":display,"version":get(child.0,"DisplayVersion"),"publisher":get(child.0,"Publisher"),"install_location":get(child.0,"InstallLocation"),"scope":scope}));
                }
                if list.len() >= 1000 {
                    break;
                }
            }
        }
    }
    Ok(
        json!({"applications":list,"limit":1000,"inventory":"Registered classic Windows applications; packaged Store apps may be absent"}),
    )
}
#[cfg(not(windows))]
fn services() -> Result<Value> {
    bail!("Windows services require Windows")
}
#[cfg(not(windows))]
fn applications() -> Result<Value> {
    bail!("Windows application inventory requires Windows")
}
