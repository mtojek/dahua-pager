//! Explicitly invoked live probe; never prints credentials or raw event metadata.
use pager_dahua::{Config, DahuaClient, Message};
use std::{sync::mpsc::RecvTimeoutError, time::Duration};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .map(String::as_str)
        .unwrap_or("config.local.json");
    let config = Config::load(path)?;
    let client = DahuaClient::new(config.nvr)?;
    if args.get(1).is_some_and(|s| s == "snapshot") {
        let cameras = client.cameras()?;
        let channel = cameras.first().ok_or("No discovered cameras")?.index;
        let bytes = client.snapshot(channel)?;
        println!(
            "Snapshot received: {} bytes, JPEG signature verified (not saved)",
            bytes.len()
        );
    } else if args.get(1).is_some_and(|s| s == "monitor") {
        let subscription = client.subscribe()?;
        let mut previous = None;
        let (mut heartbeats, mut events) = (0u64, 0u64);
        loop {
            let state = subscription.state();
            if previous.as_ref() != Some(&state) {
                println!("Connection: {state:?}");
                previous = Some(state);
            }
            match subscription.messages.recv_timeout(Duration::from_secs(1)) {
                Ok(Message::Heartbeat) => {
                    heartbeats += 1;
                    println!(
                        "Traffic: heartbeats={heartbeats}, events={events}, dropped={}",
                        subscription.dropped_messages()
                    );
                }
                Ok(Message::Event(_)) => events += 1,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    } else {
        let mut stream = client.events()?;
        println!("Connected; waiting for up to 10 messages (reads have an idle timeout)");
        for _ in 0..10 {
            match stream.next_message()? {
                Some(Message::Heartbeat) => println!("Heartbeat"),
                Some(Message::Event(event)) => println!(
                    "Event: code={} action={} channel={}",
                    event.code, event.action, event.channel
                ),
                None => {
                    println!("Stream closed");
                    break;
                }
            }
        }
    }
    Ok(())
}
