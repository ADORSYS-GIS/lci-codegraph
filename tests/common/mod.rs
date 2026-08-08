//! Shared helpers for the black-box integration suites under `tests/`. Kept in a `tests/common/`
//! subdirectory (not `tests/common.rs`) so cargo does not treat it as its own test binary — the
//! standard pattern for code shared across integration-test crates.

use std::fs;
use std::path::Path;

/// Write `body` to `root/rel`, creating parent directories as needed. Mirrors the private helper
/// `src/walk.rs`'s own `#[cfg(test)]` module uses, duplicated here because integration tests can't
/// reach into a crate's private test-only items.
#[allow(dead_code)]
pub fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

/// Write raw `bytes` to `root/rel`, creating parent directories as needed — for binary/PDF fixtures.
#[allow(dead_code)]
pub fn write_bytes(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

/// A genuinely valid, minimal single-page PDF whose content stream draws `text` — built with `lopdf`
/// (a proper xref/trailer is fiddly to hand-write) so extraction through `pdf-extract` is a real
/// round trip, not a fake fixture. Lifted from the idiom `src/pdf.rs`'s own unit tests use
/// (`minimal_pdf_with_text`), duplicated here because it's a private test-only helper there too.
#[allow(dead_code)]
pub fn minimal_pdf_with_text(text: &str) -> Vec<u8> {
    use lopdf::content::{Content, Operation};
    use lopdf::{Document, Object, Stream, dictionary};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 24.into()]),
            Operation::new("Td", vec![20.into(), 100.into()]),
            Operation::new("Tj", vec![Object::string_literal(text)]),
            Operation::new("ET", vec![]),
        ],
    };
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
    });
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("save synthetic pdf");
    buf
}

/// A minimal HTTP/1.1 stub embeddings server, built on `std::net::TcpListener` only (no new
/// dev-dependency), for integration tests that exercise the embeddings client through the public
/// `walk_checkout` API rather than `src/embed/client.rs` directly. That module has its own
/// `#[cfg(test)]` `StubServer` with fixed, pre-canned `(status, body)` responses — the pattern this
/// mirrors — but a `#[cfg(test)]` item is crate-private and unreachable from an integration-test
/// binary, and pre-canning doesn't fit here anyway: a `walk_checkout` caller can't predict the exact
/// batch count without duplicating `embed::embed_chunks`'s own batching arithmetic. Instead this
/// server inspects each request's `input` array and synthesises exactly that many `dimension`-length
/// vectors, so it answers correctly regardless of how many chunks land in a request.
#[allow(dead_code)]
pub struct EmbedStubServer {
    addr: std::net::SocketAddr,
    request_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    _handle: std::thread::JoinHandle<()>,
}

#[allow(dead_code)]
impl EmbedStubServer {
    /// Start a server bound to an ephemeral local port that answers every request with `200` and
    /// `dimension`-length vectors (one per input), and serves up to `max_requests` connections before
    /// its accept thread exits — the same bounded-lifetime shape as the unit-test `StubServer`, so a
    /// test that (correctly) never sends more requests than it budgeted for never leaves the thread
    /// blocked waiting on a connection that isn't coming.
    pub fn start(dimension: usize, max_requests: usize) -> Self {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind embed stub server");
        let addr = listener.local_addr().expect("embed stub server local addr");
        let request_count = Arc::new(AtomicUsize::new(0));
        let request_count_for_thread = Arc::clone(&request_count);

        let handle = std::thread::spawn(move || {
            for (i, stream) in listener.incoming().enumerate() {
                if i >= max_requests {
                    break;
                }
                let Ok(mut stream) = stream else { break };
                let Some(body) = read_request_body(&mut stream) else {
                    break;
                };
                let input_len = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| {
                        v.get("input")
                            .and_then(|input| input.as_array().map(Vec::len))
                    })
                    .unwrap_or(0);
                let data: Vec<String> = (0..input_len)
                    .map(|idx| {
                        // Deterministic, distinct-per-index vector — just enough for a test to assert
                        // a real vector (not a zero-filled placeholder) landed on every chunk.
                        let values: Vec<String> = (0..dimension)
                            .map(|d| format!("{:.3}", (idx * dimension + d + 1) as f32 / 10.0))
                            .collect();
                        format!(r#"{{"index":{idx},"embedding":[{}]}}"#, values.join(","))
                    })
                    .collect();
                let response_body = format!(r#"{{"data":[{}]}}"#, data.join(","));
                write_http_response(&mut stream, &response_body);
                request_count_for_thread.fetch_add(1, Ordering::Relaxed);
                if i + 1 >= max_requests {
                    break;
                }
            }
        });

        fn read_request_body(stream: &mut std::net::TcpStream) -> Option<String> {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            let header_end = loop {
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos;
                }
                let n = stream.read(&mut chunk).ok()?;
                if n == 0 {
                    return None;
                }
                buf.extend_from_slice(&chunk[..n]);
            };

            let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
            let mut content_length = 0usize;
            for line in head.split("\r\n").skip(1) {
                if let Some((k, v)) = line.split_once(':')
                    && k.trim().eq_ignore_ascii_case("content-length")
                {
                    content_length = v.trim().parse().unwrap_or(0);
                }
            }

            let mut body_bytes = buf[header_end + 4..].to_vec();
            while body_bytes.len() < content_length {
                let n = stream.read(&mut chunk).ok()?;
                if n == 0 {
                    break;
                }
                body_bytes.extend_from_slice(&chunk[..n]);
            }
            Some(String::from_utf8_lossy(&body_bytes).into_owned())
        }

        fn write_http_response(stream: &mut std::net::TcpStream, body: &str) {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }

        Self {
            addr,
            request_count,
            _handle: handle,
        }
    }

    /// `http://` base URL suitable for `EmbedConfig::base_url`.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Number of requests the server has served so far.
    pub fn request_count(&self) -> usize {
        self.request_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}
