use anyhow::Result;
use fuser::MountOption;
use std::path::PathBuf;

use crate::{system::System, vdisk::VDisk};

pub struct FlemisSystem {
    data_dir: PathBuf,
}

impl FlemisSystem {
    pub fn new(vdisk: PathBuf,  mountpoint: PathBuf) -> Result<Self> {
        let fs = super::fs::SimpleExt4FS::new(vdisk)?;
        let options = vec![
            MountOption::FSName("fuser".to_string()),
            MountOption::AutoUnmount,
        ];

        let data_dir = mountpoint.clone();

        std::thread::spawn(move || {
            fuser::mount2(fs, mountpoint.clone(), &options).expect("expected to mount");
        });

        Ok(Self { data_dir })
    }
}

impl System for FlemisSystem {
    fn touch(&mut self, cmd: &crate::complete_command::TouchCommand) -> miette::Result<()> {
        todo!()
    }

    fn mv(&mut self, cmd: &crate::complete_command::MoveCommand) -> miette::Result<()> {
        todo!()
    }

    fn make_dir(&mut self, cmd: &crate::complete_command::MakeDirCommand) -> miette::Result<()> {
        todo!()
    }

    fn remove(&mut self, cmd: &crate::complete_command::RemoveCommand) -> miette::Result<()> {
        todo!()
    }

    fn head(
        &self,
        cmd: &crate::complete_command::HeadCommand,
    ) -> miette::Result<Vec<crate::system::Number>> {
        todo!()
    }

    fn list(
        &self,
        cmd: &crate::complete_command::ListCommand,
    ) -> miette::Result<crate::system::ListCommandOutput> {
        todo!()
    }

    fn sort(&self, cmd: &crate::complete_command::SortCommand) -> miette::Result<()> {
        todo!()
    }

    fn cat(&self, cmd: &crate::complete_command::CatCommand) -> miette::Result<PathBuf> {
        todo!()
    }

    fn exit(&self, cmd: &crate::complete_command::ExitCommand) -> miette::Result<()> {
        todo!()
    }
}
