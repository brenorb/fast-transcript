import os
import subprocess
import sys
import tempfile
from pathlib import Path


source = Path(sys.argv[1] if len(sys.argv) > 1 else "build.rs").resolve()
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp).resolve()
    compiler = root / "probe"
    subprocess.run(["rustc", str(source), "-o", str(compiler)], check=True)
    developer = root / "Developer"
    valid = developer / "usr/lib/clang/17/lib/darwin"
    empty = developer / "lib/clang/16/lib/darwin"
    valid.mkdir(parents=True)
    empty.mkdir(parents=True)
    (valid / "libclang_rt.osx.a").touch()
    selector = root / "xcode-select"
    selector.write_text('#!/bin/sh\nprintf "%s\\n" "$DEVELOPER_DIR"\ntouch "$SELECTOR_MARKER"\n')
    selector.chmod(0o755)
    marker = root / "selector-called"
    env = dict(os.environ, DEVELOPER_DIR=str(developer), SELECTOR_MARKER=str(marker))
    env["PATH"] = str(root) + os.pathsep + env["PATH"]

    def run(target):
        result = subprocess.run(
            [str(compiler)], env=dict(env, TARGET=target),
            check=True, capture_output=True, text=True,
        )
        return [line for line in result.stdout.splitlines() if str(root) in line]

    assert run("x86_64-unknown-linux-gnu") == []
    assert not marker.exists()
    assert run("aarch64-apple-darwin") == [f"cargo:rustc-link-search=native={valid}"]
    assert marker.exists()
print("Target bypass, duplicate root, and required archive checks passed")
