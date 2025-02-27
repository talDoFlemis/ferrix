use anyhow::{anyhow, bail};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::Path,
};

use super::{block_group_size, types::Superblock, SUPERBLOCK_SIZE};

pub fn make<P>(path: P, file_size: u64, blk_size: u32) -> anyhow::Result<Superblock>
where
    P: AsRef<Path>,
{
    let bg_size = block_group_size(blk_size);
    if file_size < (bg_size - 2 * blk_size as u64) {
        bail!("file size too small");
    }

    let groups = (file_size as f64 / bg_size as f64).ceil();
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut buf = BufWriter::new(&file);
    let uid = nix::unistd::geteuid().as_raw();
    let gid = nix::unistd::getegid().as_raw();
    let mut sb = Superblock::new(blk_size, groups as _, uid, gid);

    sb.serialize_into(&mut buf)?;

    buf.flush()?;

    file.set_len(SUPERBLOCK_SIZE + bg_size * groups as u64)?;

    Ok(sb)
}
