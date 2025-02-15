use std::{
    collections::BinaryHeap,
    io::{Read, Seek, Write},
    num::NonZero,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use bytemuck::{AnyBitPattern, NoUninit};

use rayon::{
    iter::{IntoParallelRefMutIterator, ParallelIterator},
    slice::ParallelSliceMut,
};

use crate::ext_arr::ExtArr;

struct ExtItem<T, R> {
    item: T,
    source: R,
}

impl<T: Ord, R> Ord for ExtItem<T, R> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.item.cmp(&self.item)
    }
}

impl<T: PartialOrd, R> PartialOrd for ExtItem<T, R> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        other.item.partial_cmp(&self.item)
    }
}

impl<T: PartialEq, R> PartialEq for ExtItem<T, R> {
    fn eq(&self, other: &Self) -> bool {
        self.item.eq(&other.item)
    }
}

impl<T: Eq, R> Eq for ExtItem<T, R> {}

pub struct ExtSorter;

impl ExtSorter {
    pub fn sort<T, RW, F>(ext_arr: &mut ExtArr<T, RW>, buf: &mut [u8], f: F) -> std::io::Result<()>
    where
        T: Ord + bytemuck::Pod,
        RW: Read + Write + Seek,
        F: Fn(usize) -> std::io::Result<ExtArr<T, RW>>,
    {
        let mut tmp_arrs = Self::sort_chunks(buf, ext_arr, &f)?;
        ext_arr.rewind()?;
        Self::merge_chunks(buf, ext_arr, tmp_arrs.iter_mut())
    }

    pub fn parallel_sort<T, RW, F>(
        ext_arr: &mut ExtArr<T, RW>,
        buf: &'static mut [u8],
        f: F,
        workers: NonZero<usize>,
    ) -> std::io::Result<()>
    where
        T: Ord + bytemuck::Pod + Send,
        RW: Read + Write + Seek + Send + Clone + 'static,
        F: Fn(usize) -> std::io::Result<ExtArr<T, RW>> + Send + Sync + 'static,
    {
        let workers = workers.get();
        let chunk_size = buf.len() / workers;
        let mut handles = Vec::with_capacity(workers);
        let f = Arc::new(f);
        let buf = Arc::new(Mutex::new(buf));

        for i in 0..workers {
            let f = Arc::clone(&f);
            let buf = Arc::clone(&buf);
            let mut ext_arr = ext_arr.clone();

            let handle = std::thread::spawn(move || {
                let mut buf = buf.lock().unwrap(); // Lock buf to access it safely in the thread
                let chunk = &mut buf[i * chunk_size..(i + 1) * chunk_size]; // Create a slice for each chunk
                Self::sort_chunks(chunk, &mut ext_arr, f.as_ref())
            });

            handles.push(handle);
        }

        let mut tmp_arrs = Vec::with_capacity(workers);
        for handle in handles {
            let worker_tmp_arrs = handle.join().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::Other, "Worker has panicked")
            })??;
            tmp_arrs.push(worker_tmp_arrs);
        }
        let mut tmp_arrs: Vec<_> = tmp_arrs.into_iter().flatten().collect();
        ext_arr.rewind()?;

        Self::merge_chunks(
            buf.lock()
                .expect("Should get Mutex to lock to buffer")
                .as_mut(),
            ext_arr,
            tmp_arrs.iter_mut(),
        )?;
        Ok(())
    }

    fn sort_chunks<T, R, F>(
        mut buf: &mut [u8],
        reader: &mut ExtArr<T, R>,
        f: &F,
    ) -> std::io::Result<Vec<ExtArr<T, R>>>
    where
        T: Ord + bytemuck::Pod,
        R: Read + Write + Seek,
        F: Fn(usize) -> std::io::Result<ExtArr<T, R>>,
    {
        let mut chunk_id: usize = 0;
        let mut tmp_arrs = Vec::new();
        loop {
            let read = reader.read(&mut buf)?;
            if read.is_empty() {
                break;
            }

            // Sort numbers
            read.sort_unstable();

            // Write number order to a tmp external array
            let mut tmp_ext_arr = f(chunk_id)?;
            tmp_ext_arr.write(read)?;
            tmp_ext_arr.flush()?;
            tmp_ext_arr.rewind()?;
            tmp_arrs.push(tmp_ext_arr);

            chunk_id += 1;
        }
        Ok(tmp_arrs)
    }

    fn merge_chunks<'b, T, W, I, R>(
        buf: &mut [u8],
        writer: &mut ExtArr<T, W>,
        chunk_readers: I,
    ) -> std::io::Result<()>
    where
        T: Ord + AnyBitPattern + NoUninit,
        I: IntoIterator<Item = &'b mut ExtArr<T, R>>,
        <I as IntoIterator>::IntoIter: ExactSizeIterator,
        W: Write,
        R: Read + 'b,
    {
        let sources = chunk_readers.into_iter();
        let mut heap = BinaryHeap::with_capacity(sources.len());
        let (mut num_buffer, _) = buf.split_at_mut(std::mem::size_of::<T>());

        for source in sources {
            let item = source.read(&mut num_buffer)?[0];

            heap.push(ExtItem { item, source });
        }

        while let Some(ExtItem { item, source }) = heap.pop() {
            writer.write(&[item])?;
            let read = source.read(&mut num_buffer)?;
            if !read.is_empty() {
                heap.push(ExtItem {
                    item: read[0],
                    source,
                });
            }
        }
        writer.flush()?;
        Ok(())
    }
}

pub struct RayonExtSorter<'a> {
    buf: &'a mut [u8],
    workers: usize,
}

