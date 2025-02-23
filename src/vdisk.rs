use std::{
    fs::{File, OpenOptions},
    io,
    path::PathBuf,
};

/// One gigabyte in bytes
pub static DEFAULT_SIZE_IN_BYTES: u32 = 1e9 as u32;

pub type VDiskResult<T> = io::Result<T>;

pub type VDiskSize = u32;

pub struct VDisk {
    pub size: VDiskSize,
    pub disk: File,
}

impl Clone for VDisk {
    fn clone(&self) -> Self {
        Self {
            size: self.size,
            disk: self.disk.try_clone().expect("Failed to clone disk"),
        }
    }
}

impl VDisk {
    pub fn new(path: PathBuf, size: u32) -> VDiskResult<Self> {
        match path.exists() {
            true => Self::try_from(File::open(path)?),
            false => Self::create_new_disk(path, size),
        }
    }

    #[cfg(target_os = "linux")]
    fn create_new_disk(path: PathBuf, size: u32) -> VDiskResult<VDisk> {
        use nix::fcntl::{fallocate, FallocateFlags};
        use std::os::fd::AsRawFd;

        let disk = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;

        fallocate(disk.as_raw_fd(), FallocateFlags::empty(), 0, size.into())?;

        Ok(Self { size, disk })
    }

    #[cfg(target_os = "macos")]
    fn create_new_disk(path: PathBuf, size: u32) -> VDiskResult<VDisk> {
        use std::io::Write;
        let mut disk = OpenOptions::new()
            .write(true)
            .create(true)
            .open(path)
            .into_diagnostic()?;

        let file_size = size as usize;
        let chunk_size = 1024 * 1024;
        let mut buffer = vec![0; chunk_size];

        let mut written = 0;

        while written < file_size {
            let to_write = std::cmp::min(chunk_size, file_size - written);
            disk.write_all(&buffer[..to_write]).into_diagnostic()?;
            written += to_write;
        }

        Ok(Self { size, disk })
    }

    #[cfg(target_family = "windows")]
    fn create_new_disk(path: PathBuf, size: u32) -> Result<VDisk> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileAllocationInfo, SetFileInformationByHandle, FILE_ALLOCATION_INFO,
        };

        let disk = OpenOptions::new()
            .write(true)
            .create(true)
            .open(path)
            .into_diagnostic()?;

        let handle = disk.as_raw_handle();

        let allocation_info = FILE_ALLOCATION_INFO {
            AllocationSize: size as i64,
        };

        let result = unsafe {
            SetFileInformationByHandle(
                handle as _,
                FileAllocationInfo,
                &allocation_info as *const _ as *mut _,
                std::mem::size_of::<FILE_ALLOCATION_INFO>() as u32,
            )
        };

        if result == 0 {
            return Err(std::io::Error::last_os_error()).into_diagnostic();
        }

        disk.set_len(size as u64).into_diagnostic()?;
        disk.sync_all().into_diagnostic()?;

        Ok(Self { size, disk })
    }
}

impl TryFrom<File> for VDisk {
    type Error = io::Error;

    fn try_from(disk: File) -> std::result::Result<Self, Self::Error> {
        let size = disk.metadata()?.len() as u32;
        Ok(Self { size, disk })
    }
}

impl TryFrom<PathBuf> for VDisk {
    type Error = io::Error;

    fn try_from(disk: PathBuf) -> std::result::Result<Self, Self::Error> {
        let disk = File::open(disk)?;
        let size = disk.metadata()?.len() as u32;
        Ok(Self { size, disk })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_new_disk_creation() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("test_disk.vd");
        let size = 1024 * 1024; // 1MB

        let vdisk = VDisk::new(path.clone(), size)?;
        assert_eq!(vdisk.size, size);

        // Verify file exists and has correct size
        let metadata = fs::metadata(path)?;
        assert_eq!(metadata.len() as u32, size);

        Ok(())
    }

    #[test]
    fn test_existing_disk_open() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("existing_disk.vd");
        let size = 1024 * 1024; // 1MB

        // Create initial disk
        let _vdisk = VDisk::new(path.clone(), size)?;

        // Try opening existing disk
        let vdisk2 = VDisk::new(path.clone(), size)?;
        assert_eq!(vdisk2.size, size);

        Ok(())
    }

    #[test]
    fn test_try_from_pathbuf() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("convert_disk.vd");
        let size = 1024 * 1024; // 1MB

        // Create initial disk
        let _vdisk = VDisk::new(path.clone(), size)?;

        // Convert from PathBuf
        let vdisk2 = VDisk::try_from(path)?;
        assert_eq!(vdisk2.size, size);

        Ok(())
    }

    #[cfg(target_family = "unix")]
    mod unix_tests {
        use super::*;
        use std::os::unix::fs::FileExt;

        #[test]
        fn test_unix_specific_disk_ops() -> Result<()> {
            let dir = tempdir()?;
            let path = dir.path().join("unix_disk.vd");
            let size = 1024 * 1024; // 1MB

            let vdisk = VDisk::new(path, size)?;

            // Test Unix-specific file operations
            let written = vdisk.disk.write_at(b"test", 0)?;
            assert_eq!(written, 4);

            Ok(())
        }
    }

    #[cfg(target_family = "windows")]
    mod windows_tests {
        use super::*;
        use anyhow::Result;
        use std::os::windows::fs::FileExt;

        #[test]
        fn test_windows_specific_disk_ops() -> Result<()> {
            let dir = tempdir()?;
            let path = dir.path().join("windows_disk.vd");
            let size = 1024 * 1024; // 1MB

            let vdisk = VDisk::new(path, size)?;

            // Test Windows-specific file operations
            let written = vdisk.disk.seek_write(b"test", 0)?;
            assert_eq!(written, 4);

            Ok(())
        }
    }
}
