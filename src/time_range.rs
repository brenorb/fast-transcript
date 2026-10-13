//! User-selected media interval, in seconds from the original media start.
use anyhow::{bail, Context, Result};

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct TimeRange {
    pub(crate) start: Option<f64>,
    pub(crate) end: Option<f64>,
}

impl TimeRange {
    pub(crate) fn is_selected(self) -> bool {
        self.start.is_some() || self.end.is_some()
    }

    pub(crate) fn start_seconds(self) -> f64 {
        self.start.unwrap_or(0.0)
    }

    pub(crate) fn validate(self) -> Result<()> {
        if let Some(end) = self.end {
            if end <= self.start_seconds() {
                bail!("--end must be greater than --start (default: 0 seconds)");
            }
        }
        Ok(())
    }
}

pub(crate) fn parse_media_time(value: &str, flag: &str) -> Result<f64> {
    fn parse(value: &str) -> Option<f64> {
        let fields: Vec<&str> = value.split(':').collect();
        if fields.is_empty() || fields.len() > 3 {
            return None;
        }
        let mut total = 0.0;
        for (index, field) in fields.iter().enumerate() {
            let last = index == fields.len() - 1;
            if field.is_empty()
                || !field.bytes().any(|byte| byte.is_ascii_digit())
                || !field
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (last && byte == b'.'))
            {
                return None;
            }
            let number = field.parse::<f64>().ok()?;
            if !number.is_finite() || (index > 0 && number >= 60.0) {
                return None;
            }
            total = total * 60.0 + number;
        }
        total.is_finite().then_some(total)
    }
    parse(value).with_context(|| {
        format!("invalid {flag} value {value:?}; expected non-negative seconds, MM:SS, or HH:MM:SS (fractional seconds allowed)")
    })
}

#[cfg(test)]
mod tests {
    use super::parse_media_time;

    #[test]
    fn timestamps_and_seconds_have_the_same_value() {
        assert_eq!(parse_media_time("00:10:00.5", "--start").unwrap(), 600.5);
        assert_eq!(parse_media_time("600.5", "--start").unwrap(), 600.5);
        assert_eq!(parse_media_time("01:30", "--start").unwrap(), 90.0);
    }
}
