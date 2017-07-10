use std::collections::BinaryHeap;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender};
use std::thread::{self, JoinHandle};

pub trait Runable: Ord + Send + Sync + 'static {
    fn run(&self);
    fn abandon(&self);
}

type TaskQueue<T> = Arc<Mutex<BinaryHeap<Arc<T>>>>;

pub struct ThreadPool<T: Runable> {
    tasks: TaskQueue<T>,
    sender: Option<Sender<TaskQueue<T>>>,
    workers: Vec<JoinHandle<()>>,
}

impl<T: Runable> ThreadPool<T> {
    pub fn new(num_worker: usize) -> Self {
        let (sender, receiver) = channel::<TaskQueue<T>>();
        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = vec![];
        for _ in 0..num_worker {
            let recv = receiver.clone();
            workers.push(thread::spawn(move || loop {
                let message = {
                    let lock = recv.lock().unwrap();
                    lock.recv()
                };

                match message {
                    Ok(queue) => {
                        let run = {
                            let mut lock = queue.lock().unwrap();
                            lock.pop()
                        };
                        // run may be None when threadpool is dropping.
                        run.map(|t| t.run());
                    }
                    Err(_) => break,
                }
            }));
        }

        ThreadPool {
            tasks: Default::default(),
            sender: Some(sender),
            workers: workers,
        }
    }

    pub fn accept(&self, task: Arc<T>) {
        {
            let mut lock = self.tasks.lock().unwrap();
            lock.push(task);
        }
        self.sender
            .as_ref()
            .unwrap()
            .send(self.tasks.clone())
            .unwrap();
    }

    pub fn waiting_tasks_num(&self) -> usize {
        let lock = self.tasks.lock().unwrap();
        lock.len()
    }
}

impl<T: Runable> Drop for ThreadPool<T> {
    fn drop(&mut self) {
        {
            let mut lock = self.tasks.lock().unwrap();
            while let Some(run) = lock.pop() {
                run.abandon();
            }
        }
        // drop sender
        self.sender.take();
        while let Some(w) = self.workers.pop() {
            w.join().unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{self, AtomicUsize};
    use std::cmp;
    use std::thread;

    struct TaskA<F: FnOnce() + Send + 'static> {
        pri: usize,
        run_count: Arc<AtomicUsize>,
        abandon_fn: Mutex<Option<F>>,
    }

    impl<F> TaskA<F>
    where
        F: FnOnce() + Send + 'static,
    {
        fn new(pri: usize, run_count: Arc<AtomicUsize>) -> Self {
            TaskA {
                pri: pri,
                run_count: run_count,
                abandon_fn: Default::default(),
            }
        }

        fn register_abandon(&self, f: F) {
            let mut lock = self.abandon_fn.lock().unwrap();
            *lock = Some(f);
        }
    }

    impl<F> Ord for TaskA<F>
    where
        F: FnOnce() + Send + 'static,
    {
        fn cmp(&self, other: &Self) -> cmp::Ordering {
            self.pri.cmp(&other.pri)
        }
    }

    impl<F> PartialOrd for TaskA<F>
    where
        F: FnOnce() + Send + 'static,
    {
        fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    impl<F> Eq for TaskA<F>
    where
        F: FnOnce() + Send + 'static,
    {
    }

    impl<F> PartialEq for TaskA<F>
    where
        F: FnOnce() + Send + 'static,
    {
        fn eq(&self, other: &Self) -> bool {
            self.pri == other.pri
        }
    }

    impl<F> Runable for TaskA<F>
    where
        F: FnOnce() + Send + 'static,
    {
        fn run(&self) {
            self.run_count.fetch_add(1, atomic::Ordering::SeqCst);
        }

        fn abandon(&self) {
            self.abandon_fn.lock().unwrap().take().map(|t| t());
        }
    }

    #[test]
    fn test_new() {
        let run_count = Arc::new(AtomicUsize::new(0));
        let task_num = 1000_usize;
        {
            let tp = ThreadPool::new(1);
            for i in 0..task_num {
                let t = Arc::new(TaskA::new(i, run_count.clone()));
                t.register_abandon(|| {});
                tp.accept(t)
            }
            loop {
                if tp.waiting_tasks_num() == 0 {
                    break;
                }
                thread::sleep(std::time::Duration::from_secs(1));
            }
        }

        assert_eq!(task_num, run_count.load(atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_drop() {
        let run_count = Arc::new(AtomicUsize::new(0));
        let abandon_count = Arc::new(AtomicUsize::new(0));
        let task_num = 1 << 20;
        {
            let tp = ThreadPool::new(1);
            for i in 0..task_num {
                let t = Arc::new(TaskA::new(i, run_count.clone()));
                let a = abandon_count.clone();
                t.register_abandon(move || { a.fetch_add(1, atomic::Ordering::SeqCst); });
                tp.accept(t)
            }
        }
        let num = run_count.load(atomic::Ordering::SeqCst) +
            abandon_count.load(atomic::Ordering::SeqCst);
        assert_eq!(num, task_num);
    }
}
