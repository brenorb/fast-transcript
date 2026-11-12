export const title = "Add speaker labels to an fscript transcript";
export const state = ({d}) => ({
  transcribed: "Transcript available",
  waiting: "Detecting speakers",
  unlabeled: "Return transcript without speaker labels",
  merging: "Matching speakers to transcript segments",
  failed: "Report diarization failure",
  merged: "Return transcript with speaker metadata",
})[d.phase];
export const step = action => ({
  NoRequest: "speaker detection not selected",
  StartHelper: "run the selected backend",
  NoSpeech: "backend reports no speech",
  HelperFails: "backend or its result fails",
  HelperSucceeds: "read detected speaker segments",
  ApplySpeakers: "assign speakers by overlap",
  Done: "finish",
})[action];
