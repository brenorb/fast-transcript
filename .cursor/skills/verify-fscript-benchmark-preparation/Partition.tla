------------------------------ MODULE Partition ------------------------------
EXTENDS Naturals, Sequences
CONSTANT KeepBoundary
VARIABLES total, cursor, parts, done
Init == total \in 1..8 /\ cursor = 0 /\ parts = <<>> /\ done = FALSE
AppendPart ==
    /\ cursor < total /\ ~done
    /\ \E end \in (cursor + 1)..total :
        /\ IF total - cursor > 4 THEN end - cursor \in 2..3 ELSE end = total
        /\ parts' = Append(parts, [first |-> IF KeepBoundary \/ cursor = 0 THEN cursor ELSE cursor + 1,
                                  last |-> end])
        /\ cursor' = end
    /\ UNCHANGED <<total, done>>
Finish == ~done /\ cursor = total /\ done' = TRUE /\ UNCHANGED <<total, cursor, parts>>
Done == done /\ UNCHANGED <<total, cursor, parts, done>>
Next == AppendPart \/ Finish \/ Done
NoGapOrOverlap ==
    /\ Len(parts) > 0 => parts[1].first = 0 /\ parts[Len(parts)].last = cursor
    /\ \A i \in 2..Len(parts) : parts[i - 1].last = parts[i].first
    /\ \A i \in 1..Len(parts) : parts[i].last - parts[i].first \in 1..4
FinishedCoversInput == done => cursor = total /\ Len(parts) > 0
=============================================================================
