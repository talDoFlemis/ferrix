use crate::{simple_ext4::mkfs::make, vdisk::VDisk};

use super::{
    fs_in_fs::check_access,
    types::{Directory, Group, Inode, Superblock},
    DIRECT_POINTERS, INODE_SIZE, ROOT_INODE, SUPERBLOCK_SIZE,
};
use anyhow::anyhow;
use fs::OpenOptions;
use fuser::{
    FileAttr, FileType, Filesystem, KernelConfig, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyWrite, Request
};
use io::{Cursor, SeekFrom};
use memmap::MmapMut;
use nix::{
    errno::Errno,
    sys::stat::{Mode, SFlag},
};
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, prelude::*},
    mem,
    path::Path,
};
use std::{
    path::PathBuf,
    time::{Duration, UNIX_EPOCH},
};
use tracing::debug;

pub type FSResult<T> = Result<T, nix::Error>;

#[derive(Debug, Default)]
pub struct SimpleExt4FS {
    pub sb: Option<Superblock>,
    pub mmap: Option<MmapMut>,
    pub groups: Option<Vec<Group>>,
}

impl SimpleExt4FS {
    pub fn new<P>(path: P) -> anyhow::Result<Self>
    where
        P: AsRef<Path>,
    {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mmap = unsafe { MmapMut::map_mut(&file)? };
        let mut cursor = Cursor::new(&mmap);

        let sb = Superblock::deserialize_from(&mut cursor)?;

        let groups = Group::deserialize_from(&mut cursor, sb.block_size, sb.groups as usize)?;

        let mut fs = Self {
            sb: Some(sb),
            groups: Some(groups),
            mmap: Some(mmap),
        };

        fs.create_root()?;

        Ok(fs)
    }

    pub fn create_root(&mut self) -> anyhow::Result<()> {
        let group = self.groups_mut().get_mut(0).unwrap();
        if group.has_inode(ROOT_INODE as _) {
            return Ok(());
        }

        let mut inode = Inode::new(self.superblock().block_size);
        inode.mode = SFlag::S_IFDIR.bits() | 0o777;
        inode.hard_links = 2;

        let dir = Directory::default();

        let index = self
            .allocate_inode()
            .ok_or_else(|| anyhow!("No space left for inodes"))?;
        assert_eq!(index, ROOT_INODE);

        inode.add_block(
            self.allocate_data_block()
                .ok_or_else(|| anyhow!("No space left for data"))?,
            0,
        )?;
        self.save_inode(inode, index)?;
        self.save_dir(dir, index)
    }

    fn save_inode(&mut self, mut inode: Inode, index: u32) -> anyhow::Result<()> {
        let offset = self.inode_seek_position(index);
        let buf = self.mmap_mut().as_mut();
        let mut cursor = Cursor::new(buf);
        debug!("save_inode: offset={}", offset);
        cursor.seek(SeekFrom::Start(offset))?;

        Ok(inode.serialize_into(&mut cursor)?)
    }

    fn save_dir(&mut self, mut dir: Directory, index: u32) -> anyhow::Result<()> {
        debug!("save_dir: index={}, dir={:?}", index, dir);
        let mut inode = self.find_inode(index)?;
        debug!("save_dir: inode={:?}", inode);
        inode.update_modified_at();
        self.save_inode(inode, index)?;

        let offset = self.data_block_seek_position(index);
        let buf = self.mmap_mut().as_mut();
        let mut cursor = Cursor::new(buf);
        cursor.seek(SeekFrom::Start(offset))?;

        Ok(dir.serialize_into(&mut cursor)?)
    }

    fn find_inode(&self, index: u32) -> FSResult<Inode> {
        debug!("find_inode: index={}", index);
        let (group_index, _bitmap_index) = self.inode_offsets(index);
        if !self
            .groups()
            .get(group_index as usize)
            .unwrap()
            .has_inode(index as usize)
        {
            return Err(Errno::ENOENT);
        }
        debug!("find_inode: group_index={}", group_index);

        let offset = self.inode_seek_position(index);
        debug!("find_inode: offset={}", offset);
        let buf = self.mmap();
        let mut cursor = Cursor::new(buf);
        cursor
            .seek(SeekFrom::Start(offset))
            .inspect_err(|e| debug!("seek failed {}", e))
            .unwrap();

        let inode = Inode::deserialize_from(cursor).map_err(|_e| Errno::EIO)?;
        debug!("find_inode: inode={:?}", inode);
        Ok(inode)
    }

    fn find_inode_from_path<P>(&self, path: P) -> FSResult<(Inode, u32)>
    where
        P: AsRef<Path>,
    {
        match path.as_ref().parent() {
            None => Ok((self.find_inode(ROOT_INODE)?, ROOT_INODE)),
            Some(parent) => {
                let (parent, _) = self.find_dir(parent)?;
                let index = parent.entry(
                    path.as_ref()
                        .file_name()
                        .ok_or(Errno::EINVAL)?
                        .to_os_string(),
                )?;
                Ok((self.find_inode(index)?, index))
            }
        }
    }

    fn find_dir<P>(&self, path: P) -> FSResult<(Directory, u32)>
    where
        P: AsRef<Path>,
    {
        let mut current = self.find_dir_from_inode(ROOT_INODE)?;
        let mut index = ROOT_INODE;
        for c in path.as_ref().components().skip(1) {
            index = current.entry(c)?;
            current = self.find_dir_from_inode(index)?;
        }

        Ok((current, index))
    }

    fn find_dir_from_inode(&self, index: u32) -> FSResult<Directory> {
        debug!("find_dir_from_inode: index={}", index);
        let inode = self.find_inode(index)?;
        if !inode.is_dir() {
            return Err(Errno::ENOTDIR);
        }

        // TODO: support more blocks
        let block = inode.direct_blocks[0];
        let (group_index, _) = self.data_block_offsets(index);
        if !self
            .groups()
            .get(group_index as usize)
            .unwrap()
            .has_data_block(block as usize)
        {
            return Err(Errno::ENOENT.into());
        }

        let mut cursor = Cursor::new(self.mmap().as_ref());
        cursor
            .seek(SeekFrom::Start(self.data_block_seek_position(block)))
            .map_err(|_| Errno::EIO)?;

        Directory::deserialize_from(cursor).map_err(|_| Errno::EIO.into())
    }

