use std::path::PathBuf;

use clap::Parser;

use crate::{simple_ext4::DEFAULT_BLOCK_SIZE, vdisk::DEFAULT_SIZE_IN_BYTES};

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct FerrixCLI {
    /// The path to the virtual disk
    #[arg(short, long, default_value = "ferrix.vdisk")]
    pub vdisk_path: PathBuf,

    /// Size of the virtual disk in bytes
    #[arg(short, long, default_value_t = DEFAULT_SIZE_IN_BYTES)]
    pub size_in_bytes: u32,

    /// Block size
    #[arg(short, long, default_value_t = DEFAULT_BLOCK_SIZE)]
    pub block_size: u32,
}
