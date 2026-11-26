//! Bounded arriving-audio chunking. The inference/merge path is shared with batch.

use anyhow::{bail, Context, Result};

pub(crate) struct PcmChunker {
    buffer: Vec<f32>,
    chunk: usize,
    target: usize,
    overlap: usize,
    start: usize,
    decoded_end: usize,
    received: usize,
}

impl PcmChunker {
    pub(crate) fn new(chunk: usize, overlap: usize, opening: usize) -> Result<Self> {
        if chunk == 0 || opening == 0 || opening > chunk || overlap >= opening {
            bail!("stream chunks must be positive with overlap smaller than the opening chunk");
        }
        Ok(Self {
            buffer: Vec::new(),
            chunk,
            target: opening,
            overlap,
            start: 0,
            decoded_end: 0,
            received: 0,
        })
    }

    pub(crate) fn received(&self) -> usize {
        self.received
    }

    pub(crate) fn push(
        &mut self,
        mut samples: &[f32],
        mut on_chunk: impl FnMut(usize, &[f32]) -> Result<()>,
    ) -> Result<()> {
        while !samples.is_empty() {
            let take = (self.target - self.buffer.len()).min(samples.len());
            self.received = self
                .received
                .checked_add(take)
                .context("audio timeline overflow")?;
            self.buffer.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.buffer.len() == self.target {
                on_chunk(self.start, &self.buffer)?;
                self.decoded_end = self.received;
                let consumed = self.buffer.len() - self.overlap;
                self.start += consumed;
                self.buffer.copy_within(consumed.., 0);
                self.buffer.truncate(self.overlap);
                self.target = self.chunk;
            }
        }
        Ok(())
    }

    pub(crate) fn finish(
        &mut self,
        mut on_chunk: impl FnMut(usize, &[f32]) -> Result<()>,
    ) -> Result<()> {
        // Exact chunk boundaries leave only already-decoded overlap, not a new chunk.
        if self.received > self.decoded_end {
            on_chunk(self.start, &self.buffer)?;
            self.decoded_end = self.received;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::SpeechEngine;
    use crate::inference::{build_chunk_ranges, transcribe_samples, InferenceOutcome};
    use transcribe_rs::{TranscriptionResult, TranscriptionSegment};

    #[test]
    fn property_arbitrary_arrival_boundaries_match_batch_ranges_and_preserve_every_sample() {
        let mut seed = 4242u64;
        let mut random = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            seed as usize
        };
        for _ in 0..3000 {
            let length = random() % 2000;
            let chunk = random() % 128 + 1;
            let overlap = random() % chunk;
            let samples: Vec<_> = (0..length).map(|v| v as f32).collect();
            let mut stream = PcmChunker::new(chunk, overlap, chunk).unwrap();
            let mut actual = Vec::new();
            let mut on_chunk = |start: usize, audio: &[f32]| {
                assert!(audio.len() <= chunk);
                assert_eq!(audio, &samples[start..start + audio.len()]);
                actual.push((start, start + audio.len()));
                Ok(())
            };
            let mut at = 0;
            while at < length {
                let end = (at + random() % 500 + 1).min(length);
                stream.push(&samples[at..end], &mut on_chunk).unwrap();
                assert!(stream.buffer.len() < chunk);
                at = end;
            }
            stream.finish(&mut on_chunk).unwrap();
            stream.finish(&mut on_chunk).unwrap();
            assert_eq!(stream.received(), length);
            assert_eq!(
                actual,
                build_chunk_ranges(length, 1, chunk as f64, overlap as f64).unwrap()
            );
        }
    }

    #[test]
    fn short_opening_emits_before_steady_chunk_and_does_not_repeat_final_overlap() {
        let mut chunker = PcmChunker::new(8, 1, 2).unwrap();
        let mut ranges = Vec::new();
        let mut collect = |start, samples: &[f32]| {
            ranges.push((start, start + samples.len()));
            Ok(())
        };
        chunker.push(&[0.0; 2], &mut collect).unwrap();
        chunker.push(&[0.0; 7], &mut collect).unwrap();
        chunker.finish(&mut collect).unwrap();
        assert_eq!(ranges, [(0, 2), (1, 9)]);
        assert!(PcmChunker::new(8, 2, 2).is_err());
        assert!(PcmChunker::new(0, 0, 0).is_err());
    }

    #[test]
    fn arriving_and_batch_audio_have_identical_transcripts_and_timestamps() {
        struct Engine(usize);
        impl SpeechEngine for Engine {
            fn transcribe(&mut self, samples: &[f32]) -> Result<TranscriptionResult> {
                self.0 += 1;
                let text = format!("chunk {} value {}", self.0, samples[0]);
                Ok(TranscriptionResult {
                    text: text.clone(),
                    segments: Some(vec![TranscriptionSegment {
                        start: 0.0,
                        end: samples.len() as f32 / crate::SAMPLE_RATE as f32,
                        text,
                    }]),
                })
            }
        }
        let samples: Vec<_> = (0..4 * crate::SAMPLE_RATE + 111)
            .map(|v| v as f32)
            .collect();
        let batch = transcribe_samples(&mut Engine(0), &samples, Some(1.5), 0.5).unwrap();
        let mut engine = Engine(0);
        let mut stream = InferenceOutcome::default();
        let mut chunker = PcmChunker::new(24000, 8000, 24000).unwrap();
        let mut decode =
            |start, audio: &[f32]| stream.decode_chunk(&mut engine, audio, start, true);
        for arriving in samples.chunks(777) {
            chunker.push(arriving, &mut decode).unwrap();
        }
        chunker.finish(&mut decode).unwrap();
        assert_eq!(stream.text, batch.text);
        assert_eq!(stream.segments, batch.segments);
        assert_eq!(
            stream
                .chunks
                .iter()
                .map(|c| (c.start_s, c.end_s))
                .collect::<Vec<_>>(),
            batch
                .chunks
                .iter()
                .map(|c| (c.start_s, c.end_s))
                .collect::<Vec<_>>()
        );
        assert_eq!(engine.0, batch.chunks.len());
    }

    #[test]
    fn chunk_failure_stops_the_feed_immediately() {
        let mut chunker = PcmChunker::new(8, 1, 2).unwrap();
        let mut calls = 0;
        let error = chunker
            .push(&[0.0; 100], |_, _| {
                calls += 1;
                bail!("failed inference")
            })
            .unwrap_err();
        assert_eq!(calls, 1);
        assert!(error.to_string().contains("failed inference"));
        assert_eq!(chunker.received(), 2);
    }
}
