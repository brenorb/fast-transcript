-------------------------------- MODULE Resume --------------------------------
CONSTANT VerifyFingerprint
VARIABLE r
Init ==
    \E resume, metadata, match, prior \in BOOLEAN :
        r = [phase |-> "check", resume |-> resume, metadata |-> metadata,
             match |-> match, prior |-> prior, reused |-> FALSE]
Check ==
    /\ r.phase = "check"
    /\ r' = [r EXCEPT !.phase =
        CASE r.resume /\ r.metadata /\ (r.match \/ ~VerifyFingerprint) -> "measure"
          [] r.resume /\ r.metadata -> "failed"
          [] r.prior -> "failed"
          [] OTHER -> "measure"]
Reuse ==
    /\ r.phase = "measure" /\ r.resume /\ r.prior
    /\ r' = [r EXCEPT !.phase = "finished", !.reused = TRUE]
Measure ==
    /\ r.phase = "measure" /\ ~(r.resume /\ r.prior)
    /\ r' = [r EXCEPT !.phase = "finished"]
Fail ==
    /\ r.phase = "measure"
    /\ r' = [r EXCEPT !.phase = "failed"]
Done == r.phase \in {"failed", "finished"} /\ UNCHANGED r
Next == Check \/ Reuse \/ Measure \/ Fail \/ Done
ReuseRequiresMatchingInputs == r.reused => r.resume /\ r.metadata /\ r.match
=============================================================================
