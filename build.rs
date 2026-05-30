use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");

    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("apple-darwin") {
        return;
    }

    for path in clang_runtime_dirs() {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
}

fn clang_runtime_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for root in developer_roots() {
        dirs.extend(runtime_dirs_for_root(&root));
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

fn developer_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(path) = env::var_os("DEVELOPER_DIR") {
        roots.push(PathBuf::from(path));
    }

    if let Ok(output) = Command::new("xcode-select").arg("-p").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                roots.push(PathBuf::from(path));
            }
        }
    }

    roots.push(PathBuf::from("/Applications/Xcode.app/Contents/Developer"));
    roots.push(PathBuf::from("/Library/Developer/CommandLineTools"));

    roots.sort();
    roots.dedup();
    roots
}

fn runtime_dirs_for_root(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for base in [
        root.join("Toolchains/XcodeDefault.xctoolchain/usr/lib/clang"),
        root.join("usr/lib/clang"),
        root.join("lib/clang"),
    ] {
        dirs.extend(runtime_dirs_for_clang_base(&base));
    }
    dirs
}

fn runtime_dirs_for_clang_base(base: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(base) else {
        return Vec::new();
    };

    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let runtime_dir = entry.path().join("lib/darwin");
        if runtime_dir.join("libclang_rt.osx.a").is_file() {
            dirs.push(runtime_dir);
        }
    }
    dirs
}
