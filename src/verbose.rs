use std::sync::atomic::{AtomicBool, Ordering};
pub static VERBOSE: AtomicBool = AtomicBool::new(false);
pub fn is_verbose() -> bool { VERBOSE.load(Ordering::Relaxed) }
pub fn set_verbose(v: bool) { VERBOSE.store(v, Ordering::Relaxed) }
