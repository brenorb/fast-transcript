"""Verify contiguous decoded PCM from the production partitioner."""
import struct
import sys
import wave
from pathlib import Path
from tempfile import TemporaryDirectory

sys.path.insert(0, str(Path.cwd() / "scripts"))
from prepare_asr_benchmarks import partition

sample_rate = 16000
for count in (sample_rate, 31 * sample_rate, 31 * sample_rate + 1, 63 * sample_rate):
    with TemporaryDirectory() as temporary:
        root = Path(temporary)
        source = root / "source.wav"
        pattern = struct.pack("<256h", *range(256))
        pcm = (pattern * (count // 256 + 1))[:count * 2]
        with wave.open(str(source), "wb") as handle:
            handle.setparams((1, 2, sample_rate, count, "NONE", "not compressed"))
            handle.writeframes(pcm)

        parts, seconds = partition(source, root / "parts")
        assert seconds == count / sample_rate
        cursor = 0
        joined = bytearray()
        for part in parts:
            assert part["start_sample"] == cursor
            end = part["end_sample"]
            assert 0 < end - cursor <= 31 * sample_rate
            with wave.open(part["path"], "rb") as handle:
                assert handle.getnframes() == end - cursor
                joined.extend(handle.readframes(handle.getnframes()))
            cursor = end
        assert cursor == count
        assert joined == pcm
print("All partitioned WAVs reconstructed the exact source samples.")
