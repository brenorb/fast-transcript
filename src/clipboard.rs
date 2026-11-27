//! Explicit desktop text delivery shared by CLI export and the app.
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

pub(crate) fn deliver(text: &str, paste_target: Option<i32>) -> Result<()> {
    if let Some(pid) = paste_target {
        if pid <= 0 {
            bail!("paste target must be a positive process ID");
        }
        #[cfg(target_os = "macos")]
        if !unsafe { macos::CGPreflightPostEventAccess() } {
            bail!("macOS has not granted event-posting access to this running process; the transcript is still available. Check the responsible app or terminal in Privacy & Security (Accessibility / Device Control and Data Access). An enabled entry for a different build may not apply to this process");
        }
        #[cfg(not(target_os = "macos"))]
        bail!("automatic paste currently requires macOS");
    }
    let binary = std::env::var_os("FSCRIPT_CLIPBOARD_BINARY").map(std::path::PathBuf::from).or_else(|| if cfg!(target_os = "macos") { Some("/usr/bin/pbcopy".into()) } else { None }).context("clipboard delivery requires macOS or FSCRIPT_CLIPBOARD_BINARY pointing to a stdin clipboard command")?;
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("cannot start clipboard command")?;
    let write = child
        .stdin
        .take()
        .context("clipboard stdin is missing")?
        .write_all(text.as_bytes());
    if let Err(error) = write {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error).context("cannot send transcript to clipboard");
    }
    let output = child
        .wait_with_output()
        .context("cannot finish clipboard delivery")?;
    if !output.status.success() {
        bail!(
            "clipboard command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    #[cfg(target_os = "macos")]
    if let Some(pid) = paste_target {
        macos::paste(pid)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::ffi::c_void;
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        pub(super) fn CGPreflightPostEventAccess() -> bool;
        fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> *mut c_void;
        fn CGEventSetFlags(event: *mut c_void, flags: u64);
        fn CGEventPostToPid(pid: i32, event: *const c_void);
        fn CGEventSourceFlagsState(state: i32) -> u64;
        fn CFRelease(value: *const c_void);
    }
    struct Event(*mut c_void);
    impl Drop for Event {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) }
        }
    }
    pub(super) fn paste(pid: i32) -> Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while unsafe { CGEventSourceFlagsState(0) } & 0x1e0000 != 0 {
            if std::time::Instant::now() >= deadline {
                bail!("text copied; release modifier keys before pasting");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // Target the explicitly requested application, even if focus changes in
        // the small gap between the desktop's focus check and event delivery.
        // Allocate the whole chord before posting anything, so an allocation
        // failure cannot leave Command held down in the target application.
        let events = [
            (55, true, 1 << 20),
            (9, true, 1 << 20),
            (9, false, 1 << 20),
            (55, false, 0),
        ]
        .into_iter()
        .map(|(key, down, flags)| unsafe {
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), key, down);
            if event.is_null() {
                bail!("text copied but keyboard event allocation failed");
            }
            CGEventSetFlags(event, flags);
            Ok(Event(event))
        })
        .collect::<Result<Vec<_>>>()?;
        for event in events {
            unsafe { CGEventPostToPid(pid, event.0) }
        }
        Ok(())
    }
}
