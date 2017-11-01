//! A collection of synchronization primitives that build on the primitives available in the
//! standard library.
//!
//! This library contains the following special-purpose synchronization primitives:
//!
//! * [`CountdownEvent`], a primitive that keeps a counter and allows a thread to wait until the
//!   counter reaches zero.
//! * [`SignalEvent`], a primitive that allows one or more threads to wait on a signal from another
//!   thread.
//! * [`WriterReaderPhaser`], a primitive that allows multiple wait-free "writer critical sections"
//!   against a "reader phase flip" that waits for currently-active writers to finish.
//!
//! [`CountdownEvent`]: struct.CountdownEvent.html
//! [`SignalEvent`]: struct.SignalEvent.html
//! [`WriterReaderPhaser`]: struct.WriterReaderPhaser.html

#![deny(warnings, missing_docs)]

//Name source: http://bulbapedia.bulbagarden.net/wiki/Synchronoise_(move)

extern crate crossbeam;

mod util;

use std::sync::{Arc, Mutex, Condvar, LockResult, MutexGuard};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::isize::MIN as ISIZE_MIN;
use std::time::Duration;
use std::thread;

use crossbeam::sync::MsQueue;

/// A synchronization primitive that signals when its count reaches zero.
///
/// With a `CountdownEvent`, it's possible to cause one thread to wait on a set of computations
/// occurring in other threads by making the other threads interact with the counter as they perform
/// their work.
///
/// The main limitation of a CountdownEvent is that once its counter reaches zero (even by starting
/// there), any attempts to update the counter will return `CountdownError::AlreadySet` until the
/// counter is reset by calling `reset` or `reset_to_count`.
///
/// `CountdownEvent` is a port of [System.Threading.CountdownEvent][src-link] from .NET (also
/// called `CountDownLatch` in Java).
///
/// [src-link]: https://msdn.microsoft.com/en-us/library/system.threading.countdownevent(v=vs.110).aspx
///
/// # Example
///
/// ```
/// use synchronoise::CountdownEvent;
/// use std::sync::Arc;
/// use std::thread;
/// use std::time::Duration;
///
/// let counter = Arc::new(CountdownEvent::new(5));
///
/// for i in 0..5 {
///     let signal = counter.clone();
///     thread::spawn(move || {
///         thread::sleep(Duration::from_secs(3));
///         println!("thread {} activated!", i);
///         signal.decrement();
///     });
/// }
///
/// counter.wait();
///
/// println!("all done!");
/// ```
pub struct CountdownEvent {
    initial: isize,
    counter: Mutex<isize>,
    lock: Condvar,
}

///The collection of errors that can be returned by `CountdownEvent` methods.
pub enum CountdownError {
    ///Returned when adding to a counter would have caused it to overflow.
    SaturatedCounter,
    ///Returned when attempting to signal would have caused the counter to go below zero.
    TooManySignals,
    ///Returned when attempting to modify the counter after it has reached zero.
    AlreadySet,
}

impl CountdownEvent {
    ///Creates a new `CountdownEvent`, initialized to the given count.
    pub fn new(count: isize) -> CountdownEvent {
        CountdownEvent {
            initial: count,
            counter: Mutex::new(count),
            lock: Condvar::new(),
        }
    }

    ///Resets the counter to the count given to `new`.
    ///
    ///This function is safe because the `&mut self` enforces that no other references or locks
    ///exist.
    pub fn reset(&mut self) {
        self.counter = Mutex::new(self.initial);
        self.lock = Condvar::new();
    }

    ///Resets the counter to the given count.
    ///
    ///This function is safe because the `&mut self` enforces that no other references or locks
    ///exist.
    pub fn reset_to_count(&mut self, count: isize) {
        self.initial = count;
        self.reset();
    }

    ///Returns the current counter value.
    pub fn count(&self) -> isize {
        let lock = util::guts(self.counter.lock());

        *lock
    }

    ///Adds the given count to the counter.
    ///
    ///# Errors
    ///
    ///If the counter is already at or below zero, this function will return an error.
    ///
    ///If the given count would overflow an `isize`, this function will return an error.
    pub fn add(&self, count: isize) -> Result<(), CountdownError> {
        let mut lock = util::guts(self.counter.lock());

        if *lock <= 0 {
            return Err(CountdownError::AlreadySet);
        }

        if let Some(new_count) = count.checked_add(*lock) {
            *lock = new_count;
        } else {
            return Err(CountdownError::SaturatedCounter);
        }

        Ok(())
    }

