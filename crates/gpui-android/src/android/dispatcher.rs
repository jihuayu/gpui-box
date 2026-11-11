use super::bridge::post;
use gpui::{
    PlatformDispatcher, Priority, RunnableVariant,
    queue::{PriorityQueueReceiver, PriorityQueueSender},
};
use std::{
    sync::Arc,
    thread::{self, ThreadId},
    time::Duration,
};

pub(super) struct Dispatcher {
    main_thread: ThreadId,
    main: PriorityQueueSender<RunnableVariant>,
    background: PriorityQueueSender<RunnableVariant>,
}

impl Dispatcher {
    pub fn new() -> (Arc<Self>, PriorityQueueReceiver<RunnableVariant>) {
        let (main, receiver) = PriorityQueueReceiver::new();
        let (background, workers) = PriorityQueueReceiver::<RunnableVariant>::new();
        for index in 0..4 {
            let mut worker = workers.clone();
            thread::Builder::new()
                .name(format!("gpui-worker-{index}"))
                .spawn(move || {
                    while let Ok(task) = worker.pop() {
                        task.run();
                    }
                })
                .expect("start GPUI worker");
        }
        (
            Arc::new(Self {
                main_thread: thread::current().id(),
                main,
                background,
            }),
            receiver,
        )
    }
}

impl PlatformDispatcher for Dispatcher {
    fn is_main_thread(&self) -> bool {
        thread::current().id() == self.main_thread
    }
    fn dispatch(&self, task: RunnableVariant, priority: Priority) {
        let _ = self.background.send(priority, task);
    }
    fn dispatch_on_main_thread(&self, task: RunnableVariant, priority: Priority) {
        if self.main.send(priority, task).is_ok() {
            post(0);
        }
    }
    fn dispatch_after(&self, duration: Duration, task: RunnableVariant) {
        // M0 timers are isolated from the worker pool so sleeps cannot starve it.
        // A shared deadline queue can replace these threads after profiling.
        thread::spawn(move || {
            thread::sleep(duration);
            task.run();
        });
    }
    fn spawn_realtime(&self, f: Box<dyn FnOnce() + Send>) {
        thread::spawn(f);
    }
}
