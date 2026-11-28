# Automatic meetings

`fscript meeting watch` arms foreground automatic recording. It needs no terminal
or extra flags. The shared call monitor confirms five seconds of microphone and
output activity attributed to the same recognized app, then starts the existing
meeting capture service. Twenty seconds of continuously observed microphone
absence ends an automatically owned recording. Unknown observations and sleep
gaps cannot establish absence. The calendar supplies optional titles, never a
recording trigger. See [call detection](call-detection.md) for recognized apps and
heuristic limits, including browser voice assistants that can resemble a call.

```bash
fscript meeting watch
fscript meeting watch --duration 3600
fscript meeting watch --json
fscript meeting watch --control --json
fscript meeting auto status --json
fscript meeting auto enable
fscript meeting auto disable
```

Watch explicitly authorizes recording for that invocation, independently of the
saved desktop-startup preference. `meeting auto` defaults to status; enable and
disable persist that preference without inspecting devices, requesting permission,
loading a model or starting a daemon. The desktop gear/menu toggle saves this same preference and owns one controlled
watcher while the app runs. It arms on startup only when previously enabled.
Refreshing status never restarts a failed or cancelled watcher.

By default, watch prints provisional text/status on stderr and one absolute
transcript path per completed recording on stdout. `--json` selects versioned
events instead. `--quiet` suppresses status in both watcher and meeting child;
paths/results and errors remain. `--model`, `--model-dir`, `--device` and
`--output-dir` modify the same underlying meeting command. Otherwise each session
uses the current shared model preference. Models are loaded only after admission.
Microphone and system access are governed by macOS when actual capture starts;
ordinary call inspection and preference status never request either permission.

## Ownership and episodes

One continuous confirmed call interval is an episode. Overlapping call apps share
one recording. An episode is accepted at most once. Stop consumes that episode:
the same continuing call cannot immediately restart recording. Confirmed absence
re-arms detection for the next episode. A call beginning while the previous
transcript finishes remains eligible after the old session has been reaped, if
it is still active. A pending offer does not open capture or load a model.

One nonblocking OS lock allows one watcher per settings profile. Live manual and
automatic meetings share a second lock through final recognition. A manual
recording already covering the detected call consumes its episode; the watcher
neither duplicates nor stops that manual recording. A competing start between
admission and capture is rejected by the actual capture lock. Saved-audio replay
and inspection remain available independently.

`FSCRIPT_MEETING_CONFIG_FILE` overrides the profile's preference file, otherwise
`meetings.json` lives beside model/calendar settings. Preferences contain
`{"version":1,"enabled":false}` and default to disabled when absent. Inspection
does not create them. Writes replace a private temporary file atomically. Watch
and capture locks use `.watch.lock` and `.capture.lock` suffixes on that path.
Lock files can remain after exit; the OS lock, not file existence, establishes
ownership. Use the same profile across CLI and GUI for exclusion.

Watch launches exactly the existing `fscript meeting --control --json` as a child
of its one supervised job. It deliberately retains the job environment and
process group. No nested supervisor or separate capture implementation exists.
The asynchronous bounded output queue keeps call detection and control responsive
while recognition is blocked. A child must publish its successful final event,
close its output, and exit zero before another session can start.

Ctrl-C, controller EOF, Cancel and Shutdown terminate the whole watch generation;
the outer supervisor bounds cleanup of capture, converters and model descendants.
Saved audio remains recoverable. Cancellation/error is not followed by an eager
watch restart: a fresh detector cannot establish that the previous call ended.
Re-enable/restart explicitly after addressing the failure. Recognition failure
continues saving audio until Stop/call end, as in manual meetings, then fails the
watch without erasing audio. A metadata-read failure also fails the watch and
retains recoverable audio; it is not reported as a normal call end.

`--duration` limits watching, then gracefully stops active capture and waits for
final recognition. It is not an inference timeout. Ctrl-C still cancels blocked
recognition. Use `meeting transcribe SESSION` to retry retained audio.

## Control protocol

`--control` requires an admission response for each offer. It never accepts an
offer automatically. A GUI can reserve its workspace and stop warm dictation
before accepting; a busy client can leave offers pending without recording.
Offers repeat on observations while eligible and unconsumed. Each JSONL request
must fit 64 KiB and end with a newline:

```json
{"action":"accept","episode":1}
{"action":"skip","episode":1}
{"action":"stop","episode":1}
{"action":"cancel","episode":1}
{"action":"suppress"}
{"action":"shutdown"}
```

Accept admits a current offer only if no session is owned. Stale, skipped or
consumed offers produce `offer_expired`. Skip consumes the current episode without
recording. Stop finishes only that automatically owned session and suppresses its
continuing call. Cancel terminates the whole watch generation. Suppress covers
external/manual recording, including input seen before call confirmation; it
clears only on a complete observation with neither confirmed calls nor recognized
call microphone activity. Shutdown cancels the watch regardless of episode.
Requests naming a different active session cannot stop or cancel it.

