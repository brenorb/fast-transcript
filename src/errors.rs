//! User-facing error rendering. Messages we write stay; technical causes are
//! summarized in plain words, or shown in full only with `FSCRIPT_DEBUG=1`.

use std::error::Error as StdError;
use std::fmt;
use std::io;

/// Wraps a library error whose text is too technical for the default output.
/// The rendered message stops before it unless `FSCRIPT_DEBUG` is set.
pub(crate) struct Technical(Box<dyn StdError + Send + Sync + 'static>);

impl fmt::Display for Technical {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Debug for Technical {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl StdError for Technical {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.0.source()
    }
}

/// Hide an error's own text from the default output while keeping it for debugging.
pub(crate) fn technical(error: impl Into<Box<dyn StdError + Send + Sync + 'static>>) -> Technical {
    Technical(error.into())
}

/// Our message plus an optional next step, wrapping the technical cause.
/// Renders as `message: plain reason. Hint`.
pub(crate) struct Explained {
    message: String,
    hint: &'static str,
    source: Box<dyn StdError + Send + Sync + 'static>,
}

impl fmt::Display for Explained {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl fmt::Debug for Explained {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl StdError for Explained {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.source.as_ref())
    }
}

pub(crate) trait Explain<T> {
    /// Attach a user-facing message and a recovery step to a failed operation.
    fn explain(self, message: impl FnOnce() -> String, hint: &'static str) -> anyhow::Result<T>;
}

impl<T, E: Into<Box<dyn StdError + Send + Sync + 'static>>> Explain<T> for Result<T, E> {
    fn explain(self, message: impl FnOnce() -> String, hint: &'static str) -> anyhow::Result<T> {
        self.map_err(|error| {
            anyhow::Error::new(Explained {
                message: message(),
                hint,
                source: error.into(),
            })
        })
    }
}

pub(crate) const TEMP_HINT: &str =
    "Free some space in your temporary folder, or set TMPDIR to another folder, and try again";

/// Temporary files are an implementation detail; failures share one message.
pub(crate) fn temp<T, E: Into<Box<dyn StdError + Send + Sync + 'static>>>(
    result: Result<T, E>,
) -> anyhow::Result<T> {
    result.explain(
        || "could not create a temporary file".to_string(),
        TEMP_HINT,
    )
}

pub(crate) fn debug_enabled() -> bool {
    std::env::var_os("FSCRIPT_DEBUG").is_some_and(|value| !value.is_empty() && value != "0")
}

/// Short plain-language reason for an operating-system error, without error codes.
pub(crate) fn io_reason(error: &io::Error) -> String {
    use io::ErrorKind::*;
    let reason = match error.kind() {
        NotFound => "not found",
        PermissionDenied => "permission denied",
        AlreadyExists => "it already exists",
        StorageFull => "the disk is full",
        ReadOnlyFilesystem => "the disk is read-only",
        IsADirectory => "it is a folder",
        NotADirectory => "part of the path is not a folder",
        InvalidFilename => "the name is too long or not allowed",
        TimedOut => "it timed out",
        Interrupted => "it was interrupted",
        OutOfMemory => "the computer ran out of memory",
        _ => {
            let text = error.to_string();
            let text = match text.rfind(" (os error ") {
                Some(index) => text[..index].to_string(),
                None => text,
            };
            return lowercase_first(&text);
        }
    };
    reason.to_string()
}

fn lowercase_first(text: &str) -> String {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) if characters.clone().next().is_some_and(char::is_lowercase) => {
            first.to_lowercase().chain(characters).collect()
        }
        _ => text.to_string(),
    }
}

/// Render an error for the terminal: our messages joined by ": ", ending at
/// the first technical cause. Operating-system causes become a short reason.
pub fn render(error: &anyhow::Error) -> String {
    if debug_enabled() {
        return format!("{error:#}");
    }
    let mut parts: Vec<String> = Vec::new();
    let mut hint = "";
    for cause in error.chain() {
        if cause.is::<Technical>() {
            break;
        }
        if let Some(explained) = cause.downcast_ref::<Explained>() {
            if hint.is_empty() {
                hint = explained.hint;
            }
            parts.push(explained.message.clone());
            continue;
        }
        if let Some(io_error) = cause.downcast_ref::<io::Error>() {
            // io::Error can wrap a custom message (e.g. tempfile adds the path); only
            // the kind-based reason is user-facing.
            parts.push(io_reason(io_error));
            break;
        }
        let text = cause.to_string();
        if parts.last() != Some(&text) {
            parts.push(text);
        }
    }
    let message = parts.join(": ");
    match (hint.is_empty(), message.ends_with(['.', '?', '!'])) {
        (true, _) => message,
        (false, true) => format!("{message} {hint}"),
        (false, false) => format!("{message}. {hint}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;

    #[test]
    fn os_errors_render_as_plain_reasons_without_codes() {
        let error = Err::<(), _>(io::Error::from_raw_os_error(2))
            .context("could not save the transcript to out.txt")
            .unwrap_err();
        assert_eq!(
            render(&error),
            "could not save the transcript to out.txt: not found"
        );
        let error = anyhow::Error::from(io::Error::from_raw_os_error(13));
        assert_eq!(render(&error), "permission denied");
    }

    #[test]
    fn technical_causes_are_hidden_unless_debugging() {
        let parse_error = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let error = Err::<(), _>(technical(parse_error))
            .context("the saved settings are damaged")
            .unwrap_err();
        assert_eq!(render(&error), "the saved settings are damaged");
        assert!(format!("{error:#}").contains("EOF"));
    }

    #[test]
    fn explained_errors_put_the_reason_before_the_next_step() {
        let error = Err::<(), _>(io::Error::from_raw_os_error(13))
            .explain(
                || "could not save the transcript to out.txt".to_string(),
                "Choose another folder",
            )
            .context("transcription finished")
            .unwrap_err();
        assert_eq!(
            render(&error),
            "transcription finished: could not save the transcript to out.txt: permission denied. Choose another folder"
        );
        let error = temp(Err::<(), _>(io::Error::from_raw_os_error(28))).unwrap_err();
        assert_eq!(
            render(&error),
            format!("could not create a temporary file: the disk is full. {TEMP_HINT}")
        );
    }

    #[test]
    fn unknown_os_errors_drop_the_numeric_code() {
        let error = io::Error::from_raw_os_error(111);
        let reason = io_reason(&error);
        assert!(!reason.contains("os error"), "{reason}");
        assert!(!reason.is_empty());
    }
}
