use serde_json::{json, Value};
use sysinfo::System;

pub fn system_info() -> Value {
    let mut s = System::new_all();
    s.refresh_all();
    let total = s.total_memory();
    let avail = s.available_memory();
    let cpus = s.cpus().len();
    let brand = s.cpus().first().map(|c| c.brand().to_owned()).unwrap_or_default();
    json!({
        "os": std::env::consts::OS,
        "os_version": System::os_version().unwrap_or_default(),
        "host": System::host_name().unwrap_or_default(),
        "cpu": {"logical": cpus, "brand": brand},
        "ram": {"total_kb": total, "available_kb": avail},
        "uptime_secs": System::uptime(),
    })
}
