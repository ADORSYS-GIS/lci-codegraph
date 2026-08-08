//! The one blocking HTTP call this crate makes: `POST {base_url}/embeddings` against an
//! OpenAI-compatible endpoint. Deliberately not async — see the `ureq` rationale in `Cargo.toml`.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::EmbedConfig;

/// Request body. `encoding_format` is **always** sent explicitly as `"float"`: some
/// OpenAI-compatible gateways default to base64 when it's omitted, which would deserialize into
/// garbage here rather than a clean error — being explicit trades a few request bytes for never
/// hitting that failure mode.
#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
    encoding_format: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<u32>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedDatum>,
}

#[derive(Deserialize)]
struct EmbedDatum {
    index: usize,
    embedding: Vec<f32>,
}

/// Longest non-2xx response body kept in an error message, so a gateway's HTML error page or a huge
/// stack trace doesn't blow up logs/error chains.
const MAX_ERROR_BODY_CHARS: usize = 512;

/// Send one batch of `inputs` to the embeddings endpoint and return one vector per input, in request
/// order. Retries 429s, 5xxs, and transport errors up to `config.max_retries` times with exponential
/// backoff (`500ms * 2^attempt`), incrementing `*retries` once per retry actually taken. A 4xx other
/// than 429 is a configuration error (bad model name, bad key) — retrying only delays the report, so
/// it fails immediately.
pub(crate) fn embed_batch(
    config: &EmbedConfig,
    inputs: &[String],
    retries: &mut usize,
) -> anyhow::Result<Vec<Vec<f32>>> {
    let url = format!("{}/embeddings", config.base_url.trim_end_matches('/'));
    let body = serde_json::to_vec(&EmbedRequest {
        model: &config.model,
        input: inputs,
        encoding_format: "float",
        dimensions: config.dimensions,
    })?;

    // `http_status_as_error(false)`: we want the response body on a non-2xx status (for the error
    // snippet), and ureq's default behaviour turns 4xx/5xx into an `Err` before we ever get to read
    // it. Checking `status()` ourselves below is the price of keeping that body.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(config.timeout))
        .http_status_as_error(false)
        .build()
        .into();

    let mut attempt: u32 = 0;
    loop {
        let mut request = agent.post(&url).content_type("application/json");
        if let Some(key) = &config.api_key {
            request = request.header("Authorization", format!("Bearer {key}"));
        }

        match request.send(&body) {
            Ok(mut response) => {
                let status = response.status();
                if status.is_success() {
                    let text = response
                        .body_mut()
                        .read_to_string()
                        .map_err(|e| anyhow::anyhow!("reading embeddings response body: {e}"))?;
                    let parsed: EmbedResponse = serde_json::from_str(&text)
                        .map_err(|e| anyhow::anyhow!("parsing embeddings response JSON: {e}"))?;
                    if parsed.data.len() != inputs.len() {
                        anyhow::bail!(
                            "embeddings endpoint returned {} vectors for {} inputs — refusing to \
                             align a short response against the wrong chunks",
                            parsed.data.len(),
                            inputs.len()
                        );
                    }
                    // The spec does not guarantee response order matches request order, so `index` —
                    // not position — decides which input a vector belongs to. Sorting alone is not
                    // enough: a gateway echoing a constant or duplicated `index` would sort into a
                    // stable-but-wrong order and silently attach vectors to the wrong chunks, which
                    // no downstream consumer could ever detect. Verify it is a true permutation of
                    // `0..len` first, and fail loudly if it isn't.
                    let mut data = parsed.data;
                    data.sort_by_key(|d| d.index);
                    if data.iter().enumerate().any(|(i, d)| d.index != i) {
                        anyhow::bail!(
                            "embeddings endpoint returned indices that are not a permutation of \
                             0..{} — refusing to align vectors against the wrong chunks",
                            inputs.len()
                        );
                    }
                    return Ok(data.into_iter().map(|d| d.embedding).collect());
                }

                let retryable = status.as_u16() == 429 || status.is_server_error();
                if retryable && attempt < config.max_retries {
                    *retries += 1;
                    std::thread::sleep(backoff(attempt));
                    attempt += 1;
                    continue;
                }
                // Only the response body goes into the error, never `body` — that is the request
                // payload, and it carries the repo's own source code. An error string tends to end
                // up in a log aggregator, which is not somewhere source belongs.
                let snippet = response.body_mut().read_to_string().unwrap_or_default();
                anyhow::bail!(
                    "embeddings endpoint returned {status}: {}",
                    truncate_chars(&snippet, MAX_ERROR_BODY_CHARS)
                );
            }
            Err(err) => {
                if attempt < config.max_retries {
                    *retries += 1;
                    std::thread::sleep(backoff(attempt));
                    attempt += 1;
                    continue;
                }
                return Err(anyhow::Error::new(err).context("embeddings request failed"));
            }
        }
    }
}

