use std::{
    fs::File,
    io::{BufWriter, Write},
    os::unix::fs::MetadataExt,
    path::PathBuf,
};

use anyhow::Result;
use clap::Parser;
use ferrix::{
    cli::FerrixCLI,
    repl_v2::{FerrixPromptSegment, ReplV2},
    simple_ext4::{block_group_size, types::Superblock},
    vdisk::VDisk,
};

fn main() -> Result<()> {
    let cli = FerrixCLI::parse();

    let vdisk = VDisk::new(cli.vdisk_path.clone(), cli.size_in_bytes)?;

    let mount_point = PathBuf::from("/tmp/flemisfs");

    let mut system = ferrix::simple_ext4::flemis_system::FlemisSystem::new(vdisk, mount_point)?;
    let segment = FerrixPromptSegment::WorkingDirectory;

    ReplV2::run(&mut system, segment)?;

    Ok(())
}
