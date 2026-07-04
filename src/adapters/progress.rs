//! The `insert` "fidget spinner": a live, animated row list.
//!
//! This is the **only** place ANSI cursor control and the spinner thread
//! appear. When stderr is not a TTY (piped, redirected, CI) `start` returns a
//! no-op list, so non-interactive output shows nothing here and only the final
//! report (via the `Reporter`) is emitted.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::ports::{Progress, ProgressList, RowState};

/// Tri-blade fidget-spinner frames: the arms whirl around a fixed hub `●`.
const FRAMES: [&str; 4] = ["╲●╱", "─●─", "╱●╲", "│●│"];
const TICK: Duration = Duration::from_millis(110);
/// Sentinel for "no row is active".
const NONE: usize = usize::MAX;

// ANSI styling (only ever written to a confirmed TTY).
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub struct SpinnerProgress;

impl Progress for SpinnerProgress {
    type List = SpinnerList;

    fn start(&self, rows: &[String]) -> SpinnerList {
        // No animation when there is nothing to show or output is redirected.
        if rows.is_empty() || !std::io::stderr().is_terminal() {
            return SpinnerList::noop();
        }
        let shared = Arc::new(Shared {
            rows: Mutex::new(
                rows.iter()
                    .map(|label| RowCell {
                        label: label.clone(),
                        done: None,
                    })
                    .collect(),
            ),
            len: rows.len(),
            active: AtomicUsize::new(NONE),
            stop: AtomicBool::new(false),
        });
        let worker = {
            let shared = Arc::clone(&shared);
            thread::spawn(move || animate(&shared))
        };
        SpinnerList {
            shared: Some(shared),
            worker: Some(worker),
        }
    }
}

/// One row's mutable render state.
struct RowCell {
    label: String,
    /// `None` while pending/active; `Some(state)` once resolved.
    done: Option<RowState>,
}

/// State shared between the driving thread and the animation worker.
struct Shared {
    rows: Mutex<Vec<RowCell>>,
    /// Number of rows — fixed for the block's lifetime (for cursor math).
    len: usize,
    /// Index of the spinning row, or [`NONE`].
    active: AtomicUsize,
    stop: AtomicBool,
}

pub struct SpinnerList {
    shared: Option<Arc<Shared>>,
    worker: Option<JoinHandle<()>>,
}

impl SpinnerList {
    fn noop() -> Self {
        Self {
            shared: None,
            worker: None,
        }
    }
}

impl ProgressList for SpinnerList {
    fn activate(&self, i: usize) {
        if let Some(shared) = &self.shared {
            shared.active.store(i, Ordering::Relaxed);
        }
    }

    fn resolve(&self, i: usize, state: RowState, label: &str) {
        if let Some(shared) = &self.shared {
            if let Ok(mut rows) = shared.rows.lock() {
                if let Some(row) = rows.get_mut(i) {
                    row.label = label.to_string();
                    row.done = Some(state);
                }
            }
        }
    }

    fn finish(mut self) {
        stop(&mut self);
    }
}

impl Drop for SpinnerList {
    fn drop(&mut self) {
        // Safety net: erase the block even if the caller forgot `finish`
        // (e.g. an early `?` return through an active list).
        stop(self);
    }
}

/// Signal the worker to stop, join it, and erase the list block.
fn stop(list: &mut SpinnerList) {
    let Some(shared) = list.shared.take() else {
        return; // no-op list, or already stopped
    };
    shared.stop.store(true, Ordering::Relaxed);
    if let Some(worker) = list.worker.take() {
        // Join *before* touching stderr so the worker has released its lock.
        let _ = worker.join();
    }
    // Jump to the top of the n-line block and clear it to end of screen, so a
    // report printed afterwards takes its place.
    let n = shared.len;
    let mut err = std::io::stderr().lock();
    let _ = write!(err, "\r\x1b[{n}A\x1b[J");
    let _ = err.flush();
}

/// Glyph + colour for a resolved row.
fn glyph(state: RowState) -> (&'static str, &'static str) {
    match state {
        RowState::Ok => (GREEN, "✓ "),
        RowState::Warn => (YELLOW, "⚠ "),
        RowState::Fail => (RED, "✗ "),
        RowState::Skip => (DIM, "· "),
    }
}

/// The worker loop: redraw the block once per tick until told to stop. The
/// stderr lock is taken per tick (not held) so the driving thread can interleave
/// if it must.
fn animate(shared: &Shared) {
    let n = shared.len;
    let mut frame = 0usize;
    let mut drawn = false;

    while !shared.stop.load(Ordering::Relaxed) {
        let active = shared.active.load(Ordering::Relaxed);
        let mut err = std::io::stderr().lock();
        // Return to the top of the block before every redraw after the first.
        if drawn {
            let _ = write!(err, "\r\x1b[{n}A");
        }
        if let Ok(rows) = shared.rows.lock() {
            for (i, row) in rows.iter().enumerate() {
                let _ = write!(err, "\r\x1b[K");
                match row.done {
                    Some(state) => {
                        let (colour, mark) = glyph(state);
                        let _ = writeln!(err, "  {colour}{mark}{RESET} {}", row.label);
                    }
                    None if i == active => {
                        let _ = writeln!(err, "  {CYAN}{}{RESET} {}", FRAMES[frame], row.label);
                    }
                    None => {
                        let _ = writeln!(err, "  {DIM}☐  {RESET} {}", row.label);
                    }
                }
            }
        }
        let _ = err.flush();
        drop(err);

        drawn = true;
        frame = (frame + 1) % FRAMES.len();
        thread::sleep(TICK);
    }
}
