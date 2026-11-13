------------------------------ MODULE WorkerRun -------------------------------
CONSTANT CheckSequence
VARIABLE w
Init ==
    w = [phase |-> "starting", cold |-> FALSE, warm |-> FALSE,
         inOrder |-> TRUE, healthy |-> FALSE]
Start == w.phase = "starting" /\ w' = [w EXCEPT !.phase = "collect"]
Cold ==
    /\ w.phase = "collect" /\ ~w.cold
    /\ w' = [w EXCEPT !.cold = TRUE, !.inOrder = @ /\ ~w.warm]
Warm ==
    /\ w.phase = "collect" /\ ~w.warm
    /\ w' = [w EXCEPT !.warm = TRUE, !.inOrder = @ /\ w.cold]
Unexpected ==
    /\ w.phase = "collect"
    /\ w' = [w EXCEPT !.inOrder = FALSE]
Exit ==
    /\ w.phase = "collect"
    /\ \E success \in BOOLEAN :
        w' = [w EXCEPT !.healthy = success,
           !.phase = IF success /\ (~CheckSequence \/ (w.cold /\ w.warm /\ w.inOrder))
                     THEN "accepted" ELSE "failed"]
Abort ==
    /\ w.phase = "collect"
    /\ w' = [w EXCEPT !.phase = "failed"]
Done == w.phase \in {"accepted", "failed"} /\ UNCHANGED w
Next == Start \/ Cold \/ Warm \/ Unexpected \/ Exit \/ Abort \/ Done
CompleteRun == w.phase = "accepted" => w.healthy /\ w.cold /\ w.warm /\ w.inOrder
=============================================================================
