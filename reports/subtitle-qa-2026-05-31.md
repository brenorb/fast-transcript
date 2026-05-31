# Subtitle QA Report

Date: `2026-05-31`

Goal of this report:
- inspect the current subtitle quality before changing heuristics again
- separate "renderer bug fixed" from "ASR/coverage still weak"
- provide concrete timestamps for manual review

Files reviewed:
- `The Taste Of Things` reference:
  - `/Users/breno/Movies/The Taste Of Things (2023) [720p] [WEBRip] [YTS.MX]/Subs/French SDH.fre.HI.srt`
- `The Taste Of Things` generated:
  - `/Users/breno/Movies/The Taste Of Things (2023) [720p] [WEBRip] [YTS.MX]/The.Taste.Of.Things.2023.720p.WEBRip.x264.AAC-[YTS.MX].srt`
- `Noi Vivi` Part 1 generated:
  - `/Users/breno/Movies/Noi Vivi  We The Living (1942)/Noi Vivi  We The Living - Part 1.srt`
- `Noi Vivi` Part 2 generated:
  - `/Users/breno/Movies/Noi Vivi  We The Living (1942)/Noi Vivi  We The Living - Part 2.srt`

## Summary

What is clearly fixed:
- oversized cues are gone in all generated files
- `chars_max` is now `84` in all generated files
- there are `0` cues above `84` chars and `0` above `120` chars
- the old `Noi Vivi` Part 1 stuck cue bug is fixed:
  - before: cue 2 stayed on screen until `00:03:48.390`
  - now: cue 2 ends at `00:02:32.690`

What still looks weak:
- `Taste of Things` remains too sparse compared with the local subtitle reference
- generated cue count is much lower than the reference
- there are still very large inter-cue gaps, which likely mean missed speech coverage or severe ASR drift
- text quality on `Taste of Things` is still visibly rough in many places

Interpretation:
- the subtitle renderer itself is now much safer
- the remaining issue is no longer "giant cue stuck on screen"
- the remaining issue is "coverage, timing density, and ASR quality"

## Taste of Things

### Metrics

Reference (`French SDH.fre.HI.srt`):
- cue count: `1113`
- mean duration: `2.50s`
- p95 duration: `4.33s`
- max duration: `5.83s`
- mean chars: `33.79`
- p95 chars: `65`
- max chars: `79`

Generated (`The.Taste.Of.Things...srt`):
- cue count: `606`
- mean duration: `3.39s`
- p95 duration: `6.00s`
- max duration: `7.95s`
- mean chars: `39.04`
- p95 chars: `80`
- max chars: `84`
- cues over `72` chars: `101`
- cues over `84` chars: `0`

### Main concerns

The renderer is no longer pathological, but the subtitle track is still too sparse:
- reference cue count: `1113`
- generated cue count: `606`

This means the generated file is missing a lot of subtitle events that the reference track considers worth showing.

The biggest red flags are long gaps:
- cue `375 -> 376`: `01:01:09.080` to `01:12:33.590`, gap `684.51s`
- cue `560 -> 561`: `01:54:00.840` to `02:00:25.350`, gap `384.51s`
- cue `388 -> 389`: `01:16:56.910` to `01:23:04.150`, gap `367.24s`
- cue `4 -> 5`: `00:07:14.520` to `00:11:43.910`, gap `269.39s`
- cue `498 -> 499`: `01:35:53.320` to `01:40:18.000`, gap `264.68s`
- cue `561 -> 562`: `02:00:29.560` to `02:04:03.670`, gap `214.11s`

Those are much more likely to be missing coverage than legitimate silence.

### Longest generated cues

These are no longer absurd, but they are still good manual checkpoints:

1. cue `331`
   - `00:53:34.070 --> 00:53:37.888`
   - `84 chars`
   - `Nous vous serions reconnaissant si vous pouviez nous faire savoir ce qu'il conservir`
2. cue `342`
   - `00:55:24.466 --> 00:55:30.466`
   - `83 chars`
   - `pointe de pilotes est un excellent morceau bonne pour des rations de gras et de mer`
3. cue `307`
   - `00:52:09.510 --> 00:52:14.108`
   - `83 chars`
   - `Il n'y a pas que les ouvrages d'un monde qui exige tant d'attention et d'analyse se`
4. cue `74`
   - `00:30:34.883 --> 00:30:40.883`
   - `83 chars`
   - `avec vous atravers que vous mangez quoi dire de plus d'autre part il n'y a rien que`
5. cue `75`
   - `00:30:48.917 --> 00:30:54.917`
   - `83 chars`
   - `vous mangez à table que je ne mange pas le turbo par exemple bien avant vous mangez`

### Early-track manual checkpoints

Generated opening cues:

1.
`00:02:38.310 --> 00:02:39.570`
`Encore une fois.`

