# Local calendar context

Calendar context is optional. In the desktop gear menu, enable **Use calendar
meeting names**, or run `fscript calendar connect`. It uses accounts already
configured in macOS Calendar through EventKit. No separate Google/Microsoft login
or cloud calendar client is required. Calendar times do not start recordings.
Automatic recording remains the next milestone-7 step.

```bash
fscript calendar
fscript calendar status --json
fscript calendar connect
fscript calendar --start 2026-11-01T00:00:00-03:00 --end 2026-12-01T00:00:00-03:00 --json
fscript calendar match 2026-11-27T14:00:00Z 2026-11-27T15:00:00Z --json
fscript calendar disconnect
```

Bare calendar lists the next 24 hours. Explicit periods require both RFC 3339
timestamps, including timezone; the end is exclusive. Listing inventories every
calendar and overlapping event visible to EventKit, including personal, declined,
cancelled and all-day entries. Recurrence occurrences remain separate. Reads are
split into 90-day queries: EventKit otherwise silently truncates predicates longer
than four years. Returned calendar coverage means local EventKit visibility, not
proof that every remote account has synchronized.

Only Connect can request access. macOS calls its read/write permission **full
access**; Fast Transcript only reads calendar metadata and does not modify events.
Status inspects authorization and preference without querying events. Listing
without full access returns `queried: false`, rather than claiming an empty
calendar. Matching without full access fails. Denied/restricted grants require a
change in System Settings; the app does not repeatedly request them. Disconnect
disables future naming but leaves OS authorization and existing titles intact.
The desktop refreshes status when activated and through **Refresh calendar status**.

## Matching and saved meetings

The shared Rust matcher considers the full retained recording interval. An event
must have a title, positive duration, and another attendee or a recognized call
link. All-day, cancelled and declined events are excluded. Greatest overlap wins;
ties prefer the shorter event, then earlier start and stable calendar/event IDs.
Overlap must cover at least one quarter of the shorter interval. Calendar times
provide context, not evidence that a call occurred.

After capture stops, or before saved-audio transcription, enabled naming checks
current access without requesting permission. The chosen `calendar_event` is
saved atomically in the version-1 session manifest before final recognition.
Saved/restored events and the completed document's `meeting` field share this
manifest. Naming failure warns but does not discard audio or fail transcription.
An existing name survives disconnect, replay and recognition failure. Session
directory IDs stay stable; the GUI presents the title as the document name.
Older manifests without the optional field remain readable. Older readers that
reject unknown fields cannot read newly named manifests.

The stored event contains IDs, title, start/end UTC milliseconds, and boolean
eligibility metadata. Participant identities, email addresses, event notes and
meeting links are not stored. Notes/location/URL are inspected only to recognize
call hosts. Saved transcripts and explicit calendar JSON output can contain event
titles; share them accordingly.

## Machine contract

`--json` prints one version-1 object: `supported`, `authorization`, `name_meetings`,
`queried`, `window`, `calendars`, `events`, and `matched_event`. Window is null for
status/connect/disconnect. Calendar entries contain `id`, `title`, `source`; events
contain `id`, `calendar_id`, `title`, `start_ms`, `end_ms`, `all_day`, `cancelled`,
`declined`, `with_others`, `has_call_link`. Only match fills `matched_event`.
Authorization is `not_determined`, `restricted`, `denied`, `write_only`,
`full_access`, `unsupported`, or `unknown`. Unsupported inspection succeeds;
Connect requires macOS 14.2 or later. Errors use stderr/nonzero status. Ctrl-C
cancels the supervised request, including a pending permission response.

Preferences are an atomically replaced private `calendar.json` alongside model
settings, containing `{"version":1,"name_meetings":true}`. Override its path with
`FSCRIPT_CALENDAR_CONFIG_FILE`. Missing preferences default to disabled. Inspection
does not create preferences or load a speech model. The GUI calls these exact CLI
actions and has no EventKit code or duplicate matching policy.

## Validation coverage

Rust property checks cover full-period chunk coverage and timestamp offsets;
independent time-grid differential checks exercise matching and input order.
Process E2E covers complete six-year inventory, recurrence, eligibility, permission
states, connect/disconnect/revocation, invalid input, and cancellation after a
confirmed blocked request. Meeting E2E checks durable title agreement and replay
after disconnect. Native presenter tests use the actual CLI with explicit
calendar fixtures, including bounded Quit during a blocked permission helper.
These fixtures never request an OS grant. A passive native status probe reported
`not_determined` on this host; real account-event delivery and the first-use macOS
grant remain unverified until that grant can be tested with the user present.

The design follows OpenOats' separation of optional calendar titles from actual
call detection; see [reference study](reference-apps.md). Relevant Apple contracts:
[EventKit authorization](https://developer.apple.com/documentation/eventkit/ekeventstore/authorizationstatus(for:)),
[full access](https://developer.apple.com/documentation/eventkit/ekeventstore/requestfullaccesstoevents(completion:)),
and [event period queries](https://developer.apple.com/documentation/eventkit/ekeventstore/predicateforevents(withstart:end:calendars:)).


On 2026-11-27 the isolated native GUI was checked with prerecorded public speech
and a full-access calendar fixture, using the real cached parakeet-v3-int8 model
on CPU. The gear-menu toggle enabled naming, Refresh kept its state, Record/Stop
produced a completed document titled **Calendar acceptance ✍️**, and disabling
naming followed by Open last meeting retained that title and both channel rows.
No native audio endpoint or real calendar account was opened. Quit closed the
app. The CLI process suite passed 39 calendar/single-entrypoint/meeting tests;
the Rust suite passed 254 checks (one separate release-only hour simulation was
ignored in debug); the native suite passed 26 protocol/process/presenter checks.
The new calendar presenter checks include cancellation after the fixture confirms
that its permission request has begun. CI runs the release-only clock test.
