-------------------------------- MODULE Delivery --------------------------------

CONSTANTS AtomicStaging, BrokenPipeSucceeds
VARIABLE output

Init ==
    \E mode \in {"file", "stdout"}, previous \in {"missing", "previous transcript"}:
        output = [mode |-> mode, previous |-> previous, disk |-> previous,
                  committed |-> FALSE, phase |-> IF mode = "file" THEN "resolve" ELSE "write stdout"]

CheckDestination ==
    /\ output.phase = "resolve"
    /\ output' = [output EXCEPT !.phase = "ready to write"]

OpenDestination ==
    /\ output.phase = "ready to write"
    /\ output' = [output EXCEPT !.phase = "writing",
                              !.disk = IF AtomicStaging THEN @ ELSE "incomplete"]

WriteCompletes ==
    /\ output.phase = "writing"
    /\ output' = IF AtomicStaging
        THEN [output EXCEPT !.phase = "flush and sync"]
        ELSE [output EXCEPT !.phase = "resolve saved path",
                             !.disk = "new transcript", !.committed = TRUE]

FlushAndSync ==
    /\ output.phase = "flush and sync"
    /\ output' = [output EXCEPT !.phase = "ready to rename"]

Rename ==
    /\ output.phase = "ready to rename"
    /\ output' = [output EXCEPT !.phase = "resolve saved path",
                               !.disk = "new transcript", !.committed = TRUE]

FileStepFails ==
    /\ output.phase \in {"resolve", "ready to write", "writing", "flush and sync",
                           "ready to rename", "resolve saved path"}
    /\ output' = [output EXCEPT !.phase = "failed"]

ResolveSavedPath ==
    /\ output.phase = "resolve saved path"
    /\ output' = [output EXCEPT !.phase = "write stdout"]

AfterStdout == IF output.mode = "file" THEN "write status" ELSE "done"

WriteStdout ==
    /\ output.phase = "write stdout"
    /\ output' = [output EXCEPT !.phase = AfterStdout]

StdoutConsumerCloses ==
    /\ output.phase = "write stdout"
    /\ output' = [output EXCEPT !.phase = IF BrokenPipeSucceeds THEN AfterStdout ELSE "failed"]

OutputError ==
    /\ output.phase \in {"write stdout", "write status"}
    /\ output' = [output EXCEPT !.phase = "failed"]

StatusWritten ==
    /\ output.phase = "write status"
    /\ output' = [output EXCEPT !.phase = "done"]

Done ==
    /\ output.phase \in {"failed", "done"}
    /\ UNCHANGED output

Next == CheckDestination \/ OpenDestination \/ WriteCompletes \/ FlushAndSync \/ Rename
        \/ FileStepFails \/ ResolveSavedPath \/ WriteStdout \/ StdoutConsumerCloses
        \/ OutputError \/ StatusWritten \/ Done

OriginalPathChangesOnlyAtCommit == ~output.committed => output.disk = output.previous

=============================================================================
