//! The one blocking HTTP client sources use: fixed timeouts, a `ft/<ver>`
//! user agent, the OS trust store (corporate TLS proxies work), and error
//! classification into [`SourceError`] — `Offline` for anything that means
//! "can't reach the server".

use std::time::Duration;

use super::SourceError;

/// A credential that never prints.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Secret(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// A JSON response plus the headers pagination needs.
#[derive(Debug)]
pub struct JsonResponse {
    pub json: serde_json::Value,
    /// Raw `Link` header (GitHub pagination).
    pub link: Option<String>,
}

pub struct Http {
    agent: ureq::Agent,
    authorization: Secret,
}

/// Longest `Retry-After` we wait for before giving up with `RateLimited`.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

impl Http {
    /// `authorization` is the full `Authorization` header value.
    pub fn new(authorization: Secret) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent(format!("ft/{}", env!("CARGO_PKG_VERSION")))
            .http_status_as_error(false)
            .tls_config(tls)
            .build()
            .into();
        Http {
            agent,
            authorization,
        }
    }

    /// GET `url` with `Accept: application/json`, retrying once on a 429
    /// whose `Retry-After` is within [`MAX_RETRY_AFTER`].
    pub fn get_json(&self, url: &str, accept: &str) -> Result<JsonResponse, SourceError> {
        for attempt in 0..2 {
            let mut resp = self
                .agent
                .get(url)
                .header("Authorization", self.authorization.expose())
                .header("Accept", accept)
                .call()
                .map_err(classify)?;
            let status = resp.status().as_u16();
            let header = |name: &str| {
                resp.headers()
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
            };
            match status {
                200..=299 => {
                    let link = header("link");
                    let body = resp
                        .body_mut()
                        .with_config()
                        .limit(64 * 1024 * 1024)
                        .read_to_string()
                        .map_err(classify)?;
                    let json = serde_json::from_str(&body)
                        .map_err(|e| SourceError::Parse(e.to_string()))?;
                    return Ok(JsonResponse { json, link });
                }
                401 | 403 => return Err(SourceError::Unauthorized { status }),
                429 => {
                    let wait = header("retry-after")
                        .and_then(|v| v.trim().parse::<u64>().ok())
                        .map(Duration::from_secs);
                    match wait {
                        Some(w) if attempt == 0 && w <= MAX_RETRY_AFTER => std::thread::sleep(w),
                        _ => return Err(SourceError::RateLimited),
                    }
                }
                _ => {
                    let body = resp.body_mut().read_to_string().unwrap_or_default();
                    let excerpt: String = body.chars().take(200).collect();
                    return Err(SourceError::Remote { status, excerpt });
                }
            }
        }
        Err(SourceError::RateLimited)
    }
}

/// Map a transport-level error. Everything that means "couldn't talk to
/// the server" is `Offline`.
fn classify(e: ureq::Error) -> SourceError {
    match e {
        ureq::Error::Timeout(_)
        | ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Io(_) => SourceError::Offline(e.to_string()),
        other => SourceError::Parse(other.to_string()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serve `responses` (raw HTTP/1.1 response texts) to successive
    /// connections on a local port; returns the base URL and the requests
    /// received (raw text) once all responses were served.
    pub(crate) fn serve(responses: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for resp in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                seen.push(String::from_utf8_lossy(&buf[..n]).into_owned());
                stream.write_all(resp.as_bytes()).unwrap();
            }
            seen
        });
        (url, handle)
    }

    pub(crate) fn ok_json(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn status(code: u16, extra: &str) -> String {
        format!("HTTP/1.1 {code} X\r\nContent-Length: 4\r\nConnection: close\r\n{extra}\r\nnope")
    }

    // NOT VALID — test data only.
    fn fake_auth() -> Secret {
        Secret::new("Bearer FAKE_TEST_token")
    }

    #[test]
    fn get_json_ok_sends_auth_and_reads_link() {
        let body = r#"{"a":1}"#;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nLink: <x>; rel=\"next\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let (url, h) = serve(vec![resp]);
        let r = Http::new(fake_auth())
            .get_json(&format!("{url}/x"), "application/json")
            .unwrap();
        assert_eq!(r.json["a"], 1);
        assert_eq!(r.link.as_deref(), Some("<x>; rel=\"next\""));
        let req = &h.join().unwrap()[0];
        assert!(
            req.contains("authorization: Bearer FAKE_TEST_token")
                || req.contains("Authorization: Bearer FAKE_TEST_token")
        );
        assert!(req.to_lowercase().contains("user-agent: ft/"));
    }

    #[test]
    fn get_json_classifies_statuses() {
        let (url, _h) = serve(vec![status(401, ""), status(500, "")]);
        let http = Http::new(fake_auth());
        assert_eq!(
            http.get_json(&url, "application/json").unwrap_err(),
            SourceError::Unauthorized { status: 401 }
        );
        assert!(matches!(
            http.get_json(&url, "application/json").unwrap_err(),
            SourceError::Remote { status: 500, .. }
        ));
    }

    #[test]
    fn get_json_retries_once_on_429() {
        let (url, h) = serve(vec![status(429, "Retry-After: 0\r\n"), ok_json("{}")]);
        Http::new(fake_auth())
            .get_json(&url, "application/json")
            .unwrap();
        assert_eq!(h.join().unwrap().len(), 2);
        let (url, _h) = serve(vec![
            status(429, "Retry-After: 0\r\n"),
            status(429, "Retry-After: 0\r\n"),
        ]);
        assert_eq!(
            Http::new(fake_auth())
                .get_json(&url, "application/json")
                .unwrap_err(),
            SourceError::RateLimited
        );
    }

    #[test]
    fn connection_refused_is_offline_and_secret_never_prints() {
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let err = Http::new(fake_auth())
            .get_json(&format!("http://127.0.0.1:{port}/"), "application/json")
            .unwrap_err();
        assert!(matches!(err, SourceError::Offline(_)), "{err:?}");
        assert!(!format!("{err} {err:?}").contains("FAKE_TEST"));
        assert_eq!(format!("{:?}", fake_auth()), "Secret(***)");
    }
}