Outer events have `version:1`, contiguous `sequence`, `type`, null `session_id`,
and `data`. Types are `watch_ready` (`requires_accept`), `call_status`
(`observed_ms`, `complete`, `active`, `episode`), `offer` (`episode`, `apps`),
`accepted`, `offer_expired`, `covered` (`episode`, `reason:manual_recording`),
`suppressed`, `meeting`, `session_exit`, `watch_cancelled`, `watch_completed`.
Episode values are scoped to one watch generation. A meeting event contains
`episode` and the original complete meeting `event`, including its original
sequence/session ID. Use a fresh [meeting decoder](live-meetings.md) per accepted
episode. `session_exit` reports status zero only after successful publication and
reaping. Watch completion/cancellation describes the watcher, never recognition
success for a missing inner transcript. EOF alone is not success.

## Validation and remaining acceptance

Process E2E uses explicit prerecorded microphone/system WAVs, a deterministic
ASR worker, and time-compressed call metadata. It covers two complete calls,
overlapping apps, uncertainty, pending/stale/skipped admission, manual Stop and
suppression, one watcher per profile, manual capture exclusion, and cancellation
after the model confirms an inference request. Independent PCM-slice hashes check
every final segment. Property checks exercise 131,072 policy transitions without
accepting an episode twice; detector checks separately cover attribution and
five-/twenty-second monotonic timing. Call traces require both explicit WAV
transport overrides before this command can run, preventing a fixture call from
accidentally opening native devices.

Native microphone/system capture and passive native Core Audio attribution were
validated separately in earlier milestones. A real native call triggering the
entire automatic lifecycle remains unverified; desktop control acceptance is
described below.
The fixture checks do not establish those native acceptance results. The design
was reviewed with Astra against the existing supervisor and pinned OpenOats
detector/older automatic-recording controller; see [reference study](reference-apps.md).


On 2026-11-27, eight automatic-meeting process E2E cases passed on macOS, as did
seven existing call-inspection E2E cases. A preceding regression run also passed
all 28 existing meeting cases. The full Rust suite passed 256 checks (one hour-long
release-only simulation ignored in debug); the native desktop suite passed 26
protocol/process/presenter checks using isolated settings. The blocked-inference
case observed the model request before sending Ctrl-C and verified that every
logged model PID disappeared. These are fixture/process acceptance results,
not a claim of a real Zoom/Teams call or automatic GUI acceptance.


## Desktop controls and acceptance

Enable **Record calls automatically** in the gear menu or the ✍️ menu-bar menu.
An idle watcher leaves file transcription, model/agent choices and dictation
available. Rust offers a confirmed episode; the desktop reserves its single
workspace synchronously, stops warm dictation, then accepts. Offers received
while another job is busy remain pending. The GUI does not calculate attribution,
five-/twenty-second timing, overlap, episode deduplication or calendar matching.

The existing meeting view shows provisional Microphone/System rows, then replaces
them with the completed document. Stop finishes the automatic session and suppresses
its continuing call while the watcher stays armed. Turning automatic recording
off during capture finishes that transcript before shutting down the watcher.
Cancel disarms the watcher and saves the disabled preference; it never eagerly
restarts into the same call. Quit cancels the owned group and preserves the explicit
startup preference for a future app launch. Failed watching stays visibly disarmed
in the menu bar, with its failure status and off/on retry available.

The monochrome ✍️ item remains while armed, including when the document window is
closed. Its idle label says that no audio is recorded. Removing the active item
disables watching/cancels owned capture, preventing an invisible recorder. An
initially hidden item is not a removal request and must not write preferences.

On 2026-11-27 the isolated native bundle was checked with explicit call metadata
and microphone/system WAV transports plus real cached parakeet-v3-int8 inference
on CPU. No native capture endpoint or real Zoom call was opened. Enabling the
actual gear-menu toggle started capture after confirmation; live recognized words
appeared on both channels before automatic Stop. The final document had six
segments and 258,017 retained frames per channel. Independent batch recognition
of each saved track matched all three segments per channel exactly.

A continuing-call fixture then verified actual toggle-off during capture: it
completed a stopped manifest with 233,376 frames on each track and disarmed the
watcher. A separate actual Stop-button check retained 196,627 frames per track,
completed successfully, and did not restart the continuing call. The final toggle
was off, the test window size was restored, and Quit closed the app. Earlier
popup interactions outside the cropped test window could not be verified; enlarging
the window kept the menu inside it and resolved that Computer Use limitation.
Those unsuccessful attempts were not counted as successful toggle-off checks.

The GUI check also exposed macOS reporting an initially hidden status item through
its binding setter. Treating that as removal repeatedly wrote the disabled
preference and kept controls busy. The handler now requires an actually visible,
owned indicator; a native regression checks that startup creates no preference or
operation. The complete native suite passes 35 tests: 20 protocol/process checks
and 15 presenter/window checks. This includes 128 randomized byte-fragmentation
runs over two independent watch episodes, invalid/unfinished lifecycle rejection,
busy admission, idle resource availability, manual Stop, graceful disable,
Cancel/refresh suppression, visible failure, and bounded Quit after a confirmed
blocked inference request with no surviving logged model PID.

These desktop checks use the real CLI and speech engine where stated. They do
not prove actual native call metadata initiating capture; that remaining native
acceptance gate and the first-use calendar grant are recorded in the roadmap.
