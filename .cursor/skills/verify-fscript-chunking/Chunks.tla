------------------------------- MODULE Chunks -------------------------------
EXTENDS Naturals
CONSTANT AdvanceExtra
VARIABLE c
Init ==
    \E total \in 0..6, size, overlap \in 0..3, chunked \in BOOLEAN :
        c = [total |-> total, size |-> size, overlap |-> overlap, chunked |-> chunked,
             start |-> 0, covered |-> {}, planned |-> 0, completed |-> 0, phase |-> "plan"]
AllSamples == {i \in 0..6 : i < c.total}
Valid == c.size > 0 /\ c.overlap < c.size
Whole ==
    /\ c.phase = "plan" /\ ~c.chunked
    /\ c' = [c EXCEPT !.covered = AllSamples, !.planned = 1, !.phase = "infer"]
Invalid ==
    /\ c.phase = "plan" /\ c.chunked /\ ~Valid
    /\ c' = [c EXCEPT !.phase = "invalid"]
Chunk ==
    /\ c.phase = "plan" /\ c.chunked /\ Valid /\ c.start < c.total
    /\ LET end == IF c.start + c.size < c.total THEN c.start + c.size ELSE c.total
       IN c' = [c EXCEPT !.start = IF end = c.total THEN c.total ELSE @ + c.size - c.overlap + AdvanceExtra,
                         !.covered = @ \cup {i \in AllSamples : c.start <= i /\ i < end},
                         !.planned = @ + 1]
PlanReady ==
    /\ c.phase = "plan" /\ c.chunked /\ Valid /\ c.start >= c.total
    /\ c' = [c EXCEPT !.phase = "infer"]
MergeChunk ==
    /\ c.phase = "infer" /\ c.completed < c.planned
    /\ c' = [c EXCEPT !.completed = @ + 1]
EngineFails ==
    /\ c.phase = "infer" /\ c.completed < c.planned
    /\ c' = [c EXCEPT !.phase = "inference failed"]
Return ==
    /\ c.phase = "infer" /\ c.completed = c.planned
    /\ c' = [c EXCEPT !.phase = "returned"]
Done == c.phase \in {"invalid", "inference failed", "returned"} /\ UNCHANGED c
Next == Whole \/ Invalid \/ Chunk \/ PlanReady \/ MergeChunk \/ EngineFails \/ Return \/ Done
InferencePlanCoversInput == c.phase \in {"infer", "inference failed", "returned"} => c.covered = AllSamples
=============================================================================
