use std::io::BufReader;

use ferrix::fs::Filesystem;
use miette::Result;

struct MockedFS;

impl Filesystem for MockedFS {}

fn main() -> Result<()> {
    let reader = BufReader::new(std::io::stdin());
    let writer = std::io::stdout();

    let mut repl = ferrix::repl::Repl::new(reader, writer, MockedFS);
    repl.run()?;

    Ok(())
}
