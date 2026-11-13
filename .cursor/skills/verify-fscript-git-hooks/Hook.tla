-------------------------------- MODULE Hook ----------------------------------
CONSTANT EnforceFormat
VARIABLE h
Init ==
    \E rust, fmt \in BOOLEAN :
        h = [phase |-> "install", rust |-> rust, fmt |-> fmt, invoked |-> FALSE]
Install == h.phase = "install" /\ h' = [h EXCEPT !.phase = "inspect"]
Inspect ==
    /\ h.phase = "inspect"
    /\ h' = [h EXCEPT !.phase = IF h.rust THEN "format" ELSE "allowed"]
Format ==
    /\ h.phase = "format"
    /\ h' = [h EXCEPT !.invoked = TRUE,
                      !.phase = IF h.fmt \/ ~EnforceFormat THEN "allowed" ELSE "blocked"]
Done == h.phase \in {"allowed", "blocked"} /\ UNCHANGED h
Next == Install \/ Inspect \/ Format \/ Done
AllowedRustIsFormatted == h.phase = "allowed" /\ h.rust => h.invoked /\ h.fmt
NoFormatForNonRust == ~h.rust => ~h.invoked
=============================================================================
