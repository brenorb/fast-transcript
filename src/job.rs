//! CLI-only ownership of a CLI job's process group and scratch directory.

use anyhow::{Context, Result};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const JOB_ENV: &str = "FSCRIPT_INTERNAL_STREAM_JOB";

static CANCELLED: AtomicBool = AtomicBool::new(false);
static INTERRUPTS: AtomicUsize = AtomicUsize::new(0);

#[cfg(unix)]
extern "C" fn signal(signal: libc::c_int) {
    if signal == libc::SIGINT {
        INTERRUPTS.fetch_add(1, Ordering::SeqCst);
    } else {
        CANCELLED.store(true, Ordering::SeqCst);
    }
}

fn install_handlers() -> Result<()> {
    #[cfg(unix)]
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = signal as *const () as libc::sighandler_t;
        libc::sigemptyset(&mut action.sa_mask);
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                return Err(std::io::Error::last_os_error())
                    .context("cannot install job cancellation handler");
            }
        }
    }
    #[cfg(not(unix))]
    ctrlc::set_handler(|| CANCELLED.store(true, Ordering::SeqCst))
        .context("cannot install job cancellation handler")?;
    Ok(())
}

pub(crate) fn run(
    raw_args: Vec<String>,
    worker: fn(Vec<String>) -> Result<()>,
    supervised: bool,
    finish_on_interrupt: bool,
) -> Result<i32> {
    if !supervised || std::env::var_os(JOB_ENV).is_some() {
        return worker(raw_args).map(|()| 0);
    }

    install_handlers()?;
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
        if finish_on_interrupt {
            // Queue an early finish signal until the worker installs its handler.
            unsafe {
                command.pre_exec(|| {
                    let mut mask: libc::sigset_t = std::mem::zeroed();
                    libc::sigemptyset(&mut mask);
                    libc::sigaddset(&mut mask, libc::SIGUSR1);
                    if libc::sigprocmask(libc::SIG_BLOCK, &mask, std::ptr::null_mut()) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
    }
    let child = command.spawn().context("cannot start job worker")?;
    let mut worker = Worker(child);
    let mut stop_sent = false;
    loop {
        let interrupts = INTERRUPTS.load(Ordering::SeqCst);
        if finish_on_interrupt && interrupts == 1 && !stop_sent {
            #[cfg(unix)]
            unsafe {
                libc::kill(worker.0.id() as i32, libc::SIGUSR1);
            }
            stop_sent = true;
        }
        if CANCELLED.load(Ordering::SeqCst) || interrupts > usize::from(finish_on_interrupt) {
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
