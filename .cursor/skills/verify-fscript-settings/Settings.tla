------------------------------- MODULE Settings -------------------------------
CONSTANT PreferEnvironment
VARIABLE c
Init ==
    \E disk \in {"missing", "saved A", "saved B", "malformed"},
       environment, explicit \in BOOLEAN :
        c = [disk |-> disk, environment |-> environment, explicit |-> explicit,
             phase |-> "idle", staged |-> "none", result |-> "none"]
Reset ==
    /\ c.phase = "idle"
    /\ c' = [c EXCEPT !.disk = "missing", !.result = "reset"]
Get ==
    /\ c.phase = "idle"
    /\ c' = [c EXCEPT !.result = CASE c.disk = "missing" -> "built-in"
                                 [] c.disk = "malformed" -> "error"
                                 [] OTHER -> c.disk]
RejectSet ==
    /\ c.phase = "idle"
    /\ c' = [c EXCEPT !.result = "error"]
Stage ==
    /\ c.phase = "idle"
    /\ \E selected \in {"saved A", "saved B"} :
        c' = [c EXCEPT !.phase = "staging", !.staged = selected, !.result = "none"]
Commit ==
    /\ c.phase = "staging"
    /\ c' = [c EXCEPT !.phase = "idle", !.disk = c.staged, !.staged = "none", !.result = "saved"]
Abort ==
    /\ c.phase = "staging"
    /\ c' = [c EXCEPT !.phase = "idle", !.staged = "none", !.result = "error"]
Select ==
    /\ c.phase = "idle"
    /\ c' = [c EXCEPT !.phase = "selected", !.result =
        CASE c.explicit -> "explicit"
          [] (c.environment /\ (PreferEnvironment \/ c.disk = "missing")) -> "environment"
          [] c.disk = "malformed" -> "error"
          [] c.disk # "missing" -> c.disk
          [] OTHER -> "built-in"]
Done == c.phase = "selected" /\ UNCHANGED c
Next == Reset \/ Get \/ RejectSet \/ Stage \/ Commit \/ Abort \/ Select \/ Done
EnvironmentBeatsSaved ==
    (c.phase = "selected" /\ c.environment /\ ~c.explicit) => c.result = "environment"
=============================================================================