impl<'a> RayonExtSorter<'a> {
    pub fn new(buf: &'a mut [u8], workers: NonZero<usize>) -> Self {
        Self {
            buf,
            workers: workers.get(),
        }
    }

    pub fn sort<T, RW, F>(&mut self, ext_arr: &mut ExtArr<T, RW>, f: F) -> std::io::Result<()>
    where
        T: Ord + bytemuck::Pod + Sync + Send,
        RW: Read + Write + Seek + Send + Sync + Clone,
        F: Fn(usize) -> std::io::Result<ExtArr<T, RW>> + Sync,
    {
        let mut tmp_arrs = self.sort_chunks(ext_arr, f)?;
        ext_arr.rewind()?;

        self.merge_chunks(ext_arr, &mut tmp_arrs)?;
        Ok(())
    }

    pub fn sort_with_linear_merge<T, RW, F>(
        &mut self,
        ext_arr: &mut ExtArr<T, RW>,
        f: F,
    ) -> std::io::Result<()>
    where
        T: Ord + bytemuck::Pod + Sync + Send,
        RW: Read + Write + Seek + Send + Sync + Clone,
        F: Fn(usize) -> std::io::Result<ExtArr<T, RW>> + Sync,
    {
        let mut tmp_arrs = self.sort_chunks(ext_arr, f)?;
        ext_arr.rewind()?;

        self.merge_chunks_linear(ext_arr, &mut tmp_arrs)?;
        Ok(())
    }

    fn sort_chunks<T, R, F>(
        &mut self,
        reader: &mut ExtArr<T, R>,
        f: F,
    ) -> std::io::Result<Vec<ExtArr<T, R>>>
    where
        T: Ord + bytemuck::Pod + Send + Sync,
        R: Read + Write + Seek + Send + Sync + Clone,
        F: Fn(usize) -> std::io::Result<ExtArr<T, R>> + Sync,
    {
        let chunk_id = AtomicUsize::new(0);

        let chunk_size = self.buf.len() / self.workers;
        let tmp_arrs: Vec<_> = self
            .buf
            .par_chunks_mut(chunk_size)
            .flat_map(|mut chunk| {
                let mut reader = reader.clone();
                let mut tmp_arrs = Vec::new();
                loop {
                    let read = reader.read(&mut chunk).unwrap();
                    if read.is_empty() {
                        break;
                    }

                    // Sort numbers
                    read.par_sort_unstable();

                    // Write number order to a tmp external array
                    let mut tmp_ext_arr = f(chunk_id.load(Ordering::Relaxed)).unwrap();
                    tmp_ext_arr.write(read).unwrap();
                    tmp_ext_arr.flush().unwrap();
                    tmp_ext_arr.rewind().unwrap();
                    tmp_arrs.push(tmp_ext_arr);

                    chunk_id.fetch_add(1, Ordering::SeqCst);
                }
                tmp_arrs
            })
            .collect();
        Ok(tmp_arrs)
    }

    fn merge_chunks<'i, T, W, I, R>(
        &mut self,
        writer: &mut ExtArr<T, W>,
        chunk_readers: &'i mut I,
    ) -> std::io::Result<()>
    where
        T: Ord + AnyBitPattern + NoUninit + Send,
        I: IntoParallelRefMutIterator<'i, Item = &'i mut ExtArr<T, R>>,
        W: Write,
        R: Read + Send + 'i,
    {
        let sources = chunk_readers.par_iter_mut();
        let mem_slots: Vec<_> = self
            .buf
            .par_chunks_exact_mut(std::mem::size_of::<T>())
            .map(|slot| Arc::new(Mutex::new(slot)))
            .collect();
        let mem_slots = Arc::new(mem_slots);

        let mut heap: BinaryHeap<_> = sources
            .map(|source| {
                let mut slot_lock = loop {
                    if let Some(lock) = (*mem_slots).iter().find_map(|slot| (*slot).try_lock().ok())
                    {
                        break lock;
                    }
                };
                let item = source.read(&mut *slot_lock).unwrap()[0];
                ExtItem { item, source }
            })
            .collect();

        let mut num_slot = mem_slots[0].lock().unwrap();
        while let Some(ExtItem { item, source }) = heap.pop() {
            writer.write(&[item])?;
            let read = source.read(&mut *num_slot)?;
            if !read.is_empty() {
                heap.push(ExtItem {
                    item: read[0],
                    source,
                });
            }
        }
        writer.flush()
    }

    fn merge_chunks_linear<'b, T, W, I, R>(
        &mut self,
        writer: &mut ExtArr<T, W>,
        chunk_readers: I,
    ) -> std::io::Result<()>
    where
        T: Ord + AnyBitPattern + NoUninit,
        I: IntoIterator<Item = &'b mut ExtArr<T, R>>,
        <I as IntoIterator>::IntoIter: ExactSizeIterator,
        W: Write,
        R: Read + 'b,
    {
        let sources = chunk_readers.into_iter();
        let mut heap = BinaryHeap::with_capacity(sources.len());
        let (mut num_buffer, _) = self.buf.as_mut().split_at_mut(std::mem::size_of::<T>());

        for source in sources {
            let item = source.read(&mut num_buffer)?[0];

            heap.push(ExtItem { item, source });
        }

        while let Some(ExtItem { item, source }) = heap.pop() {
            writer.write(&[item])?;
            let read = source.read(&mut num_buffer)?;
            if !read.is_empty() {
                heap.push(ExtItem {
                    item: read[0],
                    source,
                });
            }
        }
        writer.flush()?;
        Ok(())
    }
}
