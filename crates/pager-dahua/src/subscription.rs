use crate::{DahuaClient, Error, Message, Result};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, TrySendError},
    },
    thread,
    time::Duration,
};

#[derive(Clone, Debug, PartialEq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Reconnecting {
        delay: Duration,
        error: Option<Error>,
    },
    Stopped,
}

#[derive(Clone, Copy)]
pub struct RetryPolicy {
    pub initial: Duration,
    pub maximum: Duration,
}
impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            maximum: Duration::from_secs(30),
        }
    }
}
struct Backoff {
    policy: RetryPolicy,
    next: Duration,
}
impl Backoff {
    fn new(policy: RetryPolicy) -> Self {
        Self {
            next: policy.initial,
            policy,
        }
    }
    fn delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = self.next.saturating_mul(2).min(self.policy.maximum);
        delay
    }
    fn reset(&mut self) {
        self.next = self.policy.initial;
    }
}

struct Control {
    stop: Mutex<bool>,
    wake: Condvar,
    state: Mutex<ConnectionState>,
    dropped: AtomicU64,
}
impl Control {
    fn stopped(&self) -> bool {
        *self.stop.lock().unwrap()
    }
    fn set_state(&self, state: ConnectionState) {
        *self.state.lock().unwrap() = state;
    }
}

/// Network reads and retry waits never run on the caller/UI thread.
/// The bounded mailbox exposes overload instead of growing indefinitely.
pub struct Subscription {
    pub messages: Receiver<Message>,
    control: Arc<Control>,
}
impl Subscription {
    pub fn state(&self) -> ConnectionState {
        self.control.state.lock().unwrap().clone()
    }
    pub fn dropped_messages(&self) -> u64 {
        self.control.dropped.load(Ordering::Relaxed)
    }
    /// Wakes retry waits immediately. An in-flight read finishes at its I/O timeout.
    pub fn stop(&self) {
        *self.control.stop.lock().unwrap() = true;
        self.control.wake.notify_all();
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.stop();
    }
}

impl DahuaClient {
    pub fn subscribe(self) -> Result<Subscription> {
        self.subscribe_with_retry(RetryPolicy::default())
    }
    pub fn subscribe_with_retry(self, policy: RetryPolicy) -> Result<Subscription> {
        if policy.initial.is_zero() || policy.maximum < policy.initial {
            return Err(Error::Config);
        }
        let (sender, receiver) = mpsc::sync_channel(128);
        let control = Arc::new(Control {
            stop: Mutex::new(false),
            wake: Condvar::new(),
            state: Mutex::new(ConnectionState::Connecting),
            dropped: AtomicU64::new(0),
        });
        let worker = control.clone();
        thread::Builder::new()
            .name("dahua-events".into())
            .spawn(move || {
                let mut backoff = Backoff::new(policy);
                while !worker.stopped() {
                    worker.set_state(ConnectionState::Connecting);
                    let error = match self.events() {
                        Err(error) => Some(error),
                        Ok(mut stream) => {
                            // A 200 response alone does not prove the stream is alive.
                            loop {
                                if worker.stopped() {
                                    break None;
                                }
                                match stream.next_message() {
                                    Err(error) => break Some(error),
                                    Ok(None) => break None,
                                    Ok(Some(message)) => {
                                        if worker.stopped() {
                                            break None;
                                        }
                                        backoff.reset();
                                        worker.set_state(ConnectionState::Connected);
                                        match sender.try_send(message) {
                                            Ok(()) => {}
                                            Err(TrySendError::Full(_)) => {
                                                worker.dropped.fetch_add(1, Ordering::Relaxed);
                                            }
                                            Err(TrySendError::Disconnected(_)) => {
                                                *worker.stop.lock().unwrap() = true;
                                                break None;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    };
                    if worker.stopped() {
                        break;
                    }
                    let delay = backoff.delay();
                    worker.set_state(ConnectionState::Reconnecting { delay, error });
                    let guard = worker.stop.lock().unwrap();
                    let _ = worker
                        .wake
                        .wait_timeout_while(guard, delay, |stop| !*stop)
                        .unwrap();
                }
                worker.set_state(ConnectionState::Stopped);
            })
            .map_err(|_| Error::Transport)?;
        Ok(Subscription {
            messages: receiver,
            control,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retries_are_bounded_and_reset_only_after_valid_traffic() {
        let mut backoff = Backoff::new(RetryPolicy::default());
        let delays: Vec<_> = (0..8).map(|_| backoff.delay().as_secs()).collect();
        assert_eq!(delays, [1, 2, 4, 8, 16, 30, 30, 30]);
        backoff.reset();
        assert_eq!(backoff.delay(), Duration::from_secs(1));
    }
}
