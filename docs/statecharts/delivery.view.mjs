export const title = "fscript output to a file or pipeline";

export function state({ output }) {
  if (output.phase === "failed") {
    return output.committed ? "Transcript saved, completion reporting failed" : "Output failed";
  }
  if (output.phase === "done") {
    return output.committed ? "Transcript saved" : "Pipeline command completed";
  }
  if (output.committed) return "Transcript saved, reporting its location";
  if (output.mode === "stdout") return "Writing the transcript to the pipeline";
  if (output.phase === "resolve") return "Checking the output path";
  return "Preparing the replacement transcript";
}

export function step(action, from) {
  switch (action) {
    case "CheckDestination": return "the destination can be used";
    case "OpenDestination": return "a temporary file opens";
    case "WriteCompletes": return "the temporary write finishes";
    case "FlushAndSync": return "the temporary file is flushed and synced";
    case "Rename": return "the completed transcript replaces the destination";
    case "FileStepFails": return from.output.committed ? "the saved path cannot be resolved" : "the save fails";
    case "ResolveSavedPath": return "the absolute path is available";
    case "WriteStdout": return from.output.mode === "stdout" ? "the transcript is delivered" : "the saved path is printed";
    case "StdoutConsumerCloses": return "the downstream command closes the pipe";
    case "OutputError": return "output reports an error";
    case "StatusWritten": return "completion is reported";
    case "Done": return "the command exits";
    default: throw new Error(`Unexpected action ${action}`);
  }
}
