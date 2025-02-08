use std::path::PathBuf;
use std::process::exit;

use miette::Result;

use crate::complete_command::{
    CatCommand, ExitCommand, HeadCommand, ListCommand, MakeDirCommand, MoveCommand, RemoveCommand,
    SortCommand, TouchCommand,
};
use crate::fs::Filesystem;
use crate::vdisk::VDiskSize;

pub type Number = u16;

pub struct NodeInfo {
    pub name: String,
    pub size: VDiskSize,
}

pub struct ListCommandOutput {
    pub nodes: Vec<NodeInfo>,
    pub total_disk_space_in_bytes: VDiskSize,
    pub remaining_disk_space_in_bytes: VDiskSize,
}

/// A system that can execute commands
///
/// This trait is used to define the interface for a system that can execute commands.
pub trait System {
    /// Get the current working directory
    fn get_cwd(&self) -> Result<PathBuf>;
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
    fn get_cwd(&self) -> Result<PathBuf> {
        Ok(PathBuf::from("/path/to/cwd"))
    }

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
        todo!()
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
