export const title = "fscript transcription from a URL";

const labels = {
  metadata: "Checking the URL",
  routing: "Choosing a transcript source",
  subtitles: "Retrieving manual captions",
  "downloading audio": "Downloading audio",
  transcribing: "Transcribing on this device",
  rendering: "Preparing the transcript",
  failed: "Command failed",
};

export function state({ run }) {
  if (run.phase === "done") {
    return run.result === "subtitles" ? "Platform transcript delivered" : "Local transcript delivered";
  }
  if (run.phase === "rendering") {
    return run.result === "subtitles" ? "Preparing the platform transcript" : "Preparing the local transcript";
  }
  return labels[run.phase];
}

export function step(action, from, to) {
  switch (action) {
    case "MetadataFails": return "the URL could not be inspected";
    case "MetadataLoaded": return "media details are available";
    case "FetchManualSubtitles": return "manual captions are available [no local override or speaker labels]";
    case "ChooseAudio": return "captions are absent, local mode is requested, or speaker labels are enabled";
    case "SubtitlesReady": return "the platform transcript is usable";
    case "SubtitlesUnavailable": return "the captions are empty or unavailable";
    case "AudioDownloaded": return "the audio is ready";
    case "TranscriptionReady": return "the local model finishes";
    case "AudioOrOutputFails":
      return {
        "downloading audio": "the audio download fails",
        transcribing: "transcription fails",
        rendering: "the transcript cannot be rendered or written",
      }[from.run.phase];
    case "OutputDelivered": return "the transcript is printed or saved";
    case "Done": return "the command has exited";
    default: throw new Error(`Unexpected action ${action} from ${from.run.phase} to ${to.run.phase}`);
  }
}
