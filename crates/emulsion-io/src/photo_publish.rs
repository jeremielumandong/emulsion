//! Explicit HTTPS/WebDAV publishing of finished exports. Credentials stay in
//! environment/keychain launch configuration, never preset JSON or catalog logs.
use crate::{IoError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path, time::Duration};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub url: String,
    #[serde(default)]
    pub authorization_env: Option<String>,
}
fn bad(s: impl Into<String>) -> IoError {
    IoError::Manifest(s.into())
}
impl Destination {
    pub fn validate(&self) -> Result<()> {
        let uri: ureq::http::Uri = self.url.parse().map_err(|_| bad("Invalid publish URL"))?;
        let local = matches!(uri.host(), Some("127.0.0.1" | "localhost" | "[::1]"));
        if !(uri.scheme_str() == Some("https") || local && uri.scheme_str() == Some("http"))
            || uri.authority().is_none()
            || uri.authority().unwrap().as_str().contains('@')
            || uri.query().is_some()
            || self.url.contains('#')
        {
            return Err(bad(
                "Publish destination must be an HTTPS directory without credentials, query or fragment",
            ));
        }
        if self.authorization_env.as_ref().is_some_and(|s| {
            s.is_empty()
                || s.len() > 100
                || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }) {
            return Err(bad("Invalid publishing credential variable name"));
        }
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(16385)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 16384 {
            return Err(bad("Publish settings too large"));
        }
        let v: Self = serde_json::from_slice(&bytes).map_err(|e| bad(e.to_string()))?;
        v.validate()?;
        Ok(v)
    }
}
pub fn publish(path: &Path, stem: &str, destination: &Destination) -> Result<String> {
    destination.validate()?;
    let digest = crate::raw::source_digest(path)?;
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if !["jpg", "jpeg", "png", "tif", "tiff", "webp"].contains(&ext.as_str()) {
        return Err(bad("Only rendered photo exports can be published"));
    }
    let name: String = stem
        .chars()
        .take(80)
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let url = format!(
        "{}/{}-{}.{}",
        destination.url.trim_end_matches('/'),
        name,
        digest,
        ext
    );
    let size = std::fs::metadata(path)?.len();
    if size > 2 * 1024 * 1024 * 1024 {
        return Err(bad("Publish file exceeds 2 GiB"));
    }
    let authorization = destination
        .authorization_env
        .as_ref()
        .map(|name| {
            std::env::var(name).map_err(|_| bad("Publishing credential variable is not available"))
        })
        .transpose()?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into();
    let mut request = agent
        .put(&url)
        .header("If-None-Match", "*")
        .header("Content-Length", size.to_string())
        .header(
            "Content-Type",
            match ext.as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "png" => "image/png",
                "webp" => "image/webp",
                _ => "image/tiff",
            },
        );
    if let Some(value) = &authorization {
        request = request.header("Authorization", value);
    }
    let response=request.send(std::fs::File::open(path)?).map_err(|_|bad("Publishing request failed; check connection and credentials. Local export is retained."))?;
    if response.status().is_success() {
        return Ok(url);
    }
    if response.status().as_u16() == 412 {
        let mut get = agent.get(&url);
        if let Some(value) = &authorization {
            get = get.header("Authorization", value);
        }
        let mut response = get
            .call()
            .map_err(|_| bad("Cannot verify existing remote photo"))?;
        if response.status().is_success() {
            let mut reader = response.body_mut().as_reader().take(size + 1);
            let mut hash = Sha256::new();
            let mut count = 0u64;
            let mut buf = [0; 65536];
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                count += n as u64;
                hash.update(&buf[..n]);
            }
            let remote: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
            if count == size && remote == digest {
                return Ok(url);
            }
        }
        return Err(bad(
            "Remote filename conflict; existing photo was not overwritten",
        ));
    }
    Err(bad(format!(
        "Publishing returned HTTP {}; local export is retained",
        response.status().as_u16()
    )))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    #[test]
    fn publishing_is_idempotent_and_never_overwrites_remote_files() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut stored = vec![];
            for index in 0..3 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut headers = vec![];
                while !headers.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    socket.read_exact(&mut byte).unwrap();
                    headers.push(byte[0]);
                }
                let head = String::from_utf8(headers).unwrap();
                if index < 2 {
                    assert!(head.starts_with("PUT "));
                    assert!(head.to_ascii_lowercase().contains("if-none-match: *"));
                    let length = head
                        .lines()
                        .find_map(|s| {
                            s.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    let mut body = vec![0; length];
                    socket.read_exact(&mut body).unwrap();
                    if index == 0 {
                        stored = body;
                        socket.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                    } else {
                        assert_eq!(body, stored);
                        socket.write_all(b"HTTP/1.1 412 Precondition Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                    }
                } else {
                    assert!(head.starts_with("GET "));
                    write!(
                        socket,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        stored.len()
                    )
                    .unwrap();
                    socket.write_all(&stored).unwrap();
                }
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        std::fs::write(&path, b"rendered-test-photo").unwrap();
        let dest = Destination {
            url: format!("http://{addr}/photos"),
            authorization_env: None,
        };
        let first = publish(&path, "Photo", &dest).unwrap();
        assert_eq!(publish(&path, "Photo", &dest).unwrap(), first);
        server.join().unwrap();
        assert!(
            Destination {
                url: "https://name:password@example.com/photos".into(),
                authorization_env: None
            }
            .validate()
            .is_err()
        );
    }
}
