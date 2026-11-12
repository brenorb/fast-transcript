-------------------------------- MODULE Input --------------------------------
CONSTANT RejectTerminal
VARIABLE m
Init ==
    \E source \in {"local", "pipe", "terminal", "download"} :
        m = [source |-> source, phase |-> "input", spool |-> FALSE,
             converted |-> FALSE, readStdin |-> FALSE]
UsePath ==
    /\ m.phase = "input" /\ m.source \in {"local", "download"}
    /\ m' = [m EXCEPT !.phase = "probe"]
RejectInteractive ==
    /\ m.phase = "input" /\ m.source = "terminal" /\ RejectTerminal
    /\ m' = [m EXCEPT !.phase = "failed"]
Spool ==
    /\ m.phase = "input"
    /\ m.source = "pipe" \/ (m.source = "terminal" /\ ~RejectTerminal)
    /\ m' = [m EXCEPT !.phase = "reading", !.spool = TRUE, !.readStdin = TRUE]
SpoolFinished ==
    /\ m.phase = "reading"
    /\ m' = [m EXCEPT !.phase = "probe"]
ReuseWav ==
    /\ m.phase = "probe"
    /\ m' = [m EXCEPT !.phase = "read samples"]
Convert ==
    /\ m.phase = "probe"
    /\ m' = [m EXCEPT !.phase = "converting", !.converted = TRUE]
ConversionFinished ==
    /\ m.phase = "converting"
    /\ m' = [m EXCEPT !.phase = "read samples"]
ReadSamples ==
    /\ m.phase = "read samples"
    /\ m' = [m EXCEPT !.phase = "transcribing"]
Finish ==
    /\ m.phase = "transcribing"
    /\ m' = [m EXCEPT !.phase = "ready", !.converted = FALSE]
Deliver ==
    /\ m.phase = "ready"
    /\ m' = [m EXCEPT !.phase = "done", !.spool = FALSE]
Fail ==
    /\ m.phase \notin {"done", "failed"}
    /\ m' = [m EXCEPT !.phase = "failed", !.converted = FALSE, !.spool = FALSE]
Done == m.phase \in {"done", "failed"} /\ UNCHANGED m
Next == UsePath \/ RejectInteractive \/ Spool \/ SpoolFinished \/ ReuseWav \/ Convert
        \/ ConversionFinished \/ ReadSamples \/ Finish \/ Deliver \/ Fail \/ Done
TerminalNeverRead == m.source = "terminal" => ~m.readStdin
=============================================================================
