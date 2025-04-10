use crate::futures::futures::{
    Future, Sink, SinkExt, StreamExt,
    channel::mpsc,
    select,
    task::{Context, Poll},
};
use crate::runtime::Action;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};

/// An event loop proxy with backpressure that implements `Sink`.
#[derive(Debug)]
pub struct Proxy<T: 'static> {
    raw: winit::event_loop::EventLoopProxy,
    sender: mpsc::Sender<Action<T>>,
    notifier: mpsc::Sender<usize>,
    action_sender: mpsc::UnboundedSender<Action<T>>,
    action_queue: Arc<Mutex<Vec<Action<T>>>>,
}

impl<T: 'static> Clone for Proxy<T> {
    fn clone(&self) -> Self {
        Self {
            raw: self.raw.clone(),
            sender: self.sender.clone(),
            notifier: self.notifier.clone(),
            action_sender: self.action_sender.clone(),
            action_queue: self.action_queue.clone(),
        }
    }
}

impl<T: 'static> Proxy<T> {
    const MAX_SIZE: usize = 100;

    /// Creates a new [`Proxy`] from an `EventLoopProxy`.
    pub fn new(
        raw: winit::event_loop::EventLoopProxy,
    ) -> (Self, impl Future<Output = ()>, impl Future<Output = ()>) {
        let (notifier, mut processed) = mpsc::channel(Self::MAX_SIZE);
        let (sender, mut receiver) = mpsc::channel(Self::MAX_SIZE);
        let (mut action_sender, mut action_receiver) = mpsc::unbounded();
        let action_queue = Arc::new(Mutex::new(Vec::new()));

        let proxy = raw.clone();
        let action_sender_clone = action_sender.clone();
        let action_queue_clone = action_queue.clone();

        let worker = async move {
            let mut count = 0;

            loop {
                if count < Self::MAX_SIZE {
                    select! {
                        message = receiver.select_next_some() => {
                            action_sender.send(message).await.expect("Failed to send message");
                            count += 1;
                        }
                        amount = processed.select_next_some() => {
                            count = count.saturating_sub(amount);
                        }
                        complete => break,
                    }
                } else {
                    select! {
                        amount = processed.select_next_some() => {
                            count = count.saturating_sub(amount);
                        }
                        complete => break,
                    }
                }
            }
        };

        let action_worker = async move {
            loop {
                let action = action_receiver.select_next_some().await;
                let mut queue = action_queue.lock().unwrap();
                queue.push(action);
                proxy.wake_up();
            }
        };

        (
            Self {
                raw,
                sender,
                notifier,
                action_sender: action_sender_clone,
                action_queue: action_queue_clone,
            },
            worker,
            action_worker,
        )
    }

    /// Sends a value to the event loop.
    ///
    /// Note: This skips the backpressure mechanism with an unbounded
    /// channel. Use sparingly!
    pub fn send(&mut self, value: T)
    where
        T: std::fmt::Debug,
    {
        self.send_action(Action::Output(value));
    }

    /// Sends an action to the event loop.
    ///
    /// Note: This skips the backpressure mechanism with an unbounded
    /// channel. Use sparingly!
    pub fn send_action(&mut self, action: Action<T>)
    where
        T: std::fmt::Debug,
    {
        self.action_sender
            .start_send(action)
            .expect("Failed to send action");
    }

    /// Frees an amount of slots for additional messages to be queued in
    /// this [`Proxy`].
    pub fn free_slots(&mut self, amount: usize) {
        let _ = self.notifier.start_send(amount);
    }

    /// Drains the action queue and returns all actions.
    pub fn drain(&mut self) -> Vec<Action<T>> {
        let mut queue = self.action_queue.lock().unwrap();
        let mut actions = Vec::new();

        while let Some(action) = queue.pop() {
            actions.push(action);
        }

        actions
    }
}

impl<T: 'static> Sink<Action<T>> for Proxy<T> {
    type Error = mpsc::SendError;

    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.sender.poll_ready(cx)
    }

    fn start_send(
        mut self: Pin<&mut Self>,
        action: Action<T>,
    ) -> Result<(), Self::Error> {
        self.sender.start_send(action)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match self.sender.poll_ready(cx) {
            Poll::Ready(Err(ref e)) if e.is_disconnected() => {
                // If the receiver disconnected, we consider the sink to be flushed.
                Poll::Ready(Ok(()))
            }
            x => x,
        }
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.sender.disconnect();
        Poll::Ready(Ok(()))
    }
}
