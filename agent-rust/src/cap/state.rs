use std::sync::atomic::{AtomicBool, Ordering};

static PAUSED: AtomicBool = AtomicBool::new(false);

pub fn paused() -> bool {
    PAUSED.load(Ordering::Relaxed)
}

pub fn set_paused(v: bool) {
    PAUSED.store(v, Ordering::Relaxed);
}
