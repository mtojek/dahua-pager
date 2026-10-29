//! Cached local route availability. No ping, internet probe or private logging.
use pager_dahua::Subscription;

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::{
        ffi::CString,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        thread,
        time::Duration,
    };
    use system_configuration::network_reachability::{ReachabilityFlags, SCNetworkReachability};

    pub struct NetworkPath {
        stop: Arc<AtomicBool>,
        sample: Arc<AtomicU64>,
        seen: u64,
    }
    fn available(flags: ReachabilityFlags) -> bool {
        flags.contains(ReachabilityFlags::REACHABLE)
            && !flags.contains(ReachabilityFlags::CONNECTION_REQUIRED)
    }
    impl NetworkPath {
        pub fn new(host: &str) -> Option<Self> {
            let host = CString::new(host).ok()?;
            let stop = Arc::new(AtomicBool::new(false));
            let sample = Arc::new(AtomicU64::new(0));
            let worker_stop = stop.clone();
            let worker_sample = sample.clone();
            thread::Builder::new()
                .name("nvr-network-path".into())
                .spawn(move || {
                    let Some(reachability) = SCNetworkReachability::from_host(&host) else {
                        return;
                    };
                    let mut previous = None;
                    let mut generation = 0;
                    while !worker_stop.load(Ordering::Relaxed) {
                        if let Ok(flags) = reachability.reachability() {
                            let current = available(flags);
                            let path = (current, flags.bits());
                            if previous != Some(path) {
                                generation += 4;
                                worker_sample.store(
                                    generation | if current { 2 } else { 1 },
                                    Ordering::Release,
                                );
                                previous = Some(path);
                            }
                        }
                        thread::sleep(Duration::from_millis(250));
                    }
                })
                .ok()?;
            Some(Self {
                stop,
                sample,
                seen: 0,
            })
        }
        pub fn poll(&mut self, subscription: &Subscription) {
            let value = self.sample.load(Ordering::Acquire);
            if value == 0 || value == self.seen {
                return;
            }
            let is_available = value & 3 == 2;
            // Preserve a down/up observed by macOS between two UI frames.
            if is_available && self.seen != 0 && self.seen & 3 == 2 {
                subscription.set_network_available(false);
            }
            subscription.set_network_available(is_available);
            self.seen = value;
        }
    }
    impl Drop for NetworkPath {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn local_route_requires_reachable_and_established_network() {
            assert!(!available(ReachabilityFlags::empty()));
            assert!(available(
                ReachabilityFlags::REACHABLE | ReachabilityFlags::IS_DIRECT
            ));
            assert!(!available(
                ReachabilityFlags::REACHABLE | ReachabilityFlags::CONNECTION_REQUIRED
            ));
        }
    }
}
#[cfg(target_os = "macos")]
pub use native::NetworkPath;

#[cfg(not(target_os = "macos"))]
pub struct NetworkPath;
#[cfg(not(target_os = "macos"))]
impl NetworkPath {
    pub fn new(_: &str) -> Option<Self> {
        None
    }
    pub fn poll(&mut self, _: &Subscription) {}
}
