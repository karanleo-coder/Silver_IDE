//! Is any audio playing on this machine? Polled in a background
//! thread every couple of seconds so drawing never blocks; the UI
//! only animates its equalizer wave while something really plays.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct MediaWatch {
    playing: Arc<AtomicBool>,
}

impl MediaWatch {
    pub fn start() -> Self {
        let playing = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&playing);
        std::thread::spawn(move || loop {
            flag.store(detect(), Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(2));
        });
        Self { playing }
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
}

/// macOS: while any app plays audio, coreaudiod holds a power
/// assertion — visible to everyone, no permissions needed.
#[cfg(target_os = "macos")]
fn detect() -> bool {
    let Ok(out) = Command::new("pmset").args(["-g", "assertions"]).output() else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.contains("coreaudiod") && l.contains("PreventUserIdleSystemSleep"))
}

/// Linux: any live PulseAudio/PipeWire stream counts as playing.
#[cfg(target_os = "linux")]
fn detect() -> bool {
    let Ok(out) = Command::new("pactl").args(["list", "short", "sink-inputs"]).output() else {
        return false;
    };
    !String::from_utf8_lossy(&out.stdout).trim().is_empty()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn detect() -> bool {
    false
}
