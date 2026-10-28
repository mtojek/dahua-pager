use pager_dahua::{
    Auth, Config, ConnectionState, DahuaClient, Error, Message, NvrConfig, RetryPolicy,
};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

fn config(base_url: String) -> NvrConfig {
    NvrConfig {
        base_url,
        username: "test-user".into(),
        password: "test-password".into(),
        auth: Auth::Digest,
        connect_timeout_secs: 2,
        session_timeout_secs: 2,
        tls_insecure: false,
    }
}
fn accept(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                return stream;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(2))
            }
            Err(e) => panic!("mock server accept failed: {e}"),
        }
    }
}
fn request(stream: &mut impl Read) -> String {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() < 16_384);
    }
    String::from_utf8(bytes).unwrap()
}
fn response(stream: &mut impl Write, status: &str, headers: &str, body: &[u8]) {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nConnection: close\r\n{headers}Content-Length: {}\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(body).unwrap();
    stream.flush().unwrap();
}
fn server(f: impl FnOnce(TcpListener) + Send + 'static) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    (url, thread::spawn(move || f(listener)))
}
fn auth_value(request: &str) -> &str {
    request
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("authorization")
                .then(|| value.trim())
        })
        .expect("authorization header")
}

#[test]
fn digest_challenge_signs_exact_uri_and_chunked_multipart_is_decoded() {
    let (url, handle) = server(|listener| {
        let mut first = accept(&listener);
        let initial = request(&mut first);
        assert!(initial.starts_with(
            "GET /cgi-bin/eventManager.cgi?action=attach&codes=%5BAll%5D&heartbeat=5 HTTP/1.1"
        ));
        assert!(!initial.to_lowercase().contains("authorization:"));
        response(
            &mut first,
            "401 Unauthorized",
            "WWW-Authenticate: Digest realm=\"Dahua\", nonce=\"test-nonce\", algorithm=MD5, qop=\"auth\"\r\n",
            b"",
        );
        drop(first);
        let mut second = accept(&listener);
        let req = request(&mut second);
        let mut auth = digest_auth::AuthorizationHeader::parse(auth_value(&req)).unwrap();
        assert_eq!(auth.username, "test-user");
        assert_eq!(auth.nonce, "test-nonce");
        assert_eq!(
            auth.uri,
            "/cgi-bin/eventManager.cgi?action=attach&codes=%5BAll%5D&heartbeat=5"
        );
        let signed = auth.response.clone();
        let uri = auth.uri.clone();
        auth.digest(&digest_auth::AuthContext::new(
            "test-user",
            "test-password",
            &uri,
        ));
        assert_eq!(auth.response, signed);
        assert!(!req.contains("test-password"));
        write!(second, "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: multipart/x-mixed-replace; boundary=\"myboundary\"\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap();
        for chunk in
            include_bytes!("../../../fixtures/dahua-events/video-motion.multipart").chunks(7)
        {
            write!(second, "{:x}\r\n", chunk.len()).unwrap();
            second.write_all(chunk).unwrap();
            second.write_all(b"\r\n").unwrap();
        }
        second.write_all(b"0\r\n\r\n").unwrap();
    });
    let client = DahuaClient::new(config(url)).unwrap();
    let mut stream = client.events().unwrap();
    assert!(matches!(
        stream.next_message().unwrap(),
        Some(Message::Event(_))
    ));
    assert_eq!(stream.next_message().unwrap(), Some(Message::Heartbeat));
    assert!(matches!(
        stream.next_message().unwrap(),
        Some(Message::Event(_))
    ));
    assert!(matches!(
        stream.next_message().unwrap(),
        Some(Message::Event(_))
    ));
    assert!(stream.next_message().unwrap().is_none());
    handle.join().unwrap();
}

#[test]
fn authentication_failure_is_bounded_and_redacted() {
    let (url, handle) = server(|listener| {
        for _ in 0..2 {
            let mut stream = accept(&listener);
            request(&mut stream);
            response(
                &mut stream,
                "401 Unauthorized",
                "WWW-Authenticate: Digest realm=\"Dahua\", nonce=\"n\", qop=\"auth\"\r\n",
                b"private test-password",
            );
        }
    });
    let client = DahuaClient::new(config(url)).unwrap();
    let error = client.events().err().unwrap();
    assert_eq!(error, Error::Authentication);
    assert!(!format!("{error:?} {error}").contains("test-password"));
    handle.join().unwrap();
}

#[test]
fn discovered_camera_drives_snapshot_without_camera_configuration() {
    let (url, handle) = server(|listener| {
        let mut titles = accept(&listener);
        assert!(request(&mut titles).starts_with(
            "GET /cgi-bin/configManager.cgi?action=getConfig&name=ChannelTitle HTTP/1.1"
        ));
        response(
            &mut titles,
            "200 OK",
            "Content-Type: text/plain\r\n",
            b"table.ChannelTitle[1].Name=Backyard 2\r\n",
        );
        drop(titles);
        let mut stream = accept(&listener);
        let req = request(&mut stream);
        assert!(req.starts_with("GET /cgi-bin/snapshot.cgi?channel=2 HTTP/1.1"));
        assert_eq!(auth_value(&req), "Basic dGVzdC11c2VyOnRlc3QtcGFzc3dvcmQ=");
        response(
            &mut stream,
            "200 OK",
            "Content-Type: image/jpeg\r\n",
            &[0xff, 0xd8, 0xff, 0xd9],
        );
    });
    let mut cfg = config(url);
    cfg.auth = Auth::Basic;
    let client = DahuaClient::new(cfg).unwrap();
    let camera = client.cameras().unwrap().pop().unwrap();
    assert_eq!(camera.event_index().unwrap(), 1);
    assert_eq!(
        client.snapshot(camera.index).unwrap(),
        [0xff, 0xd8, 0xff, 0xd9]
    );
    handle.join().unwrap();
}

#[test]
fn login_html_and_invalid_jpeg_are_not_snapshots() {
    for (kind, body) in [
        ("text/html", b"<html>login</html>".as_slice()),
        ("image/jpeg", b"not an image".as_slice()),
    ] {
        let kind = kind.to_owned();
        let body = body.to_vec();
        let (url, handle) = server(move |listener| {
            let mut stream = accept(&listener);
            request(&mut stream);
            response(
                &mut stream,
                "200 OK",
                &format!("Content-Type: {kind}\r\n"),
                &body,
            );
        });
        assert_eq!(
            DahuaClient::new(config(url)).unwrap().snapshot(1),
            Err(Error::Snapshot)
        );
        handle.join().unwrap();
    }
}

#[test]
fn redirects_and_http_errors_are_not_followed() {
    for (status, expected) in [
        ("302 Found", Error::Http(302)),
        ("503 Unavailable", Error::Http(503)),
        ("403 Forbidden", Error::Authentication),
    ] {
        let (url, handle) = server(move |listener| {
            let mut stream = accept(&listener);
            request(&mut stream);
            response(
                &mut stream,
                status,
                "Location: http://127.0.0.1:1/private\r\n",
                b"",
            );
        });
        assert_eq!(
            DahuaClient::new(config(url))
                .unwrap()
                .events()
                .err()
                .unwrap(),
            expected
        );
        handle.join().unwrap();
    }
}

#[test]
fn incorrect_stream_type_and_missing_boundary_are_rejected() {
    for kind in ["text/html", "multipart/x-mixed-replace"] {
        let (url, handle) = server(move |listener| {
            let mut stream = accept(&listener);
            request(&mut stream);
            response(
                &mut stream,
                "200 OK",
                &format!("Content-Type: {kind}\r\n"),
                b"",
            );
        });
        assert_eq!(
            DahuaClient::new(config(url))
                .unwrap()
                .events()
                .err()
                .unwrap(),
            Error::Protocol
        );
        handle.join().unwrap();
    }
}

#[test]
fn silent_peer_hits_session_timeout() {
    let (url, handle) = server(|listener| {
        let mut stream = accept(&listener);
        request(&mut stream);
        thread::sleep(Duration::from_millis(1400));
    });
    let mut cfg = config(url);
    cfg.session_timeout_secs = 1;
    let start = Instant::now();
    assert_eq!(
        DahuaClient::new(cfg).unwrap().events().err().unwrap(),
        Error::Transport
    );
    assert!(start.elapsed() < Duration::from_secs(3));
    handle.join().unwrap();
}

#[test]
fn self_signed_wrong_hostname_requires_explicit_insecure_option() {
    for insecure in [false, true] {
        let cert = rustls::pki_types::CertificateDer::from_pem_slice(include_bytes!(
            "../../../fixtures/tls/test-cert.pem"
        ))
        .unwrap();
        let key = rustls::pki_types::PrivateKeyDer::from_pem_slice(include_bytes!(
            "../../../fixtures/tls/test-key.pem"
        ))
        .unwrap();
        let tls = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .unwrap();
        let (url, handle) = server(move |listener| {
            let tcp = accept(&listener);
            let connection = rustls::ServerConnection::new(Arc::new(tls)).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, tcp);
            // A strict client terminates during the handshake, before sending HTTP.
            if !insecure {
                let mut byte = [0];
                assert!(stream.read(&mut byte).is_err());
                return;
            }
            request(&mut stream);
            response(
                &mut stream,
                "200 OK",
                "Content-Type: image/jpeg\r\n",
                &[0xff, 0xd8, 0xff, 0xd9],
            );
        });
        let mut cfg = config(url.replacen("http:", "https:", 1));
        cfg.tls_insecure = insecure;
        cfg.auth = Auth::Basic;
        let result = DahuaClient::new(cfg).unwrap().snapshot(1);
        if insecure {
            assert!(result.is_ok());
        } else {
            assert_eq!(result, Err(Error::Transport));
        }
        handle.join().unwrap();
    }
}

