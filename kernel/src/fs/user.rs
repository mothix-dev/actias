//! userspace filesystem support

use super::{HandleNum, RequestCallback};
use crate::{arch::PhysicalAddress, array::ConsistentIndexArray};
use alloc::{boxed::Box, string::String, vec::Vec};
use common::{Errno, EventKind, EventResponse, FileStat, FilesystemEvent, GroupId, OpenFlags, Permissions, ResponseData, UnlinkFlags, UserId};
use core::mem::size_of;
use crossbeam::queue::SegQueue;
use spin::Mutex;

enum CallbackKind {
    NoValue(Box<dyn RequestCallback<()>>),
    Handle(Box<dyn RequestCallback<HandleNum>>),
    Slice(Box<dyn for<'a> RequestCallback<&'a [u8]>>),
    MutableSlice(Box<dyn for<'a> RequestCallback<&'a mut [u8]>>),
}

impl CallbackKind {
    fn callback_error(self, error: Errno, blocked: bool) {
        match self {
            Self::NoValue(callback) => callback(Err(error), blocked),
            Self::Handle(callback) => callback(Err(error), blocked),
            Self::Slice(callback) => callback(Err(error), blocked),
            Self::MutableSlice(callback) => callback(Err(error), blocked),
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
    waiting_queue: SegQueue<Box<dyn for<'a> RequestCallback<&'a [u8]>>>,
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
    pub fn wait_for_event(&self, callback: Box<dyn for<'a> RequestCallback<&'a [u8]>>) {
        if let Some(data) = self.send_queue.pop() {
            callback(Ok(&data), false);
        } else {
            self.waiting_queue.push(callback);
        }
    }

    /// responds to an event
    pub fn respond(&self, response: &EventResponse) {
        let callback = self.in_progress.lock().remove(response.id);

        if let Some(callback) = callback {
            match response.data {
                ResponseData::Error { error } => callback.callback_error(error, true),
                ResponseData::Buffer { addr, len } => {
                    let buffer = match crate::process::ProcessBuffer::from_current_process(addr, len) {
                        Ok(buffer) => buffer,
                        Err(err) => return callback.callback_error(err, true),
                    };

                    // TODO: handle errors while mapping sanely
                    match callback {
                        CallbackKind::Slice(callback) => { buffer.map_in(|slice| callback(Ok(slice), true)).unwrap() },
                        CallbackKind::MutableSlice(callback) => { buffer.map_in_mut(|slice| callback(Ok(slice), true)).unwrap() },
                        _ => callback.callback_error(Errno::TryAgain, true),
                    }
                }
                ResponseData::Handle { handle } => match callback {
                    CallbackKind::Handle(callback) => callback(Ok(handle), true),
                    _ => callback.callback_error(Errno::TryAgain, true),
                },
                ResponseData::None => match callback {
                    CallbackKind::NoValue(callback) => callback(Ok(()), true),
                    _ => callback.callback_error(Errno::TryAgain, true),
                },
            }
        }
    }

    fn make_request(&self, handle: HandleNum, kind: EventKind, string_option: Option<String>, callback: Option<CallbackKind>) {
        // TODO: handle this sanely
        let id = callback
            .map(|callback| self.in_progress.lock().add(callback).expect("ran out of memory while trying to queue new filesystem event"))
            .unwrap_or_default();

        // get the portable request as raw bytes that a process can read from
        let event = FilesystemEvent { id, handle, kind };
        //trace!("sending event {event:#?}");

        let data = match Self::make_data(event, string_option) {
            Ok(data) => data,
            Err(_) => return self.in_progress.lock().remove(id).unwrap().callback_error(Errno::OutOfMemory, false),
        };

        // send the event to a waiting task, or just push it onto the queue if there are none
        if let Some(callback) = self.waiting_queue.pop() {
            callback(Ok(&data), true);
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

impl super::Filesystem for UserspaceFs {
    fn get_root_dir(&self) -> super::HandleNum {
        0
    }

    fn chmod(&self, handle: HandleNum, permissions: Permissions, callback: Box<dyn RequestCallback<()>>) {
        self.make_request(handle, EventKind::Chmod { permissions }, None, Some(CallbackKind::NoValue(callback)));
    }

    fn chown(&self, handle: HandleNum, owner: UserId, group: GroupId, callback: Box<dyn RequestCallback<()>>) {
        self.make_request(handle, EventKind::Chown { owner, group }, None, Some(CallbackKind::NoValue(callback)));
    }

    fn close(&self, handle: HandleNum) {
        if handle != 0 {
            self.make_request(handle, EventKind::Close, None, None);
        }
    }

    fn open(&self, handle: HandleNum, name: String, flags: OpenFlags, callback: Box<dyn RequestCallback<HandleNum>>) {
        self.make_request(handle, EventKind::Open { name_length: name.len(), flags }, Some(name), Some(CallbackKind::Handle(callback)));
    }

    fn read(&self, handle: HandleNum, position: i64, length: usize, callback: Box<dyn for<'a> RequestCallback<&'a [u8]>>) {
        self.make_request(handle, EventKind::Read { position, length }, None, Some(CallbackKind::Slice(callback)));
    }

    fn stat(&self, handle: HandleNum, callback: Box<dyn RequestCallback<FileStat>>) {
        self.make_request(
            handle,
            EventKind::Stat,
            None,
            Some(CallbackKind::Slice(Box::new(|res, blocked| match res {
                Ok(slice) => {
                    if slice.len() < size_of::<FileStat>() {
                        callback(Err(Errno::TryAgain), blocked);
                    } else {
                        callback(Ok(unsafe { *(slice.as_ptr() as *const FileStat) }), blocked);
                    }
                }
                Err(err) => callback(Err(err), blocked),
            }))),
        );
    }

    fn truncate(&self, handle: HandleNum, length: i64, callback: Box<dyn RequestCallback<()>>) {
        self.make_request(handle, EventKind::Truncate { length }, None, Some(CallbackKind::NoValue(callback)));
    }

    fn unlink(&self, handle: HandleNum, name: String, flags: UnlinkFlags, callback: Box<dyn RequestCallback<()>>) {
        self.make_request(handle, EventKind::Unlink { name_length: name.len(), flags }, Some(name), Some(CallbackKind::NoValue(callback)));
    }

    fn write(&self, handle: HandleNum, position: i64, length: usize, callback: Box<dyn for<'a> RequestCallback<&'a mut [u8]>>) {
        self.make_request(handle, EventKind::Write { position, length }, None, Some(CallbackKind::MutableSlice(callback)));
    }

    fn get_page(&self, _handle: super::HandleNum, _offset: i64, _callback: Box<dyn FnOnce(Option<PhysicalAddress>, bool)>) {
        todo!();
    }
}
