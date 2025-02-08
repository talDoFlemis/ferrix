use crate::vdisk::VDisk;

pub trait Filesystem {}

pub struct BasicFS {
    vdisk: VDisk,
}

impl BasicFS {
    pub fn new(vdisk: VDisk) -> Self {
        Self { vdisk }
    }
}

impl Filesystem for BasicFS {}

impl Clone for BasicFS {
    fn clone(&self) -> Self {
        Self {
            vdisk: self.vdisk.clone(),
        }
    }
}
