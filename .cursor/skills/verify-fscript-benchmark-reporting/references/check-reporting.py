"""Run report generation and historical rescoring against local fixture files."""
import json
import shutil
import subprocess
import sys
from pathlib import Path
from tempfile import TemporaryDirectory


def run(script, *args):
    return subprocess.run([sys.executable, f"scripts/{script}.py", *args], capture_output=True, text=True)


with TemporaryDirectory() as temporary:
    root = Path(temporary)
    runs = root / "runs"
    runs.mkdir()
    source = Path("experiments/transcription_benchmarks/runs/2026-09-30-unified-v2")
    for name in ("environment.json", "summary.json", "validation.json"):
        shutil.copy2(source / name, runs / name)
    validation = json.loads((runs / "validation.json").read_text())
    validation["protocol"] = "wrong"
    (runs / "validation.json").write_text(json.dumps(validation))
    report = runs / "REPORT.md"
    result = run("report_asr_benchmark", "--runs-dir", str(runs))
    assert result.returncode != 0 and "AssertionError" in result.stderr, result
    assert not report.exists(), "A report was written despite a protocol mismatch"
    shutil.copy2(source / "validation.json", runs / "validation.json")
    result = run("report_asr_benchmark", "--runs-dir", str(runs))
    assert result.returncode == 0, result.stderr
    def stable_lines(path):
        return [line for line in path.read_text().splitlines() if "run-results folder currently occupies" not in line]

    assert stable_lines(report) == stable_lines(source / "REPORT.md")

    reference = root / "reference.json"
    reference_data = {
        "audio_id": "fixture", "text": "one two", "complete": False,
        "reference_type": "fixture", "review_date": "2000-01-01",
    }
    reference.write_text(json.dumps(reference_data))
    sample = runs / "sample.json"
    original = json.dumps({
        "metrics": {"audio_id": "fixture", "engine": "fixture", "device": "cpu"},
        "result": {"text": "one three"},
    })
    sample.write_text(original)
    out = root / "scores"
    args = ("--reference", str(reference), "--runs-dir", str(runs), "--out-dir", str(out))
    result = run("rescore_transcription_benchmarks", *args)
    assert result.returncode != 0 and "complete, nonempty" in result.stderr, result
    assert not out.exists()
    reference_data["complete"] = True
    reference.write_text(json.dumps(reference_data))
    result = run("rescore_transcription_benchmarks", *args)
    assert result.returncode == 0, result.stderr
    assert json.loads((out / "scores.json").read_text())["runs"][0]["normalized_wer"] == 0.5
    assert (out / "scores.csv").exists()
    assert sample.read_text() == original
print("Report and rescoring gates held, and the original measurements stayed unchanged.")
