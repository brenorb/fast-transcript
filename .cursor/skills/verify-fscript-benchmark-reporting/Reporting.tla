------------------------------ MODULE Reporting ------------------------------
CONSTANT GuardInputs
VARIABLE r
Init ==
    \E mode \in {"rescore", "validate", "report"}, valid, matches \in BOOLEAN :
        r = [phase |-> "read", mode |-> mode, valid |-> valid, matches |-> matches,
             outputs |-> {}, originalChanged |-> FALSE]
Inspect ==
    /\ r.phase = "read"
    /\ r' = [r EXCEPT !.phase =
        IF (r.valid \/ ~GuardInputs) /\ r.matches THEN "produce" ELSE "failed"]
FirstOutput ==
    /\ r.phase = "produce"
    /\ r' = [r EXCEPT !.phase = IF r.mode = "report" THEN "done" ELSE "next",
        !.outputs = {IF r.mode = "rescore" THEN "scores" ELSE IF r.mode = "report" THEN "report" ELSE "verified"}]
SecondOutput ==
    /\ r.phase = "next"
    /\ r' = [r EXCEPT !.phase = IF r.mode = "rescore" THEN "done" ELSE "validation",
        !.outputs = @ \cup {IF r.mode = "rescore" THEN "csv" ELSE "alignments"}]
ValidationOutput ==
    /\ r.phase = "validation"
    /\ r' = [r EXCEPT !.phase = "done", !.outputs = @ \cup {"validation"}]
WriteError ==
    /\ r.phase \in {"produce", "next", "validation"}
    /\ r' = [r EXCEPT !.phase = "failed"]
Done == r.phase \in {"done", "failed"} /\ UNCHANGED r
Next == Inspect \/ FirstOutput \/ SecondOutput \/ ValidationOutput \/ WriteError \/ Done
OutputRequiresValidInput == r.outputs # {} => r.valid /\ r.matches
OriginalRunsUnchanged == ~r.originalChanged
=============================================================================
