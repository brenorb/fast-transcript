//! Durable, separate meeting tracks. Capture and inference never own the only copy.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const RATE: u32 = crate::SAMPLE_RATE as u32;
const MAX_READ_FRAMES: usize = crate::SAMPLE_RATE * 120;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Track {
    Microphone,
    System,
}

impl Track {
    pub(crate) const ALL: [Self; 2] = [Self::Microphone, Self::System];

    fn index(self) -> usize {
        match self {
            Self::Microphone => 0,
            Self::System => 1,
        }
    }

    fn filename(self) -> &'static str {
        match self {
            Self::Microphone => "microphone.pcm",
            Self::System => "system.pcm",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Recording,
    Stopped,
    Interrupted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub(crate) version: u32,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
    pub(crate) sample_format: String,
    pub(crate) started_at_unix_ms: u64,
    pub(crate) state: State,
    /// Last checkpoint, in microphone/system order. No per-track path is trusted.
    pub(crate) track_frames: [u64; 2],
}

fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    options
}

struct Lock {
    _file: File,
}

impl Lock {
    fn acquire(directory: &Path, create: bool) -> Result<Self> {
        let file = options()
            .read(true)
            .write(true)
            .create_new(create)
            .open(directory.join(".lock"))
            .context("cannot open meeting lock")?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(std::io::Error::last_os_error())
                    .context("meeting is still owned by another process; wait for recording or transcription to finish");
            }
            Ok(Self { _file: file })
        }
        #[cfg(not(unix))]
        {
            let _ = file;
            bail!("meeting storage currently requires a Unix platform")
        }
    }
}

// Closing releases the lock. Never unlink it: another process may already have
// this inode open while waiting to acquire ownership.

pub(crate) fn inspect(directory: &Path) -> Result<Manifest> {
    let manifest: Manifest = serde_json::from_reader(
        options()
            .read(true)
            .open(directory.join("meeting.json"))
            .context("cannot read meeting manifest")?,
    )
    .context("invalid meeting manifest")?;
    if manifest.version != 1
        || manifest.sample_rate != RATE
        || manifest.channels != 1
        || manifest.sample_format != "s16le"
    {
        bail!("unsupported meeting storage version or audio format");
    }
    Ok(manifest)
}

fn save(directory: &Path, manifest: &Manifest) -> Result<()> {
    let mut pending = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer_pretty(pending.as_file_mut(), manifest)?;
    pending.write_all(b"\n")?;
    pending.as_file().sync_all()?;
    pending.persist(directory.join("meeting.json"))?;
    File::open(directory)?.sync_all()?;
    Ok(())
}

pub(crate) struct Writer {
    directory: PathBuf,
    _lock: Lock,
    tracks: [File; 2],
    positions: [u64; 2],
    manifest: Manifest,
    failed: bool,
}

