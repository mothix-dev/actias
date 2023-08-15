//! userspace filesystem support

use core::mem::size_of;

use super::{Request, RequestCallback};
use crate::{arch::PhysicalAddress, array::ConsistentIndexArray};
use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use common::{Errno, EventKind, FilesystemEvent};
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
}

impl super::Filesystem for UserspaceFs {
    fn get_root_dir(&self) -> super::HandleNum {
        todo!();
    }

    fn make_request(&self, handle: super::HandleNum, request: Request) {
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

        let event = FilesystemEvent { id, handle, kind };
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
