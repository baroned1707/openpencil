//! In-memory [`FontSource`] for tests (own and dependents'). Serves canned
//! bodies by exact URL, records every request, and can hold requests at a gate
//! so concurrency limits are observable.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

use crate::catalog::CATALOG_URL;
use crate::license::License;
use crate::source::{FetchError, FontSource, Response};
use crate::urls::{font_file_url, license_url, metadata_url};

#[derive(Default)]
struct Gate {
    closed: Mutex<bool>,
    opened: Condvar,
}

#[derive(Default)]
pub struct FakeFontSource {
    routes: Mutex<HashMap<String, Result<Response, FetchError>>>,
    log: Mutex<Vec<String>>,
    active: AtomicUsize,
    max_active: AtomicUsize,
    gate: Gate,
    offline: Mutex<bool>,
}

impl FakeFontSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn route(&self, url: &str, result: Result<Response, FetchError>) {
        self.routes.lock().unwrap().insert(url.to_string(), result);
    }

    pub fn route_bytes(&self, url: &str, bytes: &[u8]) {
        self.route(
            url,
            Ok(Response {
                bytes: bytes.to_vec(),
                etag: Some("\"fake-etag\"".into()),
            }),
        );
    }

    pub fn serve_catalog(&self, entries: &[(&str, &str, &str)]) {
        self.route_bytes(CATALOG_URL, catalog_json(entries).as_bytes());
    }

    /// Serve a whole family under `dir_license/<id without dashes>/`.
    pub fn serve_family(
        &self,
        id: &str,
        dir_license: License,
        metadata_text: &str,
        files: &[(&str, &[u8])],
    ) {
        let directory: String = id.chars().filter(|c| *c != '-').collect();
        self.route_bytes(
            &metadata_url(dir_license, &directory),
            metadata_text.as_bytes(),
        );
        for (name, bytes) in files {
            self.route_bytes(&font_file_url(dir_license, &directory, name), bytes);
        }
        self.route_bytes(&license_url(dir_license, &directory), b"licence text");
    }

    pub fn set_offline(&self, offline: bool) {
        *self.offline.lock().unwrap() = offline;
    }

    pub fn close_gate(&self) {
        *self.gate.closed.lock().unwrap() = true;
    }

    pub fn open_gate(&self) {
        *self.gate.closed.lock().unwrap() = false;
        self.gate.opened.notify_all();
    }

    pub fn requests(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    pub fn request_count(&self) -> usize {
        self.log.lock().unwrap().len()
    }

    pub fn active_now(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    pub fn max_concurrent(&self) -> usize {
        self.max_active.load(Ordering::SeqCst)
    }
}

impl FontSource for FakeFontSource {
    fn get(&self, url: &str, max_bytes: usize) -> Result<Response, FetchError> {
        self.log.lock().unwrap().push(url.to_string());
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(now, Ordering::SeqCst);
        {
            let mut closed = self.gate.closed.lock().unwrap();
            while *closed {
                closed = self.gate.opened.wait(closed).unwrap();
            }
        }
        let outcome = if *self.offline.lock().unwrap() {
            Err(FetchError::Offline)
        } else {
            self.routes
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .unwrap_or(Err(FetchError::NotFound))
        };
        self.active.fetch_sub(1, Ordering::SeqCst);
        outcome.and_then(|response| {
            if response.bytes.len() > max_bytes {
                Err(FetchError::TooLarge)
            } else {
                Ok(response)
            }
        })
    }
}

/// Fontsource-shaped catalog JSON: `(id, family, licence)` per Google entry.
pub fn catalog_json(entries: &[(&str, &str, &str)]) -> String {
    let rows: Vec<String> = entries
        .iter()
        .map(|(id, family, license)| {
            serde_json::json!({
                "id": id, "family": family, "license": license, "type": "google",
                "weights": [400, 700], "styles": ["normal"], "variable": false,
            })
            .to_string()
        })
        .collect();
    format!("[{}]", rows.join(","))
}

/// A static-family `METADATA.pb`: `(filename, style, weight)` per file.
pub fn metadata_static(name: &str, license: &str, files: &[(&str, &str, u16)]) -> String {
    let mut text = format!("name: \"{name}\"\nlicense: \"{license}\"\n");
    for (file, style, weight) in files {
        text.push_str(&format!(
            "fonts {{\n  name: \"{name}\"\n  style: \"{style}\"\n  weight: {weight}\n  filename: \"{file}\"\n}}\n"
        ));
    }
    text
}

/// A variable-family `METADATA.pb` with a `wght` axis of 100..900.
pub fn metadata_variable(name: &str, license: &str, files: &[(&str, &str)]) -> String {
    let mut text = format!("name: \"{name}\"\nlicense: \"{license}\"\n");
    for (file, style) in files {
        text.push_str(&format!(
            "fonts {{\n  name: \"{name}\"\n  style: \"{style}\"\n  weight: 400\n  filename: \"{file}\"\n}}\n"
        ));
    }
    text.push_str("axes {\n  tag: \"wght\"\n  min_value: 100.0\n  max_value: 900.0\n}\n");
    text
}

/// Smallest byte string `looks_like_truetype` accepts.
pub const TTF_STUB: &[u8] = &[0x00, 0x01, 0x00, 0x00, 0, 0, 0, 0];