    ///Subtracts the given count to the counter, and returns whether this caused any waiting
    ///threads to wake up.
    ///
    ///# Errors
    ///
    ///If the counter was already at or below zero, this function will return an error.
    ///
    ///If the given count is greater than the current counter, this function will return an error.
    pub fn signal(&self, count: isize) -> Result<bool, CountdownError> {
        let mut lock = util::guts(self.counter.lock());

        if *lock == 0 {
            return Err(CountdownError::AlreadySet);
        }

        if count <= *lock {
            *lock -= count;
        } else {
            return Err(CountdownError::TooManySignals);
        }

        if *lock == 0 {
            self.lock.notify_all();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    ///Adds one to the count.
    ///
    ///# Errors
    ///
    ///See [`add`] for the situations where this function will return an error.
    ///
    ///[`add`]: #method.add
    pub fn increment(&self) -> Result<(), CountdownError> {
        self.add(1)
    }

    ///Subtracts one from the counter, and returns whether this caused any waiting threads to wake
    ///up.
    ///
    ///# Errors
    ///
    ///See [`signal`] for the situations where this function will return an error.
    ///
    ///[`signal`]: #method.signal
    pub fn decrement(&self) -> Result<bool, CountdownError> {
        self.signal(1)
    }

    ///Increments the counter, then returns a guard object that will decrement the counter upon
    ///drop.
    ///
    ///# Errors
    ///
    ///This function will return the same errors as `add`. If the event has already signaled by the
    ///time the guard is dropped (and would cause its `decrement` call to return an error), then
    ///the error will be silently ignored.
    pub fn guard(&self) -> Result<CountdownGuard, CountdownError> {
        CountdownGuard::new(self)
    }

    ///Blocks the current thread until the counter reaches zero.
    ///
    ///This function will block indefinitely until the counter reaches zero. It will return
    ///immediately if it is already at zero.
    pub fn wait(&self) {
        let mut count = util::guts(self.counter.lock());

        while *count > 0 {
            count = util::guts(self.lock.wait(count));
        }
    }

    ///Blocks the current thread until the timer reaches zero, or until the given timeout elapses,
    ///returning the count at the time of wakeup and whether the timeout is known to have elapsed.
    ///
    ///This function will return immediately if the counter was already at zero. Otherwise, it will
    ///block for roughly no longer than `timeout`. Due to limitations in the platform specific
    ///implementation of `std::sync::Condvar`, this method could spuriously wake up both before the
    ///timeout elapsed and without the count being zero.
    pub fn wait_timeout(&self, timeout: Duration) -> (isize, bool) {
        let count = util::guts(self.counter.lock());

        if *count == 0 {
            return (*count, false);
        }

        let (count, status) = util::guts(self.lock.wait_timeout(count, timeout));

        (*count, status.timed_out())
    }
}

///An opaque guard struct that decrements the count of a borrowed `CountdownEvent` on drop.
pub struct CountdownGuard<'a> {
    event: &'a CountdownEvent,
}

impl<'a> CountdownGuard<'a> {
    fn new(event: &'a CountdownEvent) -> Result<CountdownGuard<'a>, CountdownError> {
        try!(event.increment());
        Ok(CountdownGuard {
                event: event,
        })
    }
}

impl<'a> Drop for CountdownGuard<'a> {
    fn drop(&mut self) {
        //if decrement() returns an error, then the event has already been signaled somehow. i'm
        //not gonna care about it tho
        self.event.decrement().ok();
    }
}

///Determines the reset behavior of a `SignalEvent`.
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum SignalKind {
    ///An activated `SignalEvent` automatically resets when a thread is resumed.
    ///
    ///`SignalEvent`s with this kind will only resume one thread at a time.
    Auto,
    ///An activated `SignalEvent` must be manually reset to block threads again.
    ///
    ///`SignalEvent`s with this kind will signal every waiting thread to continue at once.
    Manual,
}

/// A synchronization primitive that allows one or more threads to wait on a signal from another
/// thread.
///
/// With a `SignalEvent`, it's possible to have one or more threads gate on a signal from another
/// thread. The behavior for what happens when an event is signaled depends on the value of the
/// `signal_kind` parameter given to `new`:
///
/// * A value of `SignalKind::Auto` will automatically reset the signal when a thread is resumed by
///   this event. If more than one thread is waiting on the event when it is signaled, only one
///   will be resumed.
/// * A value of `SignalKind::Manual` will remain signaled until it is manually reset. If more than
///   one thread is waiting on the event when it is signaled, all of them will be resumed. Any
///   other thread that tries to wait on the signal before it is reset will not be blocked at all.
///
/// `SignalEvent` is a port of [System.Threading.EventWaitHandle][src-link] from .NET.
///
/// [src-link]: https://msdn.microsoft.com/en-us/library/system.threading.eventwaithandle(v=vs.110).aspx
///
/// # Example
///
/// ```
/// use synchronoise::{SignalEvent, SignalKind};
/// use std::sync::Arc;
/// use std::thread;
/// use std::time::Duration;
///
/// let start_signal = Arc::new(SignalEvent::new(false, SignalKind::Manual));
/// let stop_signal = Arc::new(SignalEvent::new(false, SignalKind::Auto));
/// let mut thread_count = 5;
///
/// for i in 0..thread_count {
///     let start = start_signal.clone();
///     let stop = stop_signal.clone();
///     thread::spawn(move || {
///         //as a Manual-reset signal, all the threads will start at the same time
///         start.wait();
///         thread::sleep(Duration::from_secs(i));
///         println!("thread {} activated!", i);
///         stop.signal();
///     });
/// }
///
/// start_signal.signal();
///
/// while thread_count > 0 {
///     //as an Auto-reset signal, this will automatically reset when resuming
///     //so when the loop comes back, we don't have to reset before blocking again
///     stop_signal.wait();
///     thread_count -= 1;
/// }
///
/// println!("all done!");
/// ```
pub struct SignalEvent {
    reset: SignalKind,
    signal: AtomicBool,
    waiting: MsQueue<thread::Thread>,
}

impl SignalEvent {
    ///Creates a new `SignalEvent` with the given starting state and reset behavior.
    ///
    ///If `init_state` is `true`, then this `SignalEvent` will start with the signal already set,
    ///so that threads that wait will immediately unblock.
    pub fn new(init_state: bool, signal_kind: SignalKind) -> SignalEvent {
        SignalEvent {
            reset: signal_kind,
            signal: AtomicBool::new(init_state),
            waiting: MsQueue::new(),
        }
    }

