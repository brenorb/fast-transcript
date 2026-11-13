------------------------------ MODULE Release ------------------------------
EXTENDS Naturals

CONSTANTS RequireBuild, PublishOnlyMac, SkipExisting
OS == {"mac", "linux"}
Trigger == {"manual", "tag"}
Registry == {"same", "different", "unavailable"}

VARIABLES trigger, stage, github, pypi, crates, registry
vars == <<trigger, stage, github, pypi, crates, registry>>

Init ==
  /\ trigger \in Trigger
  /\ stage = [os \in OS |-> 0]
  /\ github = FALSE
  /\ pypi = {}
  /\ crates = "pending"
  /\ registry \in Registry

Advance(os, result) ==
  /\ stage[os] \in 0..5
  /\ result \in {stage[os] + 1, 7}
  /\ stage' = [stage EXCEPT ![os] = result]
  /\ UNCHANGED <<trigger, github, pypi, crates, registry>>

Built == \A os \in OS : stage[os] = 6
Gate == trigger = "tag" /\ (~RequireBuild \/ Built)
PublishGitHub ==
  /\ Gate /\ ~github
  /\ github' = TRUE
  /\ UNCHANGED <<trigger, stage, pypi, crates, registry>>
PublishPyPI ==
  /\ Gate /\ pypi = {}
  /\ pypi' = IF PublishOnlyMac THEN {"mac"} ELSE OS
  /\ UNCHANGED <<trigger, stage, github, crates, registry>>
DecideCrates ==
  /\ Gate /\ crates = "pending"
  /\ crates' = IF registry = "same" /\ SkipExisting THEN "skip" ELSE "publish"
  /\ UNCHANGED <<trigger, stage, github, pypi, registry>>

Next ==
  \/ \E os \in OS, result \in 1..7 : Advance(os, result)
  \/ PublishGitHub \/ PublishPyPI \/ DecideCrates
  \/ UNCHANGED vars

BuildGate ==
  (github \/ pypi /= {} \/ crates /= "pending") => (trigger = "tag" /\ Built)
MacWheelOnly == pypi \subseteq {"mac"}
KnownVersionSkipped == registry = "same" => crates /= "publish"
=============================================================================
