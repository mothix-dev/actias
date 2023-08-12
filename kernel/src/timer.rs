use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use log::warn;
use spin::{Mutex, RwLock};

type Registers = <crate::arch::InterruptManager as crate::arch::bsp::InterruptManager>::Registers;

pub trait TimeoutCallback = FnMut(&mut Registers, u64) -> Option<u64>;

/// contains the expiration time and callback for a timeout
pub struct Timeout {
    /// when this timeout expires and the callback should run. if set to u64::MAX, this timeout will never expire
    pub expires_at: AtomicU64,

    /// the callback to run when this timeout expires
    pub callback: Mutex<Box<dyn TimeoutCallback>>,
}

unsafe impl Send for Timeout {}
unsafe impl Sync for Timeout {}

/// a timer that manages any number of timeouts and runs their callbacks when they expire
pub struct Timer {
    jiffies: AtomicU64,
    hz: u64,
    timers: RwLock<Vec<Arc<Timeout>>>,
}

unsafe impl Send for Timer {}
unsafe impl Sync for Timer {}

impl Timer {
    /// creates a new timer with the specified tick rate
    pub fn new(hz: u64) -> Self {
        Self {
            jiffies: AtomicU64::new(0),
            hz,
            timers: RwLock::new(Vec::new()),
        }
    }

    /// adds a timeout that can be set to expire at a certain point in time
    ///
    /// callbacks are given the register context from the timer interrupt and the current timer jiffies count,
    /// and can return `Some(any number)` to have the timeout occur at that time. if `None` is returned,
    /// the timeout will be set to not trigger again until specified otherwise
    ///
    /// the newly added timeout won't trigger automatically, and must be set to do so manually
    pub fn add_timeout<F: TimeoutCallback + 'static>(&self, callback: F) -> Arc<Timeout> {
        let timeout = Arc::new(Timeout {
            expires_at: AtomicU64::new(u64::MAX),
            callback: Mutex::new(Box::new(callback)),
        });
        self.timers.write().push(timeout.clone());
        timeout
    }

    /// ticks the timer, running any expired timeouts
    pub fn tick(&self, registers: &mut Registers) {
        let jiffy = self.jiffies.fetch_add(1, Ordering::SeqCst);

        let timers = match self.timers.try_read() {
            Some(timers) => timers,
            None => {
                warn!("timer state is locked, timers will expire late");
                return;
            }
        };

        (crate::arch::PROPERTIES.enable_interrupts)();

        // process any expired timers
        for timer in timers.iter() {
            let expires_at = timer.expires_at.load(Ordering::Acquire);

            if jiffy >= expires_at && expires_at != u64::MAX {
                let next = (timer.callback.lock())(registers, jiffy).unwrap_or(u64::MAX);
                let _ = timer.expires_at.compare_exchange(expires_at, next, Ordering::Release, Ordering::Relaxed);
            }
        }
    }

    /// returns the current jiffies counter of the timer
    pub fn jiffies(&self) -> u64 {
        self.jiffies.load(Ordering::SeqCst)
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