#[test]
fn connection_config_does_not_require_cameras_and_rejects_invalid_base_urls() {
    let template = include_str!("../../../config.example.json");
    let mut value: serde_json::Value = serde_json::from_str(template).unwrap();
    value["nvr"]["username"] = "user".into();
    value["nvr"]["password"] = "secret".into();
    let valid: Config = serde_json::from_value(value.clone()).unwrap();
    assert!(valid.validate().is_ok());
    for url in [
        "https://user:secret@localhost",
        "ftp://localhost",
        "https://localhost/path",
    ] {
        value["nvr"]["base_url"] = url.into();
        assert!(
            serde_json::from_value::<Config>(value.clone())
                .unwrap()
                .validate()
                .is_err()
        );
    }
}

use rustls::pki_types::pem::PemObject;

#[test]
fn channel_title_api_returns_one_based_camera_indices_and_names() {
    let (url, handle) = server(|listener| {
        let mut stream = accept(&listener);
        let req = request(&mut stream);
        assert!(req.starts_with(
            "GET /cgi-bin/configManager.cgi?action=getConfig&name=ChannelTitle HTTP/1.1"
        ));
        response(
            &mut stream,
            "200 OK",
            "Content-Type: text/plain\r\n",
            "table.ChannelTitle[1].Name=Backyard 2\r\ntable.ChannelTitle[0].Name=Garage żółty=1\r\n"
                .as_bytes(),
        );
    });
    let cameras = DahuaClient::new(config(url)).unwrap().cameras().unwrap();
    assert_eq!(cameras.len(), 2);
    assert_eq!(cameras[0].index, 1);
    assert_eq!(cameras[0].name, "Garage żółty=1");
    assert_eq!(cameras[1].index, 2);
    assert_eq!(cameras[1].name, "Backyard 2");
    handle.join().unwrap();
}