    ///Returns the current signal status of the `SignalEvent`.
    pub fn status(&self) -> bool {
        self.signal.load(Ordering::SeqCst)
    }

    ///Sets the signal on this `SignalEvent`, potentially waking up one or all threads waiting on
    ///it.
    ///
    ///If more than one thread is waiting on the event, the behavior is different depending on the
    ///`SignalKind` passed to the event when it was created. For a value of `Auto`, one thread will
    ///be resumed. For a value of `Manual`, all waiting threads will be resumed.
    ///
    ///If no thread is currently waiting on the event, its state will be set regardless. Any future
    ///attempts to wait on the event will unblock immediately, except for a `SignalKind` of Auto,
    ///which will immediately unblock the first thread only.
    pub fn signal(&self) {
        self.signal.store(true, Ordering::SeqCst);

        match self.reset {
            //there may be duplicate handles in the queue due to spurious wakeups, so just loop
            //until we know the signal got reset - any that got woken up wrongly will also observe
            //the reset signal and push their handle back in
            SignalKind::Auto => while self.signal.load(Ordering::SeqCst) {
                if let Some(thread) = self.waiting.try_pop() {
                    thread.unpark();
                } else {
                    break;
                }
            }
            //for manual resets, just unilaterally drain the queue
            SignalKind::Manual => while let Some(thread) = self.waiting.try_pop() {
                thread.unpark();
            }
        }
    }

    ///Resets the signal on this `SignalEvent`, allowing threads that wait on it to block.
    pub fn reset(&self) {
        self.signal.store(false, Ordering::SeqCst);
    }

