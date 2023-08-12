//! godawful async vfs

pub mod proc;
pub mod sys;
pub mod tar;

use crate::array::ConsistentIndexArray;
use alloc::{
    boxed::Box,
    collections::{BTreeMap, VecDeque},
    format,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use common::{Errno, FileKind, OpenFlags, SeekKind};
use log::debug;
use core::sync::atomic::{AtomicI64, AtomicU8, AtomicUsize, Ordering};
use spin::{Mutex, RwLock};

/// a callback ran when a filesystem request has been completed. it's passed the result of the operation, and whether the operation blocked before completing
/// (so that any task associated with it won't be re-queued multiple times)
pub trait RequestCallback<T> = FnMut(common::Result<T>, bool);

/// a handle denoting a unique open file in a filesystem
pub type HandleNum = usize;

/// an async request that can be made to a filesystem. all requests are associated with a file handle, which isn't provided in the request object itself out of convenience
pub enum Request {
    /// change the permissions of a file to those provided
    Chmod {
        permissions: common::Permissions,
        callback: Box<dyn RequestCallback<()>>,
    },

    /// change the owner and group of a file to those provided
    Chown {
        owner: common::UserId,
        group: common::GroupId,
        callback: Box<dyn RequestCallback<()>>,
    },

    /// close this file handle
    Close,

    /// open a new file in the directory pointed to by this file handle
    Open {
        name: String,
        flags: common::OpenFlags,
        callback: Box<dyn RequestCallback<HandleNum>>,
    },

    /// read from a file at the specified position
    Read {
        position: i64,
        length: usize,
        callback: Box<dyn for<'a> RequestCallback<&'a [u8]>>,
    },

    /// get information about a file
    Stat { callback: Box<dyn RequestCallback<common::FileStat>> },

    /// truncate a file to the given length
    Truncate { len: i64, callback: Box<dyn RequestCallback<()>> },

    /// remove a file from the directory pointed to by this file handle
    Unlink {
        name: String,
        flags: common::UnlinkFlags,
        callback: Box<dyn RequestCallback<()>>,
    },

    /// write to a file at the specified position
    Write {
        length: usize,
        position: i64,
        callback: Box<dyn for<'a> RequestCallback<&'a mut [u8]>>,
    },
}

impl Request {
    /// calls the callback for this request with the given error and blocked state
    pub fn callback_error(&mut self, error: Errno, blocked: bool) {
        match self {
            Self::Chmod { callback, .. } => callback(Err(error), blocked),
            Self::Chown { callback, .. } => callback(Err(error), blocked),
            Self::Close => (),
            Self::Open { callback, .. } => callback(Err(error), blocked),
            Self::Read { callback, .. } => callback(Err(error), blocked),
            Self::Stat { callback, .. } => callback(Err(error), blocked),
            Self::Truncate { callback, .. } => callback(Err(error), blocked),
            Self::Unlink { callback, .. } => callback(Err(error), blocked),
            Self::Write { callback, .. } => callback(Err(error), blocked),
        }
    }
}

pub trait Filesystem: Send + Sync {
    /// gets a handle to the root directory of this filesystem
    fn get_root_dir(&self) -> HandleNum;

    /// makes an async request to the filesystem
    ///
    /// if state must be locked to complete this request, it must be unlocked before calling its callback
    fn make_request(&self, handle: HandleNum, request: Request);
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
        let fs_list = Arc::new(KernelFs::new(Box::new(FsList { namespace: namespace.clone() })));
        let fs_list_dir = OpenFile {
            handle: Arc::new(FileHandle {
                filesystem: fs_list.clone(),
                handle: fs_list.get_root_dir().into(),
            }),
            seek_pos: Arc::new(AtomicI64::new(0)),
            path: Arc::new(Mutex::new(AbsolutePath { path: vec![], name: "..".to_string() })),
            flags: RwLock::new(OpenFlags::Read),
            kind: AtomicU8::new(common::FileKind::Directory as u8),
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

    pub fn fork(&self) -> common::Result<Self> {
        let mut file_descriptors = ConsistentIndexArray::new();

        // duplicate all open file descriptors
        {
            let existing_fds = self.file_descriptors.lock();
            for (index, open_file) in existing_fds.as_slice().iter().enumerate() {
                if let Some(file) = open_file && *file.flags.read() & common::OpenFlags::CloseOnExec != common::OpenFlags::None {
                    file_descriptors.set(index, file.clone()).map_err(|_| Errno::OutOfMemory)?;
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
    pub fn chmod(&self, file_descriptor: usize, permissions: common::Permissions, mut callback: Box<dyn RequestCallback<()>>) {
        if let Some(file) = self.file_descriptors.lock().get(file_descriptor) {
            file.chmod(permissions, callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// implements POSIX `chown`, blocking
    pub fn chown(&self, file_descriptor: usize, owner: common::UserId, group: common::GroupId, mut callback: Box<dyn RequestCallback<()>>) {
        if let Some(file) = self.file_descriptors.lock().get(file_descriptor) {
            file.chown(owner, group, callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// implements POSIX `close`, non-blocking
    pub fn close(&self, file_descriptor: usize) -> common::Result<()> {
        self.file_descriptors.lock().remove(file_descriptor).ok_or(Errno::BadFile).map(|_| ())
    }

    /// parses a path, removing any . or .. elements, and detects whether the new path is relative or absolute
    fn simplify_path(container_path: &[String], path: &str) -> (Vec<String>, bool) {
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

        (path_stack, is_absolute)
    }

    /// iterates path elements, double checking permissions and resolving symlinks
    #[allow(clippy::too_many_arguments)] // it's probably fine since theyre local to this function
    fn resolve_internal(arc_self: Arc<Self>, at: Arc<FileHandle>, path_vec: Arc<Mutex<Vec<String>>>, path: VecDeque<String>, no_follow: bool, callback: Box<dyn RequestCallback<PartiallyResolved>>) {
        fn open_step(
            arc_self: Arc<FsEnvironment>,
            last: Option<Arc<FileHandle>>,
            at: Arc<FileHandle>,
            path: Arc<Mutex<VecDeque<String>>>,
            path_vec: Arc<Mutex<Vec<String>>>,
            kind: FileKind,
            no_follow: bool,
            blocked: bool,
            callback: Arc<Mutex<Box<dyn RequestCallback<PartiallyResolved>>>>,
        ) {
            let back = path.lock().pop_back();
            let component = match back {
                Some(component) => component,
                None => return (callback.lock())(Ok(PartiallyResolved::Handle(last.unwrap_or(at), AtomicU8::new(kind as u8))), blocked), // resolved!
            };

            // makes an open request for the next component in the path
            at.clone().make_request(Request::Open {
                name: component.to_string(),
                flags: OpenFlags::Read,
                callback: Box::new(move |res, open_blocked| match res {
                    Ok(handle) => {
                        let arc_self = arc_self.clone();
                        let filesystem = at.filesystem.clone();
                        let prev = at.clone();
                        let path = path.clone();
                        let path_vec = path_vec.clone();
                        let callback = callback.clone();

                        stat_step(
                            arc_self,
                            Some(prev),
                            Arc::new(FileHandle { filesystem, handle: handle.into() }),
                            path,
                            path_vec,
                            no_follow,
                            blocked || open_blocked,
                            callback,
                        );
                    }
                    Err(err) => (callback.lock())(Err(err), blocked || open_blocked),
                }),
            });
        }

        fn stat_step(
            arc_self: Arc<FsEnvironment>,
            last: Option<Arc<FileHandle>>,
            at: Arc<FileHandle>,
            path: Arc<Mutex<VecDeque<String>>>,
            path_vec: Arc<Mutex<Vec<String>>>,
            no_follow: bool,
            blocked: bool,
            callback: Arc<Mutex<Box<dyn RequestCallback<PartiallyResolved>>>>,
        ) {
            // makes a stat request for the current component in the path and handles it accordingly
            at.clone().make_request(Request::Stat {
                callback: Box::new(move |res, stat_blocked| match res {
                    Ok(stat) => {
                        let arc_self = arc_self.clone();
                        let last = last.clone();
                        let at = at.clone();
                        let callback = callback.clone();
                        let path = path.clone();
                        let path_vec = path_vec.clone();

                        match stat.mode.kind {
                            common::FileKind::Directory => (),
                            common::FileKind::SymLink => {
                                return symlink_step(arc_self, last, at, path, path_vec, no_follow, blocked || stat_blocked, callback, stat.size);
                            }
                            _ => {
                                if path.lock().back().is_some() {
                                    // there are still more components in the path and this isn't a directory or a symlink to one, so give up
                                    return (callback.lock())(Err(Errno::NotDirectory), blocked || stat_blocked);
                                }
                            }
                        }

                        let last = last.clone();

                        open_step(arc_self, last, at, path, path_vec, stat.mode.kind, no_follow, blocked || stat_blocked, callback);
                    }
                    Err(err) => (callback.lock())(Err(err), blocked || stat_blocked),
                }),
            });
        }

        fn symlink_step(
            arc_self: Arc<FsEnvironment>,
            last: Option<Arc<FileHandle>>,
            at: Arc<FileHandle>,
            path: Arc<Mutex<VecDeque<String>>>,
            path_vec: Arc<Mutex<Vec<String>>>,
            no_follow: bool,
            blocked: bool,
            callback: Arc<Mutex<Box<dyn RequestCallback<PartiallyResolved>>>>,
            length: i64,
        ) {
            let length: usize = match length.try_into() {
                Ok(length) => length,
                Err(_) => return (callback.lock())(Err(Errno::FileTooBig), blocked),
            };

            // makes a read request to read the target of the symlink
            at.clone().make_request(Request::Read {
                position: 0,
                length,
                callback: Box::new(move |res, read_blocked| {
                    let actual_blocked = blocked || read_blocked;
                    let arc_self = arc_self.clone();
                    let callback = callback.clone();

                    let path_len = path.lock().len();
                    let absolute_path;
                    let filename;
                    {
                        let path_vec = path_vec.lock();
                        if path_len < path_vec.len() {
                            absolute_path = path_vec[..path_len].to_vec();
                            filename = path_vec[path_len].to_string();
                        } else {
                            absolute_path = vec![];
                            filename = "..".to_string();
                        }
                    }

                    match res {
                        Ok(slice) => match core::str::from_utf8(slice) {
                            // got a valid string, recurse to find the target of the symlink and use that
                            Ok(str) => FsEnvironment::resolve_container(
                                arc_self,
                                Some(OpenFile {
                                    handle: last.clone().unwrap_or_else(|| at.clone()),
                                    seek_pos: Arc::new(0.into()),
                                    path: Mutex::new(AbsolutePath { path: absolute_path, name: filename }).into(),
                                    flags: OpenFlags::Read.into(),
                                    kind: AtomicU8::new(common::FileKind::Directory as u8),
                                }),
                                str.to_string(),
                                no_follow,
                                Box::new(move |res, blocked| (callback.lock())(res.map(PartiallyResolved::Full), blocked || actual_blocked)),
                            ),
                            Err(_) => (callback.lock())(Err(Errno::LinkSevered), actual_blocked),
                        },
                        Err(err) => (callback.lock())(Err(err), actual_blocked),
                    }
                }),
            });
        }

        open_step(arc_self, None, at, Mutex::new(path).into(), path_vec, FileKind::Regular, no_follow, false, Mutex::new(callback).into());
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
    fn resolve_absolute_path(arc_self: Arc<Self>, mut path: Vec<String>, no_follow: bool, mut callback: Box<dyn RequestCallback<ResolvedHandle>>) {
        let mut path_queue = Self::slice_to_deque(&path);
        let name = Arc::new(Mutex::new(path.pop().map(|n| n.to_string()).unwrap_or_else(|| "..".to_string())));
        let path = Arc::new(Mutex::new(path));

        if let Some(fs) = path_queue.pop_back() {
            if let Some(fs) = arc_self.namespace.read().get(&fs) {
                if path_queue.is_empty() {
                    // path queue is empty, just use the fs list. open() can just check for this and open the right root directory, unlink() doesn't give a shit because the fs list is read only
                    callback(
                        Ok(ResolvedHandle {
                            container: arc_self.fs_list_dir.handle.clone(),
                            path: Arc::new(Mutex::new(AbsolutePath {
                                path: path.lock().to_vec(),
                                name: name.lock().to_string(),
                            })),
                            kind: AtomicU8::new(common::FileKind::Directory as u8),
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
                        Box::new(move |res, blocked| {
                            let path = path.clone();
                            let name = name.clone();

                            // call the callback with the proper resolved object
                            callback(
                                res.map(move |container| match container {
                                    PartiallyResolved::Handle(container, kind) => ResolvedHandle {
                                        container,
                                        path: Arc::new(Mutex::new(AbsolutePath {
                                            path: path.lock().to_vec(),
                                            name: name.lock().to_string(),
                                        })),
                                        kind,
                                    },
                                    PartiallyResolved::Full(handle) => handle,
                                }),
                                blocked,
                            );
                        }),
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
                    path: Arc::new(Mutex::new(AbsolutePath {
                        path: path.lock().to_vec(),
                        name: name.lock().to_string(),
                    })),
                    kind: AtomicU8::new(common::FileKind::Directory as u8),
                }),
                false,
            );
        }
    }

    /// resolves a relative path in the filesystem (i.e. one that doesn't start with /) after simplification
    fn resolve_relative_path(arc_self: Arc<Self>, at: Arc<FileHandle>, absolute_path: Vec<String>, mut path: Vec<String>, no_follow: bool, mut callback: Box<dyn RequestCallback<ResolvedHandle>>) {
        let path_queue = Self::slice_to_deque(&path);
        let name = Arc::new(Mutex::new(path.pop().map(|n| n.to_string()).unwrap_or_else(|| "..".to_string())));
        let absolute_path = Arc::new(Mutex::new(absolute_path));

        Self::resolve_internal(
            arc_self,
            at,
            absolute_path.clone(),
            path_queue,
            no_follow,
            Box::new(move |res, blocked| {
                let absolute_path = absolute_path.clone();
                let name = name.clone();

                // call the callback with the proper resolved object
                callback(
                    res.map(move |container| match container {
                        PartiallyResolved::Handle(container, kind) => ResolvedHandle {
                            container,
                            path: Arc::new(Mutex::new(AbsolutePath {
                                path: absolute_path.lock().to_vec(),
                                name: name.lock().to_string(),
                            })),
                            kind,
                        },
                        PartiallyResolved::Full(handle) => handle,
                    }),
                    blocked,
                );
            }),
        );
    }

    fn resolve_container(arc_self: Arc<Self>, at: Option<OpenFile>, path: String, no_follow: bool, mut callback: Box<dyn RequestCallback<ResolvedHandle>>) {
        match path.chars().next() {
            Some('/') => {
                // parse absolute path
                let root = arc_self.root.read().clone();
                let root_path = root.path.lock();
                let (path, is_absolute) = Self::simplify_path(&root_path.path, &path);

                if is_absolute || (root_path.path.is_empty() && &*root_path.name == "..") {
                    // simplified path resolves from /../
                    drop(root_path);
                    Self::resolve_absolute_path(arc_self, path, no_follow, callback);
                } else {
                    // simplified path resolves from root
                    let new_path = Self::concat_slices(&root_path.path, &root_path.name, &path);
                    drop(root_path);
                    Self::resolve_relative_path(arc_self, root.handle, new_path, path, no_follow, callback);
                }
            }
            Some(_) => {
                // parse relative path
                if let Some(fd) = at {
                    let fd_path = fd.path.lock();
                    let (path, is_absolute) = Self::simplify_path(&fd_path.path, &path);

                    if is_absolute || (fd_path.path.is_empty() && &*fd_path.name == "..") {
                        // simplified path resolves from /../
                        drop(fd_path);
                        Self::resolve_absolute_path(arc_self, path, no_follow, callback);
                    } else {
                        // simplified path resolves from file descriptor
                        let new_path = Self::concat_slices(&fd_path.path, &fd_path.name, &path);
                        drop(fd_path);
                        Self::resolve_relative_path(arc_self, fd.handle, new_path, path, no_follow, callback);
                    }
                } else {
                    let cwd = arc_self.cwd.read().clone();
                    let cwd_path = cwd.path.lock();
                    let (path, is_absolute) = Self::simplify_path(&cwd_path.path, &path);

                    if is_absolute || (cwd_path.path.is_empty() && &*cwd_path.name == "..") {
                        // simplified path resolves from /../
                        drop(cwd_path);
                        Self::resolve_absolute_path(arc_self, path, no_follow, callback);
                    } else {
                        // simplified path resolves from cwd
                        let new_path = Self::concat_slices(&cwd_path.path, &cwd_path.name, &path);
                        drop(cwd_path);
                        Self::resolve_relative_path(arc_self, cwd.handle, new_path, path, no_follow, callback);
                    }
                }
            }
            None => callback(Err(Errno::InvalidArgument), false),
        }
    }

    /// implements POSIX `open`, blocking
    pub fn open(arc_self: Arc<Self>, at: usize, path: String, flags: common::OpenFlags, mut callback: Box<dyn RequestCallback<usize>>) {
        let at = if flags & common::OpenFlags::AtCWD == common::OpenFlags::None {
            match arc_self.file_descriptors.lock().get(at) {
                Some(fd) => Some(fd.clone()),
                None => return callback(Err(Errno::BadFile), false),
            }
        } else {
            None
        };

        let file_descriptors = arc_self.file_descriptors.clone();
        let namespace = arc_self.namespace.clone();
        let callback = Arc::new(Mutex::new(callback));

        Self::resolve_container(
            arc_self,
            at,
            path,
            flags & common::OpenFlags::AtCWD != common::OpenFlags::None,
            Box::new(move |res, blocked| match res {
                Ok(resolved) => {
                    if resolved.path.lock().path.is_empty() {
                        // this path points to the root of a filesystem
                        if let Some(fs) = namespace.read().get(&*resolved.path.lock().name) {
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
                            (callback.lock())(file_descriptors.lock().add(open_file).map_err(|_| Errno::OutOfMemory), blocked);
                        } else {
                            (callback.lock())(Err(Errno::NoSuchFileOrDir), blocked);
                        }
                    } else {
                        let callback = callback.clone();
                        let name = resolved.path.lock().name.clone();
                        let path = resolved.path.clone();
                        let kind = resolved.kind.load(Ordering::SeqCst);
                        let file_descriptors = file_descriptors.clone();
                        let filesystem = resolved.container.filesystem.clone();

                        // open the file with the proper flags
                        resolved.container.make_request(Request::Open {
                            name: name.to_string(),
                            flags: flags & !(common::OpenFlags::CloseOnExec | common::OpenFlags::AtCWD),
                            callback: Box::new(move |res, open_blocked| match res {
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
                                    (callback.lock())(file_descriptors.lock().add(open_file).map_err(|_| Errno::OutOfMemory), blocked || open_blocked);
                                }
                                Err(err) => (callback.lock())(Err(err), blocked || open_blocked),
                            }),
                        });
                    }
                }
                Err(err) => (callback.lock())(Err(err), blocked),
            }),
        );
    }

    /// implements POSIX `read`, blocking
    pub fn read(&self, file_descriptor: usize, length: usize, mut callback: Box<dyn for<'a> RequestCallback<&'a [u8]>>) {
        if let Some(file) = self.file_descriptors.lock().get(file_descriptor) {
            file.read(length, callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// implements POSIX `seek`, partially blocking
    pub fn seek(&self, file_descriptor: usize, offset: i64, kind: common::SeekKind, mut callback: Box<dyn RequestCallback<i64>>) {
        if let Some(file) = self.file_descriptors.lock().get(file_descriptor) {
            file.seek(offset, kind, callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// implements POSIX `stat`, blocking
    pub fn stat(&self, file_descriptor: usize, mut callback: Box<dyn RequestCallback<common::FileStat>>) {
        if let Some(file) = self.file_descriptors.lock().get(file_descriptor) {
            file.stat(callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// implements POSIX `truncate`, blocking
    pub fn truncate(&self, file_descriptor: usize, len: i64, mut callback: Box<dyn RequestCallback<()>>) {
        if let Some(file) = self.file_descriptors.lock().get(file_descriptor) {
            file.truncate(len, callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// implements POSIX `unlink`, blocking
    pub fn unlink(arc_self: Arc<Self>, at: usize, path: String, flags: common::UnlinkFlags, mut callback: Box<dyn RequestCallback<()>>) {
        let at = if flags & common::UnlinkFlags::AtCWD == common::UnlinkFlags::None {
            match arc_self.file_descriptors.lock().get(at) {
                Some(fd) => Some(fd.clone()),
                None => return callback(Err(Errno::BadFile), false),
            }
        } else {
            None
        };

        let callback = Arc::new(Mutex::new(callback));

        Self::resolve_container(
            arc_self,
            at,
            path,
            false,
            Box::new(move |res, blocked| match res {
                Ok(resolved) => {
                    let callback = callback.clone();
                    let name = resolved.path.lock().name.to_string();

                    // unlink the file
                    resolved.container.make_request(Request::Unlink {
                        name,
                        flags,
                        callback: Box::new(move |res, blocked| (callback.lock())(res, blocked)),
                    });
                }
                Err(err) => (callback.lock())(Err(err), blocked),
            }),
        );
    }

    /// implements POSIX `write`, blocking
    pub fn write(&self, file_descriptor: usize, length: usize, mut callback: Box<dyn for<'a> RequestCallback<&'a mut [u8]>>) {
        if let Some(file) = self.file_descriptors.lock().get(file_descriptor) {
            file.write(length, callback);
        } else {
            callback(Err(Errno::BadFile), false);
        }
    }

    /// changes the root directory of this environment to the directory pointed to by the given file descriptor
    pub fn chroot(&self, file_descriptor: usize) -> common::Result<()> {
        *self.root.write() = self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.clone();
        Ok(())
    }

    /// changes the current working directory of this environment to the directory pointed to by the given file descriptor
    pub fn chdir(&self, file_descriptor: usize) -> common::Result<()> {
        *self.cwd.write() = self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.clone();
        Ok(())
    }

    fn get_path_to(&self, open_file: &OpenFile) -> String {
        let root = self.root.read();
        let root_path = root.path.lock();
        let open_file_path = open_file.path.lock();
        //debug!("root path is {:?}, root name is {:?}, open_file path is {:?}", root.path, root.name, open_file.path);

        // try to format the path relative to the root path if possible
        for (index, name) in root_path.path.iter().chain(Some(&root_path.name)).enumerate() {
            if open_file_path.path.get(index) != Some(name) {
                if open_file_path.path.is_empty() && open_file_path.name == ".." {
                    return "/..".to_string();
                } else {
                    let joined = open_file_path.path.join("/");
                    return format!("/../{joined}{}{}", if joined.is_empty() { "" } else { "/" }, open_file_path.name);
                }
            }
        }

        let joined = open_file_path.path[root_path.path.len() + 1..].join("/");
        format!("/{joined}{}{}", if joined.is_empty() { "" } else { "/" }, open_file_path.name)
    }

    /// gets the path to the current working directory of the current process
    pub fn get_cwd_path(&self) -> String {
        self.get_path_to(&self.cwd.read())
    }

    /// gets the path of the file pointed to by the given file descriptor
    pub fn get_path(&self, file_descriptor: usize) -> common::Result<String> {
        Ok(self.get_path_to(self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?))
    }

    /// gets the underlying open file object associated with the given file descriptor
    pub fn get_open_file(&self, file_descriptor: usize) -> Option<OpenFile> {
        self.file_descriptors.lock().get(file_descriptor).cloned()
    }
}

struct ResolvedHandle {
    container: Arc<FileHandle>,
    path: Arc<Mutex<AbsolutePath>>,
    kind: AtomicU8,
}

enum PartiallyResolved {
    Handle(Arc<FileHandle>, AtomicU8),
    Full(ResolvedHandle),
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

impl FileDescriptor for FsList {
    fn open(&self, name: String, flags: OpenFlags) -> common::Result<alloc::boxed::Box<dyn FileDescriptor>> {
        if flags & (OpenFlags::Write | OpenFlags::Create) != OpenFlags::None {
            return Err(Errno::ReadOnlyFilesystem);
        }

        if name == ".." {
            Ok(Box::new(self.clone()))
        } else {
            Err(Errno::NoSuchFileOrDir)
        }
    }

    fn read(&self, position: i64, _length: usize, mut callback: Box<dyn for<'a> RequestCallback<&'a [u8]>>) {
        let position: usize = match position.try_into() {
            Ok(position) => position,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };

        if let Some(entry) = self.namespace.read().keys().nth(position) {
            let mut data = Vec::new();
            data.extend_from_slice(&(0_u32.to_ne_bytes()));
            data.extend_from_slice(entry.as_bytes());
            data.push(0);

            callback(Ok(&data), false);
        } else {
            callback(Ok(&[]), false);
        }
    }

    fn stat(&self) -> common::Result<common::FileStat> {
        Ok(common::FileStat {
            mode: common::FileMode {
                permissions: common::Permissions::OwnerRead
                    | common::Permissions::OwnerExecute
                    | common::Permissions::GroupRead
                    | common::Permissions::GroupExecute
                    | common::Permissions::OtherRead
                    | common::Permissions::OtherExecute,
                kind: common::FileKind::Directory,
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
    /// makes a request to the filesystem associated with this handle
    pub fn make_request(&self, request: Request) {
        self.filesystem.make_request(self.handle.load(Ordering::SeqCst), request);
    }
}

impl Drop for FileHandle {
    fn drop(&mut self) {
        self.make_request(Request::Close);
    }
}

pub struct OpenFile {
    handle: Arc<FileHandle>,
    seek_pos: Arc<AtomicI64>,
    path: Arc<Mutex<AbsolutePath>>,
    flags: RwLock<common::OpenFlags>,
    kind: AtomicU8,
}

impl Clone for OpenFile {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            seek_pos: Arc::new(AtomicI64::new(self.seek_pos.load(Ordering::SeqCst))),
            path: self.path.clone(),
            flags: RwLock::new(*self.flags.read()),
            kind: AtomicU8::new(self.kind.load(Ordering::SeqCst)),
        }
    }
}

impl OpenFile {
    pub fn kind(&self) -> common::FileKind {
        unsafe { core::mem::transmute::<u8, common::FileKind>(self.kind.load(Ordering::SeqCst)) }
    }

    pub fn handle(&self) -> Arc<FileHandle> {
        self.handle.clone()
    }

    pub fn chmod(&self, permissions: common::Permissions, callback: Box<dyn RequestCallback<()>>) {
        self.handle.make_request(Request::Chmod { permissions, callback });
    }

    pub fn chown(&self, owner: common::UserId, group: common::GroupId, callback: Box<dyn RequestCallback<()>>) {
        self.handle.make_request(Request::Chown { owner, group, callback });
    }

    pub fn open(&self, name: String, flags: common::OpenFlags, mut callback: Box<dyn RequestCallback<FileHandle>>) {
        let filesystem = self.handle.filesystem.clone();
        self.handle.make_request(Request::Open {
            name,
            flags,
            callback: Box::new(move |res, blocked| {
                let filesystem = filesystem.clone();
                callback(
                    res.map(|num| FileHandle {
                        filesystem,
                        handle: AtomicUsize::new(num),
                    }),
                    blocked,
                )
            }),
        });
    }

    pub fn read(&self, length: usize, mut callback: Box<dyn for<'a> RequestCallback<&'a [u8]>>) {
        let seek_pos = self.seek_pos.clone();
        let position = self.seek_pos.load(Ordering::SeqCst);
        let kind = self.kind();
        self.handle.make_request(Request::Read {
            position,
            length,
            callback: Box::new(move |res, blocked| {
                callback(
                    res.and_then(|slice| {
                        // try to increment the seek position by the slice length if it hasn't been changed beforehand
                        let length: i64 = slice.len().try_into().map_err(|_| Errno::ValueOverflow)?;
                        match kind {
                            FileKind::Directory => {
                                seek_pos.fetch_add(1, Ordering::SeqCst);
                            }
                            FileKind::SymLink => (),
                            _ => {
                                let _ = seek_pos.compare_exchange(position, position + length, Ordering::SeqCst, Ordering::Relaxed);
                            }
                        }
                        Ok(slice)
                    }),
                    blocked,
                )
            }),
        });
    }

    pub fn seek(&self, offset: i64, kind: common::SeekKind, mut callback: Box<dyn RequestCallback<i64>>) {
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
                self.handle.make_request(Request::Stat {
                    callback: Box::new(move |res, blocked| {
                        callback(res.map(|res| seek_pos.fetch_add(res.size.saturating_add(offset), Ordering::SeqCst)), blocked);
                    }),
                });
            }
        }
    }

    pub fn stat(&self, callback: Box<dyn RequestCallback<common::FileStat>>) {
        self.handle.make_request(Request::Stat { callback });
    }

    pub fn truncate(&self, len: i64, callback: Box<dyn RequestCallback<()>>) {
        self.handle.make_request(Request::Truncate { len, callback });
    }

    pub fn unlink(&self, name: String, flags: common::UnlinkFlags, callback: Box<dyn RequestCallback<()>>) {
        self.handle.make_request(Request::Unlink { name, flags, callback });
    }

    pub fn write(&self, length: usize, mut callback: Box<dyn for<'a> RequestCallback<&'a mut [u8]>>) {
        let seek_pos = self.seek_pos.clone();
        let position = self.seek_pos.load(Ordering::SeqCst);
        self.handle.make_request(Request::Write {
            position,
            length,
            callback: Box::new(move |res, blocked| {
                callback(
                    res.and_then(|slice| {
                        // try to increment the seek position by the slice length if it hasn't been changed beforehand
                        let length: i64 = slice.len().try_into().map_err(|_| Errno::ValueOverflow)?;
                        let _ = seek_pos.compare_exchange(position, position + length, Ordering::SeqCst, Ordering::Relaxed);
                        Ok(slice)
                    }),
                    blocked,
                )
            }),
        });
    }
}

#[derive(Clone, Debug)]
pub struct AbsolutePath {
    pub path: Vec<String>,
    pub name: String,
}

type BoxedFileDescriptor = Arc<Mutex<Box<dyn FileDescriptor>>>;

pub struct KernelFs {
    file_handles: Mutex<ConsistentIndexArray<BoxedFileDescriptor>>,
}

// its literally in a mutex!!
unsafe impl Send for KernelFs {}
unsafe impl Sync for KernelFs {}

impl KernelFs {
    pub fn new(root: Box<dyn FileDescriptor>) -> Self {
        let mut file_handles = ConsistentIndexArray::new();
        file_handles.set(0, Arc::new(Mutex::new(root))).unwrap();

        Self {
            file_handles: Mutex::new(file_handles),
        }
    }
}

impl Filesystem for KernelFs {
    fn get_root_dir(&self) -> HandleNum {
        0
    }

    fn make_request(&self, handle: HandleNum, mut request: Request) {
        let descriptor = match self.file_handles.lock().get(handle) {
            Some(descriptor) => descriptor.clone(),
            None => return request.callback_error(Errno::BadFile, false),
        };

        if descriptor.is_locked() {
            debug!("deadlock :(");
        }

        match request {
            Request::Chmod { permissions, mut callback } => {
                let res = descriptor.lock().chmod(permissions);
                callback(res, false);
            }
            Request::Chown { owner, group, mut callback } => {
                let res = descriptor.lock().chown(owner, group);
                callback(res, false);
            }
            Request::Close => {
                if handle != 0 {
                    self.file_handles.lock().remove(handle);
                }
            }
            Request::Open { name, flags, mut callback } => {
                let res = descriptor
                    .lock()
                    .open(name, flags)
                    .and_then(|desc| self.file_handles.lock().add(Arc::new(Mutex::new(desc))).map_err(|_| Errno::OutOfMemory));
                callback(res, false);
            }
            Request::Read { position, length, callback } => descriptor.lock().read(position, length, callback),
            Request::Stat { mut callback } => {
                let res = descriptor.lock().stat();
                callback(res, false);
            }
            Request::Truncate { len, mut callback } => {
                let res = descriptor.lock().truncate(len);
                callback(res, false);
            }
            Request::Unlink { name, flags, mut callback } => {
                let res = descriptor.lock().unlink(name, flags);
                callback(res, false);
            }
            Request::Write { length, position, callback } => descriptor.lock().write(position, length, callback),
        }
    }
}

#[allow(unused_variables)]
pub trait FileDescriptor {
    /// changes the access permissions of the file pointed to by this file descriptor
    fn chmod(&self, permissions: common::Permissions) -> common::Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// changes the owner and/or group for the file pointed to by this file descriptor
    fn chown(&self, owner: common::UserId, group: common::GroupId) -> common::Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// opens the file with the given name in the directory pointed to by this file descriptor, returning a new file descriptor to the file on success.
    /// the filename must not contain slash characters
    fn open(&self, name: String, flags: common::OpenFlags) -> common::Result<Box<dyn FileDescriptor>> {
        Err(Errno::FuncNotSupported)
    }

    /// reads data from this file descriptor.
    ///
    /// if this file descriptor points to a symlink, its target will be read.
    ///
    /// if this file descriptor points to a directory, its entries will be read in order, one per every read() call,
    /// where every directory entry is formatted as its serial number as a native-endian u32 (4 bytes), followed by the bytes of its name with no null terminator.
    /// if a directory entry exceeds the given buffer length, it should be truncated to the buffer length.
    fn read(&self, position: i64, length: usize, mut callback: Box<dyn for<'a> RequestCallback<&'a [u8]>>) {
        callback(Err(Errno::FuncNotSupported), false);
    }

    /// gets information about the file pointed to by this file descriptor
    fn stat(&self) -> common::Result<common::FileStat>;

    /// shrinks or extends the file pointed to by this file descriptor to the given length
    fn truncate(&self, len: i64) -> common::Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// removes a reference to a file in the directory pointed to by this file descriptor from the filesystem,
    /// where it can then be deleted if no processes are using it or if there are no hard links to it
    fn unlink(&self, name: String, flags: common::UnlinkFlags) -> common::Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// writes data from this buffer to this file descriptor
    fn write(&self, position: i64, length: usize, mut callback: Box<dyn for<'a> RequestCallback<&'a mut [u8]>>) {
        callback(Err(Errno::FuncNotSupported), false);
    }
}
