//! SSRF guard and streaming cap of the remote-image fetcher, proven against a
//! loopback server. Production uses `GuardPolicy::http_or_https()`, which
//! refuses loopback; the tests that need the fake to be reachable pass a
//! policy that allow-lists exactly `127.0.0.1` (an allow-list, not a bypass:
//! private ranges, the metadata address and redirects to them stay refused).

use super::*;

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};

fn loopback_policy() -> GuardPolicy {
    GuardPolicy {
        extra_allowed_ips: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
        ..GuardPolicy::http_or_https()
    }
}

fn png() -> Vec<u8> {
    b"\x89PNG\r\n\x1a\nbody".to_vec()
}

/// One-shot-per-connection loopback server; `respond` writes the reply.
struct Server {
    url: String,
    hits: Arc<AtomicUsize>,
}

impl Server {
    fn start(respond: impl Fn(&mut TcpStream) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let respond = Arc::new(respond);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                counter.fetch_add(1, Ordering::SeqCst);
                let respond = respond.clone();
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let mut request = [0_u8; 2048];
                    let _ = stream.read(&mut request);
                    respond(&mut stream);
                });
            }
        });
        Self { url, hits }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

fn serve_png(stream: &mut TcpStream) {
    let body = png();
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(&body);
}

fn fetch(url: &str, policy: &GuardPolicy, cap: usize) -> Result<Vec<u8>, AssetFetchError> {
    fetch_remote_image_guarded(url, policy, cap)
}

#[test]
fn remote_image_host_serves_an_allowed_destination() {
    let server = Server::start(serve_png);
    let bytes = fetch(
        &format!("{}/a.png", server.url),
        &loopback_policy(),
        1 << 20,
    )
    .unwrap();
    assert_eq!(bytes, png());
}

#[test]
fn remote_image_host_refuses_private_and_metadata_addresses() {
    // The fake is reachable, yet the production policy must never connect.
    let server = Server::start(serve_png);
    let refused = [
        format!("{}/a.png", server.url),
        "http://127.0.0.1:9/a.png".to_string(),
        "http://10.0.0.5/a.png".to_string(),
        "http://192.168.1.1/a.png".to_string(),
        "http://169.254.169.254/latest/meta-data/".to_string(),
        "http://[::1]/a.png".to_string(),
        "http://localhost/a.png".to_string(),
        "file:///etc/passwd".to_string(),
        "http://user:pw@example.org/a.png".to_string(),
    ];
    for url in refused {
        let result = fetch(&url, &GuardPolicy::http_or_https(), 1 << 20);
        assert!(
            matches!(result, Err(AssetFetchError::Refused(_))),
            "{url} was not refused: {result:?}"
        );
    }
    assert_eq!(
        server.hits(),
        0,
        "a refused destination must never be contacted"
    );

    // Allow-listing loopback does not open the rest of the private space.
    for url in ["http://10.0.0.5/a.png", "http://169.254.169.254/latest/"] {
        assert!(matches!(
            fetch(url, &loopback_policy(), 1 << 20),
            Err(AssetFetchError::Refused(_))
        ));
    }
}

#[test]
fn remote_image_host_aborts_oversized_streaming_body() {
    // No Content-Length: the cap must hold on the streamed bytes. The body
    // is written in chunks until the client hangs up.
    let written = Arc::new(AtomicUsize::new(0));
    let progress = written.clone();
    let server = Server::start(move |stream| {
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nConnection: close\r\n\r\n"
        );
        let chunk = vec![0_u8; 4096];
        for _ in 0..4096 {
            if stream.write_all(&chunk).is_err() {
                return;
            }
            progress.fetch_add(chunk.len(), Ordering::SeqCst);
        }
    });
    let result = fetch(
        &format!("{}/big.png", server.url),
        &loopback_policy(),
        8 * 1024,
    );
    assert_eq!(result, Err(AssetFetchError::TooLarge));
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        written.load(Ordering::SeqCst) < 16 * 1024 * 1024,
        "the server was allowed to stream the whole body"
    );

    // A declared Content-Length over the cap is refused without reading.
    let declared = Server::start(|stream| {
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 999999\r\nConnection: close\r\n\r\n"
        );
    });
    assert_eq!(
        fetch(&format!("{}/x.png", declared.url), &loopback_policy(), 1024),
        Err(AssetFetchError::TooLarge)
    );
}

#[test]
fn redirect_to_private_ip_is_refused() {
    for target in [
        "http://10.0.0.5/secret.png",
        "http://169.254.169.254/latest/meta-data/",
        "http://192.168.0.1/",
        "http://[::1]/",
    ] {
        let location = target.to_string();
        let server = Server::start(move |stream| {
            let _ = write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        });
        let result = fetch(
            &format!("{}/r.png", server.url),
            &loopback_policy(),
            1 << 20,
        );
        assert!(
            matches!(result, Err(AssetFetchError::Refused(_))),
            "redirect to {target} was followed: {result:?}"
        );
    }

    // A redirect to an allowed destination is followed, a loop is cut off.
    let final_server = Server::start(serve_png);
    let target = format!("{}/final.png", final_server.url);
    let hop = Server::start(move |stream| {
        let _ = write!(
            stream,
            "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    });
    assert_eq!(
        fetch(&format!("{}/hop.png", hop.url), &loopback_policy(), 1 << 20),
        Ok(png())
    );
    let looping_url = Arc::new(std::sync::Mutex::new(String::new()));
    let own_url = looping_url.clone();
    let looping = Server::start(move |stream| {
        let location = own_url.lock().unwrap().clone();
        let _ = write!(
            stream,
            "HTTP/1.1 302 Found\r\nLocation: {location}/again\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    });
    *looping_url.lock().unwrap() = looping.url.clone();
    let result = fetch(
        &format!("{}/start", looping.url),
        &loopback_policy(),
        1 << 20,
    );
    assert!(
        matches!(result, Err(AssetFetchError::Refused(_))),
        "{result:?}"
    );
    assert!(looping.hits() <= 5, "redirect limit not enforced");
}
