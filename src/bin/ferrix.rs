use clap::Parser;
use ferrix::{
    cli::FerrixCLI,
    fs::BasicFS,
    repl_v2::{FerrixPrompt, FerrixPromptSegment, ReplV2},
    vdisk::VDisk,
};
use miette::Result;

fn main() -> Result<()> {
    let cli = FerrixCLI::parse();

    let vdisk = VDisk::new(cli.vdisk_path, cli.size_in_bytes)?;

    let basic_fs = BasicFS::new(vdisk);

    let system = ferrix::system::BasicSystem::new(basic_fs);
    let segment = FerrixPromptSegment::WorkingDirectory;

    let prompt = FerrixPrompt::new(system.clone(), segment);

    ReplV2::run(system, prompt)?;

    Ok(())
}
