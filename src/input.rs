//! Spool piped media to disk so probes, decoders, and diarization can seek it.

use anyhow::{bail, Context, Result};
use std::io::{self, Read, Write};
use tempfile::NamedTempFile;

pub(crate) fn validate_source(source: &crate::types::InputSource) -> Result<()> {
    if let crate::types::InputSource::LocalPath(path) = source {
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                bail!("input file does not exist: {}", path.display());
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("cannot access input file {}", path.display()))
            }
        };
        if metadata.is_dir() {
            bail!(
                "input path is a directory: {}; choose an audio or video file",
                path.display()
            );
        }
    }
    Ok(())
}

pub(crate) fn spool_stdin(reader: &mut impl Read, interactive: bool) -> Result<NamedTempFile> {
    if interactive {
        bail!("stdin is a terminal; pass a media path or pipe media: cat lecture.wav | fscript -");
    }
    let mut media = tempfile::Builder::new()
        .prefix("fscript-stdin-")
        .suffix(".media")
        .tempfile()
        .context("cannot create temporary file for stdin media")?;
    let bytes = io::copy(reader, &mut media).context("failed to read media from stdin")?;
    if bytes == 0 {
        bail!("stdin is empty; pipe media: cat lecture.wav | fscript -");
    }
    media.flush().context("failed to save stdin media")?;
    Ok(media)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn interactive_stdin_is_rejected_before_reading() {
        struct MustNotRead;
        impl Read for MustNotRead {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("must not wait for terminal input");
            }
        }
        let error = spool_stdin(&mut MustNotRead, true).unwrap_err();
        assert!(error.to_string().contains("stdin is a terminal"));
    }

    #[test]
    fn spool_preserves_binary_media_and_is_removed_on_drop() {
        let bytes = [0, 255, 0, 12, 10, 250];
        let media = spool_stdin(&mut Cursor::new(bytes), false).unwrap();
        let path = media.path().to_owned();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        drop(media);
        assert!(!path.exists());
    }
}
