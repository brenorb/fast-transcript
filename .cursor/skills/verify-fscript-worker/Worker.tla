-------------------------------- MODULE Worker --------------------------------
EXTENDS Naturals
CONSTANTS RequireReady, MaxRequests
VARIABLE w
Init == w = [phase |-> "spawn", ready |-> FALSE, sent |-> 0,
             received |-> 0, wav |-> FALSE, outcome |-> "none"]
Spawn ==
    /\ w.phase = "spawn"
    /\ w' = [w EXCEPT !.phase = "startup"]
SpawnFails ==
    /\ w.phase = "spawn"
    /\ w' = [w EXCEPT !.phase = "finished", !.outcome = "failure"]
StartupReply ==
    /\ w.phase = "startup"
    /\ \E ready \in BOOLEAN :
        w' = [w EXCEPT !.ready = ready, !.phase = IF ready \/ ~RequireReady THEN "idle" ELSE "closing",
                       !.outcome = IF ready \/ ~RequireReady THEN "none" ELSE "failure"]
Send ==
    /\ w.phase = "idle" /\ w.sent < MaxRequests
    /\ w' = [w EXCEPT !.phase = "waiting", !.sent = @ + 1, !.wav = TRUE]
Receive ==
    /\ w.phase = "waiting"
    /\ w' = [w EXCEPT !.phase = "idle", !.received = @ + 1, !.wav = FALSE]
RuntimeError ==
    /\ w.phase \in {"startup", "idle", "waiting"}
    /\ w' = [w EXCEPT !.phase = "closing", !.outcome = "failure", !.wav = FALSE]
Finish ==
    /\ w.phase = "idle" /\ w.received = MaxRequests
    /\ w' = [w EXCEPT !.phase = "closing", !.outcome = "success"]
CloseAndWait ==
    /\ w.phase = "closing"
    /\ w' = [w EXCEPT !.phase = "finished"]
Done == w.phase = "finished" /\ UNCHANGED w
Next == Spawn \/ SpawnFails \/ StartupReply \/ Send \/ Receive \/ RuntimeError \/
        Finish \/ CloseAndWait \/ Done
SendRequiresValidStartup == w.sent > 0 => w.ready
=============================================================================
