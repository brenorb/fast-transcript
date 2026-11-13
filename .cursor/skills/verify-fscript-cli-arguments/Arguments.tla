----------------------------- MODULE Arguments -----------------------------
EXTENDS FiniteSets, Sequences, Naturals
CONSTANT RejectConflicts
Inputs == {<<"media">>, <<"media", "json">>, <<"media", "json", "srt">>,
           <<"media", "json", "json">>, <<"media", "stdout", "path">>,
           <<"boundary", "json">>, <<"media", "boundary", "json">>}
VARIABLES tokens, position, parsing, formats, destination, result
vars == <<tokens, position, parsing, formats, destination, result>>

Init ==
  /\ tokens \in Inputs /\ position = 1 /\ parsing = TRUE
  /\ formats = {} /\ destination = {} /\ result = "pending"
Scan ==
  /\ position <= Len(tokens)
  /\ LET token == tokens[position] IN
      /\ parsing' = parsing /\ token /= "boundary"
      /\ formats' = IF parsing /\ token \in {"json", "srt"} THEN formats \cup {token} ELSE formats
      /\ destination' = IF token = "path" \/ (parsing /\ token = "stdout")
                          THEN destination \cup {token} ELSE destination
  /\ position' = position + 1
  /\ UNCHANGED <<tokens, result>>
Decide ==
  /\ position > Len(tokens) /\ result = "pending"
  /\ result' = IF RejectConflicts /\ (Cardinality(formats) > 1 \/ Cardinality(destination) > 1)
                THEN "rejected" ELSE "accepted"
  /\ UNCHANGED <<tokens, position, parsing, formats, destination>>
Next == Scan \/ Decide \/ UNCHANGED vars
AcceptedUnambiguous == result = "accepted" => Cardinality(formats) <= 1 /\ Cardinality(destination) <= 1
LiteralSuffix ==
  \A format \in formats :
    \E i \in 1..(position - 1) :
      tokens[i] = format /\ (\A j \in 1..(i - 1) : tokens[j] /= "boundary")
=============================================================================
