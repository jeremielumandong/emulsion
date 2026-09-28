//! A lifetime-owned loopback page for the official YouTube iframe player.
//! No video extraction, browser engine or remote code is bundled with documents.
use emulsion_core::design::media::YouTube;
use std::{
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub struct PlayerServer {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl PlayerServer {
    pub fn start(video: &YouTube) -> io::Result<Self> {
        video
            .validate()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let authority = listener.local_addr()?.to_string();
        let origin = format!("http://{authority}");
        let mut random = [0u8; 24];
        getrandom::fill(&mut random).map_err(|e| io::Error::other(e.to_string()))?;
        let token = random
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>();
        let path = format!("/{token}/player");
        let url = format!("{origin}{path}");
        // Both values are validated ASCII; origin comes only from the bound socket.
        // Native view bounds enforce the YouTube minimum of 200 × 200 pixels.
        let html = format!(
            r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>YouTube video</title><style>html,body{{margin:0;width:100%;height:100%;background:#16181d;overflow:hidden}}iframe{{display:block;border:0;width:100%;height:100%;min-width:200px;min-height:200px}}</style></head><body><iframe title="YouTube video" src="https://www.youtube-nocookie.com/embed/{}?start={}&amp;controls=1&amp;playsinline=1&amp;origin={}" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share" referrerpolicy="strict-origin-when-cross-origin" allowfullscreen></iframe></body></html>"#,
            video.video_id, video.start_seconds, origin
        );
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("youtube-player-page".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                            let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
                            let _ = serve(&mut stream, &path, &authority, &html);
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20))
                        }
                        Err(_) => break,
                    }
                }
            })?;
        Ok(Self {
            url,
            stop,
            worker: Some(worker),
        })
    }
    pub fn url(&self) -> &str {
        &self.url
    }
}
impl Drop for PlayerServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn serve(stream: &mut TcpStream, path: &str, authority: &str, html: &str) -> io::Result<()> {
    let mut bytes = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_millis(250);
    loop {
        if bytes.len() >= 8192 || std::time::Instant::now() >= deadline {
            return response(stream, "431 Request Header Fields Too Large", "", false);
        }
        let mut buffer = [0u8; 1024];
        let n = stream.read(&mut buffer)?;
        if n == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..n]);
        if bytes.windows(4).any(|v| v == b"\r\n\r\n") {
            break;
        }
    }
    let Ok(request) = std::str::from_utf8(&bytes) else {
        return response(stream, "400 Bad Request", "", false);
    };
    let mut lines = request.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let method = first.next().unwrap_or("");
    let requested = first.next().unwrap_or("");
    let version = first.next().unwrap_or("");
    let hosts: Vec<_> = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim())
        .collect();
    if hosts != [authority] || !matches!(version, "HTTP/1.0" | "HTTP/1.1") || first.next().is_some()
    {
        return response(stream, "400 Bad Request", "", false);
    }
    if method != "GET" && method != "HEAD" {
        return response(stream, "405 Method Not Allowed", "", false);
    }
    if requested != path {
        return response(stream, "404 Not Found", "", false);
    }
    response(stream, "200 OK", html, method == "HEAD")
}
fn response(stream: &mut TcpStream, status: &str, body: &str, head: bool) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: strict-origin-when-cross-origin\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; frame-src https://www.youtube-nocookie.com; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n\r\n",
        body.len()
    )?;
    if !head {
        stream.write_all(body.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(server: &PlayerServer, path: Option<&str>, host: Option<&str>) -> String {
        let (authority, route) = server
            .url()
            .strip_prefix("http://")
            .unwrap()
            .split_once('/')
            .unwrap();
        let mut stream = TcpStream::connect(authority).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(
            stream,
            "GET /{} HTTP/1.1\r\nHost: {}\r\n\r\n",
            path.unwrap_or(route),
            host.unwrap_or(authority)
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }
    #[test]
    fn page_is_bounded_to_its_secret_route_and_validated_video() {
        let video =
            emulsion_core::design::media::parse_youtube("https://youtu.be/dQw4w9WgXcQ?t=12")
                .unwrap();
        let server = PlayerServer::start(&video).unwrap();
        let html = request(&server, None, None);
        assert!(html.starts_with("HTTP/1.1 200 OK"));
        assert!(html.contains(
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?start=12&amp;controls=1"
        ));
        assert!(html.contains("Referrer-Policy: strict-origin-when-cross-origin"));
        assert!(request(&server, Some(""), None).starts_with("HTTP/1.1 404"));
        assert!(request(&server, Some("../../player"), None).starts_with("HTTP/1.1 404"));
        assert!(request(&server, None, Some("attacker.example")).starts_with("HTTP/1.1 400"));
        let authority = server
            .url()
            .strip_prefix("http://")
            .unwrap()
            .split_once('/')
            .unwrap()
            .0
            .to_owned();
        drop(server);
        assert!(TcpStream::connect(authority).is_err());
        let mut invalid = video;
        invalid.video_id = "\"><script>bad()".into();
        assert!(PlayerServer::start(&invalid).is_err());
    }
    #[test]
    fn native_file_roundtrips_video_metadata_and_editable_poster() {
        use emulsion_core::{Document, Editor, design::media};
        let mut editor = Editor::new(Document::new(640, 480), None);
        let id = media::insert_youtube(
            &mut editor,
            "https://youtu.be/dQw4w9WgXcQ?t=15",
            (20., 30.),
            (400., 225.),
        )
        .unwrap();
        editor.doc.design.speaker_notes = "Private presenter notes".into();
        editor.doc.design.page_transition = emulsion_core::design_metadata::PageTransition::Slide;
        editor.doc.design.transition_ms = 500;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "emulsion-video-{}-{unique}.emu",
            std::process::id()
        ));
        let project = emulsion_core::project::ProjectEditor::new_project(
            emulsion_core::project::ProjectKind::Design,
            editor.doc.clone(),
        )
        .unwrap()
        .snapshot()
        .unwrap();
        crate::project::write(&project, &path).unwrap();
        let loaded = crate::project::read(&path).unwrap().pages.remove(0).doc;
        std::fs::remove_file(path).unwrap();
        assert_eq!(loaded.design, editor.doc.design);
        assert_eq!(loaded.nodes, editor.doc.nodes);
        assert_eq!(media::bounds(&loaded, id), Some((20., 30., 400., 225.)));
    }
}
