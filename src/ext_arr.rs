use std::{
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Read, Seek, Write},
    marker::PhantomData,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

use bytemuck::{AnyBitPattern, NoUninit};

#[derive(Debug)]
pub struct ExtArr<T, RW> {
    rw: RW,
    _marker: PhantomData<T>,
}

impl<T, RW> ExtArr<T, RW> {
    pub fn new(rw: RW) -> Self {
        Self {
            rw,
            _marker: PhantomData,
        }
    }

    pub fn into_inner(self) -> RW {
        self.rw
    }
}

impl<T, RW> ExtArr<T, RW>
where
    T: NoUninit + AnyBitPattern,
    RW: Read,
{
    pub fn read<'b, B: AsMut<[u8]>>(&mut self, buf: &'b mut B) -> std::io::Result<&'b mut [T]> {
        let buf = buf.as_mut();
        let bytes_read = self.rw.read(buf)?;

        let (read, _) = buf.split_at_mut(bytes_read);
        let read: &mut [T] = bytemuck::try_cast_slice_mut(read).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "The number of bytes readed cannot be casted into &mut [T]",
            )
        })?;
        Ok(read)
    }

    pub fn read_to_end<'b>(&mut self, buf: &'b mut Vec<u8>) -> std::io::Result<&'b mut [T]> {
        self.rw.read_to_end(buf)?;

        // Ensure the buffer size is a multiple of the size of T.
        let read = bytemuck::try_cast_slice_mut(buf.as_mut_slice()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "The number of bytes readed cannot be casted into &mut [T]",
            )
        })?;
        Ok(read)
    }
}

impl<T, RW> ExtArr<T, RW>
where
    T: NoUninit,
    RW: Write,
{
    pub fn write(&mut self, buf: &[T]) -> std::io::Result<()> {
        let buf: &[u8] = bytemuck::cast_slice(buf);
        self.rw.write_all(buf)
    }

    pub fn write_raw(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.rw.write_all(buf)
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.rw.flush()
    }
}

impl<T, RW: Seek> Seek for ExtArr<T, RW> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.rw.seek(pos)
    }
}

impl<T, RW: Clone> Clone for ExtArr<T, RW> {
    fn clone(&self) -> Self {
        Self {
            rw: self.rw.clone(),
            _marker: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct FileBufRW {
    reader: BufReader<File>,
    writer: BufWriter<File>,
}

impl FileBufRW {
    pub fn new<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        Self::try_from(file)
    }
}

impl Read for FileBufRW {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buf)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> std::io::Result<usize> {
        self.reader.read_to_end(buf)
    }
}

impl Write for FileBufRW {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl Seek for FileBufRW {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.reader.seek(pos)
    }
}

impl TryFrom<File> for FileBufRW {
    type Error = std::io::Error;

    fn try_from(value: File) -> Result<Self, Self::Error> {
        let reader = BufReader::new(value.try_clone()?);
        let writer = BufWriter::new(value);
        Ok(Self { reader, writer })
    }
}

#[derive(Debug, Clone)]
pub struct SyncRW<RW> {
    rw: Arc<Mutex<RW>>,
}

impl<RW> SyncRW<RW> {
    pub fn new(rw: RW) -> Self {
        Self {
            rw: Arc::new(Mutex::new(rw)),
        }
    }

    fn lock(&self) -> std::io::Result<MutexGuard<'_, RW>> {
        self.rw
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "SyncRW is poisoned"))
    }
}

impl<RW: Read> Read for SyncRW<RW> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.lock()?.read(buf)
    }
}

impl<RW: Write> Write for SyncRW<RW> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.lock()?.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.lock()?.flush()
    }
}

impl<RW: Seek> Seek for SyncRW<RW> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.lock()?.seek(pos)
    }
}
