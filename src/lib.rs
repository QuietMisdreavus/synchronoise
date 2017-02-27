//! A collection of synchronization primitives that build on the primitives available in the
//! standard library.
use std::sync::{Mutex, Condvar};

///A synchronization primitive that signals when its count reaches zero.
///
///With a `CountdownEvent`, it's possible to cause one thread to wait on a set of computations
///occurring in other threads by making the other threads interact with the counter as they perform
///their work.
///
///The main limitation of a CountdownEvent is that once its counter reaches zero (even by starting
///there), any attempts to update the counter will return `CountdownError::AlreadySet` until the
///counter is reset by calling `reset` or `reset_to_count`.
///
///# Example
///
///```
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
///```
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
        let lock = self.counter.lock().unwrap();

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
        let mut lock = self.counter.lock().unwrap();

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
        let mut lock = self.counter.lock().unwrap();

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
    ///[`add`]: fn.add.html
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
    ///[`signal`]: fn.signal.html
    pub fn decrement(&self) -> Result<bool, CountdownError> {
        self.signal(1)
    }

    ///Blocks the current thread until the counter reaches zero.
    ///
    ///This function will block indefinitely until the counter reaches zero. It will return
    ///immediately if it is already at zero.
    pub fn wait(&self) {
        let mut count = self.counter.lock().unwrap();

        while *count > 0 {
            count = self.lock.wait(count).unwrap();
        }
    }
}