2.
`00:04:10.310 --> 00:04:12.100`
`C'est pour l'initiative.`

3.
`00:04:12.150 --> 00:04:13.880`
`Je dois la garder aujourd'hui.`

4.
`00:07:08.790 --> 00:07:14.520`
`The light complex.`

5.
`00:11:43.910 --> 00:11:49.480`
`So I'm just getting them.`

Reference track in the same early stretch has more subtitle activity much earlier, including ambience and dialogue around `00:02:25`. So this is a good place to inspect manually in the player.

### Suggested manual review windows for Taste of Things

- `00:02:20 - 00:04:20`
  - checks whether the generated track starts too late or skips early dialogue
- `00:07:00 - 00:12:00`
  - checks the large gap after cue 4
- `00:30:30 - 00:31:00`
  - checks whether long but bounded cues are still too dense to read
- `00:52:00 - 00:55:40`
  - checks the longest generated text blocks
- `01:01:00 - 01:12:40`
  - checks whether the `684.51s` gap is obviously wrong

## Noi Vivi Part 1

### Metrics

- cue count: `1193`
- mean duration: `2.73s`
- p95 duration: `5.87s`
- max duration: `7.95s`
- mean chars: `30.56`
- p95 chars: `73`
- max chars: `84`
- cues over `84` chars: `0`
- cues over `120` chars: `0`

### Known bug check

The old bad stretch now looks like this:

1.
`00:02:26.950 --> 00:02:30.820`
`Spero che mia sorella Marusia sia contenta di rivederci.`

2.
`00:02:30.870 --> 00:02:32.690`
`Chissà come saranno ridotto.`

3.
`00:03:48.390 --> 00:03:53.620`
`Lo sai che non devi allontanarti, di no, così sola, in mezzo a certa gente.`

This means there is now a clean silence gap from `00:02:32.690` until `00:03:48.390` instead of one subtitle being held across the whole interval.

### Main concerns

There are still long gaps, but they are much less extreme than the old renderer bug:
- cue `363 -> 364`: `00:27:29.080` to `00:29:48.390`, gap `139.31s`
- cue `376 -> 377`: `00:30:40.010` to `00:32:37.990`, gap `117.98s`
- cue `1074 -> 1075`: `01:21:27.270` to `01:22:55.430`, gap `88.16s`

The gap after cue `2`:
- cue `2 -> 3`: `00:02:32.690` to `00:03:48.390`, gap `75.70s`

That specific gap is expected to be a manual validation checkpoint because it is exactly where the old "stuck subtitle" problem showed up.

### Suggested manual review windows for Noi Vivi Part 1

- `00:02:25 - 00:03:55`
  - verifies the original stuck-cue regression is gone
- `00:27:20 - 00:29:55`
  - checks the largest gap in the file
- `00:30:35 - 00:32:45`
  - checks the second-largest gap

## Noi Vivi Part 2

### Metrics

- cue count: `1230`
- mean duration: `2.82s`
- p95 duration: `5.71s`
- max duration: `7.95s`
- mean chars: `30.57`
- p95 chars: `74`
- max chars: `84`
- cues over `84` chars: `0`
- cues over `120` chars: `0`

### Main concerns

Part 2 looks structurally similar to Part 1:
- cue `793 -> 794`: `00:53:19.390` to `00:54:37.910`, gap `78.52s`
- cue `117 -> 118`: `00:10:35.320` to `00:11:51.590`, gap `76.27s`
- cue `92 -> 93`: `00:07:23.160` to `00:08:31.830`, gap `68.67s`

The longest cues are still bounded:
- max chars: `84`
- no cue above `84` chars

### Opening cues

1.
`00:00:42.390 --> 00:00:45.250`
`Buonasera Buonasera volete togliervi sapere?`

2.
`00:00:51.510 --> 00:00:54.100`
`Sì, grazie.`

3.
`00:00:54.150 --> 00:01:00.100`
`Se poi avete freddo, ci entendiamo il fuoco.`

### Suggested manual review windows for Noi Vivi Part 2

- `00:07:20 - 00:08:35`
  - checks one of the earliest large gaps
- `00:10:30 - 00:11:55`
  - checks another early large gap
- `00:53:15 - 00:54:45`
  - checks the biggest gap in the file

## Bottom line

If the question is "is the current subtitle renderer still broken in the old way?", the answer is no.

If the question is "are the generated subtitles already good enough to stop tuning?", the answer is:
- probably yes for `Noi Vivi` if the manual spot-checks look acceptable
- probably not yet for `Taste of Things`, because the file is still too sparse compared with the local subtitle reference

The safest next step after manual review is:
- keep the current renderer fix
- add a subtitle QA pass that explicitly flags suspicious gaps and sparse stretches
- then, if needed, add silence-aware end trimming or coverage heuristics without touching JSON or diarization output
