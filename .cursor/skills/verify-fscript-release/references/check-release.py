import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path


workflow = Path(sys.argv[1] if len(sys.argv) > 1 else ".github/workflows/release.yml")
source = workflow.read_text()
jobs = dict(re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)", source.split("jobs:\n")[1], re.M | re.S))
for name in ("publish", "publish-pypi", "publish-crates-io"):
    assert "if: startsWith(github.ref, 'refs/tags/')" in jobs[name], name
    assert "needs: build" in jobs[name], name
assert "name: pypi-macos" in jobs["publish-pypi"]
markers = [
    "Build release binary", "Package release archive", "Build PyPI wheel",
    "Validate release binary", "Smoke test installed wheel", "actions/upload-artifact",
]
positions = [jobs["build"].index(marker) for marker in markers]
assert positions == sorted(positions)
assert "os: macos-latest" in jobs["build"] and "os: ubuntu-latest" in jobs["build"]
if "id: crate-state" not in jobs["publish-crates-io"]:
    assert "--allow-legacy" in sys.argv, "expected a crate registry decision"
    assert "run: cargo publish --locked" in jobs["publish-crates-io"]
    print("Tagged release always attempts cargo publish after authentication")
    sys.exit(0)

guard_job = jobs["publish-crates-io"]
script = guard_job.split("id: crate-state", 1)[1].split("run: |\n", 1)[1]
script = script.split("\n      - ", 1)[0]
script = "\n".join(line.removeprefix("          ") for line in script.splitlines())
assert guard_job.count("if: steps.crate-state.outputs.already_published != 'true'") == 2

with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    commands = {
        "cargo": """echo '{"packages":[{"name":"fast-transcript","version":"1.1.3"}]}'""",
        "curl": """[ "$REGISTRY" != unavailable ] || exit 22
printf '{"crate":{"newest_version":"%s"}}\\n' "$REGISTRY" """,
    }
    for command, body in commands.items():
        executable = root / command
        executable.write_text("#!/bin/sh\n" + body + "\n")
        executable.chmod(0o755)
    for registry, expected in (("1.1.3", "true"), ("1.1.4", "false"), ("unavailable", "false")):
        output = root / "output"
        output.unlink(missing_ok=True)
        env = dict(os.environ, REGISTRY=registry, GITHUB_OUTPUT=str(output))
        env["PATH"] = str(root) + os.pathsep + env["PATH"]
        subprocess.run(["bash", "-c", script], env=env, check=True)
        assert output.read_text().strip() == f"already_published={expected}", registry
print("Release gates and crate outcomes passed")
