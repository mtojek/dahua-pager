//! Explicit live communication E2E. Never run against real hardware by default.
//! Credentials, URLs, camera labels and raw NVR responses must stay out of output.
use pager_dahua::{Config, DahuaClient, Message};
use std::path::PathBuf;

fn local_config() -> Config {
    let path = std::env::var_os("DAHUA_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config.local.json")
        });
    Config::load(path).expect("Unable to load private local NVR configuration")
}

#[test]
#[ignore = "requires explicit authorization and a private local NVR configuration"]
fn live_https_digest_event_stream() {
    let mut config = local_config();
    config.nvr.session_timeout_secs = 25;
    let client = DahuaClient::new(config.nvr).expect("Unable to initialize NVR client");
    let mut stream = client
        .events()
        .expect("Unable to authenticate/open event stream");
    let (mut heartbeats, mut events, mut human_objects) = (0, 0, 0);
    // Heartbeats prove incremental streaming even when no detection occurs.
    // Human detections are inspected if present, never required for a quiet camera.
    while heartbeats < 3 {
        match stream
            .next_message()
            .expect("NVR stream read/framing failed")
        {
            Some(Message::Heartbeat) => heartbeats += 1,
            Some(Message::Event(event)) => {
                events += 1;
                if event.code == "SmartMotionHuman" {
                    assert!(
                        matches!(event.action.as_str(), "Start" | "Stop" | "Pulse"),
                        "Unexpected human event phase"
                    );
                    if let Some(objects) = event.data.get("object").and_then(|v| v.as_array()) {
                        for object in objects {
                            if let Some(rect) = object.get("Rect") {
                                let rect = rect.as_array().expect("Human Rect must be an array");
                                assert_eq!(
                                    rect.len(),
                                    4,
                                    "Human Rect must contain four coordinates"
                                );
                                let values: Vec<u64> = rect
                                    .iter()
                                    .map(|v| {
                                        v.as_u64()
                                            .expect("Human coordinate must be nonnegative integer")
                                    })
                                    .collect();
                                assert!(
                                    values.iter().all(|v| *v <= 8192),
                                    "Human coordinate exceeds provisional normalized range"
                                );
                                assert!(
                                    values[0] < values[2] && values[1] < values[3],
                                    "Human rectangle must have positive area"
                                );
                                human_objects += 1;
                            }
                        }
                    }
                }
            }
            None => panic!("NVR stream ended before three heartbeats"),
        }
    }
    println!(
        "Live stream OK: heartbeats={heartbeats}, events={events}, validated_human_rectangles={human_objects}"
    );
    if human_objects == 0 {
        println!(
            "Human geometry not verified live: no rectangles received during this observation."
        );
    }
}

#[test]
#[ignore = "requires explicit authorization and a private local NVR configuration"]
fn live_camera_snapshots() {
    let mut config = local_config();
    config.nvr.session_timeout_secs = 10;
    let client = DahuaClient::new(config.nvr).expect("Unable to initialize NVR client");
    // The project currently targets the first six channels; discovery can list spare slots.
    let cameras = client.cameras().expect("Unable to discover cameras");
    let channels: Vec<u32> = cameras.iter().take(6).map(|c| c.index).collect();
    assert_eq!(channels.len(), 6, "Expected at least six channel slots");
    for (i, channel) in channels.iter().enumerate() {
        let bytes = client
            .snapshot(*channel)
            .expect("NVR snapshot retrieval/validation failed");
        println!(
            "Discovered camera {}: JPEG received, {} bytes (not saved)",
            i + 1,
            bytes.len()
        );
    }
    println!("Live snapshots OK: {} discovered cameras", channels.len());
}

#[test]
#[ignore = "requires explicit authorization and a private local NVR configuration"]
fn live_camera_names() {
    let mut config = local_config();
    config.nvr.session_timeout_secs = 10;
    let cameras = DahuaClient::new(config.nvr)
        .expect("Unable to initialize NVR client")
        .cameras()
        .expect("Unable to retrieve channel titles from NVR");
    assert!(!cameras.is_empty(), "No channel slots discovered");
    assert!(
        cameras.iter().all(|camera| camera.index > 0),
        "Invalid channel index"
    );
    println!(
        "Camera discovery OK: {} API channel slots; names withheld",
        cameras.len()
    );
}
