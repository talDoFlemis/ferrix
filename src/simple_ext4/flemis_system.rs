use anyhow::{bail, Result};
use byte_unit::Byte;
use clean_path::Clean;
use fuser::{BackgroundSession, MountOption};
use memmap::{Mmap, MmapMut, MmapOptions};
use rand::distr::Uniform;
use rand::Rng;
use std::{
    ffi::{OsStr, OsString},
    io::{BufReader, Cursor, Read, Seek, Write},
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::PathBuf,
    process::exit,
    sync::{Arc, Mutex},
    u16, usize,
};

use crate::{
    ext_arr::ExtArr,
    mem::FixedSizeMem,
    sort::ExtSorter,
    system::{ListCommandOutput, Number, System, SystemError, DEFAULT_MEM_SIZE},
    vdisk::{self, VDisk, VDiskSize},
};

#[derive(Debug)]
pub struct FlemisSystem {
    mount_point: PathBuf,
    session: Arc<Mutex<BackgroundSession>>,
}

impl FlemisSystem {
    pub fn new(vdisk: VDisk, mount_point: PathBuf) -> Result<Self> {
        let fs = super::fs_in_fs::FSInFS::new("/tmp/storage".into(), true, false);
        let options = vec![MountOption::FSName("flemis".to_string())];

        let bg_session = fuser::spawn_mount2(fs, mount_point.clone(), &options)?;
        let session = Arc::new(Mutex::new(bg_session));

        Ok(Self {
            mount_point,
            session,
        })
    }

    fn convert_path_to_vdisk_path(&self, path: &PathBuf) -> PathBuf {
        let mut vdisk_path = self.mount_point.clean();
        let path = PathBuf::from("/").join(path).clean();
        let path = path.strip_prefix("/").unwrap_or(path.as_path());
        vdisk_path.push(path);

        vdisk_path.clean()
    }
}

impl System for FlemisSystem {
    fn touch(&mut self, cmd: &crate::complete_command::TouchCommand) -> Result<()> {
        let file = self.convert_path_to_vdisk_path(&PathBuf::from(&cmd.file));

        if file.exists() {
            bail!(SystemError::FileAlreadyExists);
        }

        let file = std::fs::File::create(file)?;
        let mut writer = std::io::BufWriter::new(file);

        let mut rng = rand::rng();
        let data: Vec<u16> = (0..cmd.number_of_integers)
            .map(|_| rng.random_range(0..=u16::MAX))
            .collect();

        let encoded: Vec<u8> = bincode::serialize(&data)?;

        writer.write_all(&encoded)?;
        writer.flush()?;

        Ok(())
    }

    fn mv(&mut self, cmd: &crate::complete_command::MoveCommand) -> Result<()> {
        let file_to_move = self.convert_path_to_vdisk_path(&PathBuf::from(&cmd.from));

        if !file_to_move.exists() {
            bail!(SystemError::NoSuchFileOrDirectory);
        }

        let new_file = self.convert_path_to_vdisk_path(&PathBuf::from(&cmd.to));
        std::fs::rename(file_to_move, new_file)?;
        Ok(())
    }

    fn make_dir(&mut self, cmd: &crate::complete_command::MakeDirCommand) -> Result<()> {
        let dir = self.convert_path_to_vdisk_path(&PathBuf::from(&cmd.dir));

        if dir.exists() {
            bail!(SystemError::FileAlreadyExists);
        }

        std::fs::create_dir_all(dir)?;
        Ok(())
    }

    fn remove(&mut self, cmd: &crate::complete_command::RemoveCommand) -> Result<()> {
        let file_or_dir = self.convert_path_to_vdisk_path(&PathBuf::from(&cmd.file_or_dir));

        if !file_or_dir.exists() {
            bail!(SystemError::NoSuchFileOrDirectory);
        }

        if file_or_dir.is_dir() && !cmd.recursive {
            bail!(SystemError::IsDirectory);
        }

        if cmd.recursive {
            Ok(std::fs::remove_dir_all(file_or_dir)?)
        } else {
            Ok(std::fs::remove_file(file_or_dir)?)
        }
    }

    fn head(
        &self,
        cmd: &crate::complete_command::HeadCommand,
    ) -> Result<Vec<crate::system::Number>> {
        let file = self.convert_path_to_vdisk_path(&PathBuf::from(&cmd.file));

        if !file.exists() {
            bail!(SystemError::NoSuchFileOrDirectory);
        }

        let start: usize = cmd.start.try_into()?;
        let mut end: usize = cmd.end.try_into()?;
        if start > end {
            end = start + 10;
        }

        let file = std::fs::File::open(file)?;
        let mut reader = std::io::BufReader::new(file);

        // First read the vector length from bincode header
        let vec_len: u64 = bincode::deserialize_from(&mut reader)?;

        if end >= vec_len as usize {
            end = vec_len.try_into()?;
        }

        // Skip elements before start
        for _ in 0..start {
            let _: Number = bincode::deserialize_from(&mut reader)?;
        }

        // Read only the required elements
        let elements_to_read = end - start;
        let mut result = Vec::with_capacity(elements_to_read);

        for _ in 0..elements_to_read {
            let num: Number = bincode::deserialize_from(&mut reader)?;
            result.push(num);
        }

        Ok(result)
    }

