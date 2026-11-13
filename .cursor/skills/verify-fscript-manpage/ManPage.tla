------------------------------- MODULE ManPage -------------------------------
CONSTANT AcceptStale
VARIABLE m
Init ==
    \E check \in BOOLEAN, disk \in {"missing", "stale", "current"} :
        m = [phase |-> "render", disk |-> disk, original |-> disk, check |-> check,
             fenced |-> FALSE, line |-> "none", wrote |-> FALSE]
Fence ==
    /\ m.phase = "render"
    /\ m' = [m EXCEPT !.fenced = ~@]
Line ==
    /\ m.phase = "render"
    /\ m' = [m EXCEPT !.line = IF m.fenced THEN "literal" ELSE "markup"]
FinishRender ==
    /\ m.phase = "render"
    /\ m' = [m EXCEPT !.phase = IF m.check THEN "compare" ELSE "write"]
Compare ==
    /\ m.phase = "compare"
    /\ m' = [m EXCEPT !.phase = IF m.disk = "current" \/ AcceptStale THEN "ok" ELSE "failed"]
Write ==
    /\ m.phase = "write"
    /\ m' = [m EXCEPT !.phase = "ok", !.disk = "current", !.wrote = TRUE]
Fail ==
    /\ m.phase = "write"
    /\ m' = [m EXCEPT !.phase = "failed"]
Done == m.phase \in {"ok", "failed"} /\ UNCHANGED m
Next == Fence \/ Line \/ FinishRender \/ Compare \/ Write \/ Fail \/ Done
CheckIsReadOnly == m.check => m.disk = m.original /\ ~m.wrote
CheckSuccessMeansCurrent == (m.check /\ m.phase = "ok") => m.disk = "current"
=============================================================================
