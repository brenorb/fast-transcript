use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub(crate) fn render_chunk_progress(prefix: &str, current: usize, total: usize) -> String {
    let completed_chunks = current.saturating_sub(1).min(total);
    let filled = (completed_chunks * crate::PROGRESS_BAR_WIDTH)
        .checked_div(total)
        .unwrap_or(crate::PROGRESS_BAR_WIDTH);
    let empty = crate::PROGRESS_BAR_WIDTH.saturating_sub(filled);
    let bar = format!("{}{}", "█".repeat(filled), "▒".repeat(empty));
    format!("{prefix} {bar} transcribing chunk {current}/{total}")
}

pub(crate) fn render_chunk_progress_done(total: usize) -> String {
    let bar = "█".repeat(crate::PROGRESS_BAR_WIDTH);
    format!("✓ {bar} transcribing chunk {total}/{total}")
}

fn render_noninteractive_start(total: usize) -> String {
    format!("transcribing {total} chunks...")
}

fn render_noninteractive_chunk(current: usize, total: usize) -> String {
    format!("transcribing chunk {current}/{total}")
}

fn render_noninteractive_done(total: usize) -> String {
    format!("done transcribing {total} chunks")
}

pub(crate) struct ChunkProgressReporter {
    total_chunks: usize,
    current_chunk: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ChunkProgressReporter {
    pub(crate) fn start(total_chunks: usize) -> Self {
        Self::start_with_terminal(total_chunks, io::stderr().is_terminal())
    }

    fn start_with_terminal(total_chunks: usize, stderr_is_terminal: bool) -> Self {
        let current_chunk = Arc::new(AtomicUsize::new(1));
        let stop = Arc::new(AtomicBool::new(false));

        if stderr_is_terminal {
            let current_chunk_for_thread = Arc::clone(&current_chunk);
            let stop_for_thread = Arc::clone(&stop);
            let handle = thread::spawn(move || {
                let mut frame_index = 0usize;
                loop {
                    let current = current_chunk_for_thread
                        .load(Ordering::Relaxed)
                        .clamp(1, total_chunks.max(1));
                    let line = render_chunk_progress(
                        crate::SPINNER_FRAMES[frame_index],
                        current,
                        total_chunks,
                    );
                    eprint!("\r{line}");
                    let _ = io::stderr().flush();

                    if stop_for_thread.load(Ordering::Relaxed) {
                        break;
                    }

                    frame_index = (frame_index + 1) % crate::SPINNER_FRAMES.len();
                    thread::sleep(Duration::from_millis(80));
                }
            });

            Self {
                total_chunks,
                current_chunk,
                stop,
                handle: Some(handle),
            }
        } else {
            eprintln!("{}", render_noninteractive_start(total_chunks));
            Self {
                total_chunks,
                current_chunk,
                stop,
                handle: None,
            }
        }
    }

    pub(crate) fn set_current_chunk(&self, current: usize) {
        let current = current.clamp(1, self.total_chunks.max(1));
        self.current_chunk.store(current, Ordering::Relaxed);
        if self.handle.is_none() {
            eprintln!(
                "{}",
                render_noninteractive_chunk(current, self.total_chunks)
            );
        }
    }

    pub(crate) fn finish(self) {
        self.current_chunk
            .store(self.total_chunks.max(1), Ordering::Relaxed);
        self.stop.store(true, Ordering::Relaxed);

        if let Some(handle) = self.handle {
            let _ = handle.join();
            eprintln!("\r{}", render_chunk_progress_done(self.total_chunks));
        } else {
            eprintln!("{}", render_noninteractive_done(self.total_chunks));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        render_chunk_progress, render_chunk_progress_done, render_noninteractive_chunk,
        render_noninteractive_done, render_noninteractive_start, ChunkProgressReporter,
    };

    #[test]
    fn render_chunk_progress_formats_spinner_and_bar() {
        let rendered = render_chunk_progress("⠟", 12, 23);
        assert_eq!(rendered, "⠟ █████████▒▒▒▒▒▒▒▒▒▒▒ transcribing chunk 12/23");
    }

    #[test]
    fn render_chunk_progress_done_shows_complete_bar() {
        let rendered = render_chunk_progress_done(23);
        assert_eq!(rendered, "✓ ████████████████████ transcribing chunk 23/23");
    }

    #[test]
    fn noninteractive_progress_messages_are_coarse_and_final() {
        assert_eq!(render_noninteractive_start(3), "transcribing 3 chunks...");
        assert_eq!(render_noninteractive_chunk(2, 3), "transcribing chunk 2/3");
        assert_eq!(render_noninteractive_done(3), "done transcribing 3 chunks");
    }

    #[test]
    fn chunk_progress_reporter_noninteractive_lifecycle_uses_no_thread() {
        let reporter = ChunkProgressReporter::start_with_terminal(3, false);
        assert!(reporter.handle.is_none());
        reporter.set_current_chunk(2);
        reporter.finish();
    }

    #[test]
    fn chunk_progress_reporter_interactive_lifecycle_joins_thread() {
        let reporter = ChunkProgressReporter::start_with_terminal(2, true);
        assert!(reporter.handle.is_some());
        reporter.set_current_chunk(2);
        reporter.finish();
    }
}