impl Writer {
    /// The exact destination must be new. Initialize before opening capture.
    pub(crate) fn create(directory: &Path) -> Result<Self> {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(directory)
            .context("cannot create new meeting directory")?;
        let lock = Lock::acquire(directory, true)?;
        let open = |track: Track| -> Result<File> {
            Ok(options()
                .write(true)
                .create_new(true)
                .open(directory.join(track.filename()))?)
        };
        let tracks = [open(Track::Microphone)?, open(Track::System)?];
        let manifest = Manifest {
            version: 1,
            sample_rate: RATE,
            channels: 1,
            sample_format: "s16le".into(),
            started_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)?
                .as_millis()
                .try_into()?,
            state: State::Recording,
            track_frames: [0; 2],
        };
        save(directory, &manifest)?;
        if let Some(parent) = directory.parent().filter(|p| !p.as_os_str().is_empty()) {
            File::open(parent)?.sync_all()?;
        }
        Ok(Self {
            directory: directory.into(),
            _lock: lock,
            tracks,
            positions: [0; 2],
            manifest,
            failed: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn frames(&self, track: Track) -> u64 {
        self.positions[track.index()]
    }

    /// Frame offsets share the meeting clock. Preserve prior samples on overlap;
    /// fill gaps with silence using fixed storage, even after a long device gap.
    /// Called by the disk writer, never directly from a realtime audio callback.
    pub(crate) fn append(&mut self, track: Track, start: u64, samples: &[i16]) -> Result<()> {
        if self.failed {
            bail!("meeting writer failed; preserve its files and recover after shutdown");
        }
        let end = start
            .checked_add(samples.len().try_into()?)
            .context("meeting sample position overflow")?;
        end.checked_mul(2)
            .context("meeting byte position overflow")?;
        if samples.is_empty() {
            return Ok(());
        }
        let index = track.index();
        let position = &mut self.positions[index];
        let file = &mut self.tracks[index];
        let result = (|| -> Result<()> {
            let zeros = [0u8; 8192];
            while *position < start {
                let count = (start - *position).min((zeros.len() / 2) as u64) as usize;
                file.write_all(&zeros[..count * 2])?;
                *position += count as u64;
            }
            let skip = position.saturating_sub(start).min(samples.len() as u64) as usize;
            let mut bytes = [0u8; 8192];
            for chunk in samples[skip..].chunks(bytes.len() / 2) {
                for (sample, pair) in chunk.iter().zip(bytes.chunks_exact_mut(2)) {
                    pair.copy_from_slice(&sample.to_le_bytes());
                }
                file.write_all(&bytes[..chunk.len() * 2])?;
                *position += chunk.len() as u64;
            }
            debug_assert!(*position >= end);
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
        }
        result.context("cannot append meeting audio; existing audio remains recoverable")
    }

    /// Sync audio before publishing progress. The controller chooses the bounded
    /// checkpoint interval; dropping or killing a writer never marks it Stopped.
    pub(crate) fn checkpoint(&mut self) -> Result<Manifest> {
        if self.failed {
            bail!("meeting writer failed; do not publish a successful checkpoint");
        }
        let result = (|| -> Result<Manifest> {
            for file in &self.tracks {
                file.sync_data()?;
            }
            self.manifest.track_frames = self.positions;
            save(&self.directory, &self.manifest)?;
            Ok(self.manifest.clone())
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(crate) fn finish(mut self) -> Result<SavedSession> {
        self.checkpoint()?;
        self.manifest.state = State::Stopped;
        save(&self.directory, &self.manifest)?;
        Ok(SavedSession {
            _lock: self._lock,
            manifest: self.manifest,
        })
    }
}

/// Ownership remains exclusive through final transcript publication.
pub(crate) struct SavedSession {
    _lock: Lock,
    pub(crate) manifest: Manifest,
}

impl SavedSession {
    /// Repair an ownerless recording, then validate immutable audio before ASR.
    pub(crate) fn open(directory: &Path) -> Result<Self> {
        let lock = Lock::acquire(directory, false)?;
        let manifest = recover_locked(directory)?;
        for track in Track::ALL {
            let file = options()
                .read(true)
                .open(directory.join(track.filename()))?;
            let metadata = file.metadata()?;
            if !metadata.is_file()
                || metadata.len() % 2 != 0
                || metadata.len() / 2 != manifest.track_frames[track.index()]
            {
                bail!("saved meeting track length does not match its manifest");
            }
        }
        if manifest.track_frames == [0, 0] {
            bail!("meeting contains no saved audio to transcribe");
        }
        Ok(Self {
            _lock: lock,
            manifest,
        })
    }
}

/// Recover only an unowned Recording session. Retain every complete on-disk
/// sample, including audio written since the last published checkpoint.
pub(crate) fn recover(directory: &Path) -> Result<Manifest> {
    let _lock = Lock::acquire(directory, false)?;
    recover_locked(directory)
}

fn recover_locked(directory: &Path) -> Result<Manifest> {
    let mut manifest = inspect(directory)?;
    if manifest.state != State::Recording {
        return Ok(manifest);
    }
    // Open and inspect both tracks before modifying either one.
    let open = |track: Track| -> Result<(File, u64)> {
        let file = options()
            .write(true)
            .open(directory.join(track.filename()))?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            bail!("meeting track is not a regular file");
        }
        Ok((file, metadata.len()))
    };
    let tracks = [open(Track::Microphone)?, open(Track::System)?];
    for (index, (file, length)) in tracks.into_iter().enumerate() {
        if length % 2 != 0 {
            file.set_len(length - 1)?;
        }
        file.sync_data()?;
        manifest.track_frames[index] = length / 2;
    }
    manifest.state = State::Interrupted;
    save(directory, &manifest)?;
    Ok(manifest)
}

/// Bounded windows for incremental inference; never load a full long meeting.
pub(crate) fn read_frames(
    directory: &Path,
    track: Track,
    start: u64,
    count: usize,
) -> Result<Vec<i16>> {
    if count > MAX_READ_FRAMES {
        bail!("read meeting audio in windows of at most 120 seconds");
    }
    let mut file = options()
        .read(true)
        .open(directory.join(track.filename()))?;
    let available = (file.metadata()?.len() / 2).saturating_sub(start);
    let count = available.min(count as u64) as usize;
    let offset = start
        .checked_mul(2)
        .context("meeting byte offset overflow")?;
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0u8; count * 2];
    file.read_exact(&mut bytes)?;
    Ok(bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_keep_distinct_audio_and_freeze_prior_timeline() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("meeting");
        let mut writer = Writer::create(&directory).unwrap();
        writer.append(Track::Microphone, 3, &[11, 12, 13]).unwrap();
        writer.append(Track::System, 1, &[-21, -22]).unwrap();
        writer
            .append(Track::Microphone, 4, &[90, 91, 14, 15])
            .unwrap();
        writer.append(Track::System, 0, &[99]).unwrap();
        assert_eq!(writer.frames(Track::Microphone), 8);
        assert_eq!(inspect(&directory).unwrap().track_frames, [0, 0]);
        assert!(
            recover(&directory).is_err(),
            "never recover an owned session"
        );
        assert_eq!(writer.checkpoint().unwrap().track_frames, [8, 3]);
        assert_eq!(writer.finish().unwrap().manifest.state, State::Stopped);
        assert_eq!(
            read_frames(&directory, Track::Microphone, 0, 100).unwrap(),
            [0, 0, 0, 11, 12, 13, 14, 15]
        );
        assert_eq!(
            read_frames(&directory, Track::System, 0, 100).unwrap(),
            [0, -21, -22]
        );
        assert_eq!(recover(&directory).unwrap().state, State::Stopped);
    }

    #[test]
    fn saved_session_keeps_exclusive_ownership_after_capture_and_repair() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("meeting");
        let mut writer = Writer::create(&directory).unwrap();
        writer.append(Track::Microphone, 0, &[1, 2, 3]).unwrap();
        let saved = writer.finish().unwrap();
        assert_eq!(saved.manifest.state, State::Stopped);
        assert!(recover(&directory).is_err());
        assert!(SavedSession::open(&directory).is_err());
        drop(saved);
        let replay = SavedSession::open(&directory).unwrap();
        assert_eq!(replay.manifest.track_frames, [3, 0]);
        assert!(SavedSession::open(&directory).is_err());
        drop(replay);
        assert_eq!(recover(&directory).unwrap().state, State::Stopped);

        let interrupted = temp.path().join("interrupted");
        let mut writer = Writer::create(&interrupted).unwrap();
        writer.append(Track::System, 0, &[4, 5]).unwrap();
        drop(writer);
        let replay = SavedSession::open(&interrupted).unwrap();
        assert_eq!(replay.manifest.state, State::Interrupted);
        assert_eq!(replay.manifest.track_frames, [0, 2]);
        assert!(recover(&interrupted).is_err());
        drop(replay);
        assert_eq!(recover(&interrupted).unwrap().state, State::Interrupted);
    }

    #[test]
    fn saved_transcription_rejects_empty_or_modified_tracks() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("meeting");
        drop(Writer::create(&directory).unwrap().finish().unwrap());
        assert!(SavedSession::open(&directory).is_err());
        std::fs::write(directory.join("system.pcm"), [1, 2]).unwrap();
        assert!(SavedSession::open(&directory).is_err());
        std::fs::write(directory.join("system.pcm"), [1]).unwrap();
        assert!(SavedSession::open(&directory).is_err());
    }

    #[test]
    fn packet_boundaries_gaps_and_overlaps_match_independent_sample_timeline() {
        for seed in 1..=64u64 {
            let temp = tempfile::tempdir().unwrap();
            let directory = temp.path().join("meeting");
            let mut writer = Writer::create(&directory).unwrap();
            let mut expected = [Vec::new(), Vec::new()];
            let mut state = seed;
            let mut random = || {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                state
            };
            for _ in 0..128 {
                let track = Track::ALL[(random() % 2) as usize];
                let tape = &mut expected[track.index()];
                let start = random() as usize % (tape.len() + 80);
                let samples: Vec<i16> = (0..random() % 91).map(|_| random() as i16).collect();
                // Independent per-frame model: existing frames are immutable;
                // new frames contain either silence or the arriving packet.
                if !samples.is_empty() {
                    for frame in tape.len()..start + samples.len() {
                        tape.push(if frame < start {
                            0
                        } else {
                            samples[frame - start]
                        });
                    }
                }
                writer.append(track, start as u64, &samples).unwrap();
            }
            writer.finish().unwrap();
            for track in Track::ALL {
                let actual = read_frames(&directory, track, 0, MAX_READ_FRAMES).unwrap();
                assert_eq!(
                    actual,
                    expected[track.index()],
                    "seed={seed}, track={track:?}"
                );
                let offset = actual.len() / 3;
                assert_eq!(
                    read_frames(&directory, track, offset as u64, 13).unwrap(),
                    actual[offset..actual.len().min(offset + 13)]
                );
            }
        }
    }

    #[test]
    fn every_pcm_value_round_trips_and_invalid_windows_do_not_allocate() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("meeting");
        let mut writer = Writer::create(&directory).unwrap();
        let samples: Vec<i16> = (i16::MIN..=i16::MAX).collect();
        writer.append(Track::System, 0, &samples).unwrap();
        assert!(writer.append(Track::Microphone, u64::MAX, &[1]).is_err());
        writer.finish().unwrap();
        assert_eq!(
            read_frames(&directory, Track::System, 0, samples.len()).unwrap(),
            samples
        );
        assert!(read_frames(&directory, Track::System, 0, MAX_READ_FRAMES + 1).is_err());
        assert!(read_frames(&directory, Track::System, u64::MAX, 1).is_err());
    }

    #[test]
    fn write_failure_cannot_publish_success_or_continue_on_the_other_track() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("meeting");
        let mut writer = Writer::create(&directory).unwrap();
        writer.append(Track::Microphone, 0, &[10, 11]).unwrap();
        writer.checkpoint().unwrap();
        writer.tracks[0] = File::open(directory.join("microphone.pcm")).unwrap();
        assert!(writer.append(Track::Microphone, 2, &[12]).is_err());
        assert!(writer.append(Track::System, 0, &[20]).is_err());
        assert!(writer.finish().is_err());
        assert_eq!(inspect(&directory).unwrap().state, State::Recording);
        assert_eq!(recover(&directory).unwrap().track_frames, [2, 0]);
    }

