//! Begode's public firmware update API, the one the official app uses.
//!
//! `GET /api/version/one/version?firmwareTypeCode=<code>` lists the builds offered for a product;
//! each record's `apkWrap` is a path under `/imgs/`. Older builds stay downloadable at
//! `/imgs/temp/<file>` after the listing moves on.

use serde::{Deserialize, Serialize};

use crate::FwError;
use crate::http::HttpClient;

/// Scheme and host of the update server.
pub const ORIGIN: &str = "https://one-api.begode.com";

/// A build the API lists for a product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerBuild {
    /// Product code it was listed under.
    pub type_code: String,
    /// Path under `/imgs/` (`temp/pwm_<upload ms>.bin`).
    pub path: String,
    /// Short description (`briefIntroduction`), usually the model and firmware name.
    pub title: String,
    /// Release notes (`perfectBug`).
    pub notes: String,
    /// Server-side version name (`versionName`).
    pub version: String,
}

impl ServerBuild {
    /// File name without the folder (`pwm_<upload ms>.bin`).
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// Download URL.
    pub fn url(&self) -> String {
        format!("{ORIGIN}/imgs/{}", self.path)
    }
}

/// Listing URL for a product code.
pub fn listing_url(type_code: &str) -> String {
    format!("{ORIGIN}/api/version/one/version?firmwareTypeCode={type_code}")
}

/// Download URL of any build the server keeps, listed or not.
pub fn file_url(file: &str) -> String {
    format!("{ORIGIN}/imgs/temp/{file}")
}

/// The builds the API lists for `type_code`, newest first as the server orders them.
pub fn list_builds(http: &dyn HttpClient, type_code: &str) -> Result<Vec<ServerBuild>, FwError> {
    let url = listing_url(type_code);
    let body = http.get(&url)?;
    let api = |reason: String| FwError::Api { url: url.clone(), reason };
    let reply: Reply = serde_json::from_slice(&body).map_err(|e| api(format!("not the expected JSON: {e}")))?;
    if reply.code != 0 {
        return Err(api(format!("server answered code {}: {}", reply.code, reply.msg.unwrap_or_default())));
    }
    let records = reply.data.map(|page| page.records).unwrap_or_default();
    records
        .into_iter()
        .map(|record| {
            if !is_safe_path(&record.apk_wrap) {
                return Err(api(format!("refusing file path \"{}\"", record.apk_wrap)));
            }
            Ok(ServerBuild {
                type_code: type_code.to_owned(),
                path: record.apk_wrap,
                title: record.brief_introduction.unwrap_or_default(),
                notes: record.perfect_bug.unwrap_or_default(),
                version: record.version_name.unwrap_or_default(),
            })
        })
        .collect()
}

/// A relative path of plain segments: it is appended to a URL and its last segment names a file.
fn is_safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && !segment.starts_with('.')
                && segment.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        })
}

#[derive(Deserialize)]
struct Reply {
    code: i64,
    msg: Option<String>,
    data: Option<Page>,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    records: Vec<Record>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    apk_wrap: String,
    brief_introduction: Option<String>,
    perfect_bug: Option<String>,
    version_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHttp;

    /// Same shape as a real reply, synthetic values.
    const LISTING: &str = r#"{"code":0,"msg":"success","data":{"records":[
        {"appType":3,"firmwareTypeCode":"TEST01","apkWrap":"temp/pwm_2000.bin","versionName":"02",
         "perfectBug":"1.Update time:20990101","briefIntroduction":"DEMO_FW2"}],"total":"1"}}"#;

    #[test]
    fn parses_a_listing() {
        let http = FakeHttp::default().with(&listing_url("TEST01"), LISTING.as_bytes());
        let builds = list_builds(&http, "TEST01").unwrap();
        assert_eq!(builds.len(), 1);
        let build = &builds[0];
        assert_eq!(build.file_name(), "pwm_2000.bin");
        assert_eq!(build.url(), "https://one-api.begode.com/imgs/temp/pwm_2000.bin");
        assert_eq!(
            (build.title.as_str(), build.notes.as_str(), build.version.as_str()),
            ("DEMO_FW2", "1.Update time:20990101", "02")
        );
        assert_eq!(http.requests(), [listing_url("TEST01")]);
    }

    #[test]
    fn empty_listing_is_no_builds() {
        let http = FakeHttp::default().with(&listing_url("TEST01"), br#"{"code":0,"data":{"records":[]}}"#);
        assert_eq!(list_builds(&http, "TEST01").unwrap(), []);
    }

    #[test]
    fn reports_api_errors() {
        let http = FakeHttp::default()
            .with(&listing_url("A"), br#"{"code":500,"msg":"busy"}"#)
            .with(&listing_url("B"), b"<html>maintenance</html>")
            .with(&listing_url("C"), br#"{"code":0,"data":{"records":[{"apkWrap":"../../etc/passwd"}]}}"#);
        let message = |code| list_builds(&http, code).unwrap_err().to_string();
        assert!(message("A").ends_with("server answered code 500: busy"), "{}", message("A"));
        assert!(message("B").contains("not the expected JSON"), "{}", message("B"));
        assert!(message("C").ends_with("refusing file path \"../../etc/passwd\""), "{}", message("C"));
        assert!(message("D").contains("404"), "{}", message("D"));
    }
}