    ///Blocks this thread until another thread calls `signal`.
    ///
    ///If this event is already set, then this function will immediately return without blocking.
    ///For events with a `SignalKind` of `Auto`, this will reset the signal so that the next thread
    ///to wait will block.
    pub fn wait(&self) {
        //loop on the park in case we spuriously wake up
        while !self.check_signal() {
            //push every time in case there's a race between `signal` and this, since on
            //`SignalKind::Auto` it will loop until someone turns it off - but only one will
            //actually exit this loop, because `check_signal` does a CAS
            self.waiting.push(thread::current());
            thread::park();
        }
    }

    ///Blocks this thread until either another thread calls `signal`, or until the timeout elapses.
    ///
    ///This function returns the status of the signal when it woke up. If this function exits
    ///because the signal was set, and this event has a `SignalKind` of `Auto`, the signal will be
    ///reset so that the next thread to wait will block.
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        use std::time::Instant;

        let begin = Instant::now();
        let mut first = true;
        let mut remaining = timeout;
        loop {
            if self.check_signal() {
                return true;
            }

            if first {
                first = false;
            } else {
                let elapsed = begin.elapsed();
                if elapsed >= timeout {
                    return self.status();
                } else {
                    remaining = timeout - elapsed;
                }
            }

            thread::park_timeout(remaining);
        }
    }

    ///Perfoms an atomic compare-and-swap on the signal, resetting it if (1) it was set, and (2)
    ///this `SignalEvent` was configured with `SignalKind::Auto`. Returns whether the signal was
    ///previously set.
    fn check_signal(&self) -> bool {
        self.signal.compare_and_swap(true, self.reset == SignalKind::Manual, Ordering::SeqCst)
    }
}

/// A synchronization primitive that allows for multiple concurrent wait-free "writer critical
/// sections" and a "reader phase flip" that can wait for all currently-active writers to finish.
///
/// The basic interaction setup for a `WriterReaderPhaser` is as follows:
///
/// * Any number of writers can open and close a "writer critical section" with no waiting.
/// * Zero or one readers can be active at one time, by holding a "read lock". Any reader who
///   wishes to open a "read lock" while another one is active is blocked until the previous one
///   finishes.
/// * The holder of a read lock may request a "phase flip", which causes the reader to wait until
///   all current writer critical sections are finished before continuing.
///
/// `WriterReaderPhaser` is a port of the primitive of the same name from `HdrHistogram`. For a
/// summary of the rationale behind its design, see [this post by its author][wrp-blog]. Part of
/// its assumptions is that this primitive is synchronizing access to a double-buffered set of
/// counters, and the readers are expected to swap the buffers while holding a read lock but before
/// flipping the phase. This allows them to access a stable sample to read and perform calculations
/// from, while writers still have wait-free synchronization.
///
/// [wrp-blog]: https://stuff-gil-says.blogspot.com/2014/11/writerreaderphaser-story-about-new.html
///
/// "Writer critical sections" and "read locks" are represented by guard structs that allow
/// scope-based resource management of the counters and locks.
///
/// * The `PhaserCriticalSection` atomically increments and decrements the phase counters upon
///   creation and drop. These operations use `std::sync::atomic::AtomicIsize` from the standard
///   library, and provide no-wait handling for platforms with atomic addition instructions.
/// * The `PhaserReadLock` is kept in the `WriterReaderPhaser` as a Mutex, enforcing the mutual
///   exclusion of the read lock. The "phase flip" operation is defined on the read lock guard
///   itself, enforcing that only the holder of a read lock can execute one.
pub struct WriterReaderPhaser {
    start_epoch: Arc<AtomicIsize>,
    even_end_epoch: Arc<AtomicIsize>,
    odd_end_epoch: Arc<AtomicIsize>,
    read_lock: Mutex<PhaserReadLock>,
}

/// Guard struct that represents a "writer critical section" for a `WriterReaderPhaser`.
///
/// `PhaserCriticalSection` is a scope-based guard to signal the beginning and end of a "writer
/// critical section" to the phaser. Upon calling `writer_critical_section`, the phaser atomically
/// increments a counter, and when the returned `PhaserCriticalSection` drops, the `drop` call
/// atomically increments another counter. On platforms with atomic increment instructions, this
/// should result in wait-free synchronization.
///
/// # Example
///
/// ```
/// # let phaser = synchronoise::WriterReaderPhaser::new();
/// {
///     let _guard = phaser.writer_critical_section();
///     // perform writes
/// } // _guard drops, signaling the end of the section
/// ```
pub struct PhaserCriticalSection {
    end_epoch: Arc<AtomicIsize>,
}

///Upon drop, a `PhaserCriticalSection` will signal its parent `WriterReaderPhaser` that the
///critical section has ended.
impl Drop for PhaserCriticalSection {
    fn drop(&mut self) {
        self.end_epoch.fetch_add(1, Ordering::Release);
    }
}

/// Guard struct for a `WriterReaderPhaser` that allows a reader to perform a "phase flip".
///
/// The `PhaserReadLock` struct allows one to perform a "phase flip" on its parent
/// `WriterReaderPhaser`. It is held in a `std::sync::Mutex` in its parent phaser, enforcing that
/// only one reader may be active at once.
///
/// The `flip_phase` call performs a spin-wait while waiting the the currently-active writers to
/// finish. A sleep time may be added between checks by calling `flip_with_sleep` instead.
///
/// # Example
///
/// ```
/// # let phaser = synchronoise::WriterReaderPhaser::new();
/// {
///     let lock = phaser.read_lock().unwrap();
///     // swap buffers
///     lock.flip_phase();
///     // reader now has access to a stable snapshot
/// } // lock drops, relinquishing the read lock and allowing another reader to lock
/// ```
pub struct PhaserReadLock {
    start_epoch: Arc<AtomicIsize>,
    even_end_epoch: Arc<AtomicIsize>,
    odd_end_epoch: Arc<AtomicIsize>,
}

impl WriterReaderPhaser {
    ///Creates a new `WriterReaderPhaser`.
    pub fn new() -> WriterReaderPhaser {
        let start = Arc::new(AtomicIsize::new(0));
        let even = Arc::new(AtomicIsize::new(0));
        let odd = Arc::new(AtomicIsize::new(ISIZE_MIN));
        let read_lock = PhaserReadLock {
            start_epoch: start.clone(),
            even_end_epoch: even.clone(),
            odd_end_epoch: odd.clone(),
        };

        WriterReaderPhaser {
            start_epoch: start,
            even_end_epoch: even,
            odd_end_epoch: odd,
            read_lock: Mutex::new(read_lock),
        }
    }

