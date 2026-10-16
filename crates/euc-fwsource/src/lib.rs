//! Stock firmware on the user's machine: fetched from the vendor, verified, cached.
//!
//! The public repo never contains vendor binaries. Instead each wheel profile pins the sha256 of
//! every official build, and this crate gets the bytes from where the vendor publishes them
//! (Begode's update server, the same one the official app uses) or from a file the user already
//! has. Bytes that match a pin are cached by digest and marked verified; anything else is
//! reported and only kept, marked unverified, when the user asks.
//!
//! HTTP sits behind [`HttpClient`] so tests run offline; [`UreqClient`] is the real one.

pub mod begode;
mod cache;
mod fetch;
mod http;

use std::io;
use std::path::PathBuf;

use thiserror::Error;

pub use cache::{CachedImage, FirmwareCache, Provenance, Trust};
pub use fetch::{
    FetchOptions, FetchReport, FetchStatus, Fetched, ImportStatus, Imported, Listing, ListingResult, fetch, import,
};
pub use http::{HttpClient, HttpError, UreqClient, user_agent};

/// Why a firmware operation failed.
#[derive(Debug, Error)]
pub enum FwError {
    /// A request failed.
    #[error(transparent)]
    Http(#[from] HttpError),
    /// The update API answered with something we do not understand.
    #[error("unexpected reply from {url}: {reason}")]
    Api {
        /// The URL queried.
        url: String,
        /// What was wrong.
        reason: String,
    },
    /// The cache could not be read or written.
    #[error("firmware cache {}: {source}", path.display())]
    Cache {
        /// The file or folder involved.
        path: PathBuf,
        /// The OS error.
        source: io::Error,
    },
}

#[cfg(test)]
mod testing {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use crate::{HttpClient, HttpError};

    /// Serves fixed bodies by URL and answers 404 for anything else. Records every request.
    #[derive(Default)]
    pub(crate) struct FakeHttp {
        routes: HashMap<String, Vec<u8>>,
        requests: RefCell<Vec<String>>,
    }

    impl FakeHttp {
        pub(crate) fn with(mut self, url: &str, body: &[u8]) -> Self {
            self.routes.insert(url.to_owned(), body.to_vec());
            self
        }

        pub(crate) fn requests(&self) -> Vec<String> {
            self.requests.borrow().clone()
        }
    }

    impl HttpClient for FakeHttp {
        fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
            self.requests.borrow_mut().push(url.to_owned());
            self.routes
                .get(url)
                .cloned()
                .ok_or_else(|| HttpError { url: url.to_owned(), reason: "http status: 404".to_owned() })
        }
    }
}
