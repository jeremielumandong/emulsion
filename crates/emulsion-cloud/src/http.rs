//! HTTP errors deliberately omit response bodies and bearer/preauthenticated URLs.
use crate::Result;
use anyhow::{bail, ensure};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use url::Url;

static COOLDOWNS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

pub struct Response {
    pub status: u16,
    pub headers: ureq::http::HeaderMap,
    pub body: Vec<u8>,
}
impl Response {
    pub fn json(&self) -> Result<Value> {
        self.success()?;
        serde_json::from_slice(&self.body)
            .map_err(|_| anyhow::anyhow!("Provider returned invalid JSON"))
    }
    pub fn success(&self) -> Result<()> {
        if !(200..300).contains(&self.status)
            && let Some(message) = provider_error(&self.body)
        {
            bail!("{message} (HTTP {})", self.status);
        }
        status(self.status)
    }
    pub fn header(&self, key: &str) -> Result<String> {
        Ok(self
            .headers
            .get(key)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow::anyhow!("Provider response is missing a required header"))?
            .to_owned())
    }
}
// Never display provider messages, descriptions, URLs or arbitrary error codes:
// they can contain credentials or account details. Translate only known codes.
fn provider_error(body: &[u8]) -> Option<&'static str> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let error = value.get("error")?;
    let mut reasons = vec![];
    if let Some(code) = error.as_str() {
        reasons.push(code);
    }
    for list in ["errors", "details"] {
        if let Some(items) = error.get(list).and_then(Value::as_array) {
            reasons.extend(items.iter().filter_map(|item| item["reason"].as_str()));
        }
    }
    for reason in reasons {
        let message = match reason {
            "accessNotConfigured" | "SERVICE_DISABLED" => {
                "The Google API is disabled for this app registration. Enable the required API in the same Google Cloud project as the Desktop client, wait a few minutes, then reconnect"
            }
            "insufficientPermissions" | "ACCESS_TOKEN_SCOPE_INSUFFICIENT" => {
                "Required permission was not granted. Reconnect and allow the requested access on the consent screen"
            }
            "storageQuotaExceeded" => "Cloud storage is full",
            "invalid_client" | "deleted_client" => {
                "The provider rejected this app registration. Import the current Desktop client JSON and reconnect"
            }
            "invalid_grant" => {
                "The sign-in code or refresh token expired or was rejected. Start a fresh connection in Emulsion"
            }
            "invalid_scope" => "The app requested a permission this provider does not support",
            "access_denied" => {
                "Sign-in access was declined. Reconnect and allow access; test registrations must include your account as a test user"
            }
            _ => continue,
        };
        return Some(message);
    }
    None
}

