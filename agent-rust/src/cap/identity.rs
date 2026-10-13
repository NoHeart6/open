use uuid::Uuid;

pub fn device_id() -> String {
    let dir = dirs();
    let path = dir.join("device_id.txt");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim().to_owned();
        if !s.is_empty() {
            return s;
        }
    }
    let id = format!("dev_{}", Uuid::new_v4().simple());
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&path, &id);
    id
}

fn dirs() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("PROGRAMDATA") {
        return std::path::PathBuf::from(p).join("OpenCommander");
    }
    std::env::temp_dir().join("OpenCommander")
}
