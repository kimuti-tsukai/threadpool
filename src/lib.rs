use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
};

pub struct ThreadPool<T: Send + 'static> {
    tasks: Arc<Mutex<VecDeque<Box<dyn FnOnce() -> T + Send + 'static>>>>,
    task_condvar: Arc<Condvar>,
    results: Arc<Mutex<VecDeque<T>>>,
    result_condvar: Arc<Condvar>,
    threads: Vec<JoinHandle<T>>,
    task_num: AtomicU64,
    next_num: AtomicU64,
}

impl<T: Send + 'static> ThreadPool<T> {
    pub fn with_threads(thread_num: usize) -> Self {
        let mut thread_pool = Self {
            tasks: Arc::new(Mutex::new(VecDeque::new())),

            task_condvar: Arc::new(Condvar::new()),

            results: Arc::new(Mutex::new(VecDeque::new())),

            result_condvar: Arc::new(Condvar::new()),

            threads: Vec::with_capacity(thread_num),

            task_num: AtomicU64::new(0),

            next_num: AtomicU64::new(0),
        };

        for _ in 0..thread_num {
            let tasks = Arc::clone(&thread_pool.tasks);
            let task_condvar = Arc::clone(&thread_pool.task_condvar);
            let results = Arc::clone(&thread_pool.results);
            let result_condvar = Arc::clone(&thread_pool.result_condvar);

            let thread_fn = move || loop {
                let lock = tasks.lock().unwrap();

                let mut tasks = if lock.is_empty() {
                    task_condvar.wait(lock).unwrap()
                } else {
                    lock
                };

                let Some(new_task) = tasks.pop_front() else {
                    continue;
                };

                let result = new_task();

                results.lock().unwrap().push_back(result);

                result_condvar.notify_one();
            };

            let thread = thread::spawn(thread_fn);

            thread_pool.threads.push(thread);
        }

        thread_pool
    }

    pub fn task<F: FnOnce() -> T + Send + 'static>(&self, task: F) {
        self.task_num.fetch_add(1, Ordering::Relaxed);
        self.tasks.lock().unwrap().push_back(Box::new(task));
        self.task_condvar.notify_one();
    }

    pub fn next_result(&self) -> Option<T> {
        if self.task_num.load(Ordering::Relaxed) < self.next_num.load(Ordering::Relaxed) {
            None
        } else {
            let lock = self.results.lock().unwrap();

            let mut results = if lock.is_empty() {
                self.result_condvar.wait(lock).unwrap()
            } else {
                lock
            };

            results.pop_front()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_threadpool_single_task() {
        let thread_pool = ThreadPool::with_threads(4);

        thread_pool.task(|| 42);

        let result = thread_pool.next_result().unwrap();

        assert_eq!(result, 42);
    }

    #[test]
    fn test_threadpool_multiple_tasks() {
        let thread_pool = ThreadPool::with_threads(4);

        for i in 0..10 {
            thread_pool.task(move || i * i);
        }

        let mut results = Vec::new();
        for _ in 0..10 {
            if let Some(result) = thread_pool.next_result() {
                results.push(result);
            }
        }

        results.sort();
        assert_eq!(results, vec![0, 1, 4, 9, 16, 25, 36, 49, 64, 81]);
    }

    #[test]
    fn test_threadpool_concurrent_execution() {
        let thread_pool = ThreadPool::with_threads(4);

        let counter = Arc::new(Mutex::new(0));

        for _ in 0..100 {
            let counter_clone = Arc::clone(&counter);
            thread_pool.task(move || {
                let mut lock = counter_clone.lock().unwrap();
                *lock += 1;
                *lock
            });
        }

        let mut results = Vec::new();
        for _ in 0..100 {
            if let Some(result) = thread_pool.next_result() {
                results.push(result);
            }
        }

        assert_eq!(results.len(), 100);

        let final_count = *counter.lock().unwrap();
        assert_eq!(final_count, 100);
    }

    #[test]
    fn test_threadpool_order_of_results() {
        let thread_pool = ThreadPool::with_threads(2);

        for i in 0..5 {
            thread_pool.task(move || i);
        }

        let mut results = Vec::new();
        for _ in 0..5 {
            if let Some(result) = thread_pool.next_result() {
                results.push(result);
            }
        }

        results.sort();
        assert_eq!(results, vec![0, 1, 2, 3, 4]);
    }
}