/// `500ms * 2^attempt`, saturating rather than panicking on overflow — `max_retries` is an
/// operator-controlled `u32` and this must never panic regardless of how large it's set.
fn backoff(attempt: u32) -> Duration {
    let millis = 500u64.saturating_mul(1u64 << attempt.min(32));
    Duration::from_millis(millis)
}

/// Truncate `s` to at most `max_chars` chars, on a char boundary.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;

    /// One request as observed by [`StubServer`].
    #[derive(Debug, Clone)]
    struct RecordedRequest {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    /// A minimal HTTP/1.1 server over a raw [`TcpListener`] (no new dev-dependency) that replies with
    /// a fixed, ordered sequence of `(status, body)` responses — one per connection — and records
    /// every request it received. Serves exactly `responses.len()` requests then its accept thread
    /// exits, so a test that (correctly) never sends more requests than it queued responses for never
    /// leaves the thread blocked waiting on a connection that isn't coming.
    struct StubServer {
        addr: std::net::SocketAddr,
        requests: Arc<Mutex<Vec<RecordedRequest>>>,
        _handle: JoinHandle<()>,
    }

    impl StubServer {
        fn start(responses: Vec<(u16, &'static str)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub server");
            let addr = listener.local_addr().expect("stub server local addr");
            let requests = Arc::new(Mutex::new(Vec::new()));
            let requests_for_thread = Arc::clone(&requests);
            let expected = responses.len();
            let handle = std::thread::spawn(move || {
                for (i, stream) in listener.incoming().enumerate() {
                    let Ok(mut stream) = stream else { break };
                    let Some(request) = read_request(&mut stream) else {
                        break;
                    };
                    let (status, resp_body) = responses[i];
                    // Record BEFORE replying, never after. A test inspects `requests()` once
                    // `embed_batch` has returned, and the client returns as soon as it has read the
                    // response — so recording after the write is a race the server loses whenever
                    // the client is scheduled first. Ordering it this way gives the test a real
                    // happens-before edge: the push completes before the bytes the client is
                    // waiting on are ever written.
                    requests_for_thread.lock().unwrap().push(request);
                    write_response(&mut stream, status, resp_body);
                    if i + 1 >= expected {
                        break;
                    }
                }
            });
            Self {
                addr,
                requests,
                _handle: handle,
            }
        }

        fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }

        fn requests(&self) -> Vec<RecordedRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    fn read_request(stream: &mut TcpStream) -> Option<RecordedRequest> {
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
        let mut lines = head.split("\r\n");
        let request_line = lines.next()?;
        let mut parts = request_line.split_whitespace();
        let method = parts.next()?.to_string();
        let path = parts.next()?.to_string();

        let mut headers = Vec::new();
        let mut content_length = 0usize;
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                let (k, v) = (k.trim().to_string(), v.trim().to_string());
                if k.eq_ignore_ascii_case("content-length") {
                    content_length = v.parse().unwrap_or(0);
                }
                headers.push((k, v));
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
        let body = String::from_utf8_lossy(&body_bytes).into_owned();

        Some(RecordedRequest {
            method,
            path,
            headers,
            body,
        })
    }

    fn write_response(stream: &mut TcpStream, status: u16, body: &str) {
        let reason = match status {
            200 => "OK",
            401 => "Unauthorized",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            _ => "Response",
        };
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    }

    fn test_config(base_url: String) -> EmbedConfig {
        EmbedConfig::builder()
            .base_url(base_url)
            .timeout(Duration::from_secs(5))
            .max_retries(2)
            .build()
    }

    fn vectors_response(vectors: &[(usize, [f32; 2])]) -> String {
        let data: Vec<String> = vectors
            .iter()
            .map(|(i, v)| format!(r#"{{"index":{i},"embedding":[{},{}]}}"#, v[0], v[1]))
            .collect();
        format!(r#"{{"data":[{}]}}"#, data.join(","))
    }

    #[test]
    fn happy_path_returns_vectors_in_request_order() {
        let body = vectors_response(&[(0, [0.1, 0.2]), (1, [0.3, 0.4])]);
        let server = StubServer::start(vec![(200, Box::leak(body.into_boxed_str()))]);
        let config = test_config(server.base_url());
        let mut retries = 0;
        let inputs = vec!["one".to_string(), "two".to_string()];

        let vectors = embed_batch(&config, &inputs, &mut retries).unwrap();

        assert_eq!(vectors, vec![vec![0.1, 0.2], vec![0.3, 0.4]]);
        assert_eq!(retries, 0);

        let requests = server.requests();
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/embeddings");
    }

    #[test]
    fn out_of_order_response_index_is_still_assigned_correctly() {
        let body = vectors_response(&[(1, [9.0, 9.0]), (0, [1.0, 1.0])]);
        let server = StubServer::start(vec![(200, Box::leak(body.into_boxed_str()))]);
        let config = test_config(server.base_url());
        let mut retries = 0;
        let inputs = vec!["one".to_string(), "two".to_string()];

        let vectors = embed_batch(&config, &inputs, &mut retries).unwrap();

        assert_eq!(vectors, vec![vec![1.0, 1.0], vec![9.0, 9.0]]);
    }

    #[test]
    fn authorization_header_present_when_api_key_set() {
        let body = vectors_response(&[(0, [0.0, 0.0])]);
        let server = StubServer::start(vec![(200, Box::leak(body.into_boxed_str()))]);
        let config = EmbedConfig::builder()
            .base_url(server.base_url())
            .api_key("sekret-key".to_string())
            .build();
        let mut retries = 0;

        embed_batch(&config, &["x".to_string()], &mut retries).unwrap();

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        let auth = requests[0]
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"));
        assert_eq!(auth.map(|(_, v)| v.as_str()), Some("Bearer sekret-key"));
    }

    #[test]
    fn authorization_header_absent_when_api_key_unset() {
        let body = vectors_response(&[(0, [0.0, 0.0])]);
        let server = StubServer::start(vec![(200, Box::leak(body.into_boxed_str()))]);
        let config = test_config(server.base_url());
        let mut retries = 0;

        embed_batch(&config, &["x".to_string()], &mut retries).unwrap();

        let requests = server.requests();
        assert!(
            !requests[0]
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("authorization")),
            "no Authorization header expected, got {:?}",
            requests[0].headers
        );
    }

    #[test]
    fn request_body_carries_encoding_format_model_and_dimensions_only_when_configured() {
        let body = vectors_response(&[(0, [0.0, 0.0])]);
        let server = StubServer::start(vec![(200, Box::leak(body.into_boxed_str()))]);
        let config = EmbedConfig::builder()
            .base_url(server.base_url())
            .model("test-model".to_string())
            .dimensions(256)
            .build();
        let mut retries = 0;

        embed_batch(&config, &["hello".to_string()], &mut retries).unwrap();

        let requests = server.requests();
        let sent: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(sent["encoding_format"], "float");
        assert_eq!(sent["model"], "test-model");
        assert_eq!(sent["dimensions"], 256);
        assert_eq!(sent["input"][0], "hello");

        // A second config with no `dimensions` configured must omit the field entirely, not send
        // `null` (some gateways reject an explicit null rather than treating it as absent).
        let server2 = StubServer::start(vec![(
            200,
            Box::leak(vectors_response(&[(0, [0.0, 0.0])]).into_boxed_str()),
        )]);
        let config2 = test_config(server2.base_url());
        embed_batch(&config2, &["hello".to_string()], &mut retries).unwrap();
        let sent2: serde_json::Value = serde_json::from_str(&server2.requests()[0].body).unwrap();
        assert!(sent2.get("dimensions").is_none());
    }

    #[test]
    fn retries_on_429_then_succeeds() {
        let ok_body = vectors_response(&[(0, [0.5, 0.5])]);
        let server = StubServer::start(vec![
            (429, "rate limited"),
            (200, Box::leak(ok_body.into_boxed_str())),
        ]);
        let config = test_config(server.base_url());
        let mut retries = 0;

        let vectors = embed_batch(&config, &["x".to_string()], &mut retries).unwrap();

        assert_eq!(vectors, vec![vec![0.5, 0.5]]);
        assert_eq!(retries, 1);
        assert_eq!(server.requests().len(), 2);
    }

    #[test]
    fn exhausting_retries_on_500_returns_an_error_naming_the_status() {
        let server = StubServer::start(vec![(500, "boom"), (500, "boom"), (500, "boom")]);
        let config = test_config(server.base_url()); // max_retries: 2

        let err = embed_batch(&config, &["x".to_string()], &mut 0).unwrap_err();

        assert!(
            err.to_string().contains("500"),
            "error must name the status, got: {err}"
        );
        assert_eq!(server.requests().len(), 3, "initial attempt + 2 retries");
    }

    #[test]
    fn non_retryable_401_fails_without_retrying() {
        let server = StubServer::start(vec![(401, "bad key")]);
        let config = test_config(server.base_url());
        let mut retries = 0;

        let err = embed_batch(&config, &["x".to_string()], &mut retries).unwrap_err();

        assert!(err.to_string().contains("401"));
        assert_eq!(retries, 0);
        assert_eq!(
            server.requests().len(),
            1,
            "a 4xx other than 429 must not be retried"
        );
    }

    #[test]
    fn short_data_array_is_an_error() {
        // Two inputs sent, only one vector back — must error rather than silently misalign the rest.
        let body = vectors_response(&[(0, [0.0, 0.0])]);
        let server = StubServer::start(vec![(200, Box::leak(body.into_boxed_str()))]);
        let config = test_config(server.base_url());
        let inputs = vec!["one".to_string(), "two".to_string()];

        let err = embed_batch(&config, &inputs, &mut 0).unwrap_err();
        assert!(err.to_string().contains('1') && err.to_string().contains('2'));
    }

    #[test]
    fn duplicated_response_indices_are_an_error_not_a_silent_misalignment() {
        // The right length, but `index` is 0 twice — so sorting yields a stable order that is
        // nonetheless wrong. Caught here rather than shipped as vectors attached to the wrong
        // chunks, which is corruption no downstream consumer could ever notice.
        let body = vectors_response(&[(0, [1.0, 1.0]), (0, [9.0, 9.0])]);
        let server = StubServer::start(vec![(200, Box::leak(body.into_boxed_str()))]);
        let config = test_config(server.base_url());
        let inputs = vec!["one".to_string(), "two".to_string()];

        let err = embed_batch(&config, &inputs, &mut 0).unwrap_err();
        assert!(
            err.to_string().contains("permutation"),
            "error should name the index problem, got: {err}"
        );
    }

    #[test]
    fn error_message_never_contains_the_api_key() {
        let server = StubServer::start(vec![(401, "unauthorized: bad credentials")]);
        let config = EmbedConfig::builder()
            .base_url(server.base_url())
            .api_key("super-secret-value".to_string())
            .build();

        let err = embed_batch(&config, &["x".to_string()], &mut 0).unwrap_err();

        assert!(!err.to_string().contains("super-secret-value"));
    }
}
