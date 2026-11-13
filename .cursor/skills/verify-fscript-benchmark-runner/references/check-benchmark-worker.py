"""Exercise the real worker monitor with short local subprocesses."""
import sys
from pathlib import Path
from tempfile import TemporaryDirectory

sys.path.insert(0, str(Path.cwd() / "scripts"))
from benchmark_asr import execute

with TemporaryDirectory() as temporary:
    log = Path(temporary) / "worker.log"
    result = execute(
        [sys.executable, "-c", "print('{\"phase\":\"cold\"}'); print('{\"phase\":\"warm\"}')"],
        log,
    )
    assert result["cold"]["parent_received_seconds"] <= result["warm"]["parent_received_seconds"]
    for program in (
        "print('{\"phase\":\"cold\"}')",
        "print('{\"phase\":\"warm\"}'); print('{\"phase\":\"cold\"}')",
        "raise SystemExit(3)",
    ):
        try:
            execute([sys.executable, "-c", program], log)
        except RuntimeError as error:
            assert "Worker failed" in str(error)
        else:
            raise AssertionError("Accepted an incomplete or failed benchmark worker")
print("The worker monitor accepted the ordered pair and rejected incomplete and failed workers.")
