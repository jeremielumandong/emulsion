//! Running the external `ffmpeg` and `ffprobe` tools: one place that finds
//! them on PATH, keeps a console window from opening on Windows, turns a
//! missing install into a clear message, and waits with cancel and timeout.
//! Every FFmpeg user (print sources, audio, movie export) goes through here.
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Shown when FFmpeg is not installed.
pub const MISSING: &str = "This needs FFmpeg. Install FFmpeg (ffmpeg and ffprobe) and make sure it is on PATH, then try again.";

/// A command for `program` (`ffmpeg` or `ffprobe`) with stdin closed and,
/// on Windows, no console window.
pub fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

/// Start `command`; a missing program gives [`MISSING`].
pub fn spawn(command: &mut Command) -> anyhow::Result<Child> {
    command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!(MISSING)
        } else {
            anyhow::anyhow!("Cannot start FFmpeg: {e}")
        }
    })
}

/// Whether `ffmpeg` and `ffprobe` both run.
pub fn available() -> bool {
    ["ffmpeg", "ffprobe"].iter().all(|program| {
        command(program)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// How a wait ended.
#[derive(Debug)]
pub enum Waited {
    Exited(ExitStatus),
    /// `cancel` was set; the process was killed.
    Canceled,
    /// The timeout passed; the process was killed.
    TimedOut,
}

/// Wait for `child`, polling `cancel` and killing it on cancel or timeout.
pub fn wait(
    child: &mut Child,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
) -> std::io::Result<Waited> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Waited::Exited(status));
        }
        let timed_out = timeout.is_some_and(|t| start.elapsed() > t);
        if cancel.load(Ordering::Relaxed) || timed_out {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(if timed_out {
                Waited::TimedOut
            } else {
                Waited::Canceled
            });
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Collect a child's stderr on a thread (keeping the last 4 KiB), so a
/// chatty process never blocks on a full pipe. Join for the text.
pub fn stderr_tail(child: &mut Child) -> Option<std::thread::JoinHandle<String>> {
    let mut stderr = child.stderr.take()?;
    Some(std::thread::spawn(move || {
        let mut tail = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            tail.extend_from_slice(&buf[..n]);
            if tail.len() > 4096 {
                tail.drain(..tail.len() - 4096);
            }
        }
        String::from_utf8_lossy(&tail).trim().to_string()
    }))
}

/// The last line of FFmpeg's error output, for messages.
pub fn last_line(text: &str) -> &str {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_gives_a_clear_message() {
        let err = spawn(&mut command("emulsion-no-such-ffmpeg")).unwrap_err();
        assert_eq!(err.to_string(), MISSING);
        assert_eq!(last_line("a\nlast\n\n"), "last");
    }
}
