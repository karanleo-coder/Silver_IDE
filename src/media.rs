//! Is any audio playing on this machine? Polled in a background
//! thread every couple of seconds so drawing never blocks; the UI
//! only animates its equalizer wave while something really plays.

#[cfg(any(target_os = "macos", target_os = "linux"))]
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

/// Windows: WASAPI's loudness meter on the default output device —
/// anything actually making sound lifts the peak above zero.
#[cfg(target_os = "windows")]
fn detect() -> bool {
    use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    unsafe {
        // Per-thread COM setup; every call after the first just says
        // "already done", which is fine.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(devices): windows::core::Result<IMMDeviceEnumerator> =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return false;
        };
        let Ok(device) = devices.GetDefaultAudioEndpoint(eRender, eConsole) else {
            return false;
        };
        let Ok(meter) = device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None) else {
            return false;
        };
        meter.GetPeakValue().map(|peak| peak > 0.01).unwrap_or(false)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn detect() -> bool {
    false
}
