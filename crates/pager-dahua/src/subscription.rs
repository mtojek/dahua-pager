use crate::{DahuaClient, Error, MAX_HEADERS, MAX_PART, Message, MultipartReader, Result};
use std::{
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, TrySendError},
    },
    thread,
    time::Duration,
};
use tokio::{sync::watch, time::Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    NetworkUnavailable,
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
#[derive(Clone, Copy)]
struct Signal {
    stop: bool,
    available: bool,
    generation: u64,
}
struct Control {
    signal: watch::Sender<Signal>,
    state: Mutex<(ConnectionState, u64)>,
    dropped: AtomicU64,
}
impl Control {
    fn set_state(&self, state: ConnectionState, generation: u64) {
        *self.state.lock().unwrap() = (state, generation);
    }
}

/// Cancellable network task with a bounded mailbox and fresh-traffic watchdog.
/// The caller supplies local network availability; endpoint health still requires
/// complete, valid NVR messages. OS reachability alone never marks it ONLINE.
pub struct Subscription {
    pub messages: Receiver<Message>,
    control: Arc<Control>,
}
impl Subscription {
    pub fn state(&self) -> ConnectionState {
        let signal = *self.control.signal.borrow();
        if signal.stop {
            return ConnectionState::Stopped;
        }
        if !signal.available {
            return ConnectionState::NetworkUnavailable;
        }
        let (state, generation) = &*self.control.state.lock().unwrap();
        // A rapid down/up cannot resurrect the previous TCP connection's ONLINE.
        if *generation != signal.generation {
            ConnectionState::Connecting
        } else {
            state.clone()
        }
    }
    pub fn dropped_messages(&self) -> u64 {
        self.control.dropped.load(Ordering::Relaxed)
    }
    pub fn set_network_available(&self, available: bool) {
        self.control.signal.send_if_modified(|signal| {
            if signal.available == available {
                return false;
            }
            signal.available = available;
            signal.generation = signal.generation.wrapping_add(1);
            true
        });
    }
    pub fn stop(&self) {
        self.control.signal.send_modify(|signal| signal.stop = true);
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
        let liveness = Duration::from_secs(self.config.session_timeout_secs.min(15));
        self.subscribe_with_liveness(policy, liveness)
    }
    /// Injectable deadline for deterministic communication tests. Normal callers
    /// use subscribe(): 15 seconds (three requested 5-second heartbeat intervals).
    pub fn subscribe_with_liveness(
        self,
        policy: RetryPolicy,
        liveness: Duration,
    ) -> Result<Subscription> {
        if policy.initial.is_zero() || policy.maximum < policy.initial || liveness.is_zero() {
            return Err(Error::Config);
        }
        let (sender, receiver) = mpsc::sync_channel(128);
        let (signal_tx, mut signals) = watch::channel(Signal {
            stop: false,
            available: true,
            generation: 0,
        });
        let control = Arc::new(Control {
            signal: signal_tx,
            state: Mutex::new((ConnectionState::Connecting, 0)),
            dropped: AtomicU64::new(0),
        });
        let worker = control.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| Error::Transport)?;
        thread::Builder::new().name("dahua-events".into()).spawn(move || {
            // Drop the blocking client's runtime after leaving the async context.
            runtime.block_on(async {
                let mut backoff = Backoff::new(policy);
                'connection: loop {
                    let signal = *signals.borrow_and_update();
                    if signal.stop { break; }
                    if !signal.available {
                        worker.set_state(ConnectionState::NetworkUnavailable, signal.generation);
                        let _ = signals.changed().await;
                        continue;
                    }
                    worker.set_state(ConnectionState::Connecting, signal.generation);
                    let opened = tokio::select! {
                        biased;
                        _ = signals.changed() => continue,
                        result = self.open_event_stream() => result,
                    };
                    let error = match opened {
                        Err(error) => Some(error),
                        Ok((mut response, boundary)) => {
                            let mut buffer = Vec::new();
                            let mut deadline = Instant::now() + liveness;
                            'stream: loop {
                                let chunk = tokio::select! {
                                    biased;
                                    _ = signals.changed() => continue 'connection,
                                    _ = tokio::time::sleep_until(deadline) => break Some(Error::HeartbeatTimeout),
                                    result = response.chunk() => result,
                                };
                                match chunk {
                                    Err(_) => break Some(Error::Transport),
                                    Ok(None) => break Some(Error::Truncated),
                                    Ok(Some(bytes)) => buffer.extend_from_slice(&bytes),
                                }
                                loop {
                                    match buffered_message(&buffer, &boundary) {
                                        Err(error) => break 'stream Some(error),
                                        Ok(None) => break,
                                        Ok(Some((None, _))) => break 'stream None,
                                        Ok(Some((Some(message), consumed))) => {
                                            buffer.drain(..consumed);
                                            if signals.has_changed().unwrap_or(true) { continue 'connection; }
                                            deadline = Instant::now() + liveness;
                                            backoff.reset();
                                            worker.set_state(ConnectionState::Connected, signal.generation);
                                            match sender.try_send(message) {
                                                Ok(()) => {},
                                                Err(TrySendError::Full(_)) => { worker.dropped.fetch_add(1, Ordering::Relaxed); },
                                                Err(TrySendError::Disconnected(_)) => break 'connection,
                                            }
                                        }
                                    }
                                }
                                if buffer.len() > MAX_PART + MAX_HEADERS { break Some(Error::TooLarge); }
                            }
                        }
                    };
                    if signals.has_changed().unwrap_or(true) { continue; }
                    let delay = backoff.delay();
                    worker.set_state(ConnectionState::Reconnecting { delay, error }, signal.generation);
                    tokio::select! {
                        biased;
                        _ = signals.changed() => {},
                        _ = tokio::time::sleep(delay) => {},
                    }
                }
                worker.set_state(ConnectionState::Stopped, signals.borrow().generation);
            });
        }).map_err(|_| Error::Transport)?;
        Ok(Subscription {
            messages: receiver,
            control,
        })
    }
}

/// Reuse the tested MIME parser on accumulated bytes; incomplete parts need more
/// data. Account for BufReader prefetch so following parts stay in the buffer.
fn buffered_message(bytes: &[u8], boundary: &str) -> Result<Option<(Option<Message>, usize)>> {
    let mut parser = MultipartReader::new(Cursor::new(bytes), boundary)?;
    match parser.next_message() {
        Err(Error::Truncated) => Ok(None),
        Err(error) => Err(error),
        Ok(message) => {
            let consumed =
                parser.reader.get_ref().position() as usize - parser.reader.buffer().len();
            Ok(Some((message, consumed)))
        }
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

    #[test]
    fn fragmented_multipart_preserves_prefetched_messages_and_closing_boundary() {
        let fixture = include_bytes!("../../../fixtures/dahua-events/video-motion.multipart");
        for fragment_size in [1, 7, fixture.len()] {
            let mut buffer = Vec::new();
            let mut messages = Vec::new();
            let mut ended = false;
            for fragment in fixture.chunks(fragment_size) {
                buffer.extend_from_slice(fragment);
                while let Some((message, consumed)) =
                    buffered_message(&buffer, "myboundary").unwrap()
                {
                    assert!(consumed > 0);
                    buffer.drain(..consumed);
                    match message {
                        Some(message) => messages.push(message),
                        None => {
                            ended = true;
                            break;
                        }
                    }
                }
            }
            assert!(ended);
            assert!(buffer.is_empty());
            assert_eq!(messages.len(), 4);
            assert_eq!(messages[1], Message::Heartbeat);
        }
    }
}
