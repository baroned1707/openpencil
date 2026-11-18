//! Loopback HTTP fake for provider tests. Tests must never reach the real
//! network: providers take a `base_url`, and this server answers on
//! `127.0.0.1` with scripted responses while recording every request target.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) struct FakeResponse {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl FakeResponse {
    pub fn json(value: serde_json::Value) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            content_type: "application/json",
            body: value.to_string().into_bytes(),
        }
    }

    pub fn png() -> Self {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 30, 30, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode png");
        Self {
            status: 200,
            headers: Vec::new(),
            content_type: "image/png",
            body: bytes,
        }
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            content_type: "text/plain",
            body: Vec::new(),
        }
    }

    pub fn with_header(mut self, name: &'static str, value: &str) -> Self {
        self.headers.push((name, value.to_string()));
        self
    }
}

/// Handler input: the request target (`/path?query`) and its zero-based
/// arrival index.
type Handler = dyn Fn(&str, usize) -> FakeResponse + Send + Sync;

pub(crate) struct FakeHttp {
    pub base_url: String,
    log: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    address: std::net::SocketAddr,
}

impl FakeHttp {
    pub fn start(handler: impl Fn(&str, usize) -> FakeResponse + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let address = listener.local_addr().expect("local addr");
        let handler: Arc<Handler> = Arc::new(handler);
        let log = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_log, thread_stop) = (log.clone(), stop.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_stop.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(stream) = stream else { continue };
                let (handler, log) = (handler.clone(), thread_log.clone());
                std::thread::spawn(move || serve(stream, handler.as_ref(), &log));
            }
        });
        Self {
            base_url: format!("http://{address}"),
            log,
            stop,
            address,
        }
    }

    /// Every request target received so far, in arrival order.
    pub fn requests(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

impl Drop for FakeHttp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking accept so the listener thread exits.
        let _ = TcpStream::connect(self.address);
    }
}

fn serve(mut stream: TcpStream, handler: &Handler, log: &Mutex<Vec<String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&chunk[..read]),
        }
    }
    let text = String::from_utf8_lossy(&request);
    let target = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();
    let response = {
        // Index and log under one lock so arrival order is unambiguous.
        let mut log = log.lock().unwrap();
        let index = log.len();
        log.push(target.clone());
        drop(log);
        handler(&target, index)
    };
    let mut head = format!(
        "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.content_type,
        response.body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&response.body);
}

/// Value of query parameter `key` in a request target.
pub(crate) fn query_param(target: &str, key: &str) -> Option<String> {
    reqwest::Url::parse(&format!("http://fake{target}"))
        .ok()?
        .query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
}

pub(crate) fn test_client() -> reqwest::Client {
    reqwest::Client::builder()
        .use_rustls_tls()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("test client")
}

/// One Openverse result JSON object. `thumbnail == url`, so
/// `original_source` yields `None` and no original upgrade is attempted.
pub(crate) fn openverse_result(
    base_url: &str,
    id: &str,
    title: &str,
    license: &str,
) -> serde_json::Value {
    let thumb = format!("{base_url}/thumb/{id}.png");
    serde_json::json!({
        "id": id,
        "title": title,
        "tags": [{"name": title}],
        "thumbnail": thumb,
        "url": thumb,
        "license": license,
        "license_version": "4.0",
        "license_url": format!("https://creativecommons.org/licenses/{license}/4.0/"),
        "creator": "Jane Doe",
        "foreign_landing_url": format!("https://example.org/photo/{id}"),
    })
}

/// One Commons page JSON object (`imageinfo` with `extmetadata`).
pub(crate) fn commons_page(
    base_url: &str,
    page_id: u64,
    title: &str,
    license_short: &str,
    license_url: &str,
) -> serde_json::Value {
    let thumb = format!("{base_url}/thumb/{page_id}.png");
    serde_json::json!({
        "pageid": page_id,
        "title": title,
        "index": 1,
        "imageinfo": [{
            "thumburl": thumb,
            "url": thumb,
            "mime": "image/png",
            "descriptionurl": format!("https://commons.wikimedia.org/wiki/{title}"),
            "extmetadata": {
                "LicenseShortName": {"value": license_short},
                "LicenseUrl": {"value": license_url},
                "Artist": {"value": "<a href=\"//commons.wikimedia.org/wiki/User:J\">Jane &amp; Co</a>"},
            }
        }]
    })
}

pub(crate) fn commons_reply(pages: Vec<serde_json::Value>) -> FakeResponse {
    let pages: serde_json::Map<String, serde_json::Value> = pages
        .into_iter()
        .map(|page| (page["pageid"].to_string(), page))
        .collect();
    FakeResponse::json(serde_json::json!({"query": {"pages": pages}}))
}

pub(crate) fn openverse_reply(results: Vec<serde_json::Value>) -> FakeResponse {
    FakeResponse::json(serde_json::json!({"results": results}))
}
