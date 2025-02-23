pub mod fs;
pub mod mkfs;
pub mod system;
pub mod types;
use std::time::{self, SystemTime};

const FERRIX_MAGIC: u32 = 0x64627a;
const ROOT_INODE: u32 = 1;
const INODE_SIZE: u64 = 128;
pub const SUPERBLOCK_SIZE: u64 = 1024;
pub const DIRECT_POINTERS: u64 = 12;
pub const DEFAULT_BLOCK_SIZE: u32 = 4096;

#[inline]
pub fn calculate_checksum<S>(s: &S) -> u32
where
    S: serde::Serialize,
{
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&bincode::serialize(&s).unwrap());
    hasher.finalize()
}

#[inline]
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn block_group_size(blk_size: u32) -> u64 {
    let size = blk_size + // data bitmap
        blk_size + // inode bitmap
        inode_table_size(blk_size) +
        data_table_size(blk_size);
    size as u64
}

pub fn inode_table_size(blk_size: u32) -> u32 {
    blk_size * 8 * INODE_SIZE as u32
}

pub fn data_table_size(blk_size: u32) -> u32 {
    blk_size * blk_size * 8
}
