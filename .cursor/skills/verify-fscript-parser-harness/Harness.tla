------------------------------ MODULE Harness ------------------------------
EXTENDS Naturals
CONSTANT EnforceBounds

VARIABLES delimiter, hasNul, chunks, args, widest, phase
vars == <<delimiter, hasNul, chunks, args, widest, phase>>

Init ==
  /\ hasNul \in BOOLEAN
  /\ chunks \in {0, 1, 16, 17}
  /\ delimiter = "unset"
  /\ args = 0 /\ widest = 0 /\ phase = "start"

Split ==
  /\ phase = "start"
  /\ delimiter' = IF hasNul THEN "nul" ELSE "newline"
  /\ phase' = "split"
  /\ UNCHANGED <<hasNul, chunks, args, widest>>

Convert ==
  /\ phase = "split"
  /\ args' \in 0..(IF EnforceBounds /\ chunks > 16 THEN 16 ELSE chunks)
  /\ widest' \in IF EnforceBounds THEN {0, 1, 64} ELSE {0, 1, 64, 65}
  /\ phase' = "converted"
  /\ UNCHANGED <<delimiter, hasNul, chunks>>

Call ==
  /\ phase = "converted"
  /\ args' = IF args = 0 THEN 1 ELSE args
  /\ widest' = IF args = 0 THEN 9 ELSE widest
  /\ phase' = "called"
  /\ UNCHANGED <<delimiter, hasNul, chunks>>

Next == Split \/ Convert \/ Call \/ UNCHANGED vars
DelimiterChoice == phase # "start" => (delimiter = "nul") = hasNul
NonemptyInput == phase = "called" => args >= 1
Bounds == args <= 16 /\ widest <= 64
=============================================================================
