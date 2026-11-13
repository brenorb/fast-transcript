----------------------------- MODULE Measurement -----------------------------
CONSTANT CheckBackend
VARIABLE m
Init ==
    \E phonon, correctBackend, complete, processOk \in BOOLEAN :
        m = [phase |-> "invoke", phonon |-> phonon, correctBackend |-> correctBackend,
             complete |-> complete, processOk |-> processOk]
Run ==
    /\ m.phase = "invoke"
    /\ m' = [m EXCEPT !.phase = IF m.processOk THEN "inspect" ELSE "failed"]
Inspect ==
    /\ m.phase = "inspect"
    /\ m' = [m EXCEPT !.phase =
        IF ~m.phonon \/ ~CheckBackend \/ (m.correctBackend /\ m.complete) THEN "score" ELSE "failed"]
Save == m.phase = "score" /\ m' = [m EXCEPT !.phase = "saved"]
Done == m.phase \in {"saved", "failed"} /\ UNCHANGED m
Next == Run \/ Inspect \/ Save \/ Done
OnlyMatchingCompletePhononSaved == m.phase = "saved" /\ m.phonon => m.correctBackend /\ m.complete
=============================================================================
