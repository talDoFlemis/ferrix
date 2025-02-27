use std::sync::mpsc;
use std::{path::PathBuf, thread};

use anyhow::Result;
use clap::Parser;
use ferrix::complete_command::MakeDirCommand;
use ferrix::system::System;
use ferrix::vdisk::VDisk;
use ferrix::{
    cli::FerrixCLI,
    repl_v2::{FerrixPromptSegment, ReplV2},
};
use fuser::{MountOption, Session};
use tracing::{info, Level};

fn main() -> Result<()> {
    let cli = FerrixCLI::parse();

    let storage = "/tmp/storage/";
    if !cli.vdisk_path.exists() {
        std::fs::remove_dir_all(storage)?;
        VDisk::new(cli.vdisk_path.clone(), cli.size_in_bytes)?;
    };
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let mount_point = PathBuf::from("/tmp/flemisfs");
    let mount2 = mount_point.clone();

    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let options = vec![MountOption::FSName("flemis".to_string())];
        let fs = ferrix::simple_ext4::fs_in_fs::FSInFS::new(
            "/tmp/storage".into(),
            true,
            false,
            cli.block_size.into(),
        );
        // let fs = ferrix::simple_ext4::fs::SimpleExt4FS::new(&cli.vdisk_path).unwrap();
        let mut session = Session::new(fs, &mount_point, &options).unwrap();
        let session_end = session.unmount_callable();
        sender.send(session_end).expect("failed to send");
        session.run()
    });
    let mut system = ferrix::simple_ext4::flemis_system::FlemisSystem::new(mount2)?;
    let segment = FerrixPromptSegment::WorkingDirectory;

    ReplV2::run(&mut system, segment)?;

    let unmount = receiver.recv();
    unmount?.unmount()?;

    Ok(())
}