#[test]
fn invalid_or_duplicate_channel_titles_are_rejected() {
    for body in [
        "",
        "Error",
        "table.ChannelTitle[0].Name=A\ntable.ChannelTitle[0].Name=B",
        "table.ChannelTitle[4294967295].Name=A",
    ] {
        let (url, handle) = server(move |listener| {
            let mut stream = accept(&listener);
            request(&mut stream);
            response(
                &mut stream,
                "200 OK",
                "Content-Type: text/plain\r\n",
                body.as_bytes(),
            );
        });
        assert!(matches!(
            DahuaClient::new(config(url)).unwrap().cameras(),
            Err(Error::Protocol)
        ));
        handle.join().unwrap();
    }
}

#[test]
fn disconnected_stream_is_reported_and_can_be_opened_again() {
    let (url, handle) = server(|listener| {
        let mut first = accept(&listener);
        request(&mut first);
        response(
            &mut first,
            "200 OK",
            "Content-Type: multipart/x-mixed-replace; boundary=b\r\n",
            b"--b\r\nContent-Length: 9\r\n\r\nHear",
        );
        drop(first);
        let mut second = accept(&listener);
        request(&mut second);
        response(
            &mut second,
            "200 OK",
            "Content-Type: multipart/x-mixed-replace; boundary=b\r\n",
            b"--b\r\nContent-Length: 9\r\n\r\nHeartbeat\r\n--b--\r\n",
        );
    });
    let client = DahuaClient::new(config(url)).unwrap();
    assert_eq!(
        client.events().unwrap().next_message(),
        Err(Error::Truncated)
    );
    let mut second = client.events().unwrap();
    assert_eq!(second.next_message().unwrap(), Some(Message::Heartbeat));
    assert!(second.next_message().unwrap().is_none());
    handle.join().unwrap();
}

#[test]
fn stalled_body_hits_timeout_after_successful_headers() {
    let (url, handle) = server(|listener| {
        let mut stream = accept(&listener);
        request(&mut stream);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary=b\r\nContent-Length: 100\r\n\r\n--b\r\nContent-Length: 9\r\n\r\nHear").unwrap();
        stream.flush().unwrap();
        thread::sleep(Duration::from_millis(1400));
    });
    let mut cfg = config(url);
    cfg.session_timeout_secs = 1;
    let mut stream = DahuaClient::new(cfg).unwrap().events().unwrap();
    assert_eq!(stream.next_message(), Err(Error::Transport));
    handle.join().unwrap();
}

fn fast_retry() -> RetryPolicy {
    RetryPolicy {
        initial: Duration::from_millis(10),
        maximum: Duration::from_millis(40),
    }
}

