--------------------------------- MODULE Cache --------------------------------
CONSTANT UseStaging
VARIABLE c
Init ==
    \E complete, cached \in BOOLEAN :
        c = [phase |-> "inspect", modelComplete |-> complete,
             packageCached |-> cached, defaultChanged |-> FALSE]
ReuseModel ==
    /\ c.phase = "inspect" /\ c.modelComplete
    /\ c' = [c EXCEPT !.phase = "available"]
ReuseBundle ==
    /\ c.phase = "inspect" /\ ~c.modelComplete /\ c.packageCached
    /\ c' = [c EXCEPT !.phase = "extract"]
FetchBundle ==
    /\ c.phase = "inspect" /\ ~c.modelComplete /\ ~c.packageCached
    /\ c' = [c EXCEPT !.phase = "download"]
CommitBundle ==
    /\ c.phase = "download"
    /\ c' = [c EXCEPT !.phase = "extract", !.packageCached = TRUE]
Unpack ==
    /\ c.phase = "extract"
    /\ c' = [c EXCEPT !.phase = "install", !.defaultChanged = ~UseStaging]
IncompleteArchive ==
    /\ c.phase = "extract"
    /\ c' = [c EXCEPT !.phase = "validate"]
Install ==
    /\ c.phase = "install"
    /\ c' = [c EXCEPT !.phase = "validate", !.modelComplete = TRUE]
CheckComplete ==
    /\ c.phase = "validate"
    /\ c' = [c EXCEPT !.phase = IF c.modelComplete THEN "available" ELSE "failed"]
Fail ==
    /\ c.phase \in {"download", "extract", "install"}
    /\ c' = [c EXCEPT !.phase = "failed"]
Done == c.phase \in {"failed", "available"} /\ UNCHANGED c
Next == ReuseModel \/ ReuseBundle \/ FetchBundle \/ CommitBundle \/ Unpack \/ IncompleteArchive \/
        Install \/ CheckComplete \/ Fail \/ Done
DefaultDirectoryPreserved == ~c.defaultChanged
=============================================================================
