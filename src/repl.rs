use std::process::exit;

use miette::{IntoDiagnostic, Result};

use crate::{
    fs::Filesystem,
    parser::{CompleteCommand, WinnowFerrixParser},
};

#[derive(Debug, Default)]
pub struct Repl<I, O, F>
where
    I: std::io::BufRead,
    O: std::io::Write,
    F: Filesystem,
{
    input_stream: I,
    output_stream: O,
    file_system: F,
}

impl<I, O, F> Repl<I, O, F>
where
    I: std::io::BufRead,
    O: std::io::Write,
    F: Filesystem,
{
    pub fn new(input_stream: I, output_stream: O, file_system: F) -> Self {
        Self {
            input_stream,
            output_stream,
            file_system,
        }
    }

    pub fn run(&mut self) -> Result<()> {
        let mut buffer = String::new();

        loop {
            self.output_stream.write_all(b"$ ").into_diagnostic()?;
            self.output_stream.flush().into_diagnostic()?;

            self.input_stream.read_line(&mut buffer).into_diagnostic()?;

            let mut parser = WinnowFerrixParser::new(&buffer);

            match parser.get_commands() {
                Ok(commands) => {
                    for command in commands {
                        match command {
                            CompleteCommand::Exit { code } => {
                                let code = i32::try_from(*code).into_diagnostic()?;
                                exit(code);
                            }
                            _ => {
                                eprintln!("Command not implemented: {:?}", command);
                            }
                        }
                    }
                }
                Err(err) => eprintln!("{:?}", err),
            }

            buffer.clear();
        }
    }
}
