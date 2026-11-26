//! One selection contract for local decoding, remote downloads, and captions.

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct MediaRange {
    start: Option<f64>,
    end: Option<f64>,
    last: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct ResolvedRange {
    pub(crate) start_s: f64,
    pub(crate) end_s: Option<f64>,
}

impl MediaRange {
    pub(crate) fn parse(
        start: Option<&str>,
        end: Option<&str>,
        last: Option<&str>,
    ) -> Result<Self> {
        if last.is_some() && (start.is_some() || end.is_some()) {
            bail!("--last cannot be combined with --start or --end");
        }
        let range = Self {
            start: start.map(|v| parse_time(v, "start")).transpose()?,
            end: end.map(|v| parse_time(v, "end")).transpose()?,
            last: last.map(|v| parse_time(v, "last")).transpose()?,
        };
        if range
            .end
            .is_some_and(|end| end <= range.start.unwrap_or(0.0))
        {
            bail!("--end must be greater than --start (default 0)");
        }
        if range.last == Some(0.0) {
            bail!("--last must be greater than 0");
        }
        Ok(range)
    }

    pub(crate) fn is_selected(self) -> bool {
        self != Self::default()
    }

    pub(crate) fn needs_duration(self) -> bool {
        self.last.is_some()
    }

    pub(crate) fn resolve(self, duration: Option<f64>) -> Result<ResolvedRange> {
        let duration = duration.filter(|v| v.is_finite() && *v > 0.0);
        let start_s = match self.last {
            Some(last) => (duration.context("--last requires a known, finite media duration")?
                - last)
                .max(0.0),
            None => self.start.unwrap_or(0.0),
        };
        if duration.is_some_and(|duration| start_s >= duration) {
            bail!("--start is at or beyond the end of the media");
        }
        let end_s = match (self.end, duration) {
            (Some(end), Some(duration)) => Some(end.min(duration)),
            (end, _) => end.or(duration.filter(|_| self.last.is_some())),
        };
        Ok(ResolvedRange { start_s, end_s })
    }
}

impl ResolvedRange {
    pub(crate) fn is_selected(self) -> bool {
        self.start_s != 0.0 || self.end_s.is_some()
    }

    pub(crate) fn download_section(self) -> String {
        format!(
            "*{}-{}",
            self.start_s,
            self.end_s
                .map_or_else(|| "inf".into(), |end| end.to_string())
        )
    }

    pub(crate) fn clip_segments(self, segments: &mut Vec<crate::types::TranscriptSegment>) {
        segments.retain_mut(|segment| {
            segment.start_s = segment.start_s.max(self.start_s);
            if let Some(end) = self.end_s {
                segment.end_s = segment.end_s.min(end);
            }
            segment.end_s > segment.start_s
        });
    }
}

fn parse_time(value: &str, flag: &str) -> Result<f64> {
    let invalid =
        || anyhow::anyhow!("invalid --{flag} value {value:?}; use seconds, MM:SS, or HH:MM:SS");
    let parts: Vec<_> = value.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return Err(invalid());
    }
    let mut seconds = 0.0;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty()
            || !part.chars().all(|c| c.is_ascii_digit() || c == '.')
            || (index + 1 < parts.len() && part.contains('.'))
        {
            return Err(invalid());
        }
        let number: f64 = part.parse().map_err(|_| invalid())?;
        if !number.is_finite() || (index > 0 && number >= 60.0) {
            return Err(invalid());
        }
        seconds = seconds * 60.0 + number;
    }
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(invalid());
    }
    Ok(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_selection_before_media_io() {
        for (start, end, last) in [
            (Some("NaN"), None, None),
            (Some("-1"), None, None),
            (Some("1:60"), None, None),
            (Some("1.5:02"), None, None),
            (Some("1:2:3:4"), None, None),
            (Some("1"), Some("1"), None),
            (None, Some("0"), None),
            (None, None, Some("0")),
            (Some("0"), None, Some("5")),
            (None, Some("5"), Some("5")),
        ] {
            assert!(MediaRange::parse(start, end, last).is_err());
        }
        assert!(MediaRange::parse(None, None, Some("5:00"))
            .unwrap()
            .resolve(None)
            .is_err());
        assert!(MediaRange::parse(Some("10"), None, None)
            .unwrap()
            .resolve(Some(10.0))
            .is_err());
        assert_eq!(parse_time("01:02:03.5", "start").unwrap(), 3723.5);
    }

    #[test]
    fn property_last_selection_stays_inside_media_and_has_requested_or_available_length() {
        let mut state = 8128u64;
        for _ in 0..10_000 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let duration = (state % 1_000_000 + 1) as f64 / 10.0;
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let last = (state % 1_000_000 + 1) as f64 / 10.0;
            let resolved = MediaRange::parse(None, None, Some(&last.to_string()))
                .unwrap()
                .resolve(Some(duration))
                .unwrap();
            assert!(resolved.start_s >= 0.0 && resolved.start_s < duration);
            assert_eq!(resolved.end_s, Some(duration));
            assert!((duration - resolved.start_s - last.min(duration)).abs() < 1e-8);
        }
    }

    #[test]
    fn property_clock_notation_matches_seconds_and_caption_intersection_is_idempotent() {
        for value in 0..4000 {
            let clock = format!(
                "{}:{:02}:{:02}.25",
                value / 3600,
                value / 60 % 60,
                value % 60
            );
            assert_eq!(parse_time(&clock, "start").unwrap(), value as f64 + 0.25);
            let range = ResolvedRange {
                start_s: value as f64,
                end_s: Some(value as f64 + 10.0),
            };
            let mut segments = vec![crate::types::TranscriptSegment {
                start_s: value as f64 - 2.0,
                end_s: value as f64 + 12.0,
                text: "cue".into(),
                speaker: None,
            }];
            range.clip_segments(&mut segments);
            let once = segments.clone();
            range.clip_segments(&mut segments);
            assert_eq!(once, segments);
            assert_eq!(segments[0].start_s, range.start_s);
            assert_eq!(segments[0].end_s, range.end_s.unwrap());
        }
    }
}
