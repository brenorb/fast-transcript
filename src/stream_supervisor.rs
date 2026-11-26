//! CLI-only ownership of a streaming job's process group and scratch directory.

use anyhow::{Context, Result};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const JOB_ENV: &str = "FSCRIPT_INTERNAL_STREAM_JOB";

pub(crate) fn run(raw_args: Vec<String>) -> Result<i32> {
    let streaming =
        crate::cli::parse_command(&raw_args).is_ok_and(|matches| matches.get_flag("stream"));
    if !streaming || std::env::var_os(JOB_ENV).is_some() {
        return crate::app::run(raw_args).map(|()| 0);
    }

    static CANCELLED: AtomicBool = AtomicBool::new(false);
    ctrlc::set_handler(|| CANCELLED.store(true, Ordering::SeqCst))
        .context("cannot install streaming cancellation handler")?;
    let scratch = tempfile::Builder::new().prefix("fscript-job-").tempdir()?;
    let job_id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(raw_args)
        .env(JOB_ENV, job_id)
        .env("TMPDIR", scratch.path());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command.spawn().context("cannot start streaming worker")?;
    let mut worker = Worker(child);
    loop {
        if CANCELLED.load(Ordering::SeqCst) {
            worker.terminate();
            let deadline = Instant::now() + Duration::from_millis(200);
            while worker.0.try_wait()?.is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            // Drop kills any remaining descendants, reaps the worker, then scratch is removed.
            return Ok(130);
        }
        if let Some(status) = worker.0.try_wait()? {
            return Ok(status.code().unwrap_or(1));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Worker(Child);
impl Worker {
    fn terminate(&mut self) {
        #[cfg(unix)]
        // The child created this process group; never signal the caller's group.
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGTERM);
        }
        #[cfg(not(unix))]
        {
            let _ = self.0.kill();
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
