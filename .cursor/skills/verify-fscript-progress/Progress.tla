------------------------------ MODULE Progress -------------------------------
EXTENDS Naturals
CONSTANT SuppressAnimation
VARIABLE p
Init ==
    \E terminal, quiet, dumb \in BOOLEAN :
        p = [phase |-> "choose", terminal |-> terminal, quiet |-> quiet,
             dumb |-> dumb, worker |-> FALSE, chunk |-> 1]
Start ==
    /\ p.phase = "choose"
    /\ p' = [p EXCEPT !.phase = "running",
          !.worker = (p.terminal /\ (~SuppressAnimation \/ (~p.quiet /\ ~p.dumb)))]
Advance ==
    /\ p.phase = "running"
    /\ \E index \in 0..4 :
        p' = [p EXCEPT !.chunk = IF index < 1 THEN 1 ELSE IF index > 3 THEN 3 ELSE index]
Finish ==
    /\ p.phase = "running"
    /\ p' = [p EXCEPT !.phase = "done", !.worker = FALSE, !.chunk = 3]
Drop ==
    /\ p.phase = "running"
    /\ p' = [p EXCEPT !.phase = "dropped", !.worker = FALSE]
Done == p.phase \in {"done", "dropped"} /\ UNCHANGED p
Next == Start \/ Advance \/ Finish \/ Drop \/ Done
WorkerRequiresInteractiveOutput == p.worker => p.terminal /\ ~p.quiet /\ ~p.dumb
WorkerJoinedAtExit == p.phase \in {"done", "dropped"} => ~p.worker
ChunkBounded == p.chunk \in 1..3
=============================================================================
