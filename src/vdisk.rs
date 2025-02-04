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
        use nix::fcntl::{fallocate, FallocateFlags};
        use std::os::fd::AsRawFd;

        let disk = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .into_diagnostic()?;

        fallocate(disk.as_raw_fd(), FallocateFlags::empty(), 0, size.into()).into_diagnostic()?;

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
