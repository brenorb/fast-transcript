--------------------------------- MODULE Wheel ---------------------------------
CONSTANT CheckVersion
VARIABLE b
Init ==
    \E binary \in {"missing", "match", "mismatch", "invalid response"},
       expectedName \in BOOLEAN :
        b = [phase |-> "inspect", binary |-> binary, staged |-> FALSE,
             expectedName |-> expectedName]
Inspect ==
    /\ b.phase = "inspect"
    /\ b' = [b EXCEPT !.phase =
        CASE b.binary \in {"missing", "invalid response"} -> "failed"
          [] b.binary = "mismatch" /\ CheckVersion -> "failed"
          [] OTHER -> "stage"]
Stage ==
    /\ b.phase = "stage"
    /\ b' = [b EXCEPT !.phase = "build", !.staged = TRUE]
Build ==
    /\ b.phase = "build"
    /\ b' = [b EXCEPT !.phase = "finished"]
Fail ==
    /\ b.phase \in {"stage", "build"}
    /\ b' = [b EXCEPT !.phase = "failed"]
Done == b.phase \in {"finished", "failed"} /\ UNCHANGED b
Next == Inspect \/ Stage \/ Build \/ Fail \/ Done
WheelStagesMatchingBinary == b.staged => b.binary = "match"
BuiltWheelHasLauncher == b.phase = "finished" => b.expectedName
=============================================================================
