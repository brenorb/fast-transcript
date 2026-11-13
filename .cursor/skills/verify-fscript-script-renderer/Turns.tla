-------------------------------- MODULE Turns --------------------------------
EXTENDS Naturals, Sequences
CONSTANTS Merge, FinalFlush
VARIABLES rest, active, lines, done
Speakers == {"A", "B", "unknown"}
Inputs == Speakers \cup {"skip"}
Init ==
    /\ rest \in UNION {[1..n -> Inputs] : n \in 0..3}
    /\ active = "none"
    /\ lines = <<>>
    /\ done = FALSE
Emit == IF active = "none" THEN lines ELSE Append(lines, active)
Skip ==
    /\ ~done /\ Len(rest) > 0 /\ Head(rest) = "skip"
    /\ rest' = Tail(rest)
    /\ UNCHANGED <<active, lines, done>>
Read ==
    /\ ~done /\ Len(rest) > 0 /\ Head(rest) \in Speakers
    /\ rest' = Tail(rest)
    /\ active' = Head(rest)
    /\ lines' = IF Merge /\ Head(rest) = active THEN lines ELSE Emit
    /\ UNCHANGED done
Finish ==
    /\ ~done /\ rest = <<>>
    /\ lines' = IF FinalFlush THEN Emit ELSE lines
    /\ done' = TRUE
    /\ UNCHANGED <<rest, active>>
Done == done /\ UNCHANGED <<rest, active, lines, done>>
Next == Skip \/ Read \/ Finish \/ Done
FinalTurnEmitted == (done /\ active # "none") =>
    IF Len(lines) = 0 THEN FALSE ELSE lines[Len(lines)] = active
MergedAdjacentTurns == Merge =>
    \A index \in 1..(Len(lines) - 1) : lines[index] # lines[index + 1]
=============================================================================
