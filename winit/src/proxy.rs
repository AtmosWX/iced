use iced_futures::futures::SinkExt as _;

use crate::futures::futures::{
    Future, Sink, StreamExt,
    channel::mpsc,
    select,
    task::{Context, Poll},
};
use crate::runtime::Action;
use objc2_core_foundation::{
    CFIndex, CFRetained, CFRunLoopAddSource, CFRunLoopGetMain, CFRunLoopSource,
    CFRunLoopSourceContext, CFRunLoopSourceCreate, CFRunLoopSourceSignal,
    CFRunLoopWakeUp, kCFRunLoopCommonModes,
};
use std::{ffi::c_void, pin::Pin, ptr};

/// An event loop proxy with backpressure that implements `Sink`.
#[derive(Debug)]
pub struct Proxy<T: 'static> {
    raw: winit::event_loop::EventLoopProxy,
    proxy: MacProxy<Action<T>>,
    sender: mpsc::Sender<Action<T>>,
    notifier: mpsc::Sender<usize>,
}

impl<T: 'static> Clone for Proxy<T> {
    fn clone(&self) -> Self {
        Self {
            raw: self.raw.clone(),
            proxy: self.proxy.clone(),
            sender: self.sender.clone(),
            notifier: self.notifier.clone(),
        }
    }
}

impl<T: 'static> Proxy<T> {
    const MAX_SIZE: usize = 100;

    /// Creates a new [`Proxy`] from an `EventLoopProxy`.
    pub fn new(
        raw: winit::event_loop::EventLoopProxy,
    ) -> (
        Self,
        impl Future<Output = ()>,
        mpsc::UnboundedReceiver<Action<T>>,
    ) {
        let (notifier, mut processed) = mpsc::channel(Self::MAX_SIZE);
        let (sender, mut receiver) = mpsc::channel(Self::MAX_SIZE);
        let (proxy_sender, proxy_receiver) = mpsc::unbounded();
        let mac_proxy = MacProxy::new(proxy_sender.clone());
        let proxy = raw.clone();

        let mut mac_proxy_clone = mac_proxy.clone();

        let worker = async move {
            let mut count = 0;

            loop {
                if count < Self::MAX_SIZE {
                    select! {
                        message = receiver.select_next_some() => {
                            proxy.wake_up();
                            mac_proxy_clone.send_event(message);
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

        (
            Self {
                raw,
                proxy: mac_proxy,
                sender,
                notifier,
            },
            worker,
            proxy_receiver,
        )
    }

    /// Sends a value to the event loop.
    ///
    /// Note: This skips the backpressure mechanism with an unbounded
    /// channel. Use sparingly!
    pub async fn send(&mut self, value: T)
    where
        T: std::fmt::Debug,
    {
        self.send_action(Action::Output(value)).await;
    }

    /// Sends an action to the event loop.
    ///
    /// Note: This skips the backpressure mechanism with an unbounded
    /// channel. Use sparingly!
    pub async fn send_action(&mut self, action: Action<T>)
    where
        T: std::fmt::Debug,
    {
        self.raw.wake_up();
        self.proxy.send_event(action);
    }

    /// Frees an amount of slots for additional messages to be queued in
    /// this [`Proxy`].
    pub fn free_slots(&mut self, amount: usize) {
        let _ = self.notifier.start_send(amount);
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

#[derive(Debug)]
pub struct MacProxy<T> {
    sender: mpsc::UnboundedSender<T>,
    source: CFRetained<CFRunLoopSource>,
}

unsafe impl<T: Send> Send for MacProxy<T> {}
unsafe impl<T: Send> Sync for MacProxy<T> {}

impl<T> Clone for MacProxy<T> {
    fn clone(&self) -> Self {
        MacProxy::new(self.sender.clone())
    }
}

impl<T> MacProxy<T> {
    fn new(sender: mpsc::UnboundedSender<T>) -> Self {
        unsafe {
            // just wake up the eventloop
            extern "C-unwind" fn event_loop_proxy_handler(_: *mut c_void) {}

            // adding a Source to the main CFRunLoop lets us wake it up and
            // process user events through the normal OS EventLoop mechanisms.
            let rl = CFRunLoopGetMain().expect("Failed to get main run loop");
            let mut context = CFRunLoopSourceContext {
                version: 0,
                info: ptr::null_mut(),
                retain: None,
                release: None,
                copyDescription: None,
                equal: None,
                hash: None,
                schedule: None,
                cancel: None,
                perform: Some(event_loop_proxy_handler),
            };
            let source =
                CFRunLoopSourceCreate(None, CFIndex::MAX - 1, &mut context)
                    .expect("Failed to create CFRunLoopSource");
            CFRunLoopAddSource(&rl, Some(&source), kCFRunLoopCommonModes);
            CFRunLoopWakeUp(&rl);

            Self { sender, source }
        }
    }

    pub fn send_event(&mut self, event: T) {
        self.sender.start_send(event).expect("Event loop closed");
        unsafe {
            // let the main thread know there's a new event
            CFRunLoopSourceSignal(&self.source);
            let rl = CFRunLoopGetMain().expect("Failed to get main run loop");
            CFRunLoopWakeUp(&rl);
        }
    }
}