    fn find_data_block(
        &mut self,
        inode: &mut Inode,
        offset: u64,
        read: bool,
    ) -> FSResult<(u32, u32)> {
        let blk_size = self.superblock().block_size as u64;
        let index = offset / blk_size;

        let pointers_per_block = blk_size / mem::size_of::<u32>() as u64;

        let block = if index < DIRECT_POINTERS {
            inode.find_direct_block(index as usize)
        } else if index < (pointers_per_block + DIRECT_POINTERS) {
            self.find_indirect(
                inode.indirect_block,
                index - DIRECT_POINTERS,
                offset,
                pointers_per_block,
            )
            .map_err(|_| Errno::EIO)?
        } else if index
            < (pointers_per_block * pointers_per_block + pointers_per_block + DIRECT_POINTERS)
        {
            self.find_indirect(
                inode.double_indirect_block,
                index - DIRECT_POINTERS,
                offset,
                pointers_per_block,
            )
            .map_err(|_| Errno::EIO)?
        } else {
            return Err(Errno::ENOSPC.into());
        };

        if block != 0 {
            return Ok((block, ((index + 1) * blk_size - offset) as u32));
        }

        if read {
            return Err(Errno::EINVAL.into());
        }

        let mut block = self.allocate_data_block().ok_or_else(|| Errno::ENOSPC)?;
        if index < DIRECT_POINTERS {
            inode
                .add_block(block, index as usize)
                .map_err(|_| Errno::ENOSPC)?;
        } else if index < (pointers_per_block + DIRECT_POINTERS) {
            if inode.indirect_block == 0 {
                inode.indirect_block = block;
                self.write_data(&vec![0u8; blk_size as usize], 0, block)
                    .map_err(|_| Errno::EIO)?;
                block = self.allocate_data_block().ok_or_else(|| Errno::ENOSPC)?;
            }

            self.save_indirect(
                inode.indirect_block,
                block,
                index - DIRECT_POINTERS,
                pointers_per_block,
            )
            .map_err(|_| Errno::EIO)?;
        } else if index
            < (pointers_per_block * pointers_per_block + pointers_per_block + DIRECT_POINTERS)
        {
            if inode.double_indirect_block == 0 {
                inode.double_indirect_block = block;
                self.write_data(&vec![0u8; blk_size as usize], 0, block)
                    .map_err(|_| Errno::EIO)?;
                block = self.allocate_data_block().ok_or_else(|| Errno::ENOSPC)?;
            }

            let indirect_offset = (index - DIRECT_POINTERS) / pointers_per_block - 1;
            let indirect_block = match self
                .find_indirect(
                    inode.double_indirect_block,
                    indirect_offset,
                    0,
                    pointers_per_block,
                )
                .map_err(|_| Errno::EIO)?
            {
                0 => {
                    let indirect_block = block;
                    self.save_indirect(
                        inode.double_indirect_block,
                        block,
                        indirect_offset,
                        pointers_per_block,
                    )
                    .map_err(|_| Errno::EIO)?;
                    self.write_data(&vec![0u8; blk_size as usize], 0, block)
                        .map_err(|_| Errno::EIO)?;
                    block = self.allocate_data_block().ok_or_else(|| Errno::ENOSPC)?;
                    indirect_block
                }
                indirect_block => indirect_block,
            };

            self.save_indirect(
                indirect_block,
                block,
                (index - DIRECT_POINTERS) & (pointers_per_block - 1),
                pointers_per_block,
            )
            .map_err(|_| Errno::EIO)?;
        } else {
            return Err(Errno::ENOSPC.into());
        }

        Ok((block, blk_size as u32))
    }

    fn find_indirect(
        &self,
        pointer: u32,
        index: u64,
        offset: u64,
        pointers_per_block: u64,
    ) -> anyhow::Result<u32> {
        if pointer == 0 {
            return Ok(pointer);
        }

        let off = if index < pointers_per_block {
            index & (pointers_per_block - 1)
        } else {
            index / pointers_per_block - 1
        };

        let block = self.read_u32(off, pointer)?;

        if block == 0 || index < pointers_per_block {
            return Ok(block);
        }

        self.find_indirect(
            block,
            index & (pointers_per_block - 1),
            offset,
            pointers_per_block,
        )
    }

    fn save_indirect(
        &mut self,
        pointer: u32,
        block: u32,
        index: u64,
        pointers_per_block: u64,
    ) -> anyhow::Result<()> {
        assert_ne!(pointer, 0);
        let offset = index & (pointers_per_block - 1);

        if index < pointers_per_block {
            self.write_data(&block.to_le_bytes(), offset * 4, pointer)
                .map(|_| ())
        } else {
            let indirect_offset = index / pointers_per_block - 1;
            let new_pointer = self.read_u32(indirect_offset, pointer)?;
            self.save_indirect(new_pointer, block, offset, pointers_per_block)
        }
    }

    // (group_block_index, bitmap_index)
    fn inode_offsets(&self, index: u32) -> (u64, u64) {
        let inodes_per_group = self.superblock().data_blocks_per_group as u64;
        let inode_bg = (index as u64 - 1) / inodes_per_group;
        let bitmap_index = (index as u64 - 1) & (inodes_per_group - 1);
        (inode_bg, bitmap_index)
    }

    fn inode_seek_position(&self, index: u32) -> u64 {
        let (group_index, bitmap_index) = self.inode_offsets(index);
        let block_size = self.superblock().block_size;
        group_index * super::block_group_size(block_size)
            + 2 * block_size as u64
            + bitmap_index * INODE_SIZE
            + SUPERBLOCK_SIZE
    }

    fn data_block_offsets(&self, index: u32) -> (u64, u64) {
        let data_blocks_per_group = self.superblock().data_blocks_per_group as u64;
        let group_index = (index as u64 - 1) / data_blocks_per_group;
        let block_index = (index as u64 - 1) & (data_blocks_per_group - 1);

        (group_index, block_index)
    }

    fn data_block_seek_position(&self, index: u32) -> u64 {
        let (group_index, block_index) = self.data_block_offsets(index);

        let block_size = self.superblock().block_size;
        group_index * super::block_group_size(block_size)
            + 2 * block_size as u64
            + self.superblock().data_blocks_per_group as u64 * INODE_SIZE
            + SUPERBLOCK_SIZE
            + block_size as u64 * block_index
    }

    fn allocate_inode(&mut self) -> Option<u32> {
        // TODO: handle when group has run out of space
        let group_index = self.groups().iter().position(|g| g.free_inodes() > 0)?;
        self.superblock_mut().free_inodes -= 1;
        let group = self.groups_mut().get_mut(group_index).unwrap();

        let index = group.allocate_inode()?;
        Some(index as u32 + group_index as u32 * self.superblock().data_blocks_per_group)
    }

    fn allocate_data_block(&mut self) -> Option<u32> {
        // TODO: handle when group has run out of space
        let group_index = self
            .groups()
            .iter()
            .position(|g| g.free_data_blocks() > 0)?;

        self.superblock_mut().free_blocks -= 1;
        let group = self.groups_mut().get_mut(group_index).unwrap();

        let index = group.allocate_data_block()?;
        Some(index as u32 + group_index as u32 * self.superblock().data_blocks_per_group)
    }

