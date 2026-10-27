use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
/// Virtual filesystem layer over easy-fs
pub struct Inode {
    inode_id: u32,
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        inode_id: u32,
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            inode_id,
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// Return this inode's numeric id.
    /// Return this inode's numeric identifier.
    pub fn inode_id(&self) -> u32 {
        self.inode_id
    }
    /// Return whether this inode is a directory.
    /// Return whether this inode is a directory.
    pub fn is_dir(&self) -> bool {
        self.read_disk_inode(|inode| inode.is_dir())
    }
    /// Return whether this inode is a regular file.
    /// Return whether this inode is a regular file.
    pub fn is_file(&self) -> bool {
        self.read_disk_inode(|inode| inode.is_file())
    }
    /// Return the number of directory entries referring to this inode.
    /// Return this inode's hard-link count.
    pub fn nlink(&self) -> u32 {
        self.read_disk_inode(|inode| inode.nlink())
    }

    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if !dirent.name().is_empty() && dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }
    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| {
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    inode_id,
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let mut empty = DirEntry::empty();
            let reusable = (0..file_count).find(|&i| {
                root_inode.read_at(i * DIRENT_SZ, empty.as_bytes_mut(), &self.block_device);
                empty.name().is_empty()
            });
            let index = reusable.unwrap_or(file_count);
            if reusable.is_none() {
                self.increase_size(((file_count + 1) * DIRENT_SZ) as u32, root_inode, &mut fs);
            }
            root_inode.write_at(
                index * DIRENT_SZ,
                DirEntry::new(name, new_inode_id).as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            new_inode_id,
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
    }
    /// Create a directory entry for an existing regular file.
    /// Add a directory entry linking an existing file to a new name.
    pub fn link(&self, src: &str, dst: &str) -> bool {
        if src.is_empty()
            || dst.is_empty()
            || src == dst
            || src.len() > 27
            || dst.len() > 27
            || src.contains('/')
            || dst.contains('/')
        {
            return false;
        }
        let mut fs = self.fs.lock();
        let Some(source_id) = self.read_disk_inode(|dir| self.find_inode_id(src, dir)) else {
            return false;
        };
        if self
            .read_disk_inode(|dir| self.find_inode_id(dst, dir))
            .is_some()
        {
            return false;
        }
        let (block, offset) = fs.get_disk_inode_pos(source_id);
        let is_file = get_block_cache(block as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(offset, |inode: &mut DiskInode| {
                if !inode.is_file() {
                    return false;
                }
                inode.add_link();
                true
            });
        if !is_file {
            return false;
        }
        self.modify_disk_inode(|dir| {
            let count = dir.size as usize / DIRENT_SZ;
            let mut entry = DirEntry::empty();
            let reusable = (0..count).find(|&i| {
                dir.read_at(i * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                entry.name().is_empty()
            });
            let index = reusable.unwrap_or(count);
            if reusable.is_none() {
                self.increase_size(((count + 1) * DIRENT_SZ) as u32, dir, &mut fs);
            }
            dir.write_at(
                index * DIRENT_SZ,
                DirEntry::new(dst, source_id).as_bytes(),
                &self.block_device,
            );
        });
        block_cache_sync_all();
        true
    }

    /// Remove one directory entry by name.
    /// Remove one directory entry with the given name.
    pub fn unlink(&self, name: &str) -> bool {
        if name.is_empty() || name.len() > 27 || name.contains('/') {
            return false;
        }
        let fs = self.fs.lock();
        let removed = self.modify_disk_inode(|dir| {
            let count = dir.size as usize / DIRENT_SZ;
            let mut target = DirEntry::empty();
            let index = (0..count).find(|&i| {
                dir.read_at(i * DIRENT_SZ, target.as_bytes_mut(), &self.block_device);
                target.name() == name
            });
            let Some(index) = index else {
                return None;
            };
            let inode_id = target.inode_id();
            dir.write_at(
                index * DIRENT_SZ,
                DirEntry::empty().as_bytes(),
                &self.block_device,
            );
            Some(inode_id)
        });
        let Some(inode_id) = removed else {
            return false;
        };
        let (block, offset) = fs.get_disk_inode_pos(inode_id);
        get_block_cache(block as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(offset, |inode: &mut DiskInode| inode.remove_link());
        block_cache_sync_all();
        true
    }

    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                if !dirent.name().is_empty() {
                    v.push(String::from(dirent.name()));
                }
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }
}
