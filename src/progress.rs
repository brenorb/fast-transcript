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

pub(crate) struct ChunkProgressReporter {
    total_chunks: usize,
    current_chunk: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ChunkProgressReporter {
    pub(crate) fn start(total_chunks: usize) -> Self {
        let current_chunk = Arc::new(AtomicUsize::new(1));
        let stop = Arc::new(AtomicBool::new(false));

        if io::stderr().is_terminal() {
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
            eprintln!("transcribing {total_chunks} chunks...");
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
            eprintln!("transcribing chunk {current}/{}", self.total_chunks);
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
            eprintln!("done transcribing {} chunks", self.total_chunks);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{render_chunk_progress, render_chunk_progress_done};

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
}
