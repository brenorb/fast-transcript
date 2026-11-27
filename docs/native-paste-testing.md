# Native paste acceptance

This opt-in macOS check validates actual insertion by the shared fscript export
action. A zero CLI exit code alone does not establish that a destination accepted
the queued keyboard event. CI compiles these fixtures and validates rejected
specifications; it does not silently grant GUI permissions or claim this local
acceptance test ran there.

## Receiver and execution context

Compile tests/fixtures/PasteTarget.swift into a disposable .app whose bundle
identifier is com.brenorb.fast-transcript.paste-test. It opens an empty NSTextView,
has standard Edit menu key equivalents, and saves no user documents. Before
testing the product, verify normal Command-V with a known test marker and clear
the field. A text view without the standard Paste key equivalent was an invalid
earlier fixture.

Launch that receiver with FSCRIPT_PASTE_TEST_RESULT pointing to a temporary JSONL
file. The receiver records its own ready, change, and Paste callbacks, including
the controlled test field's text. FSCRIPT_PASTE_TEST_EVENTS=1 additionally prints
keycode/modifier/routing metadata, never typed characters. These are test-only
controls and are not bundled with Fast Transcript.

Compile tests/fixtures/NativePasteCheck.swift with swiftc. It takes one JSON
specification path. Run it from the same app or terminal responsible for the
actual paste grant. For GUI validation it can be a temporary custom notes agent
launched by Fast Transcript; no external AI provider is involved. Restore the
isolated agent configuration afterward.

```json
{
  "targetPid": 12345,
  "observations": "/tmp/fscript-paste-receiver.jsonl",
  "report": "/tmp/fscript-paste-report.json",
  "cases": [
    {
      "name": "current-cli",
      "binary": "/absolute/path/to/fscript",
      "transcript": "/absolute/path/to/public-fixture-transcript.json",
      "expectedInsertion": true
    }
  ]
}
```

Use the exact live receiver PID. The runner requires a ready observation from
that PID and at least one asserted positive case. It inspects paste permission
without requesting access, derives expected bytes from the CLI compact renderer,
and waits up to 90 seconds for that exact receiver to be foreground. Activating
the receiver through Finder while the observer is armed catches short genuine
foreground intervals; app-targeted automation alone does not prove global focus.

Each actual paste launches fscript export with normal process exit and no stdout
backpressure. An asserted positive case requires exactly one new native Paste
callback and the exact expected cumulative text. An asserted negative case
requires zero callbacks, unchanged text, and retained foreground identity. A null
expectation is a diagnostic comparison row and cannot make an all-observational
specification pass. The JSON report records insertion separately from CLI exit
and logs the foreground PID before and after each attempt. Missing access,
incomplete execution, or a failed assertion produces a nonzero exit.

## Sender lifetime regression (2026-11-27)

The unchanged granted app launched the old bundled CLI into the corrected native
receiver. Actual foreground identity was checked in the app-owned runner before
each attempt. Holding the same posting process alive for 500 ms with a filled
stdout pipe caused insertion; allowing immediate normal exit did not. Reversing
the order reproduced that difference. This was a diagnostic lifetime control,
not the acceptance mode described above.

The production correction keeps macOS paste alive for 100 ms after posting the
complete Command-V chord. It adds no pre-paste delay and does not affect copy-only
delivery or introduce another flag/command. The wait is a settling interval,
not an acknowledgement from an arbitrary target application.

The actual 100 ms CLI was then tested with normal stdout/exit in this order:

| Case | Implementation | CLI exit | Native Paste callbacks | Exact insertion |
| --- | --- | --- | --- | --- |
| A | Old CLI | 0 | 0 | No |
| B | 100 ms CLI | 0 | 1 | Yes |
| C | 100 ms CLI | 0 | 1 | Yes |
| D | Old CLI | 0 | 0 | No |
| E | 100 ms CLI | 0 | 1 | Yes |

All five attempts reported the same receiver PID before and after delivery. The
visible field contained only the three fixed-case markers, matching its native
callbacks and the compact renderer. This validates the chosen wait on this Mac;
it is not a claim that 100 ms is a universal minimum or a recognition benchmark.
