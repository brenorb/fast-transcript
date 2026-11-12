------------------------------- MODULE Speakers -------------------------------
CONSTANT FallbackOnNoSpeech
VARIABLE d
Init ==
    \E requested \in BOOLEAN :
        d = [phase |-> "transcribed", requested |-> requested, noSpeech |-> FALSE]
NoRequest ==
    /\ d.phase = "transcribed" /\ ~d.requested
    /\ d' = [d EXCEPT !.phase = "unlabeled"]
StartHelper ==
    /\ d.phase = "transcribed" /\ d.requested
    /\ d' = [d EXCEPT !.phase = "waiting"]
NoSpeech ==
    /\ d.phase = "waiting"
    /\ d' = [d EXCEPT !.noSpeech = TRUE,
                      !.phase = IF FallbackOnNoSpeech THEN "unlabeled" ELSE "failed"]
HelperFails ==
    /\ d.phase = "waiting"
    /\ d' = [d EXCEPT !.phase = "failed"]
HelperSucceeds ==
    /\ d.phase = "waiting"
    /\ d' = [d EXCEPT !.phase = "merging"]
ApplySpeakers ==
    /\ d.phase = "merging"
    /\ d' = [d EXCEPT !.phase = "merged"]
Done == d.phase \in {"unlabeled", "failed", "merged"} /\ UNCHANGED d
Next == NoRequest \/ StartHelper \/ NoSpeech \/ HelperFails \/ HelperSucceeds \/ ApplySpeakers \/ Done
NoSpeechKeepsTranscript == d.noSpeech => d.phase = "unlabeled"
=============================================================================
