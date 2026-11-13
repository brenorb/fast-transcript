------------------------------- MODULE Turns -------------------------------
EXTENDS Naturals, Sequences

CONSTANT RequireActiveTurn
Speakers == {"unknown", "A", "B"}
Inputs == {<<>>, <<"unknown">>, <<"A">>, <<"A", "A", "B">>,
           <<"A", "unknown">>, <<"unknown", "unknown", "A">>}
VARIABLES input, index, turn, at, pending, output, finished
vars == <<input, index, turn, at, pending, output, finished>>

Init ==
  /\ input \in Inputs
  /\ index = 1 /\ turn = "unknown" /\ at = 0 /\ pending = <<>>
  /\ output = <<>> /\ finished = FALSE

Flush == IF Len(pending) > 0 THEN Append(output, [at |-> at, first |-> pending[1]]) ELSE output
Accept ==
  /\ index <= Len(input)
  /\ LET speaker == input[index]
         merge == speaker = turn /\ (~RequireActiveTurn \/ Len(pending) > 0)
     IN /\ output' = IF merge THEN output ELSE Flush
        /\ at' = IF merge THEN at ELSE index
        /\ pending' = IF merge THEN Append(pending, index) ELSE <<index>>
        /\ turn' = speaker
  /\ index' = index + 1
  /\ UNCHANGED <<input, finished>>
Finish ==
  /\ index > Len(input) /\ ~finished
  /\ output' = Flush /\ finished' = TRUE /\ pending' = <<>>
  /\ UNCHANGED <<input, index, turn, at>>
Next == Accept \/ Finish \/ UNCHANGED vars
TimestampFromFirstSegment == \A i \in 1..Len(output) : output[i].at = output[i].first
=============================================================================
