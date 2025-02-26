use std::io::{Cursor, Seek};
use std::path::PathBuf;
use std::process::exit;

use anyhow::Result;
use tabled::Tabled;
use thiserror::Error;

use crate::complete_command::{
    CatCommand, ChangeDirCommand, ExitCommand, HeadCommand, ListCommand, MakeDirCommand,
    MoveCommand, RemoveCommand, SortCommand, TouchCommand,
};
use crate::error;
use crate::ext_arr::ExtArr;
use crate::fs::Filesystem;
use crate::mem::size::MB;
use crate::mem::FixedSizeMem;
use crate::sort::ExtSorter;
use crate::vdisk::VDiskSize;

pub const DEFAULT_MEM_SIZE: usize = MB * 2;

pub type Number = u16;

#[derive(Debug, Clone, Eq, PartialEq, Tabled)]
pub struct NodeInfo {
    pub name: String,
    pub size: VDiskSize,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ListCommandOutput {
    pub nodes: Vec<NodeInfo>,
    pub total_disk_space_in_bytes: VDiskSize,
    pub remaining_disk_space_in_bytes: VDiskSize,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum SystemError {
    #[error("No such file or directory")]
    NoSuchFileOrDirectory,
    #[error("Directory not found")]
    DirectoryNotFound,
    #[error("File already exists")]
    FileAlreadyExists,
    #[error("File is a directory")]
    IsDirectory,
    #[error("Too little files to concatenate")]
    TooLittleFiles,
    #[error("Start is greater than end")]
    StartGreaterThanEnd,
    #[error("End greater than file size")]
    EndGreaterThanFileSize,
}

/// A system that can execute commands
///
/// This trait is used to define the interface for a system that can execute commands.
pub trait System {
    /// Create a new file
    fn touch(&mut self, cmd: &TouchCommand) -> Result<()>;
    /// Move a file from one location to another
    fn mv(&mut self, cmd: &MoveCommand) -> Result<()>;
    /// Create a new directory
    fn make_dir(&mut self, cmd: &MakeDirCommand) -> Result<()>;
    /// Remove a file from the system
    fn remove(&mut self, cmd: &RemoveCommand) -> Result<()>;
    /// Read the first `n` lines of a file
    fn head(&self, cmd: &HeadCommand) -> Result<Vec<Number>>;
    /// List the contents of a directory
    fn list(&self, cmd: &ListCommand) -> Result<ListCommandOutput>;
    /// Sort the file and return the sorted file
    fn sort(&self, cmd: &SortCommand) -> Result<()>;
    /// Concatenate files together and returns the file that the content is concatenad
    fn cat(&self, cmd: &CatCommand) -> Result<PathBuf>;
    /// Exit the system with the given exit code
    fn exit(&self, cmd: &ExitCommand) -> Result<()>;
    fn chdir(&self, cmd: &ChangeDirCommand) -> Result<()> {
        todo!()
    }
}

pub struct BasicSystem<F>
where
    F: Filesystem,
{
    #[allow(dead_code)]
    file_system: F,
}

impl<F> BasicSystem<F>
where
    F: Filesystem,
{
    pub fn new(file_system: F) -> Self {
        Self { file_system }
    }
}

impl<F: Filesystem> System for BasicSystem<F> {
    fn touch(&mut self, cmd: &TouchCommand) -> Result<()> {
        todo!()
    }

    fn mv(&mut self, cmd: &MoveCommand) -> Result<()> {
        todo!()
    }

    fn make_dir(&mut self, cmd: &MakeDirCommand) -> Result<()> {
        todo!()
    }

    fn remove(&mut self, cmd: &RemoveCommand) -> Result<()> {
        todo!()
    }

    fn head(&self, cmd: &HeadCommand) -> Result<Vec<Number>> {
        todo!()
    }

    fn list(&self, cmd: &ListCommand) -> Result<ListCommandOutput> {
        todo!()
    }

    fn sort(&self, cmd: &SortCommand) -> Result<()> {
        let mut mem = FixedSizeMem::<DEFAULT_MEM_SIZE>::new();
        let mut arr = ExtArr::<Number, _>::new(Cursor::new(Vec::new()));

        // TODO: change this implementation to use the file system
        let v = [10, 5, 3, 7, 1, 9, 2, 6, 8, 4];

        arr.write(&v)?;
        arr.flush()?;
        arr.rewind()?;

        ExtSorter::sort(&mut arr, mem.as_mut(), |_| {
            Ok(ExtArr::new(Cursor::new(Vec::new())))
        })?;

        Ok(())
    }

    fn cat(&self, cmd: &CatCommand) -> Result<PathBuf> {
        todo!()
    }

    fn exit(&self, cmd: &ExitCommand) -> Result<()> {
        exit(cmd.code);
    }
}

impl<F: Filesystem + Clone> Clone for BasicSystem<F> {
    fn clone(&self) -> Self {
        Self {
            file_system: self.file_system.clone(),
        }
    }
}