    ///Enters a writer critical section, returning a guard object that signals the end of the
    ///critical section upon drop.
    pub fn writer_critical_section(&self) -> PhaserCriticalSection {
        let flag = self.start_epoch.fetch_add(1, Ordering::Release);

        if flag < 0 {
            PhaserCriticalSection {
                end_epoch: self.odd_end_epoch.clone(),
            }
        } else {
            PhaserCriticalSection {
                end_epoch: self.even_end_epoch.clone(),
            }
        }
    }

    ///Enter a reader criticial section, potentially blocking until a currently active read section
    ///finishes. Returns a guard object that allows the user to flip the phase of the
    ///`WriterReaderPhaser`, and unlocks the read lock upon drop.
    ///
    ///# Errors
    ///
    ///If another reader critical section panicked while holding the read lock, this call will
    ///return an error once the lock is acquired. See the documentation for
    ///`std::sync::Mutex::lock` for details.
    pub fn read_lock(&self) -> LockResult<MutexGuard<PhaserReadLock>> {
        self.read_lock.lock()
    }
}

impl PhaserReadLock {
    ///Wait until all currently-active writer critical sections have completed.
    pub fn flip_phase(&self) {
        self.flip_with_sleep(Duration::default());
    }

    ///Wait until all currently-active writer critical sections have completed. While waiting,
    ///sleep with the given duration.
    pub fn flip_with_sleep(&self, sleep_time: Duration) {
        let next_phase_even = self.start_epoch.load(Ordering::Relaxed) < 0;

        let start_value = if next_phase_even {
            let tmp = 0;
            self.even_end_epoch.store(tmp, Ordering::Relaxed);
            tmp
        } else {
            let tmp = ISIZE_MIN;
            self.odd_end_epoch.store(tmp, Ordering::Relaxed);
            tmp
        };

        let value_at_flip = self.start_epoch.swap(start_value, Ordering::AcqRel);

        let end_epoch = if next_phase_even {
            self.odd_end_epoch.clone()
        } else {
            self.even_end_epoch.clone()
        };

        while end_epoch.load(Ordering::Relaxed) != value_at_flip {
            if sleep_time == Duration::default() {
                thread::yield_now();
            } else {
                thread::sleep(sleep_time);
            }
        }
    }
}
