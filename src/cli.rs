use std::path::PathBuf;

use clap::Parser;

use crate::vdisk::DEFAULT_SIZE_IN_BYTES;

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct FerrixCLI {
    /// The path to the virtual disk
    #[arg(short, long, default_value = "ferrix.vdisk")]
    pub vdisk_path: PathBuf,

    /// Size of the virtual disk in bytes
    #[arg(short, long, default_value_t = DEFAULT_SIZE_IN_BYTES)]
    pub size_in_bytes: u32,
}
