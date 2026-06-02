//! Wall-clock timestamp via the `date` binary (avoids a datetime crate).

use std::process::Command;

use crate::ports::Clock;

pub struct DateClock;

impl Clock for DateClock {
    fn timestamp(&self) -> String {
        Command::new("date")
            .arg("+%Y-%m-%d %H:%M:%S")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    }
}
