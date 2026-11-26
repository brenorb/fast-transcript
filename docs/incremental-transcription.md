# Incremental transcription

Status: in development; no streaming CLI is shipped by the chunk-core refactor.

Batch inference and arriving audio share `InferenceOutcome::decode_chunk`, which
owns model invocation, timing, source offsets, and transcript/segment merging.
`PcmChunker` accepts arbitrarily sized arrivals while holding at most the selected
chunk's audio plus the caller's input block. It can emit a shorter opening chunk,
then use larger steady chunks with overlap. Finalization does not decode an
overlap-only tail a second time.

The loaded speech engine belongs to the job, not a chunk. Feed every chunk to that
same engine to preserve its warm state. Capturing/decoding and GUI rendering must
not introduce another inference or merge implementation.

Validation: 3,000 reproducible randomized arrival patterns are compared against
batch chunk ranges, including exact chunk boundaries, empty input, and large
arrivals. Every emitted sample is checked against its original position. A
differential test compares batch and incremental text, segment times, and chunk
boundaries through the shared inference path. Tests also cover shorter opening
chunks, repeated finalization, and immediate propagation of decode failure.

The next implementation step connects incremental FFmpeg decoding, CLI output,
the versioned event protocol, and process cancellation to this core.
