"""Exercise a built fscript against a fake yt-dlp. No media or model download."""

import os
import subprocess
import sys
import tempfile
from pathlib import Path

FAKE_YT_DLP = """#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

args = sys.argv[1:]
if "--version" in args:
    print("2026.01.01")
    sys.exit(0)
with Path(os.environ["FSCRIPT_ROUTE_LOG"]).open("a") as log:
    if "--dump-single-json" in args:
        print("metadata", file=log)
        print(json.dumps({"id": "fixture", "subtitles": {"en": [{"ext": "vtt"}]}}))
    elif "--write-subs" in args:
        print("subtitles", file=log)
        template = args[args.index("--output") + 1]
        target = Path(template.replace("%(id)s", "fixture").replace("%(ext)s", "vtt"))
        target.write_text("WEBVTT\\n\\n00:00:00.000 --> 00:00:01.000\\nPlatform caption.\\n")
    else:
        print("audio", file=log)
        print("fixture intentionally stops at audio download", file=sys.stderr)
        sys.exit(42)
"""


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("Usage: check-remote-route.py <built-fscript>")
    executable = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="fscript-route-") as scratch:
        root = Path(scratch)
        log = root / "calls"
        downloader = root / "yt-dlp"
        downloader.write_text(FAKE_YT_DLP)
        downloader.chmod(0o700)
        env = dict(os.environ, PATH=f"{root}{os.pathsep}{os.environ['PATH']}", FSCRIPT_ROUTE_LOG=str(log))
        command = [str(executable), "https://example.test/fixture", "--stdout", "--text=plain"]

        default = subprocess.run(command, env=env, capture_output=True, text=True, timeout=10)
        assert default.returncode == 0, default.stderr
        assert default.stdout.strip() == "Platform caption.", default.stdout
        assert log.read_text().splitlines() == ["metadata", "subtitles"]

        log.unlink()
        local = subprocess.run([*command, "--local"], env=env, capture_output=True, text=True, timeout=10)
        assert log.read_text().splitlines() == ["metadata", "audio"], "local run fetched subtitles"
        assert local.returncode != 0, "local run unexpectedly succeeded without audio"
        assert "fixture intentionally stops at audio download" in local.stderr
        print("Platform captions and explicit local routing verified.")


if __name__ == "__main__":
    main()
