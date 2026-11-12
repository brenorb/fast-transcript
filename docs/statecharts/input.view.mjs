export const title = "Prepare audio from a file or a pipe";
export const state = ({m}) => ({
  input: "Media supplied",
  reading: "Saving piped media temporarily",
  probe: "Checking audio format",
  converting: "Converting to 16 kHz mono",
  "read samples": "Reading audio samples",
  transcribing: "Transcribing audio",
  ready: "Transcript ready",
  done: "Transcript delivered",
  failed: "Command failed",
})[m.phase];
export const step = action => ({
  UsePath: "open media file",
  RejectInteractive: "reject terminal input",
  Spool: "read piped input",
  SpoolFinished: "finish reading nonempty input",
  ReuseWav: "use supported audio as supplied",
  Convert: "convert unsupported audio",
  ConversionFinished: "finish converting",
  ReadSamples: "load audio samples",
  Finish: "finish transcription",
  Deliver: "deliver transcript",
  Fail: "report failure and release temporary media",
  Done: "finish",
})[action];
