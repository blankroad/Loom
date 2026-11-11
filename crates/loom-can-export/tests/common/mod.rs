#![allow(clippy::unwrap_used, clippy::expect_used)]
use loom_can_export::*;
use loom_can_frame::{CanFrame, CanId, Direction};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Directory(pub PathBuf);
impl Directory {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "loom-export-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub fn frame(stamp: u64) -> ExportFrame {
    ExportFrame::new(
        stamp,
        0,
        Direction::Tx,
        CanFrame::classic(CanId::standard(0x123).unwrap(), &[1, 2]).unwrap(),
        Origin::Agent,
    )
}
