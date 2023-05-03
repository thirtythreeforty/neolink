//! For debugging polls
//!
//! https://users.rust-lang.org/t/how-do-i-debug-a-tokio-hang-tokio-0-3-3/51235/3
//!
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub(crate) struct TimedPoll<F> {
    name: String,
    future: F,
}

impl<F: Future> TimedPoll<F> {
    pub fn new<T: Into<String>>(name: T, future: F) -> Self {
        Self {
            name: name.into(),
            future,
        }
    }
}

impl<F: Future> Future for TimedPoll<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let name = self.name.clone();
        log::debug!("{}: Start poll", name);
        let before = std::time::Instant::now();
        let future = unsafe { Pin::map_unchecked_mut(self, |me| &mut me.future) };
        let res = future.poll(cx);
        let elapsed = before.elapsed();
        // if elapsed > std::time::Duration::from_micros(500) {
        log::debug!("{}: Stop poll: {:?}", name, elapsed);
        // }
        res
    }
}
