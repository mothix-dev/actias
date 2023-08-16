//! userspace filesystem support

use super::{Request, RequestCallback};
use crate::{arch::PhysicalAddress, array::ConsistentIndexArray};
use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use common::{Errno, EventKind, EventResponse, FileStat, FilesystemEvent, ResponseData};
use core::mem::size_of;
use crossbeam::queue::SegQueue;
use spin::Mutex;

#[allow(clippy::type_complexity)] // waiting_queue is totally fine, actually
pub struct UserspaceFs {
    /// list of events that are in progress (have been dispatched to a waiting task or stored in the queue) and haven't been completed yet
    in_progress: Mutex<ConsistentIndexArray<Request>>,

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
        let request = self.in_progress.lock().remove(response.id);

        if let Some(request) = request {
            match response.data {
                ResponseData::Error { error } => request.callback_error(error, true),
                ResponseData::Buffer { addr, len } => {
                    // TODO: ensure the buffer is actually mapped in
                    match request {
                        Request::Read { callback, .. } => callback(Ok(unsafe { core::slice::from_raw_parts(addr as *const u8, len) }), true),
                        Request::Stat { callback } => {
                            if len < size_of::<FileStat>() {
                                callback(Err(Errno::TryAgain), true);
                            } else {
                                callback(Ok(unsafe { *(addr as *const FileStat) }.clone()), true);
                            }
                        }
                        Request::Write { callback, .. } => callback(Ok(unsafe { core::slice::from_raw_parts_mut(addr as *mut u8, len) }), true),
                        _ => request.callback_error(Errno::TryAgain, true),
                    }
                }
                ResponseData::Handle { handle } => match request {
                    Request::Open { callback, .. } => callback(Ok(handle), true),
                    _ => request.callback_error(Errno::TryAgain, true),
                },
                ResponseData::None => match request {
                    Request::Chmod { callback, .. } => callback(Ok(()), true),
                    Request::Chown { callback, .. } => callback(Ok(()), true),
                    Request::Truncate { callback, .. } => callback(Ok(()), true),
                    Request::Unlink { callback, .. } => callback(Ok(()), true),
                    _ => request.callback_error(Errno::TryAgain, true),
                },
            }
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

    fn make_request(&self, handle: super::HandleNum, request: Request) {
        // ignore any attempts to close the root dir
        if matches!(request, Request::Close) && handle == 0 {
            return;
        }

        // convert the request into a portable format
        let mut string_option = None;
        let kind = match &request {
            Request::Chmod { permissions, .. } => EventKind::Chmod { permissions: *permissions },
            Request::Chown { owner, group, .. } => EventKind::Chown { owner: *owner, group: *group },
            Request::Close => EventKind::Close,
            Request::Open { name, flags, .. } => {
                string_option = Some(name.to_string());
                EventKind::Open {
                    name_length: name.len(),
                    flags: *flags,
                }
            }
            Request::Read { position, length, .. } => EventKind::Read { position: *position, length: *length },
            Request::Stat { .. } => EventKind::Stat,
            Request::Truncate { length, .. } => EventKind::Truncate { length: *length },
            Request::Unlink { name, flags, .. } => {
                string_option = Some(name.to_string());
                EventKind::Unlink {
                    name_length: name.len(),
                    flags: *flags,
                }
            }
            Request::Write { length, position, .. } => EventKind::Write { length: *length, position: *position },
        };

        // TODO: handle this sanely
        let id = self.in_progress.lock().add(request).expect("ran out of memory while trying to queue new filesystem event");

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

    fn get_page(&self, _handle: super::HandleNum, _offset: i64, _callback: Box<dyn FnOnce(Option<PhysicalAddress>, bool)>) {
        todo!();
    }
}