    fn release_data_blocks(&mut self, blocks: &[u32]) {
        for block in blocks {
            let (group_index, block_index) = self.data_block_offsets(*block);
            // TODO: release multiple blocks from the same group in a single call
            self.groups_mut()
                .get_mut(group_index as usize)
                .unwrap()
                .release_data_block(1 + block_index as usize);
        }
        self.superblock_mut().free_blocks += blocks.len() as u32;
    }

    fn release_inode(&mut self, index: u32) {
        let (group_index, _) = self.inode_offsets(index);
        self.groups_mut()
            .get_mut(group_index as usize)
            .unwrap()
            .release_inode(index as usize);
        self.superblock_mut().free_inodes += 1;
    }

    fn release_indirect_block(&mut self, block: u32) -> anyhow::Result<()> {
        let blocks = self.read_indirect_block(block)?;
        self.release_data_blocks(&blocks);
        Ok(())
    }

    fn release_double_indirect_block(&mut self, block: u32) -> anyhow::Result<()> {
        let pointers_per_block = self.superblock().block_size as usize / 4;
        let indirect_blocks = self.read_indirect_block(block)?;
        let mut blocks = Vec::with_capacity(indirect_blocks.len() * pointers_per_block);
        for b in indirect_blocks.iter().filter(|x| **x != 0) {
            blocks.append(&mut self.read_indirect_block(*b)?);
        }

        self.release_data_blocks(&indirect_blocks);
        self.release_data_blocks(&blocks);

        Ok(())
    }

    fn write_data(&mut self, data: &[u8], offset: u64, block_index: u32) -> anyhow::Result<usize> {
        let block_offset = self.data_block_seek_position(block_index);

        let buf = self.mmap_mut().as_mut();
        let mut cursor = Cursor::new(buf);
        cursor.seek(SeekFrom::Start(block_offset + offset))?;
        Ok(cursor.write(data)?)
    }

    fn read_data(&self, data: &mut [u8], offset: u64, block_index: u32) -> anyhow::Result<usize> {
        let block_offset = self.data_block_seek_position(block_index);
        let buf = self.mmap().as_ref();
        let mut cursor = Cursor::new(buf);
        cursor.seek(SeekFrom::Start(block_offset + offset))?;

        cursor.read_exact(data)?;

        Ok(data.len())
    }

    fn read_u32(&self, offset: u64, block_index: u32) -> anyhow::Result<u32> {
        let mut data = [0u8; 4];
        self.read_data(&mut data, offset * 4, block_index)?;
        Ok(u32::from_le_bytes(data))
    }

    fn read_indirect_block(&mut self, block: u32) -> anyhow::Result<Vec<u32>> {
        let pointers_per_block = self.superblock().block_size as usize / 4;
        let mut vec = Vec::with_capacity(pointers_per_block);
        for i in 0..pointers_per_block {
            let b = self.read_u32(i as u64, block)?;
            if b != 0 {
                vec.push(b);
            }
        }

        Ok(vec)
    }

    fn groups(&self) -> &[Group] {
        self.groups
            .as_ref()
            .expect("expected to get reference to group")
    }

    fn groups_mut(&mut self) -> &mut [Group] {
        self.groups.as_mut().unwrap()
    }

    fn superblock(&self) -> &Superblock {
        self.sb.as_ref().unwrap()
    }

    fn superblock_mut(&mut self) -> &mut Superblock {
        self.sb.as_mut().unwrap()
    }

    fn mmap(&self) -> &MmapMut {
        self.mmap.as_ref().unwrap()
    }

    fn mmap_mut(&mut self) -> &mut MmapMut {
        self.mmap.as_mut().unwrap()
    }
}

