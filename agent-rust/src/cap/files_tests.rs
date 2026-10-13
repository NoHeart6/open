#[cfg(test)]
mod tests {
    use crate::cap::files::{manage, read, write_atomic};

    #[test]
    fn write_then_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt").to_string_lossy().into_owned();
        write_atomic(p.clone(), b"hello", false).unwrap();
        let v = read(p.clone(), 100).unwrap();
        assert_eq!(v["content"], "hello");
    }

    #[test]
    fn write_no_overwrite_guard() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt").to_string_lossy().into_owned();
        write_atomic(p.clone(), b"one", false).unwrap();
        assert!(write_atomic(p.clone(), b"two", false).is_err());
        write_atomic(p.clone(), b"two", true).unwrap();
    }

    #[test]
    fn hash_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("h.bin").to_string_lossy().into_owned();
        std::fs::write(&p, b"abc").unwrap();
        let a = manage("hash", Some(&p), None).unwrap();
        let b = manage("hash", Some(&p), None).unwrap();
        assert_eq!(a["sha256"], b["sha256"]);
    }
}
