//! godawful async vfs

pub mod kernel;
pub mod proc;
pub mod sys;
pub mod tar;
pub mod user;

use crate::{arch::PhysicalAddress, array::ConsistentIndexArray, process::Buffer};
use alloc::{
    boxed::Box,
    collections::{BTreeMap, VecDeque},
    format,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use common::{Errno, FileKind, FileMode, FileStat, GroupId, OpenFlags, Permissions, Result, SeekKind, UnlinkFlags, UserId};
use core::sync::atomic::{AtomicI64, AtomicU8, AtomicUsize, Ordering};
use log::{debug, trace};
use spin::{Mutex, RwLock};

/// a callback ran when a filesystem request has been completed. it's passed the result of the operation, and whether the operation blocked before completing
/// (so that any task associated with it won't be re-queued multiple times)
pub trait RequestCallback<T> = FnOnce(Result<T>, bool);

/// a handle denoting a unique open file in a filesystem
pub type HandleNum = usize;

pub trait Filesystem: Send + Sync {
    /// gets a handle to the root directory of this filesystem
    fn get_root_dir(&self) -> HandleNum;

    /// change the permissions of a file to those provided
    fn chmod(&self, handle: HandleNum, permissions: Permissions, callback: Box<dyn RequestCallback<()>>);

    /// change the owner and group of a file to those provided
    fn chown(&self, handle: HandleNum, owner: UserId, group: GroupId, callback: Box<dyn RequestCallback<()>>);

    /// close this file handle
    fn close(&self, handle: HandleNum);

    /// open a new file in the directory pointed to by this file handle
    fn open(&self, handle: HandleNum, name: String, flags: OpenFlags, callback: Box<dyn RequestCallback<HandleNum>>);

    /// read from a file at the specified position
    fn read(&self, handle: HandleNum, position: i64, buffer: Buffer, callback: Box<dyn RequestCallback<usize>>);

    /// get information about a file
    fn stat(&self, handle: HandleNum, callback: Box<dyn RequestCallback<FileStat>>);

    /// truncate a file to the given length
    fn truncate(&self, handle: HandleNum, length: i64, callback: Box<dyn RequestCallback<()>>);

    /// remove a file from the directory pointed to by this file handle
    fn unlink(&self, handle: HandleNum, name: String, flags: UnlinkFlags, callback: Box<dyn RequestCallback<()>>);

    /// write to a file at the specified position
    fn write(&self, handle: HandleNum, position: i64, buffer: Buffer, callback: Box<dyn RequestCallback<usize>>);

    /// gets the physical address for a page frame containing data for the given file handle at the given position to be mapped into a process' memory map on a page fault or similar
    ///
    /// # Arguments
    /// * `map` - a reference to the memory map that the page will be mapped into, used for reference counting
    /// * `handle` - the file handle whose contents are to be mapped into memory
    /// * `offset` - the offset into the file pointed to by `handle` that memory should be mapped from. must be page aligned
    /// * `callback` - a function to call with the result of this function and whether any operations would have required the process to block until completion
    fn get_page(&self, handle: HandleNum, offset: i64, callback: Box<dyn FnOnce(Option<PhysicalAddress>, bool)>);
}

type NamespaceMap = Arc<RwLock<BTreeMap<String, Arc<dyn Filesystem>>>>;

pub struct FsEnvironment {
    pub namespace: NamespaceMap,
    cwd: RwLock<OpenFile>,
    root: RwLock<OpenFile>,
    fs_list_dir: OpenFile,
    fs_list: Arc<dyn Filesystem>,
    file_descriptors: Arc<Mutex<ConsistentIndexArray<OpenFile>>>,
}

impl FsEnvironment {
    pub fn new() -> Self {
        let namespace = Arc::new(RwLock::new(BTreeMap::new()));
        let fs_list = Arc::new(kernel::KernelFs::new(Arc::new(FsList { namespace: namespace.clone() })));
        let fs_list_dir = OpenFile {
            handle: Arc::new(FileHandle {
                filesystem: fs_list.clone(),
                handle: fs_list.get_root_dir().into(),
            }),
            seek_pos: Arc::new(AtomicI64::new(0)),
            path: AbsolutePath {
                path: vec![].into(),
                name: "..".to_string().into(),
            },
            flags: RwLock::new(OpenFlags::Read),
            kind: AtomicU8::new(FileKind::Directory as u8),
        };

        Self {
            namespace,
            cwd: RwLock::new(fs_list_dir.clone()),
            root: RwLock::new(fs_list_dir.clone()),
            fs_list_dir,
            fs_list,
            file_descriptors: Arc::new(Mutex::new(ConsistentIndexArray::new())),
        }
    }

    pub fn fork(&self) -> Result<Self> {
        let mut file_descriptors = ConsistentIndexArray::new();

        // duplicate all open file descriptors
        {
            let existing_fds = self.file_descriptors.lock();
            for (index, open_file) in existing_fds.as_slice().iter().enumerate() {
                if let Some(file) = open_file && *file.flags.read() & OpenFlags::CloseOnExec == OpenFlags::None {
                    file_descriptors.set(index, file.duplicate()).map_err(|_| Errno::OutOfMemory)?;
                }
            }
        }

        Ok(Self {
            namespace: self.namespace.clone(),
            cwd: RwLock::new(self.cwd.read().clone()),
            root: RwLock::new(self.root.read().clone()),
            fs_list_dir: self.fs_list_dir.clone(),
            fs_list: self.fs_list.clone(),
            file_descriptors: Arc::new(Mutex::new(file_descriptors)),
        })
    }

    /// implements POSIX `chmod`, blocking
    pub fn chmod(&self, file_descriptor: usize, permissions: Permissions, callback: Box<dyn RequestCallback<()>>) {
        let file = { self.file_descriptors.lock().get(file_descriptor).cloned() };
        if let Some(file) = file {
            file.chmod(permissions, callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// implements POSIX `chown`, blocking
    pub fn chown(&self, file_descriptor: usize, owner: UserId, group: GroupId, callback: Box<dyn RequestCallback<()>>) {
        let file = { self.file_descriptors.lock().get(file_descriptor).cloned() };
        if let Some(file) = file {
            file.chown(owner, group, callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// implements POSIX `close`, non-blocking
    pub fn close(&self, file_descriptor: usize) -> Result<()> {
        self.file_descriptors.lock().remove(file_descriptor).ok_or(Errno::BadFile).map(|_| ())
    }

    /// parses a path, removing any . or .. elements, and detects whether the new path is relative or absolute
    fn simplify_path(container_path: &[String], path: &str) -> (Vec<String>, bool) {
        trace!("simplifying path {path:?} at {container_path:?}");

        let mut path_stack = Vec::new();
        let mut is_absolute = false;

        for component in path.split('/') {
            match component {
                "." | "" => (),
                ".." => {
                    if path_stack.pop().is_none() && !is_absolute {
                        is_absolute = true;
                        path_stack.extend_from_slice(container_path);
                    }
                }
                _ => path_stack.push(component.to_string()),
            }
        }

        trace!("simplified to {path_stack:?}, absolute {is_absolute}");

        (path_stack, is_absolute)
    }

    /// iterates path elements, double checking permissions and resolving symlinks
    #[allow(clippy::too_many_arguments)] // it's probably fine since theyre local to this function
    fn resolve_internal(arc_self: Arc<Self>, at: Arc<FileHandle>, absolute_path: AbsolutePath, path: VecDeque<String>, no_follow: bool, callback: Box<dyn RequestCallback<ResolvedHandle>>) {
        fn open_step(
            arc_self: Arc<FsEnvironment>,
            last: Option<Arc<FileHandle>>,
            at: Arc<FileHandle>,
            path: Arc<Mutex<VecDeque<String>>>,
            absolute_path: AbsolutePath,
            kind: FileKind,
            no_follow: bool,
            blocked: bool,
            callback: Box<dyn RequestCallback<ResolvedHandle>>,
        ) {
            // get the next component of the path, or return the container and actual path (without any symlinks) if there isn't one
            let back = path.lock().pop_back();
            let component = match back {
                Some(component) => component,
                None => {
                    return callback(
                        Ok(ResolvedHandle {
                            container: last.unwrap_or(at),
                            path: absolute_path,
                            kind: AtomicU8::new(kind as u8),
                        }),
                        blocked,
                    )
                }
            };

            // makes an open request for the next component in the path
            at.clone().open(
                component.to_string(),
                OpenFlags::Read,
                Box::new(move |res, open_blocked| match res {
                    Ok(handle) => {
                        let arc_self = arc_self.clone();
                        let filesystem = at.filesystem.clone();
                        let prev = at.clone();
                        let path = path.clone();

                        stat_step(
                            arc_self,
                            Some(prev),
                            Arc::new(FileHandle { filesystem, handle: handle.into() }),
                            path,
                            absolute_path,
                            no_follow,
                            blocked || open_blocked,
                            callback,
                        );
                    }
                    Err(err) => callback(Err(err), blocked || open_blocked),
                }),
            );
        }

        fn stat_step(
            arc_self: Arc<FsEnvironment>,
            last: Option<Arc<FileHandle>>,
            at: Arc<FileHandle>,
            path: Arc<Mutex<VecDeque<String>>>,
            absolute_path: AbsolutePath,
            no_follow: bool,
            blocked: bool,
            callback: Box<dyn RequestCallback<ResolvedHandle>>,
        ) {
            // makes a stat request for the current component in the path and handles it accordingly
            at.clone().stat(Box::new(move |res, stat_blocked| match res {
                Ok(stat) => {
                    let arc_self = arc_self.clone();
                    let last = last.clone();
                    let at = at.clone();
                    let path = path.clone();

                    match stat.mode.kind {
                        FileKind::Directory => (),
                        FileKind::SymLink => {
                            if !no_follow {
                                return symlink_step(arc_self, last, at, path, absolute_path, no_follow, blocked || stat_blocked, callback, stat.size);
                            } else if path.lock().back().is_some() {
                                return callback(Err(Errno::NotDirectory), blocked || stat_blocked);
                            }
                        }
                        _ => {
                            if path.lock().back().is_some() {
                                // there are still more components in the path and this isn't a directory or a symlink to one, so give up
                                return callback(Err(Errno::NotDirectory), blocked || stat_blocked);
                            }
                        }
                    }

                    let last = last.clone();

                    open_step(arc_self, last, at, path, absolute_path, stat.mode.kind, no_follow, blocked || stat_blocked, callback);
                }
                Err(err) => callback(Err(err), blocked || stat_blocked),
            }));
        }

        fn symlink_step(
            arc_self: Arc<FsEnvironment>,
            last: Option<Arc<FileHandle>>,
            at: Arc<FileHandle>,
            path: Arc<Mutex<VecDeque<String>>>,
            absolute_path: AbsolutePath,
            no_follow: bool,
            blocked: bool,
            callback: Box<dyn RequestCallback<ResolvedHandle>>,
            length: i64,
        ) {
            let length: usize = match length.try_into() {
                Ok(length) => length,
                Err(_) => return callback(Err(Errno::FileTooBig), blocked),
            };

            let buffer = Arc::new(Mutex::new(vec![0; length].into_boxed_slice()));

            // makes a read request to read the target of the symlink
            at.clone().read(
                0,
                buffer.clone().into(),
                Box::new(move |res, read_blocked| {
                    let actual_blocked = blocked || read_blocked;
                    let arc_self = arc_self.clone();

                    let mut split_pos = absolute_path.path.len() - path.lock().len() - 1;
                    if split_pos >= absolute_path.path.len() {
                        split_pos = absolute_path.path.len() - 1;
                    }
                    let container_path = AbsolutePath {
                        path: absolute_path.path[..split_pos].to_vec().into(),
                        name: absolute_path.path[split_pos].to_string().into(),
                    };

                    let slice = buffer.lock();
                    match res.and_then(|bytes_read| core::str::from_utf8(&slice[..bytes_read]).map_err(|_| Errno::LinkSevered)) {
                        // got a valid string, recurse to find the target of the symlink and use that
                        Ok(str) => FsEnvironment::resolve_container(
                            arc_self.clone(),
                            Some(OpenFile {
                                handle: last.clone().unwrap_or_else(|| at.clone()),
                                seek_pos: Arc::new(0.into()),
                                path: container_path,
                                flags: OpenFlags::Read.into(),
                                kind: AtomicU8::new(FileKind::Directory as u8),
                            }),
                            str.to_string(),
                            no_follow,
                            Box::new(move |res, blocked| {
                                match res {
                                    Ok(handle) => {
                                        if path.lock().is_empty() {
                                            // if the path ends here just return the result
                                            callback(Ok(handle), blocked || actual_blocked);
                                        } else {
                                            // it doesn't, recurse and keep searching
                                            path.lock().push_back(handle.path.name.to_string());

                                            debug!("handle path is {:?}", handle.path);

                                            let mut new_path_vec = Vec::new();
                                            new_path_vec.extend_from_slice(&handle.path.path);
                                            new_path_vec.push(handle.path.name.to_string());
                                            if split_pos + 2 < absolute_path.path.len() {
                                                new_path_vec.extend_from_slice(&absolute_path.path[split_pos + 2..]);
                                            }

                                            let absolute_path = AbsolutePath {
                                                path: new_path_vec.into(),
                                                name: absolute_path.name,
                                            };

                                            let kind = handle.kind();
                                            open_step(arc_self, None, handle.container, path, absolute_path, kind, no_follow, actual_blocked, callback);
                                        }
                                    }
                                    Err(err) => callback(Err(err), actual_blocked),
                                }
                            }),
                        ),
                        Err(err) => callback(Err(err), actual_blocked),
                    }
                }),
            );
        }

        open_step(arc_self, None, at, Mutex::new(path).into(), absolute_path, FileKind::Directory, no_follow, false, callback);
    }

    fn concat_slices(a: &[String], b: &str, c: &[String]) -> Vec<String> {
        let mut new_vec = a.to_vec();
        new_vec.reserve_exact(c.len() + 1);
        new_vec.push(b.to_string());
        new_vec.extend_from_slice(c);
        new_vec
    }

    fn slice_to_deque(slice: &[String]) -> VecDeque<String> {
        let mut queue = VecDeque::new();
        for item in slice.iter() {
            queue.push_front(item.to_string());
        }
        queue
    }

    /// resolves an absolute path in the filesystem (i.e. one that starts with /) after simplification
    fn resolve_absolute_path(arc_self: Arc<Self>, mut path: Vec<String>, no_follow: bool, callback: Box<dyn RequestCallback<ResolvedHandle>>) {
        let mut path_queue = Self::slice_to_deque(&path);
        let name = Arc::new(path.pop().unwrap_or_else(|| "..".to_string()));
        let path = AbsolutePath { path: path.into(), name };

        if let Some(fs) = path_queue.pop_back() {
            if let Some(fs) = arc_self.namespace.read().get(&fs) {
                if path_queue.is_empty() {
                    // path queue is empty, just use the fs list. open() can just check for this and open the right root directory, unlink() doesn't give a shit because the fs list is read only
                    callback(
                        Ok(ResolvedHandle {
                            container: arc_self.fs_list_dir.handle.clone(),
                            path,
                            kind: AtomicU8::new(FileKind::Directory as u8),
                        }),
                        false,
                    );
                } else {
                    // resolve path from this namespace
                    Self::resolve_internal(
                        arc_self.clone(),
                        FileHandle {
                            filesystem: fs.clone(),
                            handle: fs.get_root_dir().into(),
                        }
                        .into(),
                        path.clone(),
                        path_queue,
                        no_follow,
                        callback,
                    );
                }
            } else {
                callback(Err(Errno::NoSuchFileOrDir), false);
            }
        } else {
            // there's nothing in the path, just return the fs list
            callback(
                Ok(ResolvedHandle {
                    container: arc_self.fs_list_dir.handle.clone(),
                    path,
                    kind: AtomicU8::new(FileKind::Directory as u8),
                }),
                false,
            );
        }
    }

    /// resolves a relative path in the filesystem (i.e. one that doesn't start with /) after simplification
    fn resolve_relative_path(arc_self: Arc<Self>, at: Arc<FileHandle>, mut absolute_path: Vec<String>, mut path: Vec<String>, no_follow: bool, callback: Box<dyn RequestCallback<ResolvedHandle>>) {
        let path_queue = Self::slice_to_deque(&path);
        let name = Arc::new(path.pop().or(absolute_path.pop()).unwrap_or_else(|| "..".to_string()));
        let path = AbsolutePath { path: absolute_path.into(), name };

        Self::resolve_internal(arc_self, at, path.clone(), path_queue, no_follow, callback);
    }

    fn resolve_container(arc_self: Arc<Self>, at: Option<OpenFile>, path: String, no_follow: bool, callback: Box<dyn RequestCallback<ResolvedHandle>>) {
        match path.chars().next() {
            Some('/') => {
                // parse absolute path
                let root = arc_self.root.read().clone();
                let (path, is_absolute) = Self::simplify_path(&root.path.path, &path);

                if is_absolute || (root.path.path.is_empty() && &*root.path.name == "..") {
                    // simplified path resolves from /../
                    drop(root.path);
                    Self::resolve_absolute_path(arc_self, path, no_follow, callback);
                } else {
                    // simplified path resolves from root
                    let new_path = Self::concat_slices(&root.path.path, &root.path.name, &path);
                    drop(root.path);
                    Self::resolve_relative_path(arc_self, root.handle, new_path, path, no_follow, callback);
                }
            }
            Some(_) => {
                // parse relative path
                if let Some(fd) = at {
                    let (path, is_absolute) = Self::simplify_path(&fd.path.path, &path);

                    if is_absolute || (fd.path.path.is_empty() && &*fd.path.name == "..") {
                        // simplified path resolves from /../
                        drop(fd.path);
                        Self::resolve_absolute_path(arc_self, path, no_follow, callback);
                    } else {
                        // simplified path resolves from file descriptor
                        let new_path = Self::concat_slices(&fd.path.path, &fd.path.name, &path);
                        drop(fd.path);
                        Self::resolve_relative_path(arc_self, fd.handle, new_path, path, no_follow, callback);
                    }
                } else {
                    let cwd = arc_self.cwd.read().clone();
                    let (path, is_absolute) = Self::simplify_path(&cwd.path.path, &path);

                    if is_absolute || (cwd.path.path.is_empty() && &*cwd.path.name == "..") {
                        // simplified path resolves from /../
                        drop(cwd.path);
                        Self::resolve_absolute_path(arc_self, path, no_follow, callback);
                    } else {
                        // simplified path resolves from cwd
                        let new_path = Self::concat_slices(&cwd.path.path, &cwd.path.name, &path);
                        drop(cwd.path);
                        Self::resolve_relative_path(arc_self, cwd.handle, new_path, path, no_follow, callback);
                    }
                }
            }
            None => callback(Err(Errno::InvalidArgument), false),
        }
    }

    /// implements POSIX `open`, blocking
    pub fn open(arc_self: Arc<Self>, at: usize, path: String, flags: OpenFlags, callback: Box<dyn RequestCallback<usize>>) {
        let at = if flags & OpenFlags::AtCWD == OpenFlags::None {
            match arc_self.file_descriptors.lock().get(at) {
                Some(fd) => Some(fd.clone()),
                None => return callback(Err(Errno::BadFile), false),
            }
        } else {
            None
        };

        let file_descriptors = arc_self.file_descriptors.clone();
        let namespace = arc_self.namespace.clone();

        Self::resolve_container(
            arc_self.clone(),
            at,
            path,
            flags & OpenFlags::NoFollow != OpenFlags::None,
            Box::new(move |res, blocked| match res {
                Ok(resolved) => {
                    if flags & OpenFlags::Directory != OpenFlags::None && resolved.kind() != FileKind::Directory {
                        return callback(Err(Errno::NotDirectory), blocked);
                    } else if flags & OpenFlags::SymLink != OpenFlags::None && resolved.kind() != FileKind::SymLink {
                        return callback(Err(Errno::NoSuchFileOrDir), blocked);
                    }

                    if resolved.path.path.is_empty() {
                        // this path points to the root of a filesystem or the filesystem list
                        let name = &*resolved.path.name;
                        if name == ".." {
                            callback(file_descriptors.lock().add(arc_self.fs_list_dir.duplicate()).map_err(|_| Errno::OutOfMemory), blocked);
                        } else if let Some(fs) = namespace.read().get(name) {
                            let handle = FileHandle {
                                filesystem: fs.clone(),
                                handle: fs.get_root_dir().into(),
                            };

                            // create the OpenFile object for this handle
                            let open_file = OpenFile {
                                handle: handle.into(),
                                seek_pos: AtomicI64::new(0).into(),
                                path: resolved.path.clone(),
                                flags: flags.into(),
                                kind: AtomicU8::new(resolved.kind.load(Ordering::SeqCst)),
                            };

                            // add the new handle to the file descriptor list
                            callback(file_descriptors.lock().add(open_file).map_err(|_| Errno::OutOfMemory), blocked);
                        } else {
                            callback(Err(Errno::NoSuchFileOrDir), blocked);
                        }
                    } else {
                        let name = resolved.path.name.clone();
                        let path = resolved.path.clone();
                        let kind = resolved.kind.load(Ordering::SeqCst);
                        let file_descriptors = file_descriptors.clone();
                        let filesystem = resolved.container.filesystem.clone();

                        // open the file with the proper flags
                        resolved.container.open(
                            name.to_string(),
                            flags & !(OpenFlags::CloseOnExec | OpenFlags::AtCWD),
                            Box::new(move |res, open_blocked| match res {
                                Ok(handle) => {
                                    let handle = FileHandle {
                                        filesystem: filesystem.clone(),
                                        handle: handle.into(),
                                    };

                                    // create the OpenFile object for this handle
                                    let open_file = OpenFile {
                                        handle: handle.into(),
                                        seek_pos: AtomicI64::new(0).into(),
                                        path: path.clone(),
                                        flags: flags.into(),
                                        kind: AtomicU8::new(kind),
                                    };

                                    // add the new handle to the file descriptor list
                                    callback(file_descriptors.lock().add(open_file).map_err(|_| Errno::OutOfMemory), blocked || open_blocked);
                                }
                                Err(err) => callback(Err(err), blocked || open_blocked),
                            }),
                        );
                    }
                }
                Err(err) => callback(Err(err), blocked),
            }),
        );
    }

    /// implements POSIX `read`, blocking
    pub fn read(&self, file_descriptor: usize, buffer: Buffer, callback: Box<dyn RequestCallback<usize>>) {
        let file = { self.file_descriptors.lock().get(file_descriptor).cloned() };
        if let Some(file) = file {
            file.read(buffer, callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// implements POSIX `seek`, partially blocking
    pub fn seek(&self, file_descriptor: usize, offset: i64, kind: SeekKind, callback: Box<dyn RequestCallback<i64>>) {
        let file = { self.file_descriptors.lock().get(file_descriptor).cloned() };
        if let Some(file) = file {
            file.seek(offset, kind, callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// implements POSIX `stat`, blocking
    pub fn stat(&self, file_descriptor: usize, callback: Box<dyn RequestCallback<FileStat>>) {
        let file = { self.file_descriptors.lock().get(file_descriptor).cloned() };
        if let Some(file) = file {
            file.stat(callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// implements POSIX `truncate`, blocking
    pub fn truncate(&self, file_descriptor: usize, len: i64, callback: Box<dyn RequestCallback<()>>) {
        let file = { self.file_descriptors.lock().get(file_descriptor).cloned() };
        if let Some(file) = file {
            file.truncate(len, callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// implements POSIX `unlink`, blocking
    pub fn unlink(arc_self: Arc<Self>, at: usize, path: String, flags: UnlinkFlags, callback: Box<dyn RequestCallback<()>>) {
        let at = if flags & UnlinkFlags::AtCWD == UnlinkFlags::None {
            match arc_self.file_descriptors.lock().get(at) {
                Some(fd) => Some(fd.clone()),
                None => return callback(Err(Errno::BadFile), false),
            }
        } else {
            None
        };

        Self::resolve_container(
            arc_self.clone(),
            at,
            path,
            false,
            Box::new(move |res, blocked| match res {
                Ok(resolved) => {
                    let name = resolved.path.name.to_string();

                    if resolved.path.path.is_empty() {
                        if name != ".." && let Some(fs) = arc_self.namespace.read().get(&name) {
                            fs.unlink(fs.get_root_dir(), name, flags, callback);
                        } else {
                            callback(Err(Errno::NoSuchFileOrDir), blocked);
                        }
                    } else {
                        resolved.container.unlink(name, flags, callback);
                    }
                }
                Err(err) => callback(Err(err), blocked),
            }),
        );
    }

    /// implements POSIX `write`, blocking
    pub fn write(&self, file_descriptor: usize, buffer: Buffer, callback: Box<dyn RequestCallback<usize>>) {
        let file = { self.file_descriptors.lock().get(file_descriptor).cloned() };
        if let Some(file) = file {
            file.write(buffer, callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// changes the root directory of this environment to the directory pointed to by the given file descriptor
    pub fn chroot(&self, file_descriptor: usize) -> Result<()> {
        *self.root.write() = self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.duplicate();
        Ok(())
    }

    /// changes the current working directory of this environment to the directory pointed to by the given file descriptor
    pub fn chdir(&self, file_descriptor: usize) -> Result<()> {
        *self.cwd.write() = self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.duplicate();
        Ok(())
    }

    fn get_path_to(&self, open_file: &OpenFile) -> String {
        let root = self.root.read();
        //trace!("root path is {:?}, root name is {:?}, open_file path is {:?}", root.path, root.name, open_file.path);

        // try to format the path relative to the root path if possible
        for (index, name) in root.path.path.iter().chain(Some(&root.path.name.to_string())).enumerate() {
            if open_file.path.path.get(index) != Some(name) {
                if open_file.path.path.is_empty() && open_file.path.name.as_str() == ".." {
                    return "/..".to_string();
                } else {
                    let joined = open_file.path.path.join("/");
                    return format!("/../{joined}{}{}", if joined.is_empty() { "" } else { "/" }, open_file.path.name);
                }
            }
        }

        let joined = open_file.path.path[root.path.path.len() + 1..].join("/");
        format!("/{joined}{}{}", if joined.is_empty() { "" } else { "/" }, open_file.path.name)
    }

    /// gets the path to the current working directory of the current process
    pub fn get_cwd_path(&self) -> String {
        self.get_path_to(&self.cwd.read())
    }

    /// gets the absolute path to the root directory of the current process
    pub fn get_root_path(&self) -> String {
        let root = self.root.read();
        let joined = root.path.path.join("/");
        format!("/../{joined}{}{}", if joined.is_empty() { "" } else { "/" }, root.path.name)
    }

    /// gets the path of the file pointed to by the given file descriptor
    pub fn get_path(&self, file_descriptor: usize) -> Result<String> {
        Ok(self.get_path_to(self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?))
    }

    /// gets the underlying open file object associated with the given file descriptor
    pub fn get_open_file(&self, file_descriptor: usize) -> Option<OpenFile> {
        self.file_descriptors.lock().get(file_descriptor).map(|file| file.duplicate())
    }

    /// implements POSIX dup()
    pub fn dup(&self, file_descriptor: usize) -> common::Result<usize> {
        let new_descriptor = self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.duplicate();
        *new_descriptor.flags.write() &= !common::OpenFlags::CloseOnExec;
        self.file_descriptors.lock().add(new_descriptor).map_err(|_| Errno::OutOfMemory)
    }

    /// implements POSIX dup2()
    pub fn dup2(&self, file_descriptor: usize, new_fd: usize) -> common::Result<()> {
        if file_descriptor == new_fd {
            Ok(())
        } else {
            let new_descriptor = self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.duplicate();
            *new_descriptor.flags.write() &= !common::OpenFlags::CloseOnExec;
            self.file_descriptors.lock().set(new_fd, new_descriptor).map_err(|_| Errno::OutOfMemory)
        }
    }
}

struct ResolvedHandle {
    container: Arc<FileHandle>,
    path: AbsolutePath,
    kind: AtomicU8,
}

impl ResolvedHandle {
    fn kind(&self) -> FileKind {
        unsafe { core::mem::transmute::<u8, FileKind>(self.kind.load(Ordering::SeqCst)) }
    }
}

impl Default for FsEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct FsList {
    namespace: NamespaceMap,
}

impl kernel::FileDescriptor for FsList {
    fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn kernel::FileDescriptor>> {
        if flags & (OpenFlags::Write | OpenFlags::Create) != OpenFlags::None {
            return Err(Errno::ReadOnlyFilesystem);
        }

        if name == ".." {
            Ok(Arc::new(self.clone()))
        } else {
            Err(Errno::NoSuchFileOrDir)
        }
    }

    fn read(&self, position: i64, buffer: Buffer, callback: Box<dyn RequestCallback<usize>>) {
        let position: usize = match position.try_into() {
            Ok(position) => position,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };

        let mut data = Vec::new();

        if let Some(entry) = self.namespace.read().keys().nth(position) {
            data.extend_from_slice(&(0_u32.to_ne_bytes()));
            data.extend_from_slice(entry.as_bytes());
            data.push(0);
        }

        let res = buffer.copy_from(&data);
        callback(res, false);
    }

    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerExecute | Permissions::GroupRead | Permissions::GroupExecute | Permissions::OtherRead | Permissions::OtherExecute,
                kind: FileKind::Directory,
            },
            ..Default::default()
        })
    }
}

pub struct FileHandle {
    filesystem: Arc<dyn Filesystem>,
    handle: AtomicUsize,
}

impl FileHandle {
    /// see `Filesystem::chmod`
    pub fn chmod(&self, permissions: Permissions, callback: Box<dyn RequestCallback<()>>) {
        self.filesystem.chmod(self.handle.load(Ordering::SeqCst), permissions, callback);
    }

    /// see `Filesystem::chown`
    pub fn chown(&self, owner: UserId, group: GroupId, callback: Box<dyn RequestCallback<()>>) {
        self.filesystem.chown(self.handle.load(Ordering::SeqCst), owner, group, callback);
    }

    /// see `Filesystem::open`
    pub fn open(&self, name: String, flags: OpenFlags, callback: Box<dyn RequestCallback<HandleNum>>) {
        self.filesystem.open(self.handle.load(Ordering::SeqCst), name, flags, callback);
    }

    /// see `Filesystem::read`
    pub fn read(&self, position: i64, buffer: Buffer, callback: Box<dyn RequestCallback<usize>>) {
        self.filesystem.read(self.handle.load(Ordering::SeqCst), position, buffer, callback);
    }

    /// see `Filesystem::stat`
    pub fn stat(&self, callback: Box<dyn RequestCallback<FileStat>>) {
        self.filesystem.stat(self.handle.load(Ordering::SeqCst), callback);
    }

    /// see `Filesystem::truncate`
    pub fn truncate(&self, length: i64, callback: Box<dyn RequestCallback<()>>) {
        self.filesystem.truncate(self.handle.load(Ordering::SeqCst), length, callback);
    }

    /// see `Filesystem::unlink`
    pub fn unlink(&self, name: String, flags: UnlinkFlags, callback: Box<dyn RequestCallback<()>>) {
        self.filesystem.unlink(self.handle.load(Ordering::SeqCst), name, flags, callback);
    }

    /// see `Filesystem::write`
    pub fn write(&self, position: i64, buffer: Buffer, callback: Box<dyn RequestCallback<usize>>) {
        self.filesystem.write(self.handle.load(Ordering::SeqCst), position, buffer, callback);
    }

    /// see `Filesystem::get_page`
    pub fn get_page(&self, offset: i64, callback: Box<dyn FnOnce(Option<PhysicalAddress>, bool)>) {
        self.filesystem.get_page(self.handle.load(Ordering::SeqCst), offset, callback);
    }
}

impl Drop for FileHandle {
    fn drop(&mut self) {
        self.filesystem.close(self.handle.load(Ordering::SeqCst))
    }
}

pub struct OpenFile {
    handle: Arc<FileHandle>,
    seek_pos: Arc<AtomicI64>,
    path: AbsolutePath,
    flags: RwLock<OpenFlags>,
    kind: AtomicU8,
}

impl Clone for OpenFile {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            seek_pos: self.seek_pos.clone(),
            path: self.path.clone(),
            flags: RwLock::new(*self.flags.read()),
            kind: AtomicU8::new(self.kind.load(Ordering::SeqCst)),
        }
    }
}

impl OpenFile {
    pub fn duplicate(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            seek_pos: Arc::new(AtomicI64::new(self.seek_pos.load(Ordering::SeqCst))),
            path: self.path.clone(),
            flags: RwLock::new(*self.flags.read()),
            kind: AtomicU8::new(self.kind.load(Ordering::SeqCst)),
        }
    }

    pub fn kind(&self) -> FileKind {
        unsafe { core::mem::transmute::<u8, FileKind>(self.kind.load(Ordering::SeqCst)) }
    }

    pub fn handle(&self) -> Arc<FileHandle> {
        self.handle.clone()
    }

    pub fn chmod(&self, permissions: Permissions, callback: Box<dyn RequestCallback<()>>) {
        self.handle.chmod(permissions, callback);
    }

    pub fn chown(&self, owner: UserId, group: GroupId, callback: Box<dyn RequestCallback<()>>) {
        self.handle.chown(owner, group, callback);
    }

    pub fn open(&self, name: String, flags: OpenFlags, callback: Box<dyn RequestCallback<FileHandle>>) {
        let filesystem = self.handle.filesystem.clone();
        self.handle.open(
            name,
            flags,
            Box::new(move |res, blocked| {
                let filesystem = filesystem.clone();
                callback(
                    res.map(|num| FileHandle {
                        filesystem,
                        handle: AtomicUsize::new(num),
                    }),
                    blocked,
                )
            }),
        );
    }

    pub fn read(&self, buffer: Buffer, callback: Box<dyn RequestCallback<usize>>) {
        let seek_pos = self.seek_pos.clone();
        let position = self.seek_pos.load(Ordering::SeqCst);
        let kind = self.kind();
        self.handle.read(
            position,
            buffer,
            Box::new(move |res, blocked| {
                if let Ok(length) = res {
                    let length: i64 = match length.try_into() {
                        Ok(length) => length,
                        Err(_) => return callback(Err(Errno::ValueOverflow), blocked),
                    };
                    match kind {
                        FileKind::Directory => {
                            seek_pos.fetch_add(1, Ordering::SeqCst);
                        }
                        FileKind::SymLink => (),
                        _ => {
                            let _ = seek_pos.compare_exchange(position, position + length, Ordering::SeqCst, Ordering::Relaxed);
                        }
                    }
                }

                callback(res, blocked);
            }),
        );
    }

    pub fn seek(&self, offset: i64, kind: SeekKind, callback: Box<dyn RequestCallback<i64>>) {
        match kind {
            SeekKind::Set => {
                self.seek_pos.store(offset, Ordering::SeqCst);
                callback(Ok(offset), false);
            }
            SeekKind::Current => {
                callback(Ok(self.seek_pos.fetch_add(offset, Ordering::SeqCst)), false);
            }
            SeekKind::End => {
                // fire off a stat request to get the file size, then complete the seek based on that
                let seek_pos = self.seek_pos.clone();
                self.handle.stat(Box::new(move |res, blocked| {
                    callback(res.map(|res| seek_pos.fetch_add(res.size.saturating_add(offset), Ordering::SeqCst)), blocked);
                }));
            }
        }
    }

    pub fn stat(&self, callback: Box<dyn RequestCallback<FileStat>>) {
        self.handle.stat(callback);
    }

    pub fn truncate(&self, length: i64, callback: Box<dyn RequestCallback<()>>) {
        self.handle.truncate(length, callback);
    }

    pub fn unlink(&self, name: String, flags: UnlinkFlags, callback: Box<dyn RequestCallback<()>>) {
        self.handle.unlink(name, flags, callback);
    }

    pub fn write(&self, buffer: Buffer, callback: Box<dyn RequestCallback<usize>>) {
        let seek_pos = self.seek_pos.clone();
        let position = self.seek_pos.load(Ordering::SeqCst);
        self.handle.write(
            position,
            buffer,
            Box::new(move |res, blocked| {
                if let Ok(length) = res {
                    let length: i64 = match length.try_into() {
                        Ok(length) => length,
                        Err(_) => return callback(Err(Errno::ValueOverflow), blocked),
                    };
                    let _ = seek_pos.compare_exchange(position, position + length, Ordering::SeqCst, Ordering::Relaxed);
                }

                callback(res, blocked);
            }),
        );
    }
}

#[derive(Clone, Debug)]
pub struct AbsolutePath {
    pub path: Arc<Vec<String>>,
    pub name: Arc<String>,
}
