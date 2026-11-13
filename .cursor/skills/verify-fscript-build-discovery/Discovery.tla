----------------------------- MODULE Discovery -----------------------------
CONSTANT RequireArchive

Roots == {"env", "selected", "xcode", "tools"}
Candidates == {"runtime1", "runtime2"}
Paths == [r \in Roots |-> Candidates]
VARIABLES apple, roots, files, searched, reported
vars == <<apple, roots, files, searched, reported>>

Init ==
  /\ apple \in BOOLEAN
  /\ roots \in {{"xcode", "tools"}, {"env", "xcode", "tools"},
                {"selected", "xcode", "tools"}, Roots}
  /\ files \in SUBSET Candidates
  /\ searched = {}
  /\ reported = {}

Search(r) ==
  /\ apple /\ r \in roots \ searched
  /\ searched' = searched \cup {r}
  /\ reported' = reported \cup (IF RequireArchive THEN Paths[r] \cap files ELSE Paths[r])
  /\ UNCHANGED <<apple, roots, files>>

Next == (\E r \in Roots : Search(r)) \/ UNCHANGED vars
NativeOnly == ~apple => searched = {} /\ reported = {}
ArchiveRequired == reported \subseteq files
Complete == (apple /\ searched = roots) => reported = files
=============================================================================
