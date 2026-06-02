//! Interactive line input from stdin.

use std::io::{self, Write};

use crate::error::{Error, Result};
use crate::ports::Prompt;

pub struct StdinPrompt;

impl Prompt for StdinPrompt {
    fn read_line(&self, prompt: &str) -> Result<String> {
        print!("{prompt}");
        io::stdout().flush().map_err(Error::Io)?;
        let mut line = String::new();
        io::stdin().read_line(&mut line).map_err(Error::Io)?;
        Ok(line.trim().to_string())
    }
}
