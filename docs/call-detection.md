# Call detection

Milestone 7 is in progress. The shared passive inspector, calendar context and
automatic CLI/desktop policy are implemented. Native automatic-call acceptance
and the first-use calendar grant remain open; see [automatic meetings](automatic-meetings.md)
and [calendar context](calendar.md). Inspection never starts recording.

```sh
fscript calls
fscript calls --json
fscript calls --watch
fscript calls --watch --duration 30 --json
```

The default is one immediate snapshot, with no flags, models or setup. Human output
reports call audio candidates. JSON contains version 1, observed_at_unix_ms,
snapshot (complete, clients, unavailable_processes), and candidates. Each active
audio client supplies pid, bundle_id, input and output. This is local process
metadata, not audio, app-window contents, or a claim that a calendar event is
actually happening. A snapshot alone does not confirm a call.

Native inspection requires macOS 14.2+. It reads Core Audio process objects and
their running input/output properties, using the existing shared metadata helper.
It creates no tap, input stream, calendar store, model, config file, permission
request or external process beyond the normal CLI supervisor. It does not read
the clipboard or inspect app windows. Missing metadata is explicitly uncertain.
Unknown active audio clients remain visible in JSON for diagnosis; only recognized
call apps and browsers participate in detection.

## Watch contract

--watch polls passive metadata every two seconds. It confirms a call after five
seconds of continuous observations in which the same recognized app uses both
microphone and output. Dotted helper bundle identities join their app family,
with case-insensitive, component-boundary matching. Audio from a different app
cannot provide corroboration. Voice/dictation apps and Fast Transcript itself
are not call apps. A running app, calendar slot, camera, or output alone cannot
start detection.

After confirmation, microphone activity keeps the call active even if its output
is idle. Twenty seconds of continuously observed microphone absence ends it.
Unknown observations or a sample gap over five seconds reset candidate and absence
timers; a sleeping computer or an unreadable process cannot silently count as
five seconds of confirmation or twenty seconds of absence. Reversed monotonic
observation time fails. One watch emits one start per active app until that call
ends. Confirmation state is in memory and is not restored across process restarts.

Plain watch prints call start/end lines on stdout, and an initial status on stderr.
--quiet hides that status. --json uses flushed JSONL: version 1, contiguous sequence
from zero, type, data. snapshot includes observed_ms, raw snapshot, candidates,
and confirmed active app names. started/ended supply app and observed_ms. Events
are ordered deterministically, with endings before starts. completed ends a
successful duration-limited watch, without inventing call endings. --duration
requires --watch and positive finite seconds. Bare watch requires no terminal;
Ctrl-C/SIGTERM cancels the supervised job with status 130. EOF alone is not a
successful watch completion.

## Reference decisions and limits

OpenOats' pinned [detector](https://github.com/yazinsai/OpenOats/blob/d8caac78ea283dc791f8d1c4185450e445d3f336/OpenOats/Sources/OatsCore/CallDetector.swift)
and [decision record](https://github.com/yazinsai/OpenOats/blob/d8caac78ea283dc791f8d1c4185450e445d3f336/docs/adr/0005-meeting-detection-asks-about-the-app-on-the-mic.md)
explain why global microphone activity plus an open meeting app caused false
recordings. Fast Transcript adopts attribution to actual audio processes and
same-app corroboration. Its [watcher](https://github.com/yazinsai/OpenOats/blob/d8caac78ea283dc791f8d1c4185450e445d3f336/OpenOats/Sources/OatsCapture/CallWatcher.swift)
uses device listeners to avoid idle polling; this first inspector uses bounded
two-second polling, shared by the explicit desktop automatic-recording preference. Camera-based
corroboration is not included. Calls whose output is unattributed or whose input
has not started cannot yet be confirmed.

Browser audio is an app-level heuristic: a browser voice assistant or microphone
test that also plays audio can qualify. This is evidence of call-like activity,
not identification of a particular tab or a guaranteed meeting. No automatic
recording preference is enabled by this command. Automatic recording is explicit
opt-in and uses this same Rust detector through `fscript meeting watch`.

## Validation

Rust timing and randomized property checks cover unrelated apps, helper-boundary
matching, confirmation/grace periods, uncertainty and sleep. A separate
sample-counter oracle checks randomized multi-app traces against detector events.
Process E2E covers default and quiet JSON snapshots, flushed events before EOF,
exactly one start/end, duration completion, cancellation, invalid traces and
arguments, and independence from transcription configuration/tools. Explicit
FSCRIPT_CALLS_TEST_TRACE fixtures use logical timestamps and never substitute for
a native recording test. Native metadata inspection and actual call transitions
have separate coverage; native transition acceptance remains to be performed.

On 2026-11-27, the clean isolated CLI passed all four detector checks, including
65,536 multi-app observations against the independent counter oracle, and eleven
call/single-command process E2E checks. Native inspection on the Apple M5 Max host
reported complete metadata before, during and after a controlled afplay process.
That process used the checked-in public speech at zero application volume: its
exact PID appeared with output=true/input=false and disappeared after exit. The
check opened no microphone, requested no grant and started no recording. This
proves the native metadata adapter and process attribution for that producer;
it does not establish genuine Zoom/Teams/browser call transitions or camera
corroboration.

The full clean Rust regression run passed 250 tests; the separate hour-long
waveform check remains an explicit release-mode CI job rather than part of this
debug invocation.
