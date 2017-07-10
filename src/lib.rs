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
