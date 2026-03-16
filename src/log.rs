use std::sync::atomic::{AtomicBool, Ordering};

static OUTPUT_SUPPRESSED: AtomicBool = AtomicBool::new(false);

/// While this guard is alive, all log output is suppressed.
pub struct OutputSuppressGuard;

impl OutputSuppressGuard {
    pub fn new() -> Self {
        OUTPUT_SUPPRESSED.store(true, Ordering::Relaxed);
        Self
    }
}

impl Drop for OutputSuppressGuard {
    fn drop(&mut self) {
        OUTPUT_SUPPRESSED.store(false, Ordering::Relaxed);
    }
}

pub fn is_suppressed() -> bool {
    OUTPUT_SUPPRESSED.load(Ordering::Relaxed)
}