#[test]
fn subscription_reconnects_after_http_failure_and_dropped_stream() {
    let (url, handle) = server(|listener| {
        let mut failed = accept(&listener);
        request(&mut failed);
        response(&mut failed, "503 Unavailable", "", b"");
        drop(failed);
        for _ in 0..2 {
            let mut stream = accept(&listener);
            request(&mut stream);
            response(
                &mut stream,
                "200 OK",
                "Content-Type: multipart/x-mixed-replace; boundary=b\r\n",
                b"--b\r\nContent-Length: 9\r\n\r\nHeartbeat\r\n",
            );
        }
    });
    let subscription = DahuaClient::new(config(url))
        .unwrap()
        .subscribe_with_retry(fast_retry())
        .unwrap();
    for _ in 0..2 {
        assert_eq!(
            subscription
                .messages
                .recv_timeout(Duration::from_secs(3))
                .unwrap(),
            Message::Heartbeat
        );
    }
    subscription.stop();
    handle.join().unwrap();
}

#[test]
fn subscription_recovers_from_blackholed_stream_after_idle_timeout() {
    let (url, handle) = server(|listener| {
        let mut blackhole = accept(&listener);
        request(&mut blackhole);
        blackhole
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary=b\r\n\r\n",
            )
            .unwrap();
        blackhole.flush().unwrap();
        thread::sleep(Duration::from_millis(1400));
        drop(blackhole);
        let mut restored = accept(&listener);
        request(&mut restored);
        response(
            &mut restored,
            "200 OK",
            "Content-Type: multipart/x-mixed-replace; boundary=b\r\n",
            b"--b\r\nContent-Length: 9\r\n\r\nHeartbeat\r\n--b--\r\n",
        );
    });
    let mut cfg = config(url);
    cfg.session_timeout_secs = 1;
    let subscription = DahuaClient::new(cfg)
        .unwrap()
        .subscribe_with_retry(fast_retry())
        .unwrap();
    assert_eq!(
        subscription
            .messages
            .recv_timeout(Duration::from_secs(4))
            .unwrap(),
        Message::Heartbeat
    );
    subscription.stop();
    handle.join().unwrap();
}

#[test]
fn healthy_stream_stays_connected_past_single_read_timeout() {
    let (url, handle) = server(|listener| {
        let mut stream = accept(&listener);
        request(&mut stream);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary=b\r\nConnection: close\r\n\r\n").unwrap();
        for _ in 0..4 {
            stream
                .write_all(b"--b\r\nContent-Length: 9\r\n\r\nHeartbeat\r\n")
                .unwrap();
            stream.flush().unwrap();
            thread::sleep(Duration::from_millis(400));
        }
    });
    let mut cfg = config(url);
    cfg.session_timeout_secs = 1;
    let subscription = DahuaClient::new(cfg)
        .unwrap()
        .subscribe_with_retry(fast_retry())
        .unwrap();
    for _ in 0..4 {
        assert_eq!(
            subscription
                .messages
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            Message::Heartbeat
        );
    }
    subscription.stop();
    handle.join().unwrap();
}

#[test]
fn retry_wait_can_be_stopped_without_waiting_for_backoff() {
    let (url, handle) = server(|listener| {
        let mut stream = accept(&listener);
        request(&mut stream);
        response(&mut stream, "503 Unavailable", "", b"");
    });
    let subscription = DahuaClient::new(config(url))
        .unwrap()
        .subscribe_with_retry(RetryPolicy {
            initial: Duration::from_secs(30),
            maximum: Duration::from_secs(30),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !matches!(subscription.state(), ConnectionState::Reconnecting { .. }) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    subscription.stop();
    let deadline = Instant::now() + Duration::from_secs(1);
    while subscription.state() != ConnectionState::Stopped {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    handle.join().unwrap();
}

#[test]
fn slow_consumer_has_bounded_mailbox_and_visible_dropped_count() {
    let (url, handle) = server(|listener| {
        let mut stream = accept(&listener);
        request(&mut stream);
        let body = b"--b\r\nContent-Length: 9\r\n\r\nHeartbeat\r\n".repeat(200);
        response(
            &mut stream,
            "200 OK",
            "Content-Type: multipart/x-mixed-replace; boundary=b\r\n",
            &body,
        );
    });
    let subscription = DahuaClient::new(config(url))
        .unwrap()
        .subscribe_with_retry(RetryPolicy {
            initial: Duration::from_secs(30),
            maximum: Duration::from_secs(30),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !matches!(subscription.state(), ConnectionState::Reconnecting { .. }) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(subscription.dropped_messages(), 72);
    assert_eq!(subscription.messages.try_iter().count(), 128);
    subscription.stop();
    handle.join().unwrap();
}
