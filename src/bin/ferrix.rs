use clap::Parser;
use ferrix::{cli::FerrixCLI, fs::BasicFS, vdisk::VDisk};
use miette::Result;

fn main() -> Result<()> {
    let cli = FerrixCLI::parse();

    let vdisk = VDisk::new(cli.vdisk_path, cli.size_in_bytes)?;

    let basic_fs = BasicFS::new(vdisk);

    let system = ferrix::system::System::new(basic_fs);
    let mut repl = ferrix::repl_v2::ReplV2::new(system);

    repl.run()?;

    Ok(())
}
