use std::sync::mpsc;
use std::{path::PathBuf, thread};

use anyhow::Result;
use clap::Parser;
use ferrix::complete_command::MakeDirCommand;
use ferrix::system::System;
use ferrix::{
    cli::FerrixCLI,
    repl_v2::{FerrixPromptSegment, ReplV2},
};
use fuser::{MountOption, Session};
use tracing::{info, Level};

fn main() -> Result<()> {
    let cli = FerrixCLI::parse();

    if !cli.vdisk_path.exists() {
        ferrix::simple_ext4::mkfs::make(&cli.vdisk_path, cli.size_in_bytes as u64, cli.block_size)?;
    };
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();

    let mount_point = PathBuf::from("/tmp/flemisfs2");
    let mount2 = mount_point.clone();

    let (sender, receiver) = mpsc::channel();
    // let fs = ferrix::simple_ext4::fs_in_fs::FSInFS::new("/tmp/storage".into(), true, false);
    thread::spawn(move || {
        let options = vec![MountOption::FSName("flemis".to_string())];
        let fs = ferrix::simple_ext4::fs::SimpleExt4FS::new(&cli.vdisk_path).unwrap();
        let mut session = Session::new(fs, &mount_point, &options).unwrap();
        let session_end = session.unmount_callable();
        sender.send(session_end).expect("failed to send");
        session.run()
    });
    let mut system = ferrix::simple_ext4::flemis_system::FlemisSystem::new(mount2)?;
    let segment = FerrixPromptSegment::WorkingDirectory;

    // system.make_dir(&MakeDirCommand {
    //     dir: "/tmp/asdf".into(),
    //     parents: false,
    // })?;
    ReplV2::run(&mut system, segment)?;

    let unmount = receiver.recv();
    unmount?.unmount()?;

    Ok(())
}
