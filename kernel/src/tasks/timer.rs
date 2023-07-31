use core::{task::{Waker, Poll}, sync::atomic::{AtomicUsize, Ordering}};

use alloc::{sync::Arc, collections::VecDeque};
use futures::Future;
use log::warn;
use spin::Mutex;

#[derive(Default)]
struct TimerFutureState {
    completed: bool,
    waker: Option<Waker>,
}

pub struct TimerFuture {
    state: Arc<Mutex<TimerFutureState>>,
}

impl Future for TimerFuture {
    type Output = ();

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock();

        if state.completed {
            Poll::Ready(())
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

struct TimerEvent {
    expires_at: u64,
    state: Arc<Mutex<TimerFutureState>>,
}

#[derive(Debug)]
pub struct TimerAddError;

#[cfg(not(target_has_atomic = "64"))]
type TimerJiffies = AtomicUsize;

#[cfg(target_has_atomic = "64")]
type TimerJiffies = AtomicU64;

pub struct Timer {
    jiffies: TimerJiffies,
    hz: u64,
    timers: Mutex<VecDeque<TimerEvent>>,
}

impl Timer {
    /// creates a new timer with the specified tick rate
    pub fn new(hz: u64) -> Self {
        Self {
            jiffies: TimerJiffies::new(0),
            hz,
            timers: Mutex::new(VecDeque::new()),
        }
    }

    /// gets a future that'll complete at the given time, in ticks
    pub fn timeout_at(&self, expires_at: u64) -> Result<TimerFuture, TimerAddError> {
        let mut timers = self.timers.lock();

        timers.try_reserve(1).map_err(|_| TimerAddError)?;

        let state = Arc::try_new(Mutex::new(Default::default())).map_err(|_| TimerAddError)?;
        let timer = TimerEvent { expires_at, state: state.clone() };

        match timers.iter().position(|t| t.expires_at >= expires_at) {
            // keep the timer queue sorted
            Some(index) => timers.insert(index, timer),
            None => timers.push_back(timer),
        }

        Ok(TimerFuture { state })
    }

    /// gets a future that'll complete in the given number of ticks from the 
    pub fn timeout_in(&self, expires_in: u64) -> Result<TimerFuture, TimerAddError> {
        self.timeout_at(self.jiffies.load(Ordering::SeqCst) as u64 + expires_in)
    }

    fn try_tick(&self) -> Option<()> {
        let jiffy = self.jiffies.fetch_add(1, Ordering::SeqCst);

        let mut timers = self.timers.try_lock()?;

        while let Some(timer) = timers.front() {
            if jiffy as u64 >= timer.expires_at {
                {
                    let mut state = timer.state.try_lock()?;

                    state.completed = true;
                    if let Some(waker) = state.waker.take() {
                        waker.wake();
                    }
                }

                // only pop after future has been completed and waked just in case it's locked
                timers.pop_front();
            } else {
                // break out of the loop since we keep the timer queue sorted
                break;
            }
        }

        Some(())
    }

    /// ticks the timer, completing `TimerFuture`s as needed
    pub fn tick(&self) {
        if self.try_tick().is_none() {
            warn!("timer state is locked, timers will expire late")
        }
    }

    /// returns the current jiffies counter of the timer
    pub fn jiffies(&self) -> u64 {
        self.jiffies.load(Ordering::SeqCst) as u64
    }

    /// returns the timer's hz value (how many ticks per second)
    pub fn hz(&self) -> u64 {
        self.hz
    }

    /// returns the number of ticks per millisecond
    pub fn millis(&self) -> u64 {
        self.hz / 1000
    }
}
