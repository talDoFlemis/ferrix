use std::ffi::OsString;

use clap::Parser;

#[derive(Debug, Parser)]
pub struct TouchCommand {
    /// The file to create
    pub file: OsString,
    /// The number of integers to write to the file
    #[arg(short, long)]
    pub number_of_integers: u32,
}

#[derive(Debug, Parser)]
pub struct MoveCommand {
    /// The node to move
    pub from: OsString,
    /// The destination of the file
    pub to: OsString,
}

#[derive(Debug, Parser)]
pub struct MakeDirCommand {
    /// The directory to create
    pub dir: OsString,
    /// Create all parent directories if they don't exist
    #[arg(short, long)]
    pub parents: bool,
}

#[derive(Debug, Parser)]
pub struct RemoveCommand {
    /// The file or path to remove
    pub file_or_dir: OsString,
    /// If true, remove all files in the directory
    #[arg(short, long)]
    pub recursive: bool,
}

#[derive(Debug, Parser)]
pub struct HeadCommand {
    /// The file to read
    pub file: OsString,
    /// The number of lines to start reading from the beginning
    #[arg(short, long, default_value = "0")]
    pub start: u32,
    /// The amount of lines to read
    #[arg(short, long, default_value = "10")]
    pub end: u32,
}

#[derive(Debug, Parser)]
pub struct ListCommand {
    /// The directory to list
    pub dir: Option<OsString>,
    /// If true, list all files including hidden files
    #[arg(short, long)]
    pub all: bool,
}

#[derive(Debug, Parser)]
pub struct ChangeDirCommand {
    /// The path to change working directory to
    pub path: Option<OsString>,
}

#[derive(Debug, Parser)]
pub struct SortCommand {
    /// The file to sort
    pub file: OsString,
    /// If true, sort the file in reverse order
    #[arg(short, long)]
    pub inverse_order: bool,
}

#[derive(Debug, Parser)]
pub struct CatCommand {
    /// The files to concatenate
    #[arg(required=true, num_args=2..)]
    pub files: Vec<OsString>,
    /// The output file to write the concatenated content to
    #[arg(short, long)]
    pub output_file: Option<OsString>,
}

#[derive(Debug, Parser)]
pub struct ExitCommand {
    /// The exit code to return
    pub code: i32,
}

#[derive(Debug, Parser)]
#[command(name = "")]
pub enum CompleteCommand {
    /// Creates a new file with a given amount of integers
    Touch(TouchCommand),
    /// Move a file from one location to another
    #[command(name = "mv")]
    Move(MoveCommand),
    /// Create a new directory
    #[command(name = "mkdir")]
    MakeDir(MakeDirCommand),
    /// Remove a given file from the ferrix fs
    #[command(name = "rm")]
    Remove(RemoveCommand),
    /// Read the content of a file and output it to stdout
    Head(HeadCommand),
    /// List directory contents with each file and dir with their size on the right size and system
    /// storage info at the bottom
    #[command(name = "ls")]
    List(ListCommand),
    /// Sort a given inline integer vector file
    Sort(SortCommand),
    /// Concat a given list of files into a stream and output it's content to a output file or
    /// fd
    Cat(CatCommand),
    /// Exit the ferrix repl
    Exit(ExitCommand),
    /// Change the current working directory
    #[command(name = "cd")]
    ChangeDir(ChangeDirCommand),
}
