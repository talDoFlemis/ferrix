use clap::Parser;
use ferrix::{
    cli::FerrixCLI,
    fs::BasicFS,
    repl_v2::{FerrixPromptSegment, ReplV2},
    vdisk::VDisk,
};
use anyhow::Result;

fn main() -> Result<()> {
    let cli = FerrixCLI::parse();

    let vdisk = VDisk::new(cli.vdisk_path, cli.size_in_bytes)?;

    let basic_fs = BasicFS::new(vdisk);

    let mut system = ferrix::system::BasicSystem::new(basic_fs);
    let segment = FerrixPromptSegment::WorkingDirectory;

    ReplV2::run(&mut system, segment)?;

    Ok(())
}
