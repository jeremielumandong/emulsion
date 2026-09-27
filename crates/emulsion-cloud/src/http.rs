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
        let response = self
            .agent
            .run(request)
            .map_err(|_| anyhow::anyhow!("Cloud connection interrupted; local work is safe"))?;
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
