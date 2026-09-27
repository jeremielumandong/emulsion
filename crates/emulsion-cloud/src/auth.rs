use crate::{Account, Provider, Result, http::{self, Client}, id, now, store::{Store, atomic}};
use anyhow::{Context, bail, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::{Read, Write}, net::TcpListener, path::Path, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};

pub const CALLBACK: &str = "http://127.0.0.1:53682/callback";
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ClientConfig {
    pub client_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client_secret: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Config { #[serde(default)] pub clients: BTreeMap<Provider, ClientConfig> }
impl Config {
    pub fn load(root: &Path) -> Result<Self> {
        match std::fs::read(root.join("clients.json")) {
            Ok(bytes) => { ensure!(bytes.len() < 128 * 1024, "Registration file is too large"); Ok(serde_json::from_slice(&bytes)?) },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()), Err(e) => Err(e.into()),
        }
    }
    /// Accept Google's downloaded Desktop JSON or an Emulsion clients.json.
    pub fn import(store: &Store, path: &Path) -> Result<()> {
        let f = std::fs::File::open(path)?; ensure!(f.metadata()?.len() < 128 * 1024, "Registration file is too large");
        let value: Value = serde_json::from_reader(f)?;
        let mut config = Self::load(&store.root)?;
        if let Some(installed) = value.get("installed") {
            let c = ClientConfig { client_id: http::field(installed, "client_id")?, client_secret: installed.get("client_secret").and_then(Value::as_str).unwrap_or("").into() };
            config.clients.insert(Provider::GoogleDrive, c.clone()); config.clients.insert(Provider::GooglePhotos, c);
        } else { let imported: Self = serde_json::from_value(value)?; ensure!(!imported.clients.is_empty(), "No provider registrations found"); config.clients.extend(imported.clients); }
        for c in config.clients.values() { ensure!(!c.client_id.trim().is_empty() && c.client_id.len() < 1024 && c.client_secret.len() < 1024, "Invalid client registration"); }
        crate::store::private_dir(&store.root)?;
        atomic(&store.root.join("clients.json"), |f| { serde_json::to_writer_pretty(f, &config)?; Ok(()) })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens { pub access_token: String, pub refresh_token: Option<String>, pub expires_at: u64 }
type SessionTokens = BTreeMap<String, Tokens>;
static SESSIONS: OnceLock<Mutex<SessionTokens>> = OnceLock::new();
fn sessions() -> &'static Mutex<SessionTokens> { SESSIONS.get_or_init(Default::default) }
fn key(account: &Account) -> String { format!("{}:{}", account.provider.key(), account.id) }
fn entry(account: &Account) -> Result<keyring::Entry> { keyring::Entry::new("org.emulsion.cloud", &key(account)).map_err(|_| anyhow::anyhow!("Operating system credential store unavailable")) }
pub fn save(account: &Account, tokens: &Tokens) -> bool {
    sessions().lock().unwrap().insert(key(account), tokens.clone());
    entry(account).and_then(|e| e.set_password(&serde_json::to_string(tokens)?).map_err(Into::into)).is_ok()
}
pub fn forget(account: &Account) -> Result<()> {
    // Attempt removal before changing UI connection state. Never silently retain persistent credentials.
    if account.persistent_credentials { entry(account)?.delete_credential().map_err(|_| anyhow::anyhow!("Could not remove saved credentials; unlock your credential store and retry"))?; }
    sessions().lock().unwrap().remove(&key(account)); Ok(())
}
pub fn access(store: &Store, account: &Account) -> Result<String> {
    let cached = sessions().lock().unwrap().get(&key(account)).cloned();
    let mut tokens: Tokens = match cached {
        Some(t) => t,
        None => serde_json::from_str(&entry(account)?.get_password().map_err(|_| anyhow::anyhow!("Reconnect this account to resume cloud work"))?).context("Saved credentials are invalid; reconnect")?,
    };
    if tokens.expires_at <= now() + 60 {
        let config = Config::load(&store.root)?;
        let client = config.clients.get(&account.provider).context("Provider registration missing")?;
        let refresh = tokens.refresh_token.as_ref().context("Session expired; reconnect this account")?;
        let mut fields = vec![("grant_type", "refresh_token".to_string()), ("refresh_token", refresh.clone()), ("client_id", client.client_id.clone())];
        if !client.client_secret.is_empty() { fields.push(("client_secret", client.client_secret.clone())); }
        let mut refreshed = exchange(account.provider, &fields)?;
        if refreshed.refresh_token.is_none() { refreshed.refresh_token = tokens.refresh_token; }
        tokens = refreshed;
        if account.persistent_credentials { save(account, &tokens); } else { sessions().lock().unwrap().insert(key(account), tokens.clone()); }
    }
    Ok(tokens.access_token)
}
fn endpoints(provider: Provider) -> (&'static str, &'static str, &'static str) {
    match provider {
        Provider::GoogleDrive => ("https://accounts.google.com/o/oauth2/v2/auth", "https://oauth2.googleapis.com/token", "openid email https://www.googleapis.com/auth/drive.file"),
        Provider::GooglePhotos => ("https://accounts.google.com/o/oauth2/v2/auth", "https://oauth2.googleapis.com/token", "openid email https://www.googleapis.com/auth/photospicker.mediaitems.readonly"),
        Provider::Dropbox => ("https://www.dropbox.com/oauth2/authorize", "https://api.dropboxapi.com/oauth2/token", "account_info.read files.metadata.read files.content.read files.content.write"),
        Provider::OneDrive => ("https://login.microsoftonline.com/common/oauth2/v2.0/authorize", "https://login.microsoftonline.com/common/oauth2/v2.0/token", "offline_access User.Read Files.ReadWrite.AppFolder"),
    }
}
fn exchange(provider: Provider, fields: &[(&str, String)]) -> Result<Tokens> {
    let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(fields.iter().map(|(k,v)| (*k,v.as_str()))).finish();
    let value = Client::default().send("POST", endpoints(provider).1, None, &[("Content-Type", "application/x-www-form-urlencoded".into())], body.as_bytes())?.json()?;
    Ok(Tokens { access_token: http::field(&value, "access_token")?, refresh_token: value.get("refresh_token").and_then(Value::as_str).map(str::to_owned), expires_at: now() + value.get("expires_in").and_then(Value::as_u64).unwrap_or(3600).min(86400) })
}
pub struct PendingLogin {
    listener: TcpListener,
    provider: Provider,
    client: ClientConfig,
    state: String,
    verifier: String,
    pub url: String,
    pub cancelled: Arc<AtomicBool>,
}
impl PendingLogin {
    pub fn start(provider: Provider, client: ClientConfig) -> Result<Self> {
        ensure!(!client.client_id.is_empty(), "Import provider registration first");
        let listener = TcpListener::bind("127.0.0.1:53682").context("Sign-in callback port is busy; finish other sign-ins and retry")?;
        listener.set_nonblocking(true)?;
        let state = format!("{}{}", id(), id()); let verifier = format!("{}{}", id(), id());
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let (auth, _, scope) = endpoints(provider);
        let mut params = vec![("client_id", client.client_id.as_str()), ("response_type", "code"), ("redirect_uri", CALLBACK), ("scope", scope), ("state", state.as_str()), ("code_challenge", challenge.as_str()), ("code_challenge_method", "S256")];
        match provider { Provider::GoogleDrive | Provider::GooglePhotos => { params.push(("access_type", "offline")); params.push(("prompt", "consent")); }, Provider::Dropbox => params.push(("token_access_type", "offline")), Provider::OneDrive => {} }
        let url = http::query(auth, &params);
        Ok(Self { listener, provider, client, state, verifier, url, cancelled: Arc::new(AtomicBool::new(false)) })
    }
    pub fn finish(self) -> Result<(Account, Tokens)> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(300) && !self.cancelled.load(Ordering::Relaxed) {
            match self.listener.accept() {
                Ok((mut stream, addr)) => {
                    ensure!(addr.ip().is_loopback(), "Invalid sign-in callback");
                    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                    let mut request = Vec::new(); let mut b = [0u8; 1];
                    while request.len() < 8192 { if stream.read(&mut b).unwrap_or(0) == 0 { break; } request.push(b[0]); if request.ends_with(b"\r\n\r\n") { break; } }
                    let request = String::from_utf8_lossy(&request);
                    let target = request.lines().next().and_then(|line| line.strip_prefix("GET ")).and_then(|rest| rest.split_whitespace().next()).unwrap_or("");
                    let code = callback_code(target, &self.state);
                    let ok = code.is_ok();
                    let message = if ok { "Sign-in received. You can return to Emulsion." } else { "This sign-in response was rejected. Return to Emulsion and try again." };
                    let _ = write!(stream, "HTTP/1.1 {}\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}", if ok { "200 OK" } else { "400 Bad Request" }, message.len(), message);
                    let Ok(code) = code else { continue; };
                    let mut fields = vec![("grant_type", "authorization_code".into()), ("code", code), ("redirect_uri", CALLBACK.into()), ("client_id", self.client.client_id), ("code_verifier", self.verifier)];
                    if !self.client.client_secret.is_empty() { fields.push(("client_secret", self.client.client_secret)); }
                    let tokens = exchange(self.provider, &fields)?;
                    let c = Client::default();
                    let (account_id, label) = match self.provider {
                        Provider::GoogleDrive | Provider::GooglePhotos => { let v = c.json("GET", "https://openidconnect.googleapis.com/v1/userinfo", Some(&tokens.access_token), None)?; (http::field(&v, "sub")?, http::field(&v, "email")?) },
                        Provider::Dropbox => { let v = c.json("POST", "https://api.dropboxapi.com/2/users/get_current_account", Some(&tokens.access_token), Some(&Value::Null))?; (http::field(&v, "account_id")?, http::field(&v, "email")?) },
                        Provider::OneDrive => { let v = c.json("GET", "https://graph.microsoft.com/v1.0/me", Some(&tokens.access_token), None)?; (http::field(&v, "id")?, http::field(&v, "displayName")?) },
                    };
                    return Ok((Account { provider: self.provider, id: account_id, label, root: String::new(), persistent_credentials: false }, tokens));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
                Err(_) => bail!("Sign-in callback failed"),
            }
        }
        bail!("Sign-in cancelled or timed out")
    }
}
fn callback_code(target: &str, state: &str) -> Result<String> {
    ensure!(target.starts_with("/callback?"), "Wrong callback path");
    let url = url::Url::parse(&format!("http://127.0.0.1{target}"))?;
    let pairs: Vec<_> = url.query_pairs().collect();
    ensure!(pairs.iter().filter(|(k,_)| k == "state").count() == 1 && pairs.iter().any(|(k,v)| k == "state" && v == state), "Sign-in state mismatch");
    ensure!(!pairs.iter().any(|(k,_)| k == "error"), "Sign-in declined");
    ensure!(pairs.iter().filter(|(k,_)| k == "code").count() == 1, "Invalid sign-in code");
    Ok(pairs.iter().find(|(k,_)| k == "code").filter(|(_,v)| !v.is_empty()).context("Missing sign-in code")?.1.to_string())
}
pub fn example_config() -> Value { json!({"clients": {"dropbox": {"client_id": "YOUR_DROPBOX_APP_KEY"}, "one_drive": {"client_id": "YOUR_MICROSOFT_APPLICATION_ID"}}}) }

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn callback_requires_matching_unique_state_and_code() {
        assert_eq!(callback_code("/callback?code=a%2Bb&state=secret", "secret").unwrap(), "a+b");
        for s in ["/callback?code=a&state=wrong", "/callback?code=a&state=secret&state=secret", "/callback?code=a&code=b&state=secret", "/other?code=a&state=secret", "/callback?error=denied&state=secret"] { assert!(callback_code(s, "secret").is_err()); }
    }
    #[test] fn import_google_desktop_credentials_for_both_features() {
        let dir = tempfile::tempdir().unwrap(); let source = dir.path().join("google.json");
        std::fs::write(&source, r#"{"installed":{"client_id":"desktop-id","client_secret":"desktop-secret"}}"#).unwrap();
        let store = Store::new(dir.path().join("cloud")); Config::import(&store, &source).unwrap();
        let config = Config::load(&store.root).unwrap(); assert_eq!(config.clients.len(), 2); assert_eq!(config.clients[&Provider::GooglePhotos].client_id, "desktop-id");
    }
}
