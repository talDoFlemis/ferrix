use ferrix::fs::Filesystem;
use miette::Result;

struct MockedFS;

impl Filesystem for MockedFS {}

fn main() -> Result<()> {
    let system = ferrix::system::System::new(MockedFS);
    let mut repl = ferrix::repl_v2::ReplV2::new(system);

    repl.run()?;

    Ok(())
}
