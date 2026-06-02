//! The `insert` "fidget spinner": a live, animated phase checklist.
//!
//! This is the **only** place ANSI cursor control and the spinner thread
//! appear. When stderr is not a TTY (piped, redirected, CI) `start` returns a
//! no-op task, so non-interactive output shows nothing here and only the final
//! report (via the `Reporter`) is emitted.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::ports::{Progress, ProgressTask};

/// Tri-blade fidget-spinner frames: the arms whirl around a fixed hub `●`.
const FRAMES: [&str; 4] = ["╲●╱", "─●─", "╱●╲", "│●│"];
const TICK: Duration = Duration::from_millis(110);

// ANSI styling (only ever written to a confirmed TTY).
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub struct SpinnerProgress;

impl Progress for SpinnerProgress {
    type Task = SpinnerTask;

    fn start(&self, phases: &[&str]) -> SpinnerTask {
        // No animation when there is nothing to show or output is redirected.
        if phases.is_empty() || !std::io::stderr().is_terminal() {
            return SpinnerTask::noop();
        }
        let shared = Arc::new(Shared {
            phases: phases.iter().map(|s| s.to_string()).collect(),
            active: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
        });
        let worker = {
            let shared = Arc::clone(&shared);
            thread::spawn(move || animate(&shared))
        };
        SpinnerTask {
            shared: Some(shared),
            worker: Some(worker),
        }
    }
}

/// State shared between the driving thread and the animation worker.
struct Shared {
    phases: Vec<String>,
    /// Index of the active phase; everything below it is done (✓).
    active: AtomicUsize,
    stop: AtomicBool,
}

pub struct SpinnerTask {
    shared: Option<Arc<Shared>>,
    worker: Option<JoinHandle<()>>,
}

impl SpinnerTask {
    fn noop() -> Self {
        Self {
            shared: None,
            worker: None,
        }
    }
}

impl ProgressTask for SpinnerTask {
    fn complete(&self) {
        if let Some(shared) = &self.shared {
            shared.active.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn finish(mut self) {
        stop(&mut self);
    }
}

impl Drop for SpinnerTask {
    fn drop(&mut self) {
        // Safety net: erase the block even if the caller forgot `finish`
        // (e.g. an early `?` return through an active task).
        stop(self);
    }
}

/// Signal the worker to stop, join it, and erase the checklist block.
fn stop(task: &mut SpinnerTask) {
    let Some(shared) = task.shared.take() else {
        return; // no-op task, or already stopped
    };
    shared.stop.store(true, Ordering::Relaxed);
    if let Some(worker) = task.worker.take() {
        // Join *before* touching stderr so the worker has released its lock.
        let _ = worker.join();
    }
    // Jump to the top of the n-line block and clear it to end of screen, so a
    // report printed afterwards takes its place.
    let n = shared.phases.len();
    let mut err = std::io::stderr().lock();
    let _ = write!(err, "\r\x1b[{n}A\x1b[J");
    let _ = err.flush();
}

/// The worker loop: redraw the block once per tick until told to stop. The
/// stderr lock is taken per tick (not held) so the driving thread can interleave
/// if it must.
fn animate(shared: &Shared) {
    let n = shared.phases.len();
    let mut frame = 0usize;
    let mut drawn = false;

    while !shared.stop.load(Ordering::Relaxed) {
        let active = shared.active.load(Ordering::Relaxed);
        let mut err = std::io::stderr().lock();
        // Return to the top of the block before every redraw after the first.
        if drawn {
            let _ = write!(err, "\r\x1b[{n}A");
        }
        for (i, phase) in shared.phases.iter().enumerate() {
            let _ = write!(err, "\r\x1b[K");
            if i < active {
                let _ = writeln!(err, "  {GREEN}✓  {RESET} {phase}");
            } else if i == active {
                let _ = writeln!(err, "  {CYAN}{}{RESET} {phase}", FRAMES[frame]);
            } else {
                let _ = writeln!(err, "  {DIM}☐  {RESET} {phase}");
            }
        }
        let _ = err.flush();
        drop(err);

        drawn = true;
        frame = (frame + 1) % FRAMES.len();
        thread::sleep(TICK);
    }
}
