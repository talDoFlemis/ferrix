use std::process::exit;

use miette::{IntoDiagnostic, Result};

use crate::complete_command::CompleteCommand;
use crate::fs::Filesystem;

pub struct System<F>
where
    F: Filesystem,
{
    #[allow(dead_code)]
    file_system: F,
}

impl<F> System<F>
where
    F: Filesystem,
{
    pub fn new(file_system: F) -> Self {
        Self { file_system }
    }

    pub fn process_command(&mut self, command: CompleteCommand) -> Result<()> {
        match command {
            CompleteCommand::Exit { code } => {
                let code = i32::try_from(code)
                    .into_diagnostic()
                    .expect("expected to convert code");
                exit(code);
            }
            _ => {
                eprintln!("Command not implemented: {:?}", command);
            }
        };

        Ok(())
    }
}