    fn list(
        &self,
        cmd: &crate::complete_command::ListCommand,
    ) -> Result<crate::system::ListCommandOutput> {
        let path = PathBuf::from(cmd.dir.as_ref().unwrap_or(&OsString::from("/")));
        let path = self.convert_path_to_vdisk_path(&path);

        if !path.exists() {
            bail!(SystemError::NoSuchFileOrDirectory);
        }

        let mut nodes = Vec::new();

        if !path.is_dir() {
            let metadata = path.metadata()?;
            let file_name = path.file_name().unwrap().to_str().unwrap().to_string();
            let size = metadata.size();

            let node_info = crate::system::NodeInfo {
                name: file_name,
                is_dir: false,
                size_in_bytes: size as vdisk::VDiskSize,
                human_readable_size: Byte::from_u64(size)
                    .get_appropriate_unit(byte_unit::UnitType::Binary)
                    .to_string(),
            };

            nodes.push(node_info);
        } else {
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                let metadata = entry.metadata()?;

                let file_name = entry.file_name();
                let size = metadata.size();

                let node_info = crate::system::NodeInfo {
                    name: file_name
                        .to_os_string()
                        .into_string()
                        .expect("expected to be a string"),
                    is_dir: metadata.is_dir(),
                    size_in_bytes: size as VDiskSize,
                    human_readable_size: Byte::from_u64(size)
                        .get_appropriate_unit(byte_unit::UnitType::Binary)
                        .to_string(),
                };

                nodes.push(node_info);
            }
        }

        let stat = nix::sys::statfs::statfs(&self.mount_point)?;

        let total_disk_space_in_bytes = (stat.blocks() * (stat.block_size() as u64)).try_into()?;
        let remaining_disk_space_in_bytes =
            (stat.blocks_available() * (stat.block_size() as u64)).try_into()?;

        Ok(ListCommandOutput {
            nodes,
            total_disk_space_in_bytes,
            remaining_disk_space_in_bytes,
        })
    }

    fn sort(&self, cmd: &crate::complete_command::SortCommand) -> Result<()> {
        let path = self.convert_path_to_vdisk_path(&PathBuf::from(&cmd.file));

        if !path.exists() {
            bail!(SystemError::NoSuchFileOrDirectory);
        }

        let file = std::fs::File::open(path.clone())?;
        let reader = BufReader::new(file);
        let numbers: Vec<u16> = bincode::deserialize_from(reader)?;
        let length = numbers.len();

        let mut mem = FixedSizeMem::<DEFAULT_MEM_SIZE>::new();
        let mut arr = ExtArr::<Number, _>::new(Cursor::new(Vec::new()));

        arr.write(&numbers)?;
        arr.flush()?;
        arr.rewind()?;

        ExtSorter::sort(&mut arr, mem.as_mut(), |_| {
            Ok(ExtArr::new(Cursor::new(Vec::new())))
        })?;

        arr.rewind()?;

        let file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)?;
        let mut writer = std::io::BufWriter::new(file);

        let mut values = Vec::with_capacity(length);
        let casted_values = arr.read_to_end(&mut values)?;
        let encoded = bincode::serialize(casted_values)?;

        writer.write_all(&encoded)?;
        writer.flush()?;

        Ok(())
    }

    fn cat(&self, cmd: &crate::complete_command::CatCommand) -> Result<PathBuf> {
        let mut files = Vec::with_capacity(cmd.files.len());

        if files.capacity() < 2 {
            bail!(SystemError::TooLittleFiles);
        }

        for file in &cmd.files {
            let path = self.convert_path_to_vdisk_path(&PathBuf::from(file));
            if !path.exists() {
                bail!(SystemError::NoSuchFileOrDirectory);
            }

            if path.is_dir() {
                bail!(SystemError::IsDirectory);
            }

            let file = std::fs::File::open(path)?;
            files.push(file);
        }

        let first_file = cmd.files.first().expect("expected the first file");
        let first_file = self.convert_path_to_vdisk_path(&PathBuf::from(first_file));

        let extension = first_file.extension().unwrap_or(OsStr::new("txt"));

        let new_file_path = self.convert_path_to_vdisk_path(&PathBuf::from(format!(
            "{}.{}",
            first_file
                .file_name()
                .expect("expected to be a file")
                .to_str()
                .unwrap(),
            extension.to_str().expect("expected to be a string")
        )));

        let new_file = std::fs::File::create(&new_file_path)?;
        let mut writer = std::io::BufWriter::new(new_file);

        let total_length: u64 = 0;
        bincode::serialize_into(&mut writer, &total_length)?;

        let mut total_numbers = 0u64;
        for file_path in &cmd.files {
            let path = self.convert_path_to_vdisk_path(&PathBuf::from(file_path));
            if !path.exists() {
                bail!(SystemError::NoSuchFileOrDirectory);
            }

            if path.is_dir() {
                bail!(SystemError::IsDirectory);
            }

            let file = std::fs::File::open(path)?;
            let mut reader = std::io::BufReader::new(file);

            // Read length of current file
            let file_length: u64 = bincode::deserialize_from(&mut reader)?;
            total_numbers += file_length;

            // Stream numbers directly from input to output
            for _ in 0..file_length {
                let number: Number = bincode::deserialize_from(&mut reader)?;
                bincode::serialize_into(&mut writer, &number)?;
            }
        }

        // Go back and update the total length
        writer.flush()?;
        writer.seek(std::io::SeekFrom::Start(0))?;
        bincode::serialize_into(&mut writer, &total_numbers)?;
        writer.flush()?;

        Ok(new_file_path)
    }

    fn exit(&self, cmd: &crate::complete_command::ExitCommand) -> Result<()> {
        exit(cmd.code)
    }

    fn chdir(&self, cmd: &crate::complete_command::ChangeDirCommand) -> Result<()> {
        let path = cmd.path.as_ref().map(PathBuf::from);
        let path = path.unwrap_or_else(|| PathBuf::from("/"));

        let path = self.convert_path_to_vdisk_path(&path);

        if !path.exists() {
            bail!(SystemError::DirectoryNotFound);
        }

        std::env::set_current_dir(path)?;

        Ok(())
    }
}
