//! A simple thread pool.
//!
//! Example
//!
//! ```
//! use std::sync::Arc;

//! use threadpool::Task;
//! use threadpool::Builder;

//! struct Simple {
//!     name: String,
//! }

//! impl Task for Simple {
//!     fn run(&mut self) {
//!         println!("{} done", self.name);
//!     }
//! }
//!
//! let pool = Builder::new()
//!     .worker_count(4)
//!     .name("simple_thread_pool")
//!     .build();
//! for i in 0..100 {
//!     let s = Simple {
//!         name: format!("{}", i),
//!     };
//!     pool.spawn(s);
//! }
//! ```

extern crate crossbeam_channel;
#[macro_use]
extern crate log;
extern crate num_cpus;

use std::thread::{self, JoinHandle};
use std::marker::PhantomData;

use crossbeam_channel::{Receiver, Sender};

/// An error indicate the thread pool was dropped.
#[derive(Debug)]
pub struct Stopped;

/// User's real task should implement this trait.
///
/// Why not just use `FnOnce()`, use a user defined type can avoid
/// dynamic dispatching.
pub trait Task: Send + 'static {
    /// Run the task.
    fn run(&mut self);
}

/// ThreadPool builder.
pub struct Builder<T: Task> {
    name: String,
    num: usize,
    _marker: PhantomData<T>,
}

/// ThreadPool
pub struct ThreadPool<T: Task> {
    name: String,
    tx: Sender<Option<T>>,
    workers: Vec<Worker<T>>,
}

/// ThreadPool handle.
///
/// It may return `Stopped` when spawning task.
pub struct Handle<T: Task> {
    tx: Sender<Option<T>>,
}

impl<T: Task> Builder<T> {
    /// Create a thread pool builder using the default configuration.
    pub fn new() -> Self {
        Builder {
            name: "threadpool".into(),
            num: num_cpus::get(),
            _marker: PhantomData,
        }
    }

    /// Set the thread pool name.
    pub fn name<N: Into<String>>(mut self, name: N) -> Self {
        self.name = name.into();
        self
    }

    /// Set worker count.
    pub fn worker_count(mut self, count: usize) -> Self {
        self.num = count;
        self
    }

    /// Create the thread pool.
    pub fn build(self) -> ThreadPool<T> {
        let mut workers = Vec::with_capacity(self.num);
        let (tx, rx) = crossbeam_channel::unbounded();
        for i in 0..self.num {
            let rx = rx.clone();
            let name = format!("{}_worker_{}", self.name, i);
            let worker = Worker::new(name, rx);
            workers.push(worker);
        }

        ThreadPool {
            name: self.name,
            tx: tx,
            workers: workers,
        }
    }
}

impl<T: Task> Default for Builder<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Task> ThreadPool<T> {
    /// Create a thread pool using the default configuration.
    pub fn new() -> Self {
        Builder::new().build()
    }

    /// Get the thread pool name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get worker count.
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    /// Spawn a task.
    pub fn spawn(&self, task: T) {
        self.tx.send(Some(task)).unwrap();
    }

    /// Get the thread pool handle.
    pub fn handle(&self) -> Handle<T> {
        Handle {
            tx: self.tx.clone(),
        }
    }
}

impl<T: Task> Default for ThreadPool<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Task> Drop for ThreadPool<T> {
    fn drop(&mut self) {
        info!("ThreadPool: {} is dropping", self.name);
        for _ in 0..self.worker_count() {
            self.tx.send(None).unwrap();
        }
    }
}

impl<T: Task> Handle<T> {
    /// Spawn a task.
    ///
    /// It may return `Stopped` when the thread pool was dropped.
    pub fn spawn(&self, task: T) -> Result<(), Stopped> {
        self.tx.send(Some(task)).map_err(|_| Stopped)
    }
}

/// Thread helper.
struct Worker<T: Task> {
    thread: Option<JoinHandle<()>>,
    _marker: PhantomData<T>,
}

impl<T: Task> Worker<T> {
    fn new<N: Into<String>>(name: N, rx: Receiver<Option<T>>) -> Self {
        let thread = thread::Builder::new()
            .name(name.into())
            .spawn(move || run(&rx))
            .unwrap();
        Worker {
            thread: Some(thread),
            _marker: PhantomData,
        }
    }
}

fn run<T: Task>(rx: &Receiver<Option<T>>) {
    while let Some(mut task) = rx.recv().unwrap() {
        task.run();
    }
}

impl<T: Task> Drop for Worker<T> {
    fn drop(&mut self) {
        self.thread.take().unwrap().join().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::{Duration, Instant};

    #[test]
    fn test_threadpool() {
        let pool = Builder::new().worker_count(2).build();

        let (tx, rx) = crossbeam_channel::unbounded();
        let start = Instant::now();
        for dur in vec![500, 1000, 1500] {
            let task = MyTask {
                dur: dur,
                tx: tx.clone(),
            };
            pool.spawn(task);
        }
        assert_eq!(rx.recv().unwrap(), 500);
        assert_eq!(rx.recv().unwrap(), 1000);
        assert_eq!(rx.recv().unwrap(), 1500);

        assert!(start.elapsed() > Duration::from_millis(2000));
        assert!(start.elapsed() < Duration::from_millis(3000));
    }

    #[test]
    fn test_handle() {
        let pool = Builder::new().worker_count(2).build();
        let handle = pool.handle();
        drop(pool);
        let res = handle.spawn(Empty);
        assert!(res.is_err());
    }

    struct MyTask {
        dur: u64,
        tx: Sender<u64>,
    }
    impl Task for MyTask {
        fn run(&mut self) {
            thread::sleep(Duration::from_millis(self.dur));
            self.tx.send(self.dur).unwrap();
        }
    }

    struct Empty;
    impl Task for Empty {
        fn run(&mut self) {}
    }
}
