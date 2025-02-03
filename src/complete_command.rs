use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "")]
pub enum CompleteCommand {
    /// Creates a new file with a given amount of integers
    Touch {
        /// The file to create
        file: PathBuf,
        /// The number of integers to write to the file
        #[arg(short, long)]
        number_of_integers: u32,
    },
    /// Move a file from one location to another
    #[command(name = "mv")]
    Move {
        /// The file to move
        from: PathBuf,
        /// The destination of the file
        to: PathBuf,
    },
    /// Create a new directory
    #[command(name = "mkdir")]
    MakeDir {
        /// The directory to create
        dir: PathBuf,
        /// Create all parent directories if they don't exist
        #[arg(short, long)]
        parents: bool,
    },
    /// Remove a given file from the ferrix fs
    #[command(name = "rm")]
    Remove {
        /// The file or path to remove
        file_or_dir: PathBuf,
        /// If true, remove all files in the directory
        #[arg(short, long)]
        recursive: bool,
    },
    /// Read the content of a file and output it to stdout
    Head {
        /// The file to read
        file: PathBuf,
        /// The number of lines to start reading from the beginning
        #[arg(short, long, default_value = "0")]
        start: u32,
        /// The amount of lines to read
        #[arg(short, long)]
        #[arg(short, long, default_value = "10")]
        end: u32,
    },
    /// List directory contents with each file and dir with their size on the right size and system
    /// storage info at the bottom
    #[command(name = "ls")]
    List {
        /// The directory to list
        dir: Option<PathBuf>,
        /// If true, list all files including hidden files
        #[arg(short, long)]
        all: bool,
    },
    /// Sort a given inline integer vector file
    Sort {
        /// The file to sort
        file: PathBuf,
        /// If true, sort the file in reverse order
        #[arg(short, long)]
        inverse_order: bool,
    },
    /// Concat a given list of files into a stream and output it's content to a output file or
    /// fd
    Cat {
        /// The files to concatenate
        files: Vec<PathBuf>,
        /// The output file to write the concatenated content to
        #[arg(short, long)]
        output_file: Option<PathBuf>,
    },
    /// Exit the ferrix repl
    Exit {
        /// The exit code to return
        code: u32,
    },
}
