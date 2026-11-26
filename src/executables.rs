//! Executable discovery shared by external integrations; discovery never runs tools.

use std::path::{Path, PathBuf};

pub(crate) fn on_path(command: &str) -> Option<PathBuf> {
    let dirs = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_default();
    in_directories(command, &dirs)
}

pub(crate) fn in_directories(command: &str, directories: &[PathBuf]) -> Option<PathBuf> {
    let path = Path::new(command);
    let candidates = if path.is_absolute() || path.components().count() > 1 {
        vec![path.to_path_buf()]
    } else {
        directories.iter().map(|dir| dir.join(command)).collect()
    };
    candidates
        .into_iter()
        .find(|path| is_executable_file(path))
        .and_then(|path| std::path::absolute(path).ok())
}

#[cfg(unix)]
pub(crate) fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}
#[cfg(not(unix))]
pub(crate) fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
