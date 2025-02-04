use std::{
    fs::{File, OpenOptions},
    path::PathBuf,
};

/// One gigabyte in bytes
pub static DEFAULT_SIZE_IN_BYTES: u32 = 1e9 as u32;

use miette::{IntoDiagnostic, Result};

pub struct VDisk {
    size: u32,
    disk: File,
}

impl VDisk {
    pub fn new(path: PathBuf, size: u32) -> Result<Self> {
        match path.exists() {
            true => Self::try_from(File::open(path).into_diagnostic()?),
            false => Self::create_new_disk(path, size),
        }
    }

    #[cfg(target_family = "unix")]
    fn create_new_disk(path: PathBuf, size: u32) -> Result<VDisk> {
        use libc::posix_fallocate;
        use std::os::fd::AsRawFd;

        let disk = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .into_diagnostic()?;

        unsafe {
            posix_fallocate(disk.as_raw_fd(), 0, size.into());
        }

        Ok(Self { size, disk })
    }

    #[cfg(target_family = "windows")]
    fn create_new_disk(path: PathBuf, size: u32) -> Result<VDisk> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            SetFileInformationByHandle, FILE_ALLOCATION_INFO,
        };

        let disk = OpenOptions::new().write(true).create(true).open(path)?;
        let handle = file.as_raw_handle() as isize;

        let allocation_info = FILE_ALLOCATION_INFO {
            AllocationSize: size as i64,
        };

        unsafe {
            SetFileInformationByHandle(
                handle,
                5,
                &allocation_info as *const _ as *mut _,
                std::mem::size_of::<FILE_ALLOCATION_INFO>() as u32,
            );
        }

        Ok(Self { size, disk })
    }
}

impl TryFrom<File> for VDisk {
    type Error = miette::Report;

    fn try_from(disk: File) -> std::result::Result<Self, Self::Error> {
        let size = disk.metadata().into_diagnostic()?.len() as u32;
        Ok(Self { size, disk })
    }
}

impl TryFrom<PathBuf> for VDisk {
    type Error = miette::Report;

    fn try_from(disk: PathBuf) -> std::result::Result<Self, Self::Error> {
        let disk = File::open(disk).into_diagnostic()?;
        let size = disk.metadata().into_diagnostic()?.len() as u32;
        Ok(Self { size, disk })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_new_disk_creation() -> Result<()> {
        let dir = tempdir().into_diagnostic()?;
        let path = dir.path().join("test_disk.vd");
        let size = 1024 * 1024; // 1MB

        let vdisk = VDisk::new(path.clone(), size)?;
        assert_eq!(vdisk.size, size);

        // Verify file exists and has correct size
        let metadata = fs::metadata(path).into_diagnostic()?;
        assert_eq!(metadata.len() as u32, size);

        Ok(())
    }

    #[test]
    fn test_existing_disk_open() -> Result<()> {
        let dir = tempdir().into_diagnostic()?;
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
        let dir = tempdir().into_diagnostic()?;
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
            let dir = tempdir().into_diagnostic()?;
            let path = dir.path().join("unix_disk.vd");
            let size = 1024 * 1024; // 1MB

            let vdisk = VDisk::new(path, size)?;

            // Test Unix-specific file operations
            let written = vdisk.disk.write_at(b"test", 0).into_diagnostic()?;
            assert_eq!(written, 4);

            Ok(())
        }
    }

    #[cfg(target_family = "windows")]
    mod windows_tests {
        use super::*;
        use std::os::windows::fs::FileExt;

        #[test]
        fn test_windows_specific_disk_ops() -> Result<()> {
            let dir = tempdir().into_diagnostic()?;
            let path = dir.path().join("windows_disk.vd");
            let size = 1024 * 1024; // 1MB

            let vdisk = VDisk::new(path, size)?;

            // Test Windows-specific file operations
            let written = vdisk.disk.seek_write(b"test", 0).into_diagnostic()?;
            assert_eq!(written, 4);

            Ok(())
        }
    }
}
