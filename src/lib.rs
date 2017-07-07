use std::collections::BinaryHeap;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread::{self, JoinHandle};

trait Runable: Ord + Send + Sync + 'static {
    fn run(&self);
    fn abandon(&self);
}

type TaskQueue<T> = Arc<Mutex<BinaryHeap<Arc<T>>>>;

struct ThreadPool<T: Runable> {
    tasks: TaskQueue<T>,
    sender: Option<Sender<TaskQueue<T>>>,
    receiver: Arc<Mutex<Receiver<TaskQueue<T>>>>,
    workers: Vec<JoinHandle<()>>,
}

impl<T: Runable> ThreadPool<T> {
    fn new(num_worker: usize) -> Self {
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
                            assert!(lock.len() > 0);
                            lock.pop().unwrap()
                        };
                        run.run();
                    }
                    Err(_) => break,
                }
            }));
        }

        ThreadPool {
            tasks: Default::default(),
            sender: Some(sender),
            receiver: receiver,
            workers: workers,
        }
    }

    fn accept(&self, task: Arc<T>) {
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
            while let Some(_) = lock.pop() {}
        }
        // drop sender
        self.sender.take();
        while let Some(w) = self.workers.pop() {
            w.join().unwrap();
        }
    }
}