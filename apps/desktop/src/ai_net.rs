//! The translate-in-place transport (Task 602, D-094): the second and last network file of the
//! project, next to `update.rs`, and the only HTTP client for translate in place.
//!
//! It implements [`darkclip_ai::Transport`] and can reach exactly one address, [`MESSAGES_URL`].
//! The address is a constant: never a parameter, a setting or an environment variable. One POST
//! per call with exactly the headers the provider passes, HTTPS only, no redirects, no cookies,
//! a 30 s limit for the whole call, the answer capped at 256 KiB, and a fixed `User-Agent`
//! (`DarkClipboard/<version>`). Non-2xx answers come back as `Status { code, body }` (the
//! Messages API puts the reason in the body), read under the same cap.
//!
//! Proxy: the same as `update.rs`, ureq's defaults, which honour the `ALL_PROXY`,
//! `HTTPS_PROXY` and `HTTP_PROXY` environment variables (and their lowercase forms). TLS stays
//! end to end to `api.anthropic.com`: a proxy sees the host name, not the request.
//!
//! Nothing here logs. Request and response bytes, headers and the key are never printed or
//! formatted, and errors carry no content (GC-06). The agent and helpers are not shared with
//! `update.rs` (anti-goal A6: no shared network path).

use darkclip_ai::{Transport, TransportError};
use std::time::Duration;

pub(crate) const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
/// The limit for the whole call: connect, TLS, request and the response body.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(30);
/// The most response bytes read (256 KiB), for success and error answers alike.
pub(crate) const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// The real transport. Holds one ureq agent built from [`config`]; nothing else.
pub(crate) struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub(crate) fn new() -> Self {
        Self {
            agent: ureq::Agent::new_with_config(config()),
        }
    }
}

/// The agent configuration: HTTPS only, 0 redirects, the global timeout, a fixed `User-Agent`,
/// and 4xx/5xx returned as answers (not errors) so their body can be read.
fn config() -> ureq::config::Config {
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .https_only(true)
        .max_redirects(0)
        .http_status_as_error(false)
        .user_agent(user_agent())
        .build()
}

fn user_agent() -> String {
    format!("DarkClipboard/{}", env!("CARGO_PKG_VERSION"))
}

/// A 2xx answer is the body; anything else is `Status` with the (capped) body.
fn map_response(status: u16, body: Vec<u8>) -> Result<Vec<u8>, TransportError> {
    if (200..300).contains(&status) {
        Ok(body)
    } else {
        Err(TransportError::Status { code: status, body })
    }
}

/// Content-free mapping of a ureq failure.
fn map_error(e: &ureq::Error) -> TransportError {
    match e {
        ureq::Error::Timeout(_) => TransportError::Timeout,
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => TransportError::Timeout,
        ureq::Error::BodyExceedsLimit(_) => TransportError::TooLarge,
        _ => TransportError::Network,
    }
}

impl Transport for UreqTransport {
    fn post_messages(
        &self,
        body: &[u8],
        headers: &[(&'static str, String)],
    ) -> Result<Vec<u8>, TransportError> {
        let mut request = self.agent.post(MESSAGES_URL);
        for (name, value) in headers {
            request = request.header(*name, value.as_str());
        }
        let mut response = request.send(body).map_err(|e| map_error(&e))?;
        let status = response.status().as_u16();
        let limit = u64::try_from(MAX_RESPONSE_BYTES).unwrap_or(u64::MAX);
        let bytes = response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|e| map_error(&e))?;
        map_response(status, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ureq::config::AutoHeaderValue;

    #[test]
    fn messages_url_is_pinned() {
        // Built from pieces so this file holds exactly one quoted address: the constant.
        let host = ["ht", "tps://api.anthropic.com/"].concat();
        assert_eq!(MESSAGES_URL, [host.as_str(), "v1/messages"].concat());
        assert!(MESSAGES_URL.starts_with(&host));
        let source = include_str!("ai_net.rs");
        let needle = ["\"", "ht", "tp"].concat();
        assert_eq!(source.matches(needle.as_str()).count(), 1);
        let line = source
            .lines()
            .find(|l| l.contains(needle.as_str()))
            .unwrap();
        assert!(line.starts_with("pub(crate) const MESSAGES_URL: &str ="));
    }

    #[test]
    fn agent_config_is_strict() {
        let c = config();
        assert!(c.https_only());
        assert_eq!(c.max_redirects(), 0);
        assert_eq!(c.timeouts().global, Some(Duration::from_secs(30)));
        assert!(!c.http_status_as_error());
        match c.user_agent() {
            AutoHeaderValue::Provided(ua) => {
                assert_eq!(
                    ua.as_str(),
                    format!("DarkClipboard/{}", env!("CARGO_PKG_VERSION"))
                );
            }
            _ => panic!("User-Agent must be the fixed DarkClipboard value"),
        }
        // The transport uses this config (and nothing else).
        let t = UreqTransport::new();
        assert!(t.agent.config().https_only());
        assert_eq!(t.agent.config().max_redirects(), 0);
        assert_eq!(MAX_RESPONSE_BYTES, 256 * 1024);
    }

    #[test]
    fn status_mapping() {
        let body = b"{}".to_vec();
        assert_eq!(map_response(200, body.clone()), Ok(body.clone()));
        assert_eq!(
            map_response(401, body.clone()),
            Err(TransportError::Status {
                code: 401,
                body: body.clone()
            })
        );
        assert_eq!(
            map_response(529, body.clone()),
            Err(TransportError::Status { code: 529, body })
        );
        // Redirects are not followed (0 allowed); a 3xx is an error answer too.
        assert!(matches!(
            map_response(301, Vec::new()),
            Err(TransportError::Status { code: 301, .. })
        ));
    }

    #[test]
    fn error_mapping_is_content_free() {
        assert_eq!(
            map_error(&ureq::Error::Timeout(ureq::Timeout::Global)),
            TransportError::Timeout
        );
        assert_eq!(
            map_error(&ureq::Error::Io(std::io::Error::from(
                std::io::ErrorKind::TimedOut
            ))),
            TransportError::Timeout
        );
        assert_eq!(
            map_error(&ureq::Error::BodyExceedsLimit(1)),
            TransportError::TooLarge
        );
        assert_eq!(
            map_error(&ureq::Error::HostNotFound),
            TransportError::Network
        );
        assert_eq!(
            map_error(&ureq::Error::ConnectionFailed),
            TransportError::Network
        );
        assert_eq!(
            map_error(&ureq::Error::Tls("handshake")),
            TransportError::Network
        );
    }
}
