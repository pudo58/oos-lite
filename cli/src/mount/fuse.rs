use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory,
    ReplyEmpty, ReplyEntry, ReplyOpen, Request,
};
use oos_lite_core::vfs::{VfsNode, VfsNodeType, VfsTree};
use oos_lite_core::StorageEngine;
use tracing::{error, info};

const TTL: Duration = Duration::from_secs(1); // 1 second attribute cache

pub fn mount_fuse(
    engine: Arc<StorageEngine>,
    mountpoint: &Path,
    cache_mb: usize,
) -> anyhow::Result<()> {
    if !mountpoint.exists() {
        std::fs::create_dir_all(mountpoint)?;
    }

    info!(
        mountpoint = %mountpoint.display(),
        cache_mb = cache_mb,
        "Building static point-in-time VFS tree for FUSE mount..."
    );

    let vfs = Arc::new(VfsTree::build(engine, cache_mb * 1024 * 1024)?);

    info!(
        mountpoint = %mountpoint.display(),
        "Mounting read-only OOS-Lite FUSE filesystem..."
    );
    println!("==> OOS-Lite Read-Only FUSE Filesystem mounted at {}", mountpoint.display());
    println!("    Structure: /current, /snapshots, /history");
    println!("    Press Ctrl+C to unmount.");

    let options = vec![
        MountOption::RO,
        MountOption::FSName("oos-lite".to_string()),
        MountOption::AutoUnmount,
    ];

    let fs = OosFuseFilesystem { vfs };
    fuser::mount2(fs, mountpoint, &options)?;

    Ok(())
}

pub struct OosFuseFilesystem {
    pub vfs: Arc<VfsTree>,
}

impl OosFuseFilesystem {
    fn node_to_attr(&self, node: &VfsNode) -> FileAttr {
        let kind = match node.kind {
            VfsNodeType::Directory => FileType::Directory,
            VfsNodeType::RegularFile => FileType::RegularFile,
            VfsNodeType::Symlink => FileType::Symlink,
        };

        // 0o555 for directories and symlinks (r-x r-x r-x)
        // 0o444 for regular files (r-- r-- r--)
        let perm = match node.kind {
            VfsNodeType::Directory | VfsNodeType::Symlink => 0o555,
            VfsNodeType::RegularFile => 0o444,
        };

        let time = UNIX_EPOCH + Duration::from_secs(node.mtime);

        FileAttr {
            ino: node.ino,
            size: node.size,
            blocks: (node.size + 511) / 512,
            atime: time,
            mtime: time,
            ctime: time,
            crtime: time,
            kind,
            perm,
            nlink: if node.kind == VfsNodeType::Directory { 2 } else { 1 },
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            flags: 0,
            blksize: 512,
        }
    }
}

impl Filesystem for OosFuseFilesystem {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = match name.to_str() {
            Some(s) => s,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        if let Some(node) = self.vfs.lookup_child(parent, name_str) {
            let attr = self.node_to_attr(node);
            reply.entry(&TTL, &attr, 0);
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        if let Some(node) = self.vfs.get_node(ino) {
            let attr = self.node_to_attr(node);
            reply.attr(&TTL, &attr);
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn readlink(&mut self, _req: &Request, ino: u64, reply: ReplyData) {
        if let Some(node) = self.vfs.get_node(ino) {
            if let Some(ref target) = node.symlink_target {
                reply.data(target.as_bytes());
            } else {
                reply.error(libc::EINVAL);
            }
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn open(&mut self, _req: &Request, ino: u64, flags: i32, reply: ReplyOpen) {
        let accmode = flags & libc::O_ACCMODE;
        if accmode != libc::O_RDONLY {
            reply.error(libc::EACCES);
            return;
        }

        if let Some(node) = self.vfs.get_node(ino) {
            if node.kind == VfsNodeType::RegularFile {
                reply.opened(0, 0);
            } else {
                reply.error(libc::EISDIR);
            }
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        if offset < 0 {
            reply.error(libc::EINVAL);
            return;
        }

        let node = match self.vfs.get_node(ino) {
            Some(n) => n,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        if node.kind != VfsNodeType::RegularFile {
            reply.error(libc::EISDIR);
            return;
        }

        let manifest_id = match &node.manifest_id {
            Some(id) => id,
            None => {
                reply.data(&[]);
                return;
            }
        };

        match self.vfs.read_range(manifest_id, offset as u64, size as u64) {
            Ok(bytes) => reply.data(&bytes),
            Err(e) => {
                error!(ino = ino, offset = offset, size = size, error = %e, "FUSE read_range failed");
                reply.error(libc::EIO);
            }
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let node = match self.vfs.get_node(ino) {
            Some(n) => n,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        if node.kind != VfsNodeType::Directory {
            reply.error(libc::ENOTDIR);
            return;
        }

        let entries = match self.vfs.readdir(ino) {
            Some(e) => e,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        let mut full_list = Vec::with_capacity(entries.len() + 2);
        full_list.push((node.ino, FileType::Directory, ".".to_string()));
        full_list.push((node.parent_ino, FileType::Directory, "..".to_string()));

        for child in entries {
            let kind = match child.kind {
                VfsNodeType::Directory => FileType::Directory,
                VfsNodeType::RegularFile => FileType::RegularFile,
                VfsNodeType::Symlink => FileType::Symlink,
            };
            full_list.push((child.ino, kind, child.name.clone()));
        }

        for (i, (child_ino, child_kind, child_name)) in full_list.into_iter().enumerate().skip(offset as usize) {
            if reply.add(child_ino, (i + 1) as i64, child_kind, &child_name) {
                break;
            }
        }

        reply.ok();
    }

    // --- Strict Read-Only Enforcement (Return EROFS for all mutating syscalls) ---

    fn mkdir(&mut self, _req: &Request, _parent: u64, _name: &OsStr, _mode: u32, _umask: u32, reply: ReplyEntry) {
        reply.error(libc::EROFS);
    }

    fn unlink(&mut self, _req: &Request, _parent: u64, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(libc::EROFS);
    }

    fn rmdir(&mut self, _req: &Request, _parent: u64, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(libc::EROFS);
    }

    fn rename(&mut self, _req: &Request, _parent: u64, _name: &OsStr, _newparent: u64, _newname: &OsStr, _flags: u32, reply: ReplyEmpty) {
        reply.error(libc::EROFS);
    }

    fn write(&mut self, _req: &Request, _ino: u64, _fh: u64, _offset: i64, _data: &[u8], _write_flags: u32, _flags: i32, _lock_owner: Option<u64>, reply: fuser::ReplyWrite) {
        reply.error(libc::EROFS);
    }

    fn setattr(&mut self, _req: &Request, _ino: u64, _mode: Option<u32>, _uid: Option<u32>, _gid: Option<u32>, _size: Option<u64>, _atime: Option<fuser::TimeOrNow>, _mtime: Option<fuser::TimeOrNow>, _ctime: Option<std::time::SystemTime>, _fh: Option<u64>, _crtime: Option<std::time::SystemTime>, _chgtime: Option<std::time::SystemTime>, _btime: Option<std::time::SystemTime>, _flags: Option<u32>, reply: ReplyAttr) {
        reply.error(libc::EROFS);
    }
}
