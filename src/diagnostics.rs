//! Scoped verbosity for library status messages. Warnings use `std::eprintln!`.

use anyhow::{Context, Result};
use std::cell::Cell;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::process::{Command, Stdio};
use tempfile::NamedTempFile;

// Keep progress and setup messages behind the same policy, including modules
// shared with SDK provisioning. Explicit std::eprintln! always reports a warning.
macro_rules! eprintln {
    () => { $crate::diagnostics::status(format_args!(""), true) };
    ($($arg:tt)*) => { $crate::diagnostics::status(format_args!($($arg)*), true) };
}
macro_rules! eprint {
    ($($arg:tt)*) => { $crate::diagnostics::status(format_args!($($arg)*), false) };
}

thread_local! { static QUIET: Cell<bool> = const { Cell::new(false) }; }

pub(crate) fn quiet() -> bool {
    QUIET.with(Cell::get)
}

pub(crate) struct Scope(bool);
impl Scope {
    pub(crate) fn enter(quiet: bool) -> Self {
        Self(QUIET.with(|current| current.replace(quiet)))
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        QUIET.with(|current| current.set(self.0));
    }
}

pub(crate) fn status(message: fmt::Arguments<'_>, newline: bool) {
    if !quiet() {
        let mut stderr = io::stderr().lock();
        let _ = stderr.write_fmt(message);
        if newline {
            let _ = stderr.write_all(b"\n");
        }
    }
}

pub(crate) struct StatusWriter;
impl Write for StatusWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if quiet() {
            Ok(bytes.len())
        } else {
            io::stderr().write(bytes)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

/// Spool quiet workers' stderr to disk, avoiding a pipe that can fill and deadlock.
/// A failing worker's final diagnostics are included in its error instead.
pub(crate) fn capture_stderr(command: &mut Command) -> Result<Option<NamedTempFile>> {
    if !quiet() {
        return Ok(None);
    }
    let file = NamedTempFile::new().context("cannot create worker diagnostic file")?;
    command.stderr(Stdio::from(file.reopen()?));
    Ok(Some(file))
}

pub(crate) fn diagnostic_tail(file: Option<&NamedTempFile>) -> String {
    let read_tail = |file: &NamedTempFile| -> io::Result<String> {
        let mut input = File::open(file.path())?;
        let length = input.metadata()?.len();
        input.seek(SeekFrom::Start(length.saturating_sub(8192)))?;
        let mut bytes = Vec::new();
        input.take(8192).read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).trim().to_string())
    };
    file.and_then(|file| read_tail(file).ok())
        .filter(|tail| !tail.is_empty())
        .map(|tail| format!("\n{tail}"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbosity_is_scoped_and_does_not_leak_into_other_threads() {
        assert!(!quiet());
        {
            let _scope = Scope::enter(true);
            assert!(quiet());
            assert!(!std::thread::spawn(quiet).join().unwrap());
            {
                let _scope = Scope::enter(false);
                assert!(!quiet());
            }
            assert!(quiet());
        }
        assert!(!quiet());
    }

    #[test]
    fn failure_diagnostics_are_bounded_and_keep_the_last_line() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&vec![b'x'; 20_000]).unwrap();
        file.write_all(b"\nbackend failed").unwrap();
        let tail = diagnostic_tail(Some(&file));
        assert!(tail.len() <= 8193);
        assert!(tail.ends_with("backend failed"));
    }
}
