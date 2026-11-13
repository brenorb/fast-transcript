----------------------------- MODULE QuietScope -------------------------------
EXTENDS Naturals, Sequences
CONSTANT Restore
VARIABLES saved, quiet
Init == saved = <<>> /\ quiet = FALSE
Enter ==
    /\ Len(saved) < 2
    /\ saved' = Append(saved, quiet)
    /\ quiet' \in BOOLEAN
Drop ==
    /\ Len(saved) > 0
    /\ saved' = SubSeq(saved, 1, Len(saved) - 1)
    /\ quiet' = IF Restore THEN saved[Len(saved)] ELSE quiet
Idle == UNCHANGED <<saved, quiet>>
Next == Enter \/ Drop \/ Idle
OutsideScopeIsVerbose == Len(saved) = 0 => ~quiet
=============================================================================
