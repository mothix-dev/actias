//! userspace filesystem support

use super::HandleNum;
use crate::{arch::PhysicalAddress, array::ConsistentIndexArray, futures::Callback, process::Buffer};
use alloc::{boxed::Box, string::String, sync::Arc, vec, vec::Vec};
use async_trait::async_trait;
use common::{Errno, EventKind, EventResponse, FileStat, FilesystemEvent, GroupId, OpenFlags, Permissions, ResponseData, UnlinkFlags, UserId};
use core::mem::size_of;
use crossbeam::queue::SegQueue;
use log::debug;
use spin::Mutex;

#[derive(Clone)]
enum CallbackKind {
    NoValue(Arc<Callback<common::Result<()>>>),
    Handle(Arc<Callback<common::Result<HandleNum>>>),
    CopyFrom(Buffer, Arc<Callback<common::Result<usize>>>),
    CopyTo(Buffer, Arc<Callback<common::Result<usize>>>),
}

impl CallbackKind {
    fn callback_error(self, error: Errno) {
        match self {
            Self::NoValue(callback) => callback.call(Err(error)),
            Self::Handle(callback) => callback.call(Err(error)),
            Self::CopyFrom(_, callback) => callback.call(Err(error)),
            Self::CopyTo(_, callback) => callback.call(Err(error)),
        }
    }
}

#[allow(clippy::type_complexity)] // waiting_queue is totally fine, actually
pub struct UserspaceFs {
    /// list of events that are in progress (have been dispatched to a waiting task or stored in the queue) and haven't been completed yet
    in_progress: Mutex<ConsistentIndexArray<CallbackKind>>,

    /// a queue of events that have occurred while no tasks are blocked waiting for events
    send_queue: SegQueue<Vec<u8>>,

    /// queue of tasks that are blocked waiting for events
    waiting_queue: SegQueue<(Buffer, Arc<Callback<common::Result<usize>>>)>,
}

unsafe impl Send for UserspaceFs {}
unsafe impl Sync for UserspaceFs {}

impl UserspaceFs {
    pub fn new() -> Self {
        Self {
            in_progress: Mutex::new(ConsistentIndexArray::new()),
            send_queue: SegQueue::new(),
            waiting_queue: SegQueue::new(),
        }
    }

    /// converts an event and its optional string into a vector of raw data that processes can read from
    ///
    /// since the string lengths are encoded in the event object (for the only ones that actually have strings included)
    /// there's no reason to have the length otherwise encoded, or to have a null terminator
    /// (null terminated strings are awful and those unfortunate enough to be using C can add them afterwards)
    fn make_data(event: FilesystemEvent, string_option: Option<String>) -> Result<Vec<u8>, alloc::collections::TryReserveError> {
        let event_slice = unsafe { core::slice::from_raw_parts(&event as *const _ as *const u8, size_of::<FilesystemEvent>()) };
        let string_slice = string_option.as_ref().map(|s| s.as_bytes()).unwrap_or(&[]);

        let mut vec = Vec::new();
        vec.try_reserve_exact(event_slice.len() + string_slice.len())?;
        vec.extend_from_slice(event_slice);
        vec.extend_from_slice(string_slice);

        Ok(vec)
    }

    /// queues a callback reading filesystem events, or calls it immediately if there are queued events
    pub async fn wait_for_event(&self, buffer: Buffer) -> common::Result<usize> {
        if let Some(data) = self.send_queue.pop() {
            buffer.copy_from(&data).await
        } else {
            let callback = Arc::new(Callback::new());
            self.waiting_queue.push((buffer, callback.clone()));
            (&*callback).await
        }
    }

    /// responds to an event
    pub fn respond(&self, response: &EventResponse) -> common::Result<Option<ResponseInProgress>> {
        debug!("responding to event id {}", response.id);

        let callback = self.in_progress.lock().remove(response.id);

        if let Some(callback) = callback {
            match response.data {
                ResponseData::Error { error } => callback.callback_error(error),
                ResponseData::Handle { handle } => match callback {
                    CallbackKind::Handle(callback) => callback.call(Ok(handle)),
                    _ => callback.callback_error(Errno::TryAgain),
                },
                ResponseData::None => {
                    if matches!(callback, CallbackKind::CopyFrom(_, _) | CallbackKind::CopyTo(_, _)) {
                        return Ok(Some(ResponseInProgress { callback }));
                    } else {
                        match callback {
                            CallbackKind::NoValue(callback) => callback.call(Ok(())),
                            _ => callback.callback_error(Errno::TryAgain),
                        }
                    }
                }
            }

            Ok(None)
        } else {
            Err(Errno::InvalidArgument)
        }
    }

