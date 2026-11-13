--------------------------- MODULE ValidationCoverage ---------------------------
EXTENDS Naturals
CONSTANT RequireCases
VARIABLES cases, validProtocol, phase
Init == cases \in 0..1 /\ validProtocol \in BOOLEAN /\ phase = "read"
Validate ==
    /\ phase = "read"
    /\ phase' = IF validProtocol /\ (cases > 0 \/ ~RequireCases) THEN "checked" ELSE "failed"
    /\ UNCHANGED <<cases, validProtocol>>
Write ==
    /\ phase = "checked"
    /\ phase' = "written"
    /\ UNCHANGED <<cases, validProtocol>>
Done == phase \in {"failed", "written"} /\ UNCHANGED <<cases, validProtocol, phase>>
Next == Validate \/ Write \/ Done
ValidationHasCases == phase = "written" => cases > 0
=============================================================================
