//! User-facing output to stdout/stderr.

use crate::ports::Reporter;

pub struct StdReporter;

impl Reporter for StdReporter {
    fn out(&self, line: &str) {
        println!("{line}");
    }

    fn status(&self, msg: &str) {
        eprintln!("{msg}");
    }
}
