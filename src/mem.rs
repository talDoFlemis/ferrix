pub mod size {
    /// Size of a kilobyte in bytes.
    pub const KB: usize = 1000;
    /// Size of a megabyte in bytes.
    pub const MB: usize = 1000 * KB;
    /// Size of a gibabyte in bytes.
    pub const GB: usize = 1000 * MB;
}

/// Memory with fixed size of length MEM_SIZE.
#[derive(Debug, Clone)]
pub struct FixedSizeMem<const MEM_SIZE: usize> {
    storage: Box<[u8; MEM_SIZE]>,
}

impl<const MEM_SIZE: usize> FixedSizeMem<MEM_SIZE> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<const MEM_SIZE: usize> Default for FixedSizeMem<MEM_SIZE> {
    fn default() -> Self {
        Self {
            storage: Box::new([0; MEM_SIZE]),
        }
    }
}

impl<const MEM_SIZE: usize> AsRef<[u8; MEM_SIZE]> for FixedSizeMem<MEM_SIZE> {
    fn as_ref(&self) -> &[u8; MEM_SIZE] {
        &self.storage
    }
}

impl<const MEM_SIZE: usize> AsRef<[u8]> for FixedSizeMem<MEM_SIZE> {
    fn as_ref(&self) -> &[u8] {
        self.storage.as_ref()
    }
}

impl<const MEM_SIZE: usize> AsMut<[u8; MEM_SIZE]> for FixedSizeMem<MEM_SIZE> {
    fn as_mut(&mut self) -> &mut [u8; MEM_SIZE] {
        &mut self.storage
    }
}

impl<const MEM_SIZE: usize> AsMut<[u8]> for FixedSizeMem<MEM_SIZE> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.storage.as_mut()
    }
}

impl<const MEM_SIZE: usize> From<FixedSizeMem<MEM_SIZE>> for Box<[u8; MEM_SIZE]> {
    fn from(val: FixedSizeMem<MEM_SIZE>) -> Self {
        val.storage
    }
}
