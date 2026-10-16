use std::time::Duration;

use euc_profile::sanitize_display;
use thiserror::Error;

/// Largest body we accept. Firmware images are tens of KiB and API replies a few KiB; anything
/// near this is not what we asked for.
const MAX_BODY: u64 = 4 << 20;
const TIMEOUT: Duration = Duration::from_secs(30);

/// A failed GET.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("GET {url}: {reason}")]
pub struct HttpError {
    /// The URL requested.
    pub url: String,
    /// What went wrong (status, DNS, TLS, timeout, size limit).
    pub reason: String,
}

/// The one HTTP operation this crate needs. Behind a trait so tests never touch the network.
pub trait HttpClient {
    /// Fetches `url` and returns the body of a 2xx reply.
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError>;
}

/// The `User-Agent` we send: says who we are and where to complain.
pub fn user_agent() -> String {
    format!("euc-studio/{} (+https://github.com/tarikbc/euc-studio)", env!("CARGO_PKG_VERSION"))
}

/// Blocking HTTPS client (ureq with rustls). Plain HTTP is refused.
#[derive(Debug, Clone)]
pub struct UreqClient {
    agent: ureq::Agent,
}

impl UreqClient {
    /// A client with our `User-Agent`, a 30 s timeout per request and HTTPS only.
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .user_agent(user_agent())
            .timeout_global(Some(TIMEOUT))
            .https_only(true)
            .build();
        Self { agent: ureq::Agent::new_with_config(config) }
    }
}

impl Default for UreqClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient for UreqClient {
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        // Error text can quote the server (a redirect target, for one).
        let error = |e: ureq::Error| HttpError { url: url.to_owned(), reason: sanitize_display(&e.to_string()) };
        let mut response = self.agent.get(url).call().map_err(error)?;
        response.body_mut().with_config().limit(MAX_BODY).read_to_vec().map_err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_names_the_project() {
        let expected = format!("euc-studio/{} (+https://github.com/tarikbc/euc-studio)", env!("CARGO_PKG_VERSION"));
        assert_eq!(user_agent(), expected);
    }

    #[test]
    fn plain_http_is_refused_before_any_connection() {
        let err = UreqClient::new().get("http://example.invalid/x.bin").unwrap_err();
        assert_eq!(err.url, "http://example.invalid/x.bin");
        assert!(err.reason.to_lowercase().contains("https"), "{err}");
    }
}
