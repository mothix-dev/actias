use alloc::{boxed::Box, collections::VecDeque};
use core::sync::atomic::{AtomicUsize, Ordering};
use crossbeam::queue::SegQueue;
use log::{error, warn};
use spin::Mutex;

type Registers = <crate::arch::InterruptManager as crate::arch::bsp::InterruptManager>::Registers;

struct Timeout {
    expires_at: u64,
    callback: Box<dyn FnMut(&mut Registers)>,
}

#[cfg(not(target_has_atomic = "64"))]
pub type AtomicJiffies = AtomicUsize;

#[cfg(target_has_atomic = "64")]
pub type AtomicJiffies = AtomicU64;

pub struct Timer {
    jiffies: AtomicJiffies,
    hz: u64,
    timers: Mutex<VecDeque<Timeout>>,
    add_queue: SegQueue<Timeout>,
    remove_queue: SegQueue<u64>,
}

unsafe impl Send for Timer {}
unsafe impl Sync for Timer {}

impl Timer {
    /// creates a new timer with the specified tick rate
    pub fn new(hz: u64) -> Self {
        Self {
            jiffies: AtomicJiffies::new(0),
            hz,
            timers: Mutex::new(VecDeque::new()),
            add_queue: SegQueue::new(),
            remove_queue: SegQueue::new(),
        }
    }

    /// adds a timeout that'll expire at a specific point in time
    pub fn timeout_at<F: FnMut(&mut Registers) + 'static>(&self, expires_at: u64, callback: F) {
        self.add_queue.push(Timeout {
            expires_at,
            callback: Box::new(callback),
        });
    }

    /// adds a timeout that'll expire in the given amount of ticks in the future
    pub fn timeout_in<F: FnMut(&mut Registers) + 'static>(&self, expires_in: u64, callback: F) -> u64 {
        let expires_at = self.jiffies.load(Ordering::SeqCst) as u64 + expires_in;
        self.timeout_at(expires_at, callback);
        expires_at
    }

    /// removes any timeouts that expire at the given time
    ///
    /// TODO: is it worth hashing callbacks and using them as keys to remove individual timeouts?
    pub fn remove(&self, expires_at: u64) {
        self.remove_queue.push(expires_at);
    }

    pub fn tick(&self, registers: &mut Registers) {
        let jiffy = self.jiffies.fetch_add(1, Ordering::SeqCst);

        let mut timers = match self.timers.try_lock() {
            Some(timers) => timers,
            None => {
                warn!("timer state is locked, timers will expire late");
                return;
            }
        };

        // add new timers to the queue
        while let Some(timer) = self.add_queue.pop() {
            if let Err(err) = timers.try_reserve(1) {
                error!("couldn't reserve memory for new timer: {err}");
                self.add_queue.push(timer);
                break;
            } else {
                match timers.iter().position(|t| t.expires_at >= timer.expires_at) {
                    // keep the timer queue sorted
                    Some(index) => timers.insert(index, timer),
                    None => timers.push_back(timer),
                }
            }
        }

        // process any expired timers
        while let Some(timer) = timers.front() {
            if jiffy as u64 >= timer.expires_at {
                (timers.pop_front().unwrap().callback)(registers);
            } else {
                // break out of the loop since we keep the timer queue sorted
                break;
            }
        }

        // remove any timers to be removed
        while let Some(expires_at) = self.remove_queue.pop() {
            for (index, timer) in timers.iter().enumerate() {
                if timer.expires_at == expires_at {
                    timers.remove(index);
                    break;
                } else if timer.expires_at > expires_at {
                    break;
                }
            }
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
