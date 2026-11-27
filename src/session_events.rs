//! Flushed session lifecycle events shared by dictation and meetings.

use anyhow::Result;
use std::io::{self, Write};

pub(crate) struct Events {
    pub(crate) json: bool,
    pub(crate) sequence: usize,
}
impl Events {
    pub(crate) fn send(
        &mut self,
        kind: &str,
        session_id: Option<&str>,
        data: serde_json::Value,
    ) -> Result<()> {
        if self.json {
            let mut output = io::stdout().lock();
            serde_json::to_writer(
                &mut output,
                &serde_json::json!({"version":1,"sequence":self.sequence,"type":kind,"session_id":session_id,"data":data}),
            )?;
            output.write_all(b"\n")?;
            output.flush()?;
            self.sequence += 1;
        }
        Ok(())
    }
}

#[cfg(unix)]
pub(crate) fn install_stop_handler(handler: extern "C" fn(std::ffi::c_int)) -> Result<()> {
    use anyhow::Context;
    #[cfg(unix)]
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handler as *const () as libc::sighandler_t;
        libc::sigemptyset(&mut action.sa_mask);
        if libc::sigaction(libc::SIGUSR1, &action, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error())
                .context("cannot install recording stop handler");
        }
        let mut mask: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut mask);
        libc::sigaddset(&mut mask, libc::SIGUSR1);
        if libc::sigprocmask(libc::SIG_UNBLOCK, &mask, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error()).context("cannot enable recording stop handler");
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn install_stop_handler(_: extern "C" fn(std::ffi::c_int)) -> Result<()> {
    Ok(())
}
