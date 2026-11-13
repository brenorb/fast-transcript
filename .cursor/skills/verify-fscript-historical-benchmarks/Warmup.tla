------------------------------- MODULE Warmup ---------------------------------
EXTENDS Naturals
CONSTANT ExcludeWarmup
VARIABLE w
Init ==
    \E protocol \in {"fresh", "resident"}, warmup \in BOOLEAN, reps \in 1..3 :
        /\ (protocol = "resident" => warmup)
        /\ w = [phase |-> "prepare", protocol |-> protocol, warmup |-> warmup,
                 reps |-> reps, measured |-> 0, countedWarmup |-> FALSE]
Prepare ==
    /\ w.phase = "prepare"
    /\ w' = [w EXCEPT !.phase = IF w.warmup THEN "warmup" ELSE "measure"]
Warm ==
    /\ w.phase = "warmup"
    /\ w' = [w EXCEPT !.phase = "measure",
                       !.measured = IF ExcludeWarmup THEN 0 ELSE 1,
                       !.countedWarmup = ~ExcludeWarmup]
Measure ==
    /\ w.phase = "measure" /\ w.measured < w.reps
    /\ w' = [w EXCEPT !.measured = @ + 1]
Emit ==
    /\ w.phase = "measure" /\ w.measured = w.reps
    /\ w' = [w EXCEPT !.phase = "done"]
Fail ==
    /\ w.phase \in {"prepare", "warmup", "measure"}
    /\ w' = [w EXCEPT !.phase = "failed"]
Done == w.phase \in {"done", "failed"} /\ UNCHANGED w
Next == Prepare \/ Warm \/ Measure \/ Emit \/ Fail \/ Done
WarmupExcluded == ~w.countedWarmup
RequestedRepetitions == w.phase = "done" => w.measured = w.reps
=============================================================================
