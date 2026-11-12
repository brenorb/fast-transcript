-------------------------------- MODULE Remote --------------------------------

CONSTANT HonorForceLocal
VARIABLE run

Init ==
    \E forceLocal, diarize, captions \in BOOLEAN:
        run = [phase |-> "metadata", forceLocal |-> forceLocal, diarize |-> diarize,
               captions |-> captions, result |-> "none"]

CanUseCaptions == ~(HonorForceLocal /\ run.forceLocal) /\ ~run.diarize

MetadataFails ==
    /\ run.phase = "metadata"
    /\ run' = [run EXCEPT !.phase = "failed"]

MetadataLoaded ==
    /\ run.phase = "metadata"
    /\ run' = [run EXCEPT !.phase = "routing"]

FetchManualSubtitles ==
    /\ run.phase = "routing" /\ CanUseCaptions /\ run.captions
    /\ run' = [run EXCEPT !.phase = "subtitles"]

ChooseAudio ==
    /\ run.phase = "routing" /\ (~CanUseCaptions \/ ~run.captions)
    /\ run' = [run EXCEPT !.phase = "downloading audio"]

SubtitlesReady ==
    /\ run.phase = "subtitles"
    /\ run' = [run EXCEPT !.phase = "rendering", !.result = "subtitles"]

SubtitlesUnavailable ==
    /\ run.phase = "subtitles"
    /\ run' = [run EXCEPT !.phase = "downloading audio"]

AudioDownloaded ==
    /\ run.phase = "downloading audio"
    /\ run' = [run EXCEPT !.phase = "transcribing"]

TranscriptionReady ==
    /\ run.phase = "transcribing"
    /\ run' = [run EXCEPT !.phase = "rendering", !.result = "local transcript"]

AudioOrOutputFails ==
    /\ run.phase \in {"downloading audio", "transcribing", "rendering"}
    /\ run' = [run EXCEPT !.phase = "failed"]

OutputDelivered ==
    /\ run.phase = "rendering"
    /\ run' = [run EXCEPT !.phase = "done"]

Done ==
    /\ run.phase \in {"done", "failed"}
    /\ UNCHANGED run

Next == MetadataFails \/ MetadataLoaded \/ FetchManualSubtitles \/ ChooseAudio
        \/ SubtitlesReady \/ SubtitlesUnavailable \/ AudioDownloaded \/ TranscriptionReady
        \/ AudioOrOutputFails \/ OutputDelivered \/ Done

ExplicitLocalSkipsSubtitles == run.forceLocal =>
    run.phase # "subtitles" /\ run.result # "subtitles"

=============================================================================