impl Filesystem for SimpleExt4FS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        debug!("lookup: parent={}, name={:?}", parent, name);
        match self.find_dir_from_inode(parent as u32) {
            Ok(dir) => match dir.entry(name) {
                Ok(index) => match self.find_inode(index) {
                    Ok(inode) => {
                        reply.entry(&Duration::from_secs(1), &inode.to_attr(index), 0);
                    }
                    Err(e) => reply.error(e as i32),
                },
                Err(e) => reply.error(e as i32),
            },
            Err(e) => reply.error(e as i32),
        }
    }

    fn statfs(&mut self, _req: &Request<'_>, _ino: u64, reply: fuser::ReplyStatfs) {
        let sb = self.superblock();
        reply.statfs(
            sb.block_count.into(),
            sb.free_blocks.into(),
            sb.free_blocks.into(),
            sb.inode_count.into(),
            sb.free_inodes.into(),
            sb.block_size,
            255,
            sb.block_size,
        );
    }

    fn getattr(&mut self, _req: &Request, ino: u64, fh: Option<u64>, reply: ReplyAttr) {
        debug!("getattr: ino={}, fh={:?}", ino, fh);
        match self.find_inode(ino as u32) {
            Ok(inode) => {
                reply.attr(&Duration::from_secs(1), &inode.to_attr(ino as u32));
            }
            Err(e) => reply.error(e as i32),
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        debug!("readdir: ino={}, fh={}, offset={}", ino, fh, offset);
        match self.find_dir_from_inode(ino as u32) {
            Ok(dir) => {
                let mut entries: Vec<(OsString, u64, FileType)> = vec![
                    (OsString::from("."), ino, FileType::Directory),
                    (OsString::from(".."), 1, FileType::Directory),
                ];

                for (name, index) in dir.entries {
                    if let Ok(inode) = self.find_inode(index) {
                        let file_type = if inode.is_dir() {
                            FileType::Directory
                        } else {
                            FileType::RegularFile
                        };
                        entries.push((name, index as u64, file_type));
                    }
                }

                for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
                    if reply.add(entry.1, (i + 1) as i64, entry.2, entry.0) {
                        break;
                    }
                }
                reply.ok();
            }
            Err(e) => reply.error(e as i32),
        }
    }

    fn create(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
        flags: i32,
        reply: ReplyCreate,
    ) {
        debug!(
            "create: parent={}, name={:?}, mode={:#o}, umask={:#o}, flags={:#x}",
            parent, name, mode, umask, flags
        );
        let index = match self.allocate_inode() {
            Some(index) => index,
            None => {
                reply.error(libc::ENOSPC);
                return;
            }
        };

        let mut inode = Inode::new(self.superblock().block_size);
        inode.mode = mode;
        inode.user_id = self.superblock().uid;
        inode.group_id = self.superblock().gid;

        match self.find_dir_from_inode(parent as u32) {
            Ok(mut parent_dir) => {
                parent_dir.entries.insert(name.to_owned(), index);
                if let Err(_) = self.save_inode(inode, index) {
                    reply.error(libc::EIO);
                    return;
                }
                if let Err(_) = self.save_dir(parent_dir, parent as u32) {
                    reply.error(libc::EIO);
                    return;
                }
                match self.find_inode(index) {
                    Ok(created_inode) => {
                        reply.created(
                            &Duration::from_secs(1),
                            &created_inode.to_attr(index),
                            0,
                            0,
                            0,
                        );
                    }
                    Err(e) => reply.error(e as i32),
                }
            }
            Err(e) => reply.error(e as i32),
        }
    }

    fn write(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        write_flags: u32,
        flags: i32,
        lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        debug!(
            "write: ino={}, fh={}, offset={}, data.len={}, write_flags={:#x}, flags={:#x}, lock_owner={:?}",
            ino, fh, offset, data.len(), write_flags, flags, lock_owner
        );
        let mut inode = match self.find_inode(ino as u32) {
            Ok(inode) => inode,
            Err(e) => {
                reply.error(e as i32);
                return;
            }
        };

        let mut total_wrote = 0;
        let overwrite = inode.size > offset as u64;
        let mut current_offset = offset as u64;
        let blk_size = self.superblock().block_size;

        while total_wrote != data.len() {
            let direct_block_index = current_offset / blk_size as u64;
            let (block_index, space_left) =
                match self.find_data_block(&mut inode, current_offset, false) {
                    Ok(result) => result,
                    Err(e) => {
                        reply.error(e as i32);
                        return;
                    }
                };

            let max_write_len = data.len().min(space_left as usize);
            let offset_in_block = if total_wrote != 0 {
                0
            } else {
                current_offset - direct_block_index * blk_size as u64
            };

            let wrote = match self.write_data(
                &data[total_wrote..data.len().min(max_write_len + total_wrote)],
                offset_in_block,
                block_index,
            ) {
                Ok(wrote) => wrote,
                Err(_) => {
                    reply.error(libc::EIO);
                    return;
                }
            };

            total_wrote += wrote;
            current_offset += wrote as u64;
        }

        inode.update_modified_at();
        if overwrite {
            inode.adjust_size(total_wrote as u64);
        } else {
            inode.increment_size(total_wrote as u64);
        }

        if let Err(_) = self.save_inode(inode, ino as u32) {
            reply.error(libc::EIO);
            return;
        }

        debug!("wrote {} bytes", total_wrote);

        reply.written(total_wrote as u32);
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        flags: i32,
        lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        debug!(
            "read: ino={}, fh={}, offset={}, size={}, flags={:#x}, lock_owner={:?}",
            ino, fh, offset, size, flags, lock_owner
        );
        let mut inode = match self.find_inode(ino as u32) {
            Ok(inode) => inode,
            Err(e) => {
                reply.error(e as i32);
                return;
            }
        };

        let mut data = vec![0u8; size as usize];
        let mut total_read = 0;
        let mut current_offset = offset as u64;
        let blk_size = self.superblock().block_size;

        let should_read = (size as usize).min(inode.size as usize);
        while total_read != should_read {
            let direct_block_index = current_offset / blk_size as u64;
            let (block_index, space_left) =
                match self.find_data_block(&mut inode, current_offset, true) {
                    Ok(result) => result,
                    Err(e) => {
                        reply.error(e as i32);
                        return;
                    }
                };

            let max_read_len = data.len().min(space_left as usize);
            let max_read_len = data.len().min(max_read_len + total_read);
            let offset_in_block = if total_read != 0 {
                0
            } else {
                current_offset - direct_block_index * blk_size as u64
            };

            let read = match self.read_data(
                &mut data[total_read..max_read_len],
                offset_in_block,
                block_index,
            ) {
                Ok(read) => read,
                Err(_) => {
                    reply.error(libc::EIO);
                    return;
                }
            };

            total_read += read;
            current_offset += read as u64;
        }

        inode.update_accessed_at();
        if let Err(_) = self.save_inode(inode, ino as u32) {
            reply.error(libc::EIO);
            return;
        }

        reply.data(&data[..total_read]);
    }

    fn access(&mut self, req: &Request<'_>, ino: u64, mask: i32, reply: ReplyEmpty) {
        match self.find_inode(ino as u32) {
            Ok(attr) => {
                if check_access(
                    attr.user_id,
                    attr.group_id,
                    attr.mode.try_into().unwrap(),
                    req.uid(),
                    req.gid(),
                    mask,
                ) {
                    reply.ok();
                } else {
                    reply.error(libc::EACCES);
                }
            }
            Err(error_code) => reply.error(error_code as i32),
        }
    }

    fn mkdir(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
        reply: ReplyEntry,
    ) {
        debug!(
            "mkdir: parent={}, name={:?}, mode={:#o}, umask={:#o}",
            parent, name, mode, umask
        );
        let index = match self.allocate_inode() {
            Some(index) => index,
            None => {
                reply.error(libc::ENOSPC);
                return;
            }
        };
        debug!("mkdir: index={}", index);

        match self.find_dir_from_inode(parent as u32) {
            Ok(mut parent_dir) => {
                parent_dir.entries.insert(name.to_owned(), index);

                let mut inode = Inode::new(self.superblock().block_size);
                inode.mode = SFlag::S_IFDIR.bits() | mode;
                inode.hard_links = 2;
                inode.user_id = self.superblock().uid;
                inode.group_id = self.superblock().gid;

                let data_block_index = match self.allocate_data_block() {
                    Some(index) => index,
                    None => {
                        reply.error(libc::ENOSPC);
                        return;
                    }
                };

                let dir = Directory::default();

                if let Err(_) = inode.add_block(data_block_index, 0) {
                    reply.error(libc::EIO);
                    return;
                }

                if let Err(_) = self.save_inode(inode, index) {
                    reply.error(libc::EIO);
                    return;
                }

                if let Err(_) = self.save_dir(dir, data_block_index) {
                    reply.error(libc::EIO);
                    return;
                }

                if let Err(e) = self.save_dir(parent_dir, parent as u32) {
                    println!("here3 {:?}", e);
                    reply.error(libc::EIO);
                    return;
                }
                println!("here4");

                match self.find_inode(index) {
                    Ok(created_inode) => {
                        reply.entry(&Duration::from_secs(1), &created_inode.to_attr(index), 0);
                    }
                    Err(e) => reply.error(e as i32),
                }
            }
            Err(e) => reply.error(e as i32),
        }
    }

    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        debug!("unlink: parent={}, name={:?}", parent, name);
        match self.find_dir_from_inode(parent as u32) {
            Ok(mut parent_dir) => match parent_dir.entries.remove(name) {
                Some(index) => match self.find_inode(index) {
                    Ok(inode) => {
                        self.release_data_blocks(&inode.direct_blocks());
                        if inode.indirect_block != 0 {
                            if let Err(_) = self.release_indirect_block(inode.indirect_block) {
                                reply.error(libc::EIO);
                                return;
                            }
                        }
                        if inode.double_indirect_block != 0 {
                            if let Err(_) =
                                self.release_double_indirect_block(inode.double_indirect_block)
                            {
                                reply.error(libc::EIO);
                                return;
                            }
                        }
                        if let Err(_) = self.save_dir(parent_dir, parent as u32) {
                            reply.error(libc::EIO);
                            return;
                        }
                        self.release_inode(index);
                        reply.ok();
                    }
                    Err(e) => reply.error(e as i32),
                },
                None => reply.error(libc::ENOENT),
            },
            Err(e) => reply.error(e as i32),
        }
    }

    fn init(&mut self, _req: &Request<'_>, config: &mut KernelConfig) -> Result<(), libc::c_int> {
        debug!("init: kernel_config={:?}", config);
        let sb = self.superblock_mut();
        sb.update_last_mounted_at();
        sb.update_modified_at();

        Ok(())
    }

    fn destroy(&mut self) {
        debug!("destroy called");
        let mut mmap = mem::replace(&mut self.mmap, None).unwrap();
        let buf = mmap.as_mut();
        let mut cursor = Cursor::new(buf);

        if let Err(e) = self.superblock_mut().serialize_into(&mut cursor) {
            println!("inside superblock {e:?}");
            return;
        }

        if let Err(e) = Group::serialize_into(&mut cursor, self.groups()) {
            println!("inside group {e:?}");
            return;
        }

        debug!("flushing mmap");
        if let Err(e) = mmap.flush() {
            println!("inside flush {e:?}");
            return;
        }
        debug!("destroyed");
    }

    fn open(&mut self, req: &Request, inode: u64, flags: i32, reply: ReplyOpen) {
        let (access_mask, read, write) = match flags & libc::O_ACCMODE {
            libc::O_RDONLY => {
                // Behavior is undefined, but most filesystems return EACCES
                if flags & libc::O_TRUNC != 0 {
                    reply.error(libc::EACCES);
                    return;
                }
                if flags & FMODE_EXEC != 0 {
                    // Open is from internal exec syscall
                    (libc::X_OK, true, false)
                } else {
                    (libc::R_OK, true, false)
                }
            }
            libc::O_WRONLY => (libc::W_OK, false, true),
            libc::O_RDWR => (libc::R_OK | libc::W_OK, true, true),
            // Exactly one access mode flag must be specified
            _ => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        match self.find_inode(inode as u32) {
            Ok(mut attr) => {
                if check_access(
                    attr.uid,
                    attr.gid,
                    attr.mode,
                    req.uid(),
                    req.gid(),
                    access_mask,
                ) {
                    attr.open_file_handles += 1;
                    self.write_inode(&attr);
                    let open_flags = 0;
                    reply.opened(self.allocate_next_file_handle(read, write), open_flags);
                } else {
                    reply.error(libc::EACCES);
                }
                return;
            }
            Err(error_code) => reply.error(error_code as i32),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        simple_ext4::mkfs,
        simple_ext4::{types::Superblock, INODE_SIZE, ROOT_INODE},
    };
    use fuser::{
        FileAttr, Filesystem, Reply, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, ReplyWrite,
        Request,
    };
    use std::time::{Duration, UNIX_EPOCH};
    use std::{ffi::OsString, path::PathBuf};

    const BLOCK_SIZE: u32 = 128;

    #[test]
    fn inode_offsets() {
        let mut fs = SimpleExt4FS::default();
        fs.sb = Some(Superblock::new(1024, 3, 0, 0));
        fs.superblock_mut().data_blocks_per_group = 1024 * 8;

        let (group_index, offset) = fs.inode_offsets(1);
        assert_eq!(group_index, 0);
        assert_eq!(offset, 0);

        let (group_index, offset) = fs.inode_offsets(1024 * 8);
        assert_eq!(group_index, 0);
        assert_eq!(offset, 8191);

        let (group_index, offset) = fs.inode_offsets(1024 * 8 - 1);
        assert_eq!(group_index, 0);
        assert_eq!(offset, 8190);

        let (group_index, offset) = fs.inode_offsets(2 * 1024 * 8 - 1);
        assert_eq!(group_index, 1);
        assert_eq!(offset, 8190);
    }

    #[test]
    fn inode_seek_position() {
        let mut fs = SimpleExt4FS::default();
        fs.sb = Some(Superblock::new(1024, 3, 0, 0));
        fs.superblock_mut().data_blocks_per_group = 1024 * 8;

        let offset = fs.inode_seek_position(1);
        assert_eq!(3072, offset);

        let offset = fs.inode_seek_position(2);
        assert_eq!(3072 + INODE_SIZE, offset);

        let offset = fs.inode_seek_position(8192);
        assert_eq!(3072 + 8191 * INODE_SIZE, offset); // superblock + data bitmap + inode bitmap + 8191 inodes

        let offset = fs.inode_seek_position(8193);
        assert_eq!(3072 + 8192 * INODE_SIZE + 1024 * 1024 * 8 + 2048, offset); // superblock + data bitmap + inode bitmap + inode table + data blocks + data bitmap + inode bitmap
    }

    // #[test]
    // fn new_fs() -> anyhow::Result<()> {
    //     let tmp_file = make_fs("new_fs")?;
    //     let fs = SimpleExt4FS::new(&tmp_file)?;
    //     let inode = fs.find_inode(ROOT_INODE)?;
    //
    //     assert_eq!(inode.mode, SFlag::S_IFDIR.bits() | 0o777);
    //     assert_eq!(inode.hard_links, 2);
    //
    //     assert!(fs.groups().first().unwrap().has_inode(ROOT_INODE as _));
    //     assert!(fs.groups().first().unwrap().has_data_block(ROOT_INODE as _));
    //
    //     assert_eq!(fs.superblock().groups, fs.groups().len() as u32);
    //     assert_eq!(fs.superblock().free_inodes, BLOCK_SIZE * 8 - 1);
    //     assert_eq!(fs.superblock().free_blocks, BLOCK_SIZE * 8 - 1);
    //
    //     Ok(std::fs::remove_file(&tmp_file)?)
    // }

    #[test]
    fn init_destroy() -> anyhow::Result<()> {
        let tmp_file = make_fs("init_destroy")?;
        let fs = SimpleExt4FS::new(&tmp_file)?;
        let tmp_dir = tempfile::tempdir()?.path().join("init_destroy");
        fs::create_dir_all(&tmp_dir)?;

        assert_eq!(fs.superblock().last_mounted_at, None);

        let fs = SimpleExt4FS::new(&tmp_file)?;

        assert_ne!(fs.superblock().last_mounted_at, None);
        assert_eq!(fs.superblock().free_inodes, BLOCK_SIZE * 8 - 1);
        assert_eq!(fs.superblock().free_blocks, BLOCK_SIZE * 8 - 1);

        Ok(std::fs::remove_file(&tmp_file)?)
    }

    #[test]
    fn data_block_seek_position() {
        let mut fs = SimpleExt4FS::default();
        let block_size = 1024;
        fs.sb = Some(Superblock::new(block_size, 3, 0, 0));
        fs.superblock_mut().data_blocks_per_group = block_size as u32 * 8;

        let prefix = SUPERBLOCK_SIZE + 2 * block_size as u64 + block_size as u64 * INODE_SIZE * 8;
        let offset = fs.data_block_seek_position(1);
        assert_eq!(prefix, offset);

        let offset = fs.data_block_seek_position(2);
        assert_eq!(prefix + block_size as u64, offset);

        let offset = fs.data_block_seek_position(8192);
        assert_eq!(prefix + 8191 * block_size as u64, offset);

        let offset = fs.data_block_seek_position(8193);
        assert_eq!(
            2 * prefix - SUPERBLOCK_SIZE + (block_size * block_size) as u64 * 8,
            offset
        );
    }

    #[test]
    fn save_dir() -> anyhow::Result<()> {
        let tmp_file = make_fs("save_dir")?;
        let fs = SimpleExt4FS::new(&tmp_file)?;
        let dir = fs.find_dir_from_inode(ROOT_INODE)?;

        assert_eq!(dir.entries.len(), 0);

        Ok(std::fs::remove_file(&tmp_file)?)
    }

    #[test]
    fn find_dir() -> anyhow::Result<()> {
        let tmp_file = make_fs("find_dir")?;
        let fs = SimpleExt4FS::new(&tmp_file)?;

        assert_eq!(fs.find_dir("/not-a-dir").err(), Some(Errno::ENOENT));

        Ok(std::fs::remove_file(&tmp_file)?)
    }

    // #[test]
    // fn read_dir() -> anyhow::Result<()> {
    //     let tmp_file = make_fs("read_dir")?;
    //     let mut fs = SimpleExt4FS::new(&tmp_file)?;
    //     let inode = fs.find_inode(ROOT_INODE)?;
    //
    //     assert_ne!(inode.accessed_at, UNIX_EPOCH);
    //
    //     struct TestReplyDirectory {
    //         entries: Vec<(u64, i64, FileType, String)>,
    //     }
    //
    //     impl ReplyDirectory for TestReplyDirectory {
    //         fn add(&mut self, ino: u64, offset: i64, kind: FileType, name: &OsStr) -> bool {
    //             self.entries
    //                 .push((ino, offset, kind, name.to_string_lossy().into_owned()));
    //             false
    //         }
    //         fn ok(&mut self) {}
    //         fn error(&mut self, _err: i32) {}
    //     }
    //
    //     let mut reply = TestReplyDirectory {
    //         entries: Vec::new(),
    //     };
    //     fs.readdir(&Request::new(0), ROOT_INODE, 0, 0, &mut reply);
    //     assert_eq!(reply.entries.len(), 2); // . and ..
    //
    //     let mut reply_create = ReplyCreate::new(0, None);
    //     fs.create(
    //         &Request::new(0),
    //         ROOT_INODE,
    //         "foo.txt",
    //         0o007,
    //         0,
    //         0,
    //         &mut reply_create,
    //     );
    //
    //     fs.create(
    //         &Request::new(0),
    //         ROOT_INODE,
    //         "bar.txt",
    //         0o700,
    //         0,
    //         0,
    //         &mut reply_create,
    //     );
    //
    //     assert_eq!(fs.superblock().free_inodes, BLOCK_SIZE * 8 - 3);
    //
    //     let mut reply = TestReplyDirectory {
    //         entries: Vec::new(),
    //     };
    //     fs.readdir(&Request::new(0), ROOT_INODE, 0, 0, &mut reply);
    //     assert_eq!(reply.entries.len(), 4); // . and .. plus 2 files
    //
    //     // Find bar.txt entry
    //     let bar = reply.entries.iter().find(|e| e.3 == "bar.txt").unwrap();
    //     assert_eq!(bar.0, 3); // inode number
    //     assert_eq!(bar.2, FileType::RegularFile);
    //
    //     // Find foo.txt entry
    //     let foo = reply.entries.iter().find(|e| e.3 == "foo.txt").unwrap();
    //     assert_eq!(foo.0, 2); // inode number
    //     assert_eq!(foo.2, FileType::RegularFile);
    //
    //     Ok(std::fs::remove_file(&tmp_file)?)
    // }

    // #[test]
    // fn open() -> anyhow::Result<()> {
    //     let tmp_file = make_fs("open")?;
    //     let mut fs = SimpleExt4FS::new(&tmp_file)?;
    //
    //     let mut reply_create = ReplyCreate::new(0, sender)
    //     fs.open(req, ino, flags, reply);
    //
    //
    //     let mut reply_create = ReplyCreate::new(0, None);
    //     fs.create(
    //         &Request::new(0),
    //         ROOT_INODE,
    //         "bar.txt",
    //         0o700,
    //         0,
    //         0,
    //         &mut reply_create,
    //     );
    //
    //     let mut reply_lookup = ReplyEntry::new(0, None);
    //     fs.lookup(&Request::new(0), ROOT_INODE, "bar.txt", &mut reply_lookup);
    //     assert_eq!(reply_lookup.error(0), ());
    //
    //     Ok(std::fs::remove_file(&tmp_file)?)
    // }

    // #[test]
    // fn write() -> anyhow::Result<()> {
    //     let tmp_file = make_fs("write")?;
    //     let mut fs = SimpleExt4FS::new(&tmp_file)?;
    //
    //     let mut open_fi = fuse_rs::fs::OpenFileInfo::default();
    //     fs.create(
    //         Path::new("/bar.txt"),
    //         nix::sys::stat::Mode::S_IRWXU,
    //         &mut open_fi,
    //     )?;
    //     let handle = open_fi.handle().unwrap();
    //
    //     fs.open(Path::new("/bar.txt"), &mut open_fi)?;
    //     let mut file_info = fuse_rs::fs::FileInfo::default();
    //     file_info.set_handle(handle);
    //
    //     let mut write_file_info = fuse_rs::fs::WriteFileInfo::from_file_info(file_info);
    //     let buf = std::iter::repeat(3).take(125).collect::<Vec<u8>>();
    //
    //     let wrote = fs.write(Path::new("/ignored.txt"), &buf, 0, &mut write_file_info)?;
    //     assert_eq!(wrote, 125);
    //
    //     let stat = fs.metadata(Path::new("/bar.txt"))?;
    //     assert_eq!(stat.st_size, 125);
    //     assert_eq!(stat.st_blocks, 1);
    //
    //     assert_eq!(read(&mut fs, 125, 0, handle)?, buf);
    //
    //     // Overwriting with larger buffer
    //     let buf = std::iter::repeat(4).take(126).collect::<Vec<u8>>();
    //     let wrote = fs.write(Path::new("/ignored.txt"), &buf, 0, &mut write_file_info)?;
    //     assert_eq!(wrote, 126);
    //
    //     let stat = fs.metadata(Path::new("/bar.txt"))?;
    //     assert_eq!(stat.st_size, 126);
    //     assert_eq!(stat.st_blocks, 1); // 126 / 512 + 1
    //
    //     assert_eq!(read(&mut fs, 126, 0, handle)?, buf);
    //
    //     let inode = fs.find_inode(2)?;
    //     assert_eq!(inode.direct_blocks[0], 2);
    //
    //     let modified_at = inode.modified_at;
    //     let changed_at = inode.changed_at;
    //
    //     // Overwriting with shorter buffer
    //     let buf = std::iter::repeat(5).take(120).collect::<Vec<u8>>();
    //     let wrote = fs.write(Path::new("/ignored.txt"), &buf, 0, &mut write_file_info)?;
    //     assert_eq!(wrote, 120);
    //
    //     let stat = fs.metadata(Path::new("/bar.txt"))?;
    //     assert_eq!(stat.st_size, 126);
    //     assert_eq!(stat.st_blocks, 1); // 126 / 512 + 1
    //
    //     assert_eq!(read(&mut fs, 120, 0, handle)?, buf);
    //     assert_eq!(
    //         read(&mut fs, 6, 120, handle)?,
    //         std::iter::repeat(4).take(6).collect::<Vec<u8>>()
    //     );
    //
    //     let inode = fs.find_inode(2)?;
    //     assert_eq!(inode.direct_blocks[0], 2);
    //
    //     // Appending
    //     let buf = std::iter::repeat(7).take(125).collect::<Vec<u8>>();
    //     let wrote = fs.write(Path::new("/ignored.txt"), &buf, 126, &mut write_file_info)?;
    //     assert_eq!(wrote, 125);
    //
    //     let stat = fs.metadata(Path::new("/bar.txt"))?;
    //     assert_eq!(stat.st_size, 251);
    //     assert_eq!(stat.st_blocks, 1); // 251 / 512 + 1
    //
    //     let inode = fs.find_inode(2)?;
    //     assert_eq!(inode.direct_blocks[0], 2);
    //     assert_eq!(inode.direct_blocks[1], 3);
    //
    //     assert_eq!(
    //         read(&mut fs, 120, 0, handle)?,
    //         std::iter::repeat(5).take(120).collect::<Vec<u8>>()
    //     );
    //     assert_eq!(
    //         read(&mut fs, 6, 120, handle)?,
    //         std::iter::repeat(4).take(6).collect::<Vec<u8>>()
    //     );
    //     assert_eq!(read(&mut fs, 125, 126, handle)?, buf);
    //
    //     // Appending again
    //     let buf = std::iter::repeat(8).take(125).collect::<Vec<u8>>();
    //     let wrote = fs.write(Path::new("/ignored.txt"), &buf, 251, &mut write_file_info)?;
    //     assert_eq!(wrote, 125);
    //
    //     let stat = fs.metadata(Path::new("/bar.txt"))?;
    //     assert_eq!(stat.st_size, 376);
    //     assert_eq!(stat.st_blocks, 1); // 376 / 512 + 1
    //
    //     let inode = fs.find_inode(2)?;
    //     assert_eq!(inode.direct_blocks[0], 2);
    //     assert_eq!(inode.direct_blocks[1], 3);
    //     assert_eq!(inode.direct_blocks[2], 4);
    //
    //     assert_eq!(
    //         read(&mut fs, 120, 0, handle)?,
    //         std::iter::repeat(5).take(120).collect::<Vec<u8>>()
    //     );
    //     assert_eq!(
    //         read(&mut fs, 6, 120, handle)?,
    //         std::iter::repeat(4).take(6).collect::<Vec<u8>>()
    //     );
    //     assert_eq!(
    //         read(&mut fs, 125, 126, handle)?,
    //         std::iter::repeat(7).take(125).collect::<Vec<u8>>()
    //     );
    //     assert_eq!(read(&mut fs, 125, 251, handle)?, buf);
    //
    //     std::thread::sleep(std::time::Duration::from_secs(1));
    //
    //     // Overwriting in the middle
    //     let buf = std::iter::repeat(9).take(125).collect::<Vec<u8>>();
    //     let wrote = fs.write(Path::new("/ignored.txt"), &buf, 126, &mut write_file_info)?;
    //     assert_eq!(wrote, 125);
    //
    //     let stat = fs.metadata(Path::new("/bar.txt"))?;
    //     assert_eq!(stat.st_size, 376);
    //     assert_eq!(stat.st_blocks, 1); // 376 / 512 + 1
    //
    //     let inode = fs.find_inode(2)?;
    //     assert_eq!(inode.direct_blocks[0], 2);
    //     assert_eq!(inode.direct_blocks[1], 3);
    //     assert_eq!(inode.direct_blocks[2], 4);
    //
    //     assert_ne!(inode.modified_at, modified_at);
    //     assert_ne!(inode.changed_at, changed_at);
    //
    //     assert_eq!(fs.superblock().free_blocks, BLOCK_SIZE * 8 - 4);
    //
    //     assert_eq!(
    //         read(&mut fs, 120, 0, handle)?,
    //         std::iter::repeat(5).take(120).collect::<Vec<u8>>()
    //     );
    //     assert_eq!(
    //         read(&mut fs, 6, 120, handle)?,
    //         std::iter::repeat(4).take(6).collect::<Vec<u8>>()
    //     );
    //     assert_eq!(read(&mut fs, 125, 126, handle)?, buf);
    //     assert_eq!(
    //         read(&mut fs, 125, 251, handle)?,
    //         std::iter::repeat(8).take(125).collect::<Vec<u8>>()
    //     );
    //
    //     Ok(std::fs::remove_file(&tmp_file)?)
    // }
    //
    // #[test]
    // fn append_only() -> anyhow::Result<()> {
    //     let tmp_file = make_fs("append_only")?;
    //     let mut fs = SimpleExt4FS::new(&tmp_file)?;
    //
    //     let mut open_fi = fuse_rs::fs::OpenFileInfo::default();
    //     fs.create(
    //         Path::new("/bar.txt"),
    //         nix::sys::stat::Mode::S_IRWXU,
    //         &mut open_fi,
    //     )?;
    //
    //     fs.open(Path::new("/bar.txt"), &mut open_fi)?;
    //     let handle = open_fi.handle().unwrap();
    //     let mut file_info = fuse_rs::fs::FileInfo::default();
    //     file_info.set_handle(handle);
    //
    //     let mut write_file_info = fuse_rs::fs::WriteFileInfo::from_file_info(file_info);
    //     let buf = std::iter::repeat(3)
    //         .take(2 * BLOCK_SIZE as usize)
    //         .collect::<Vec<u8>>();
    //
    //     let wrote = fs.write(Path::new("/ignored.txt"), &buf, 0, &mut write_file_info)?;
    //     assert_eq!(wrote, buf.len());
    //     assert_eq!(read(&mut fs, 2 * BLOCK_SIZE as usize, 0, handle)?, buf);
    //
    //     let stat = fs.metadata(Path::new("/bar.txt"))?;
    //     assert_eq!(stat.st_size, buf.len() as _);
    //     assert_eq!(stat.st_blocks, 1);
    //
    //     let inode = fs.find_inode(2)?;
    //     assert_eq!(inode.direct_blocks[0], 2);
    //     assert_eq!(inode.direct_blocks[1], 3);
    //
    //     let buf = std::iter::repeat(4)
    //         .take(BLOCK_SIZE as _)
    //         .collect::<Vec<u8>>();
    //
    //     let wrote = fs.write(
    //         Path::new("/ignored.txt"),
    //         &buf,
    //         2 * BLOCK_SIZE as u64,
    //         &mut write_file_info,
    //     )?;
    //     assert_eq!(wrote, BLOCK_SIZE as _);
    //     assert_eq!(
    //         read(&mut fs, BLOCK_SIZE as usize, 2 * BLOCK_SIZE as u64, handle)?,
    //         buf
    //     );
    //
    //     let stat = fs.metadata(Path::new("/bar.txt"))?;
    //     assert_eq!(stat.st_size, BLOCK_SIZE as i64 * 3);
    //     assert_eq!(stat.st_blocks, 1);
    //
    //     let inode = fs.find_inode(2)?;
    //     assert_eq!(inode.direct_blocks[0], 2);
    //     assert_eq!(inode.direct_blocks[1], 3);
    //     assert_eq!(inode.direct_blocks[2], 4);
    //
    //     assert_eq!(fs.superblock().free_blocks, BLOCK_SIZE * 8 - 4);
    //
    //     Ok(std::fs::remove_file(&tmp_file)?)
    // }
    //
    // #[test]
    // fn remove_file() -> anyhow::Result<()> {
    //     let tmp_file = make_fs("remove_file")?;
    //     let mut fs = SimpleExt4FS::new(&tmp_file)?;
    //
    //     let mut open_fi = fuse_rs::fs::OpenFileInfo::default();
    //     fs.create(
    //         Path::new("/bar.txt"),
    //         nix::sys::stat::Mode::S_IRWXU,
    //         &mut open_fi,
    //     )?;
    //
    //     fs.open(Path::new("/bar.txt"), &mut open_fi)?;
    //     let handle = open_fi.handle().unwrap();
    //     let mut file_info = fuse_rs::fs::FileInfo::default();
    //     file_info.set_handle(handle);
    //
    //     let mut write_file_info = fuse_rs::fs::WriteFileInfo::from_file_info(file_info);
    //     let buf = std::iter::repeat(3)
    //         .take(2 * BLOCK_SIZE as usize)
    //         .collect::<Vec<u8>>();
    //
    //     let wrote = fs.write(Path::new("/ignored.txt"), &buf, 0, &mut write_file_info)?;
    //     assert_eq!(wrote, buf.len());
    //     assert_eq!(fs.superblock().free_blocks, BLOCK_SIZE * 8 - 3);
    //
    //     let (inode, index) = fs.find_inode_from_path(Path::new("/bar.txt"))?;
    //     let blocks = vec![2u32, 3u32];
    //     assert_eq!(blocks, inode.direct_blocks());
    //     assert_eq!(index, 2);
    //
    //     fs.remove_file(Path::new("/bar.txt"))?;
    //
    //     assert_eq!(fs.superblock().free_blocks, BLOCK_SIZE * 8 - 1);
    //     assert_eq!(
    //         Errno::ENOENT,
    //         fs.metadata(Path::new("/bar.txt")).unwrap_err()
    //     );
    //
    //     let entries = fs.read_dir(Path::new("/"), 0, fuse_rs::fs::FileInfo::default())?;
    //     assert_eq!(entries.len(), 0);
    //
    //     let mut open_fi = fuse_rs::fs::OpenFileInfo::default();
    //     fs.create(
    //         Path::new("/baz.txt"),
    //         nix::sys::stat::Mode::S_IRWXU,
    //         &mut open_fi,
    //     )?;
    //
    //     fs.open(Path::new("/baz.txt"), &mut open_fi)?;
    //     let handle = open_fi.handle().unwrap();
    //     let mut file_info = fuse_rs::fs::FileInfo::default();
    //     file_info.set_handle(handle);
    //
    //     let mut write_file_info = fuse_rs::fs::WriteFileInfo::from_file_info(file_info);
    //     let buf = std::iter::repeat(3)
    //         .take(2 * BLOCK_SIZE as usize)
    //         .collect::<Vec<u8>>();
    //
    //     let wrote = fs.write(Path::new("/ignored.txt"), &buf, 0, &mut write_file_info)?;
    //     assert_eq!(wrote, buf.len());
    //     assert_eq!(fs.superblock().free_blocks, BLOCK_SIZE * 8 - 3);
    //
    //     // Check that it reuses previously freed blocks
    //     let (inode, index) = fs.find_inode_from_path(Path::new("/baz.txt"))?;
    //     let blocks = vec![2u32, 3u32];
    //     assert_eq!(blocks, inode.direct_blocks());
    //     assert_eq!(index, 2);
    //
    //     let entries = fs.read_dir(Path::new("/"), 0, fuse_rs::fs::FileInfo::default())?;
    //     assert_eq!(entries.len(), 1);
    //
    //     let bar = entries.first().unwrap();
    //     assert_eq!(bar.name, OsString::from("baz.txt"));
    //
    //     Ok(std::fs::remove_file(&tmp_file)?)
    // }

    fn make_fs(name: &str) -> anyhow::Result<PathBuf> {
        let mut tmp_file = tempfile::tempdir()?.path().to_path_buf();
        fs::create_dir_all(&tmp_file)?;
        tmp_file.push(name);
        tmp_file.set_extension("img");
        if tmp_file.exists() {
            std::fs::remove_file(&tmp_file)?;
        }

        let block_group_size = crate::simple_ext4::block_group_size(BLOCK_SIZE);
        mkfs::make(&tmp_file, block_group_size, BLOCK_SIZE)?;

        Ok(tmp_file)
    }

    // fn read(
    //     fs: &mut dyn Filesystem,
    //     len: usize,
    //     offset: i64,
    //     ino: u64,
    // ) -> anyhow::Result<Vec<u8>> {
    //     struct TestReplyData {
    //         data: Vec<u8>
    //     }
    //
    //     impl ReplyData for TestReplyData {
    //         fn data(&mut self, data: &[u8]) {
    //             self.data.extend_from_slice(data);
    //         }
    //         fn error(&mut self, _err: i32) {}
    //     }
    //
    //     let mut reply = TestReplyData { data: Vec::new() };
    //     fs.read(&Request::new(0), ino, 0, offset, len as u32, 0, None, &mut reply);
    //
    //     Ok(reply.data)
    // }
}
