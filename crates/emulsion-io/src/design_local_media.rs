//! Bounded portable media assets served only to the lifetime-owned local player.
use super::*;
use emulsion_core::design::media::{LocalMedia, LocalMediaKind, MAX_LOCAL_ASSET_BYTES};
use std::path::Path;

pub fn read_local(path: &Path) -> Result<LocalMedia, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_LOCAL_ASSET_BYTES as u64 {
        return Err("Local media must be at most 32 MiB.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_LOCAL_ASSET_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    LocalMedia::from_bytes(
        path.file_name()
            .and_then(|s| s.to_str())
            .ok_or("Invalid media filename")?
            .into(),
        bytes,
    )
}
impl PlayerServer {
    pub fn start_local(media: &LocalMedia) -> io::Result<Self> {
        media
            .validate()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let authority = listener.local_addr()?.to_string();
        let mut random = [0u8; 24];
        getrandom::fill(&mut random).map_err(|e| io::Error::other(e.to_string()))?;
        let token = random
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>();
        let path = format!("/{token}/player");
        let asset = format!("/{token}/asset");
        let url = format!("http://{authority}{path}");
        let tag = if media.kind == LocalMediaKind::Audio {
            "audio"
        } else {
            "video"
        };
        // Only validated numeric values and our random route are interpolated. Never source HTML from the document.
        let html = format!(
            r#"<!doctype html><meta charset="utf-8"><title>Emulsion media</title><style>html,body{{margin:0;height:100%;background:#16181d;color:white;display:grid;place-items:center}}video{{width:100vw;height:100vh;object-fit:contain}}audio{{width:95vw}}p{{font:14px sans-serif;padding:12px}}</style><{tag} id="media" controls autoplay playsinline preload="metadata" src="{asset}"></{tag}><p id="error" hidden></p><script>const m=document.getElementById('media'),error=document.getElementById('error');const start={},end={},repeat={};m.volume={};let ready=false,completed=false;function fail(s){{error.hidden=false;error.textContent=s;}}m.addEventListener('loadedmetadata',()=>{{if(start>=m.duration){{m.pause();fail('The trim start is beyond this media duration. Edit its trim settings.');return;}}ready=true;m.currentTime=start;m.play().catch(()=>{{}});}});m.addEventListener('seeking',()=>{{if(ready&&m.currentTime<start)m.currentTime=start;}});function finish(){{if(completed)return;if(repeat){{m.currentTime=start;m.play().catch(()=>{{}});}}else {{completed=true;m.pause();if(end!==null&&end<m.duration)m.currentTime=end;fetch("{path}/finished").catch(()=>{{}});}}}}m.addEventListener('timeupdate',()=>{{if(end!==null&&m.currentTime>=end)finish();}});setInterval(()=>{{if(ready&&!m.paused&&end!==null&&m.currentTime>=end)finish();}},20);m.addEventListener('ended',finish);m.addEventListener('error',()=>fail('This system media runtime cannot decode this file. Try MP4/H.264 video or MP3/WAV audio.'));</script>"#,
            f64::from(media.trim_start_ms) / 1000.,
            media
                .trim_end_ms
                .map(|v| (f64::from(v) / 1000.).to_string())
                .unwrap_or("null".into()),
            media.looping,
            media.volume
        );
        let html = format!("{html}{}", control::script(&path, false));
        let controls: control::SharedControls = Default::default();
        let worker_controls = controls.clone();
        let media = media.clone();
        let finished = Arc::new(AtomicBool::new(false));
        let worker_finished = finished.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("local-media-player".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                            let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
                            let _ = serve_local(
                                &mut stream,
                                LocalRequest {
                                    authority: &authority,
                                    path: &path,
                                    asset: &asset,
                                    html: &html,
                                    media: &media,
                                    finished: &worker_finished,
                                    controls: &worker_controls,
                                },
                            );
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
            finished,
            controls,
        })
    }
}
struct LocalRequest<'a> {
    authority: &'a str,
    path: &'a str,
    asset: &'a str,
    html: &'a str,
    media: &'a LocalMedia,
    finished: &'a AtomicBool,
    controls: &'a control::SharedControls,
}
fn serve_local(stream: &mut TcpStream, request: LocalRequest<'_>) -> io::Result<()> {
    let LocalRequest {
        authority,
        path,
        asset,
        html,
        media,
        finished,
        controls,
    } = request;
    let mut bytes = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_millis(250);
    loop {
        if bytes.len() >= 8192 || std::time::Instant::now() >= deadline {
            return local_response(
                stream,
                "431 Request Header Fields Too Large",
                "text/plain",
                &[],
                false,
                None,
            );
        }
        let mut block = [0; 1024];
        let n = stream.read(&mut block)?;
        if n == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&block[..n]);
        if bytes.windows(4).any(|v| v == b"\r\n\r\n") {
            break;
        }
    }
    let request = std::str::from_utf8(&bytes).unwrap_or("");
    let mut lines = request.split("\r\n");
    let first = lines.next().unwrap_or("").split(' ').collect::<Vec<_>>();
    let headers = lines
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .collect::<Vec<_>>();
    let hosts = headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("host"))
        .map(|(_, v)| v.trim())
        .collect::<Vec<_>>();
    if first.len() != 3 || !matches!(first[2], "HTTP/1.0" | "HTTP/1.1") || hosts != [authority] {
        return local_response(stream, "400 Bad Request", "text/plain", &[], false, None);
    }
    let head = first[0] == "HEAD";
    if !head && first[0] != "GET" {
        return local_response(
            stream,
            "405 Method Not Allowed",
            "text/plain",
            &[],
            false,
            None,
        );
    }
    if let Some((status, body)) = control::control_response(first[1], path, head, controls) {
        return local_response(
            stream,
            &status,
            "application/json",
            body.as_bytes(),
            head,
            None,
        );
    }
    if first[1] == format!("{path}/finished") {
        finished.store(true, Ordering::Release);
        return local_response(stream, "200 OK", "text/plain", &[], head, None);
    }
    if first[1] == path {
        return local_response(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            html.as_bytes(),
            head,
            None,
        );
    }
    if first[1] != asset {
        return local_response(stream, "404 Not Found", "text/plain", &[], head, None);
    }
    let ranges = headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("range"))
        .map(|(_, v)| v.trim())
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        return local_response(stream, "200 OK", &media.mime, &media.bytes, head, None);
    }
    let len = media.bytes.len();
    let range = if ranges.len() == 1 {
        byte_range(ranges[0], len)
    } else {
        None
    };
    match range {
        Some((start, end)) => local_response(
            stream,
            "206 Partial Content",
            &media.mime,
            &media.bytes[start..=end],
            head,
            Some(format!("bytes {start}-{end}/{len}")),
        ),
        None => local_response(
            stream,
            "416 Range Not Satisfiable",
            "text/plain",
            &[],
            head,
            Some(format!("bytes */{len}")),
        ),
    }
}
fn byte_range(value: &str, len: usize) -> Option<(usize, usize)> {
    let (a, b) = value.strip_prefix("bytes=")?.split_once('-')?;
    if a.is_empty() {
        let n = b.parse::<usize>().ok()?.min(len);
        return (n > 0).then_some((len - n, len - 1));
    }
    let start = a.parse::<usize>().ok()?;
    let end = if b.is_empty() {
        len - 1
    } else {
        b.parse::<usize>().ok()?.min(len - 1)
    };
    (start <= end && start < len).then_some((start, end))
}
fn local_response(
    stream: &mut TcpStream,
    status: &str,
    mime: &str,
    body: &[u8],
    head: bool,
    range: Option<String>,
) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; media-src 'self'; connect-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'\r\n",
        body.len()
    )?;
    if let Some(range) = range {
        write!(stream, "Content-Range: {range}\r\n")?
    }
    stream.write_all(b"\r\n")?;
    if !head {
        stream.write_all(body)?
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn design_local_ranges_are_bounded() {
        assert_eq!(byte_range("bytes=2-5", 10), Some((2, 5)));
        assert_eq!(byte_range("bytes=8-", 10), Some((8, 9)));
        assert_eq!(byte_range("bytes=-3", 10), Some((7, 9)));
        assert_eq!(byte_range("bytes=10-20", 10), None);
        assert_eq!(byte_range("bytes=1-2,4-5", 10), None);
        assert_eq!(byte_range("bytes=-0", 10), None);
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    fn wave() -> LocalMedia {
        LocalMedia::from_bytes("tone.wav".into(), b"RIFF\0\0\0\0WAVEdata".to_vec()).unwrap()
    }
    fn request(
        server: &PlayerServer,
        route: &str,
        range: Option<&str>,
        host: Option<&str>,
    ) -> Vec<u8> {
        let authority = server
            .url()
            .strip_prefix("http://")
            .unwrap()
            .split('/')
            .next()
            .unwrap();
        let mut stream = TcpStream::connect(authority).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(
            stream,
            "GET {route} HTTP/1.1\r\nHost: {}\r\n",
            host.unwrap_or(authority)
        )
        .unwrap();
        if let Some(range) = range {
            write!(stream, "Range: {range}\r\n").unwrap()
        }
        stream.write_all(b"\r\n").unwrap();
        let mut result = Vec::new();
        stream.read_to_end(&mut result).unwrap();
        result
    }
    #[test]
    fn design_local_media_server_ranges_routes_and_lifecycle() {
        let media = wave();
        let server = PlayerServer::start_local(&media).unwrap();
        let base = server
            .url()
            .strip_prefix("http://")
            .unwrap()
            .split_once('/')
            .unwrap()
            .1;
        let path = format!("/{base}");
        let asset = path.replace("/player", "/asset");
        let page = request(&server, &path, None, None);
        assert!(String::from_utf8_lossy(&page).contains("<audio"));
        let range = request(&server, &asset, Some("bytes=8-11"), None);
        let text = String::from_utf8_lossy(&range);
        assert!(text.starts_with("HTTP/1.1 206"));
        assert!(text.contains("Content-Range: bytes 8-11/16"));
        assert!(range.ends_with(b"WAVE"));
        assert!(
            String::from_utf8_lossy(&request(&server, &asset, Some("bytes=99-"), None))
                .starts_with("HTTP/1.1 416")
        );
        assert!(
            String::from_utf8_lossy(&request(&server, &asset, None, Some("evil.test")))
                .starts_with("HTTP/1.1 400")
        );
        assert!(
            String::from_utf8_lossy(&request(&server, "/etc/passwd", None, None))
                .starts_with("HTTP/1.1 404")
        );
        assert!(!server.finished());
        request(&server, &format!("{path}/finished"), None, None);
        assert!(server.finished());
        let authority = server
            .url()
            .strip_prefix("http://")
            .unwrap()
            .split('/')
            .next()
            .unwrap()
            .to_owned();
        drop(server);
        assert!(TcpStream::connect(authority).is_err());
    }
    #[test]
    fn design_local_media_project_roundtrip_and_keyframes() {
        use emulsion_core::{
            Document, Editor,
            design::media,
            design_keyframes::{self, Easing, Keyframe, Property},
        };
        let mut editor = Editor::new(Document::new(640, 480), None);
        let id = media::insert_local(&mut editor, wave(), (10., 20.), (400., 225.)).unwrap();
        media::update_local(&mut editor, id, 100, Some(1000), 0.5, true).unwrap();
        design_keyframes::set_keyframe(
            &mut editor,
            id,
            Property::Opacity,
            Keyframe {
                time_ms: 0,
                value: 0.5,
                easing: Easing::EaseOut,
            },
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "emulsion-local-media-{}-{}.emu",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
    }
}
