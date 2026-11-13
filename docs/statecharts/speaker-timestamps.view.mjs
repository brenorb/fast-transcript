export const title = "Timestamp the first speaker turn";

export function state(value) {
  const segments = Object.keys(value.pending).length;
  const wrong = Object.values(value.output).some(line => line.at !== line.first);
  if (wrong) return "First turn printed at zero";
  if (value.finished) return Object.keys(value.output).length ? "Turns printed with their start times" : "Empty transcript";
  if (segments === 0) return "Transcript ready";
  if (value.at === 0) return "Unlabeled turn buffered without start time";
  return "Turn buffered with its start time";
}

export function step(action, from, to) {
  if (to.index !== from.index) {
    if (state(to) === "First turn printed at zero") return "Speaker changes and flushes the opening turn";
    return to.at === 0 ? "First segment has no speaker label" : "First labeled segment supplies its start time";
  }
  if (to.finished && !from.finished) return "Render the final turn";
  return "Transcript remains available";
}
