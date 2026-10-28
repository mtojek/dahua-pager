use pager_dahua::{Message, MultipartReader, parse_message};
use std::io::{self, Cursor, Read};

fn body(payload: &str) -> Vec<u8> {
    format!("--myboundary\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}\r\n--myboundary--\r\n", payload.len(), payload).into_bytes()
}

#[test]
fn supplied_start_stop_and_heartbeat_are_preserved() {
    let fixture = include_bytes!("../../../fixtures/dahua-events/video-motion.multipart");
    let mut stream = MultipartReader::new(Cursor::new(fixture), "myboundary").unwrap();
    let Message::Event(start) = stream.next_message().unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(start.code, "VideoMotion");
    assert_eq!(start.action, "Start");
    assert_eq!(start.channel, 1);
    assert_eq!(start.data["Name"], "Backyard 2");
    assert_eq!(start.data["UTC"], 1793211709.0);
    assert!(start.data.get("BoundingBox").is_none());
    assert_eq!(stream.next_message().unwrap(), Some(Message::Heartbeat));
    let Message::Event(stop) = stream.next_message().unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(stop.action, "Stop");
    assert_eq!(stop.data["UTC"], 1793211714.0);
    assert_eq!(stop.data["RealUTC"], start.data["RealUTC"]);
    let Message::Event(other) = stream.next_message().unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(other.channel, 5);
    assert!(stream.next_message().unwrap().is_none());
}

struct Fragments(Cursor<Vec<u8>>);
impl Read for Fragments {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let len = out.len().min(1);
        self.0.read(&mut out[..len])
    }
}

#[test]
fn arbitrary_network_fragmentation_and_utf8_byte_lengths() {
    let payload = "Code=VideoMotion;action=Start;index=0;data={\"Name\":\"Backyard; wejście\"}";
    let mut stream =
        MultipartReader::new(Fragments(Cursor::new(body(payload))), "myboundary").unwrap();
    let Message::Event(event) = stream.next_message().unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(event.data["Name"], "Backyard; wejście");
    assert!(stream.next_message().unwrap().is_none());
}

#[test]
fn unknown_event_types_and_optional_data_are_not_discarded() {
    let Message::Event(event) = parse_message(b"Code=NewType;action=Pulse;index=5").unwrap() else {
        panic!()
    };
    assert_eq!(event.code, "NewType");
    assert!(event.data.is_null());
}

#[test]
fn malformed_events_fail_instead_of_inventing_camera_or_geometry() {
    for bad in [
        "Code=x;action=Start",
        "Code=x;action=Start;index=-1",
        "Code=x;action=Start;index=no",
        "Code=x;action=Start;index=0;data={",
        "Code=x;Code=y;action=Start;index=0",
    ] {
        assert!(parse_message(bad.as_bytes()).is_err(), "{bad}");
    }
}

#[test]
fn unbounded_or_ambiguous_parts_are_rejected() {
    for headers in [
        "Content-Length: 999999999",
        "Content-Length: 3\r\nContent-Length: 3",
        "Content-Length: nope",
        "Content-Type: text/plain",
    ] {
        let raw = format!("--myboundary\r\n{headers}\r\n\r\nHeartbeat\r\n");
        let mut stream = MultipartReader::new(Cursor::new(raw), "myboundary").unwrap();
        assert!(stream.next_message().is_err());
    }
}

#[test]
fn truncated_payload_and_unclosed_stream_are_errors() {
    for raw in [
        "--myboundary\r\nContent-Length: 9\r\n\r\nHear",
        "--myboundary\r\nContent-Length: 9\r\n\r\nHeartbeat\r\n",
    ] {
        let mut stream = MultipartReader::new(Cursor::new(raw), "myboundary").unwrap();
        if raw.ends_with("Heartbeat\r\n") {
            assert_eq!(stream.next_message().unwrap(), Some(Message::Heartbeat));
        }
        assert!(stream.next_message().is_err());
    }
}

#[test]
fn oversized_headers_are_bounded() {
    let raw = format!("--myboundary\r\nX: {}\r\n", "x".repeat(20_000));
    let mut stream = MultipartReader::new(Cursor::new(raw), "myboundary").unwrap();
    assert!(stream.next_message().is_err());
}
