//! Windows' installed .NET/GDI print stack, driven through a fixed, bundled
//! PowerShell helper. User strings travel only as JSON, never executable code.
use super::*;
use serde_json::json;
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn run(request: serde_json::Value, cancel: &AtomicBool) -> Result<serde_json::Value> {
    use std::os::windows::process::CommandExt;
    let dir = tempfile::tempdir()?;
    let script = dir.path().join("print.ps1");
    let input = dir.path().join("request.json");
    std::fs::write(&script, format!("\u{feff}{}", include_str!("windows.ps1")))?;
    std::fs::write(&input, serde_json::to_vec(&request)?)?;
    let exe = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .context("Windows system directory is unavailable")?
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut child = Command::new(exe)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .arg("-Request")
        .arg(input)
        .creation_flags(0x08000000)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut b = Vec::new();
        stdout.take(8 * 1024 * 1024).read_to_end(&mut b).map(|_| b)
    });
    let err = std::thread::spawn(move || {
        let mut b = Vec::new();
        stderr.take(1024 * 1024).read_to_end(&mut b).map(|_| b)
    });
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "Print operation stopped; check the system queue before retrying because submission may already have occurred."
            )
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let output = out
        .join()
        .map_err(|_| anyhow::anyhow!("Print helper output failed"))??;
    let error = err
        .join()
        .map_err(|_| anyhow::anyhow!("Print helper error output failed"))??;
    if !status.success() {
        bail!("{}", String::from_utf8_lossy(&error).trim())
    }
    Ok(serde_json::from_slice(&output)?)
}
pub fn discover() -> Result<Vec<Printer>> {
    Ok(serde_json::from_value(run(
        json!({"action":"discover"}),
        &AtomicBool::new(false),
    )?)?)
}
pub fn capabilities(printer: &str) -> Result<Capabilities> {
    Ok(serde_json::from_value(run(
        json!({"action":"capabilities","printer":printer}),
        &AtomicBool::new(false),
    )?)?)
}
pub fn submit(
    printer: &str,
    title: &str,
    sources: &[Source],
    layout: &JobLayout,
    s: &Settings,
    cancel: &AtomicBool,
) -> Result<String> {
    let dir = tempfile::tempdir()?;
    let mut files = vec![];
    for (i, sheet) in layout.sheets.iter().enumerate() {
        canceled(cancel)?;
        let path = dir.path().join(format!("sheet-{i}.png"));
        let max_side = (sheet.width.max(sheet.height) / 25.4
            * if s.production.enabled() {
                s.production.dpi as f64
            } else {
                300.
            })
        .ceil() as u32;
        production::device_image(sources, sheet, s, max_side)?.save(&path)?;
        files.push(path);
    }
    canceled(cancel)?;
    let result = run(
        json!({"action":"submit","printer":printer,"title":title,"paper":s.paper.id,"width":s.paper.width,"height":s.paper.height,"margins":s.paper.margins,"copies":s.copies,"landscape":s.landscape,"managed":s.production.enabled(),"grayscale":s.grayscale,"sides":s.sides,"tray":s.tray,"quality":s.quality,"files":files}),
        cancel,
    )?;
    Ok(result["message"]
        .as_str()
        .context("Missing print result")?
        .into())
}