    #[test]
    fn recovery_retains_post_checkpoint_samples_and_trims_only_incomplete_frame() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("meeting");
        let mut writer = Writer::create(&directory).unwrap();
        writer.append(Track::Microphone, 0, &[1, 2]).unwrap();
        writer.checkpoint().unwrap();
        writer.append(Track::Microphone, 2, &[3]).unwrap();
        drop(writer);
        OpenOptions::new()
            .append(true)
            .open(directory.join("microphone.pcm"))
            .unwrap()
            .write_all(&[0xee])
            .unwrap();
        assert_eq!(recover(&directory).unwrap().track_frames, [3, 0]);
        assert_eq!(
            read_frames(&directory, Track::Microphone, 0, 10).unwrap(),
            [1, 2, 3]
        );
        let first = std::fs::read(directory.join("meeting.json")).unwrap();
        assert_eq!(recover(&directory).unwrap().state, State::Interrupted);
        assert_eq!(
            std::fs::read(directory.join("meeting.json")).unwrap(),
            first
        );
    }

    #[test]
    fn unsupported_manifest_is_rejected_before_recovery_changes_audio() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("meeting");
        drop(Writer::create(&directory).unwrap());
        let mut manifest = inspect(&directory).unwrap();
        manifest.version = 99;
        save(&directory, &manifest).unwrap();
        std::fs::write(directory.join("microphone.pcm"), [1, 2, 3]).unwrap();
        assert!(recover(&directory).is_err());
        assert_eq!(
            std::fs::read(directory.join("microphone.pcm")).unwrap(),
            [1, 2, 3]
        );
        assert!(
            Writer::create(&directory).is_err(),
            "never overwrite a session"
        );
    }

    #[cfg(unix)]
    #[test]
    fn recovery_preflights_both_tracks_without_following_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("meeting");
        drop(Writer::create(&directory).unwrap());
        std::fs::write(directory.join("microphone.pcm"), [1, 2, 3]).unwrap();
        let outside = temp.path().join("outside");
        std::fs::write(&outside, [4, 5, 6]).unwrap();
        std::fs::remove_file(directory.join("system.pcm")).unwrap();
        std::os::unix::fs::symlink(&outside, directory.join("system.pcm")).unwrap();
        assert!(recover(&directory).is_err());
        assert_eq!(
            std::fs::read(directory.join("microphone.pcm")).unwrap(),
            [1, 2, 3]
        );
        assert_eq!(std::fs::read(outside).unwrap(), [4, 5, 6]);
        assert_eq!(inspect(&directory).unwrap().state, State::Recording);
    }

    #[cfg(unix)]
    #[test]
    fn crash_fixture() {
        let Some(directory) = std::env::var_os("FSCRIPT_MEETING_TEST_CHILD_DIR") else {
            return;
        };
        let directory = PathBuf::from(directory);
        let mut writer = Writer::create(&directory).unwrap();
        writer.append(Track::Microphone, 0, &[1; 10]).unwrap();
        writer.append(Track::System, 0, &[2; 7]).unwrap();
        writer.checkpoint().unwrap();
        writer.append(Track::Microphone, 10, &[3, 4]).unwrap();
        writer.append(Track::System, 7, &[5]).unwrap();
        OpenOptions::new()
            .append(true)
            .open(directory.join("system.pcm"))
            .unwrap()
            .write_all(&[0xaa])
            .unwrap();
        std::fs::write(directory.join("ready"), b"ready").unwrap();
        loop {
            std::thread::park();
        }
    }

    #[cfg(unix)]
    #[test]
    fn killed_process_releases_ownership_and_keeps_recoverable_audio() {
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("meeting");
        let mut child = Child(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "meeting_store::tests::crash_fixture"])
                .env("FSCRIPT_MEETING_TEST_CHILD_DIR", &directory)
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !directory.join("ready").exists() && std::time::Instant::now() < deadline {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "fixture exited before readiness"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(directory.join("ready").exists());
        assert!(recover(&directory).is_err());
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let manifest = recover(&directory).unwrap();
        assert_eq!(manifest.state, State::Interrupted);
        assert_eq!(manifest.track_frames, [12, 8]);
        assert_eq!(
            read_frames(&directory, Track::Microphone, 9, 10).unwrap(),
            [1, 3, 4]
        );
        assert_eq!(
            read_frames(&directory, Track::System, 6, 10).unwrap(),
            [2, 5]
        );
    }
}
