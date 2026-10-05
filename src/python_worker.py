"""Persistent local ASR worker. stdout is reserved for the JSON line protocol."""

import contextlib
import json
import os
import sys
import time
import traceback
from pathlib import Path
from importlib.metadata import version


def require_version(package, expected):
    actual = version(package)
    if actual != expected:
        raise RuntimeError(
            f"{package}=={expected} is required (found {actual}); "
            "unset FSCRIPT_PYTHON_BINARY to let uv provision the pinned runtime"
        )


def main():
    repository, revision, device, local_directory = sys.argv[1:]
    protocol = sys.stdout

    def send(payload):
        print(json.dumps(payload, ensure_ascii=False, allow_nan=False), file=protocol, flush=True)

    # Model libraries may print diagnostics; keep transcript stdout and the protocol clean.
    with contextlib.redirect_stdout(sys.stderr):
        if repository.startswith("moondream/"):
            require_version("moondream", "2.4.1")
            require_version("kestrel", "0.8.1")
            require_version("torch", "2.14.0")
            import moondream as md
            import torch
            from huggingface_hub import snapshot_download

            if device == "auto":
                device = "cuda" if torch.cuda.is_available() else (
                    "mps" if torch.backends.mps.is_available() else "cpu"
                )
            if device == "mps" and not torch.backends.mps.is_available():
                raise RuntimeError("MPS is unavailable; select --device cpu")
            if device == "cuda" and not torch.cuda.is_available():
                raise RuntimeError("CUDA is unavailable; select --device cpu")
            if local_directory:
                directory = str(Path(local_directory).resolve())
            else:
                directory = snapshot_download(
                    repo_id=repository,
                    revision=revision,
                    allow_patterns=["config.json", "tokenizer.json", "model.safetensors", "ternary.json"],
                )
            started = time.perf_counter()
            model = md.photon(repository, device=device, model_path=directory)
            load_seconds = time.perf_counter() - started

            def transcribe(path):
                result = model.transcribe(audio=path, timestamps="segment")
                if device == "mps":
                    torch.mps.synchronize()
                elif device == "cuda":
                    torch.cuda.synchronize()
                return {"text": result["text"], "segments": result.get("segments", [])}

        else:
            require_version("fermion-research", "0.2.3")
            require_version("torch", "2.14.0")
            if device != "auto":
                os.environ["FERMION_DEVICE"] = device
            else:
                os.environ.pop("FERMION_DEVICE", None)
            from fermion._speech import backends, fetch
            from fermion.transcribe import _resolve

            repo, profile, pin, _ = _resolve(repository)
            if pin["sha256"] != revision:
                raise RuntimeError("Phonon SDK artifact differs from the benchmark pin")
            device = backends.resolve("fscript --model phonon-2")
            backends.require_engine_for(device, pin["backend"])
            directory = str(Path(local_directory).resolve()) if local_directory else str(
                fetch.ensure(repo, profile, pin)
            )
            started = time.perf_counter()
            model = backends.load(device, directory, profile=profile, backend=pin["backend"], quiet=True)
            load_seconds = time.perf_counter() - started

            def transcribe(path):
                result = model.transcribe_detailed(path)
                if result.truncated:
                    raise RuntimeError("Phonon truncated the transcript; use a smaller --chunk")
                return {"text": result.text, "segments": result.segments}

        send({"ready": True, "device": device, "model_dir": directory, "load_seconds": load_seconds})
        for line in sys.stdin:
            try:
                request = json.loads(line)
                send(transcribe(request["audio_path"]))
            except Exception as error:
                send({"error": str(error)})
        if callable(getattr(model, "close", None)):
            model.close()


if __name__ == "__main__":
    try:
        main()
    except BaseException as error:
        print(json.dumps({"error": str(error)}), flush=True)
        traceback.print_exc(file=sys.stderr)
        sys.exit(1)
