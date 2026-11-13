------------------------------- MODULE Wrapper -------------------------------
CONSTANT ForwardExit
VARIABLE w
Init ==
    \E exists, executable \in BOOLEAN :
        w = [phase |-> "locate", exists |-> exists, executable |-> executable,
             childCode |-> "none", result |-> "none"]
Missing ==
    /\ w.phase = "locate" /\ ~w.exists
    /\ w' = [w EXCEPT !.phase = "returned", !.result = "error"]
Found ==
    /\ w.phase = "locate" /\ w.exists
    /\ w' = [w EXCEPT !.phase = IF w.executable THEN "launch" ELSE "chmod"]
MakeExecutable ==
    /\ w.phase = "chmod"
    /\ w' = [w EXCEPT !.executable = TRUE, !.phase = "launch"]
LaunchError ==
    /\ w.phase \in {"chmod", "launch"}
    /\ w' = [w EXCEPT !.phase = "exception"]
Launch ==
    /\ w.phase = "launch"
    /\ w' = [w EXCEPT !.phase = "running"]
Exit ==
    /\ w.phase = "running"
    /\ \E code \in {"zero", "nonzero"} :
        w' = [w EXCEPT !.phase = "returned", !.childCode = code,
                       !.result = IF ForwardExit THEN code ELSE "zero"]
Done == w.phase \in {"returned", "exception"} /\ UNCHANGED w
Next == Missing \/ Found \/ MakeExecutable \/ LaunchError \/ Launch \/ Exit \/ Done
WrapperReturnsChildCode == w.childCode # "none" => w.result = w.childCode
=============================================================================
