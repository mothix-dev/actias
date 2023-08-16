//! things for dealing with futures (mainly for async filesystem events)

use alloc::sync::Arc;
use core::{
    pin::Pin,
    task::{Context, Poll, Waker},
};
use futures::{
    future::BoxFuture,
    task::{waker_ref, ArcWake},
    Future,
};
use spin::Mutex;

/// the most barebones executor possible. takes a future and runs it when the future calls its waker
pub struct AsyncTask {
    future: Mutex<Option<BoxFuture<'static, ()>>>,
}

impl AsyncTask {
    pub fn new(future: BoxFuture<'static, ()>) -> Arc<Self> {
        let task = Arc::new(Self { future: Some(future).into() });
        Self::wake_by_ref(&task);
        task
    }
}

impl ArcWake for AsyncTask {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        let mut future_slot = arc_self.future.lock();

        if let Some(mut future) = future_slot.take() {
            let waker = waker_ref(arc_self);
            let context = &mut Context::from_waker(&waker);

            if future.as_mut().poll(context).is_pending() {
                *future_slot = Some(future);
            }
        }
    }
}

/// a basic callback future, will wait until it's called
pub struct Callback<T> {
    waker: Mutex<Option<Waker>>,
    data: Mutex<Option<T>>,
}

impl<T> Callback<T> {
    /// creates a new callback future, waiting to be called
    pub fn new() -> Self {
        Self {
            waker: None.into(),
            data: None.into(),
        }
    }

    /// calls this callback with the provided data
    pub fn call(&self, data: T) {
        *self.data.lock() = Some(data);
        let waker = self.waker.lock().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl<T> Default for Callback<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Future for &Callback<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let data = self.data.lock().take();

        if let Some(data) = data {
            Poll::Ready(data)
        } else {
            *self.waker.lock() = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
