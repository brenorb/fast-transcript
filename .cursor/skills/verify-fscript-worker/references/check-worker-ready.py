"""Run the built CLI with a model worker that explicitly reports unready."""
import os
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import wave


binary = Path(sys.argv[1]).resolve()
with TemporaryDirectory() as directory:
    root = Path(directory)
    audio = root / "audio.wav"
    with wave.open(str(audio), "wb") as stream:
        stream.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
        stream.writeframes(bytes(32000))

    ffprobe = root / "ffprobe"
    ffprobe.write_text(
        "#!/bin/sh\n"
        "printf '%s\\n' '{\"streams\":[{\"codec_type\":\"audio\",\"sample_rate\":\"16000\","
        "\"channels\":1,\"codec_name\":\"pcm_s16le\",\"bits_per_sample\":16}]}'\n"
    )
    worker = root / "worker"
    worker.write_text(
        "#!/bin/sh\n"
        "printf '%s\\n' '{\"ready\":false,\"device\":\"cpu\",\"model_dir\":\"fixture\",\"load_seconds\":0.1}'\n"
        "if IFS= read -r line; then\n"
        "  printf '%s\\n' requested > \"$FSCRIPT_WORKER_FIXTURE_MARKER\"\n"
        "  printf '%s\\n' '{\"text\":\"unexpected request\",\"segments\":[]}'\n"
        "fi\n"
    )
    ffprobe.chmod(0o700)
    worker.chmod(0o700)
    marker = root / "requested"
    environment = {name: value for name, value in os.environ.items() if not name.startswith("FSCRIPT_")}
    environment.update(
        PATH=str(root),
        FSCRIPT_CONFIG_FILE=str(root / "settings.json"),
        FSCRIPT_PYTHON_BINARY=str(worker),
        FSCRIPT_WORKER_FIXTURE_MARKER=str(marker),
    )
    result = subprocess.run(
        [str(binary), "--model", "ultra", str(audio), "--text=plain", "--stdout", "--no-diarization"],
        env=environment, capture_output=True, text=True, timeout=10,
    )
    assert not marker.exists(), "the unready worker received audio"
    assert result.returncode != 0 and not result.stdout, result
    assert "model worker did not report a valid loaded model" in result.stderr, result
    print("The unready worker received no audio.")