fn transport_error(error: ureq::Error) -> anyhow::Error {
    anyhow::anyhow!(match error {
        ureq::Error::HostNotFound =>
            "Cannot resolve the provider hostname; check your internet connection and DNS",
        ureq::Error::Timeout(_) =>
            "The provider connection timed out; check your network and retry",
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) =>
            "Cannot establish a secure connection to the provider; check your system clock and network certificate settings",
        ureq::Error::InvalidProxyUrl | ureq::Error::ConnectProxyFailed(_) =>
            "Cannot connect through the configured proxy; check your proxy settings",
        _ => "Cloud connection interrupted; check your internet connection and retry",
    })
}
pub fn status(code: u16) -> Result<()> {
    if (200..300).contains(&code) {
        return Ok(());
    }
    bail!(match code {
        401 => "Sign-in expired; reconnect this account".to_string(),
        403 => "Provider denied access; check permissions and storage quota".into(),
        404 => "Cloud item is no longer available".into(),
        409 | 412 => "Cloud item changed; refresh and retry".into(),
        429 => "Provider rate limit reached; retry later".into(),
        507 => "Cloud storage is full".into(),
        _ => format!("Cloud request failed (HTTP {code})"),
    })
}
#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
}
impl Default for Client {
    fn default() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .max_redirects(0)
                .timeout_global(Some(Duration::from_secs(90)))
                .build()
                .into(),
        }
    }
}
impl Client {
    fn response(
        &self,
        method: &str,
        url: &str,
        token: Option<&str>,
        headers: &[(&str, String)],
        body: &[u8],
    ) -> Result<ureq::http::Response<ureq::Body>> {
        let target = ensure_https(url)?;
        let host = target.host_str().unwrap_or("").to_owned();
        let cooldowns = COOLDOWNS.get_or_init(Default::default);
        if let Some(until) = cooldowns.lock().unwrap().get(&host).copied() {
            ensure!(
                until <= Instant::now(),
                "Provider requested a delay; automatic sync will retry later"
            );
        }
        let mut request = ureq::http::Request::builder().method(method).uri(url);
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let request = request
            .body(body)
            .map_err(|_| anyhow::anyhow!("Invalid cloud request"))?;
        let response = self.agent.run(request).map_err(transport_error)?;
        if matches!(response.status().as_u16(), 429 | 503) {
            let seconds = response
                .headers()
                .get("retry-after")
                .and_then(|s| s.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60)
                .clamp(1, 7 * 86400);
            cooldowns
                .lock()
                .unwrap()
                .insert(host, Instant::now() + Duration::from_secs(seconds));
        }
        Ok(response)
    }
    pub fn send(
        &self,
        method: &str,
        url: &str,
        token: Option<&str>,
        headers: &[(&str, String)],
        body: &[u8],
    ) -> Result<Response> {
        let response = self.response(method, url, token, headers, body)?;
        let code = response.status().as_u16();
        let headers = response.headers().clone();
        let mut data = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut data)
            .map_err(|_| anyhow::anyhow!("Cloud response interrupted"))?;
        ensure!(
            data.len() <= 16 * 1024 * 1024,
            "Cloud response is too large"
        );
        Ok(Response {
            status: code,
            headers,
            body: data,
        })
    }
    pub fn json(
        &self,
        method: &str,
        url: &str,
        token: Option<&str>,
        value: Option<&Value>,
    ) -> Result<Value> {
        let body = value
            .map(serde_json::to_vec)
            .transpose()?
            .unwrap_or_default();
        self.send(
            method,
            url,
            token,
            &[("Content-Type", "application/json".into())],
            &body,
        )?
        .json()
    }
    pub fn download(
        &self,
        method: &str,
        url: &str,
        token: Option<&str>,
        headers: &[(&str, String)],
        out: &mut impl Write,
        max: u64,
    ) -> Result<u64> {
        let mut url = url.to_string();
        let mut bearer = token;
        for _ in 0..6 {
            let response = self.response(method, &url, bearer, headers, &[])?;
            if response.status().is_redirection() {
                let next = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| anyhow::anyhow!("Invalid download redirect"))?
                    .to_string();
                trusted_download(&next)?;
                if Url::parse(&next)?.host_str() != Url::parse(&url)?.host_str() {
                    bearer = None;
                }
                url = next;
                continue;
            }
            status(response.status().as_u16())?;
            let n = std::io::copy(&mut response.into_body().into_reader().take(max + 1), out)
                .map_err(|_| anyhow::anyhow!("Cloud download interrupted"))?;
            ensure!(n <= max, "Cloud download exceeds size limit");
            return Ok(n);
        }
        bail!("Too many cloud download redirects")
    }
}
pub fn ensure_https(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| anyhow::anyhow!("Invalid provider URL"))?;
    ensure!(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none_or(|p| p == 443),
        "Provider URL must use HTTPS"
    );
    Ok(url)
}
pub fn trusted_download(value: &str) -> Result<()> {
    let url = ensure_https(value)?;
    let host = url.host_str().unwrap_or("");
    ensure!(
        [
            "googleapis.com",
            "googleusercontent.com",
            "1drv.com",
            "onedrive.com",
            "sharepoint.com",
            "sharepoint.us",
            "storage.live.com",
            "files.1drv.com",
            "dropboxusercontent.com"
        ]
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}"))),
        "Unrecognized provider transfer host"
    );
    Ok(())
}
pub fn query(base: &str, pairs: &[(&str, &str)]) -> String {
    let mut url = Url::parse(base).expect("static API endpoint");
    url.query_pairs_mut().extend_pairs(pairs.iter().copied());
    url.into()
}
pub fn segment(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}
pub fn field(value: &Value, name: &str) -> Result<String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("Provider omitted required field {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_errors_explain_setup_without_exposing_response_details() {
        for reason in ["accessNotConfigured", "SERVICE_DISABLED"] {
            let response = Response {
                status: 403,
                headers: Default::default(),
                body: serde_json::to_vec(&serde_json::json!({"error": {
                    "message": "private response",
                    "details": [{"reason": reason, "metadata": {"url": "private URL"}}]
                }}))
                .unwrap(),
            };
            let message = response.json().unwrap_err().to_string();
            assert!(message.contains("Google API is disabled"));
            assert!(!message.contains("private"));
        }
        let response = Response {
            status: 400,
            headers: Default::default(),
            body: br#"{"error":"invalid_grant","error_description":"private token"}"#.to_vec(),
        };
        let message = response.json().unwrap_err().to_string();
        assert!(message.contains("fresh connection"));
        assert!(!message.contains("private token"));
        let unknown = Response {
            body: br#"{"error":"private code"}"#.to_vec(),
            ..response
        };
        assert_eq!(
            unknown.json().unwrap_err().to_string(),
            "Cloud request failed (HTTP 400)"
        );
        let success = Response {
            status: 200,
            ..unknown
        };
        assert!(success.json().is_ok());
    }

    #[test]
    fn transport_errors_never_echo_credentials_or_urls() {
        let error = transport_error(ureq::Error::ConnectProxyFailed("private password".into()));
        assert!(error.to_string().contains("proxy settings"));
        assert!(!error.to_string().contains("private password"));
    }

    #[test]
    fn bearer_download_hosts_are_restricted() {
        for bad in [
            "http://googleusercontent.com/p",
            "https://googleusercontent.com.evil.test/p",
            "https://user@googleusercontent.com/p",
            "https://127.0.0.1/p",
            "https://googleusercontent.com:444/p",
        ] {
            assert!(trusted_download(bad).is_err());
        }
        assert!(trusted_download("https://lh3.googleusercontent.com/p/x").is_ok());
        assert!(trusted_download("https://tenant.sharepoint.com/files").is_ok());
    }
}