    async fn make_request(&self, handle: HandleNum, kind: EventKind, string_option: Option<String>, callback: Option<CallbackKind>) {
        let id = match callback {
            Some(callback) => match self.in_progress.lock().add(callback.clone()) {
                Ok(id) => id,
                Err(_) => return callback.callback_error(Errno::OutOfMemory),
            },
            None => 0,
        };

        // get the portable request as raw bytes that a process can read from
        let event = FilesystemEvent { id, handle, kind };
        //trace!("sending event {event:#?}");
        debug!("sending event {event:?}, {string_option:?}");

        let data = match Self::make_data(event, string_option) {
            Ok(data) => data,
            Err(_) => return self.in_progress.lock().remove(id).unwrap().callback_error(Errno::OutOfMemory),
        };

        // send the event to a waiting task, or just push it onto the queue if there are none
        if let Some((buffer, callback)) = self.waiting_queue.pop() {
            let res = buffer.copy_from(&data).await;
            callback.call(res);
        } else {
            self.send_queue.push(data);
        }
    }
}

impl Default for UserspaceFs {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl super::Filesystem for UserspaceFs {
    fn get_root_dir(&self) -> super::HandleNum {
        0
    }

    async fn chmod(&self, handle: HandleNum, permissions: Permissions) -> common::Result<()> {
        let callback = Arc::new(Callback::new());
        self.make_request(handle, EventKind::Chmod { permissions }, None, Some(CallbackKind::NoValue(callback.clone()))).await;
        (&*callback).await
    }

    async fn chown(&self, handle: HandleNum, owner: UserId, group: GroupId) -> common::Result<()> {
        let callback = Arc::new(Callback::new());
        self.make_request(handle, EventKind::Chown { owner, group }, None, Some(CallbackKind::NoValue(callback.clone()))).await;
        (&*callback).await
    }

    async fn close(&self, handle: HandleNum) {
        if handle != 0 {
            self.make_request(handle, EventKind::Close, None, None).await;
        }
    }

    async fn open(&self, handle: HandleNum, name: String, flags: OpenFlags) -> common::Result<HandleNum> {
        let callback = Arc::new(Callback::new());
        self.make_request(handle, EventKind::Open { name_length: name.len(), flags }, Some(name), Some(CallbackKind::Handle(callback.clone())))
            .await;
        (&*callback).await
    }

    async fn read(&self, handle: HandleNum, position: i64, buffer: Buffer) -> common::Result<usize> {
        let callback = Arc::new(Callback::new());
        self.make_request(handle, EventKind::Read { position, length: buffer.len() }, None, Some(CallbackKind::CopyTo(buffer, callback.clone())))
            .await;
        (&*callback).await
    }

    async fn stat(&self, handle: HandleNum) -> common::Result<FileStat> {
        let buffer = Arc::new(Mutex::new(vec![0; size_of::<FileStat>()].into_boxed_slice()));

        let callback = Arc::new(Callback::new());
        self.make_request(handle, EventKind::Stat, None, Some(CallbackKind::CopyTo(buffer.clone().into(), callback.clone())))
            .await;

        let bytes_read = (&*callback).await?;

        if bytes_read < size_of::<FileStat>() {
            Err(Errno::TryAgain)
        } else {
            buffer.lock()[..].try_into().map_err(|_| Errno::TryAgain)
        }
    }

    async fn truncate(&self, handle: HandleNum, length: i64) -> common::Result<()> {
        let callback = Arc::new(Callback::new());
        self.make_request(handle, EventKind::Truncate { length }, None, Some(CallbackKind::NoValue(callback.clone()))).await;
        (&*callback).await
    }

    async fn unlink(&self, handle: HandleNum, name: String, flags: UnlinkFlags) -> common::Result<()> {
        let callback = Arc::new(Callback::new());
        self.make_request(handle, EventKind::Unlink { name_length: name.len(), flags }, Some(name), Some(CallbackKind::NoValue(callback.clone())))
            .await;
        (&*callback).await
    }

    async fn write(&self, handle: HandleNum, position: i64, buffer: Buffer) -> common::Result<usize> {
        let callback = Arc::new(Callback::new());
        self.make_request(
            handle,
            EventKind::Write { position, length: buffer.len() },
            None,
            Some(CallbackKind::CopyFrom(buffer, callback.clone())),
        )
        .await;
        (&*callback).await
    }

    async fn get_page(&self, _handle: super::HandleNum, _offset: i64) -> Option<PhysicalAddress> {
        todo!();
    }
}

pub struct ResponseInProgress {
    callback: CallbackKind,
}

impl ResponseInProgress {
    pub async fn write(self, buffer: Buffer) -> common::Result<usize> {
        match self.callback {
            CallbackKind::CopyTo(target_buffer, callback) => {
                let res = buffer.copy_into_buffer(&target_buffer).await;
                callback.call(res);
                res
            }
            _ => Err(Errno::TryAgain),
        }
    }

    pub async fn read(self, buffer: Buffer) -> common::Result<usize> {
        match self.callback {
            CallbackKind::CopyFrom(target_buffer, callback) => {
                let res = target_buffer.copy_into_buffer(&buffer).await;
                callback.call(res);
                res
            }
            _ => Err(Errno::TryAgain),
        }
    }
}
