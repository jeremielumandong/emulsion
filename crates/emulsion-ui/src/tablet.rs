//! Pen pressure and tilt, read from the tablet beside GPUI.
//!
//! GPUI's pointer events carry position only. macOS uses an AppKit local
//! event monitor which observes native tablet events and tablet mouse
//! subtypes before GPUI dispatch, returning every event unchanged.
//! Windows observes native pointer messages on the UI thread before GPUI's
//! normal mouse dispatch. Pressure and tilt are read together from Windows Ink.
//! On Linux the tablet is also
//! an evdev device, so a background thread opens every device that reports
//! pen pressure and keeps the latest pressure, tilt and pen-down state. The
//! brush asks for the pressure that arrived within the last moment of each
//! pointer move, and falls back to speed when there is none (mouse,
//! trackpad, or a tablet the process may not read).
//!
//! Reading `/dev/input/event*` needs permission: on most distributions the
//! user must be in the `input` group. The status line says which is in use.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenSample {
    pub pressure: f32,
    /// Degrees from vertical, x and y.
    pub tilt: (f32, f32),
    pub down: bool,
    pub at: Instant,
}

#[derive(Debug, Default)]
struct State {
    latest: Option<PenSample>,
    devices: Vec<String>,
    /// Devices that looked like tablets but could not be opened.
    denied: Vec<String>,
    scanned: bool,
    backend_error: Option<String>,
}

fn state() -> &'static Arc<Mutex<State>> {
    static S: OnceLock<Arc<Mutex<State>>> = OnceLock::new();
    S.get_or_init(|| Arc::new(Mutex::new(State::default())))
}

/// How recent a tablet sample must be to pair with a pointer move.
const FRESH: Duration = Duration::from_millis(120);

fn active_sample(sample: Option<PenSample>, now: Instant) -> Option<PenSample> {
    sample.filter(|p| {
        p.down
            && now.saturating_duration_since(p.at) < FRESH
            && p.pressure.is_finite()
            && p.tilt.0.is_finite()
            && p.tilt.1.is_finite()
    })
}

/// The pressure to use for a pointer move happening now, if a pen is
/// reporting one.
pub fn pressure() -> Option<f32> {
    sample().map(|p| p.pressure)
}

pub fn tilt() -> Option<(f32, f32)> {
    sample().map(|p| p.tilt)
}

/// Read pressure and tilt from one native event under a single lock.
pub fn sample() -> Option<PenSample> {
    let s = state().lock().ok()?;
    active_sample(s.latest, Instant::now())
}

#[cfg(test)]
mod sample_tests {
    use super::*;

    #[test]
    fn tablet_samples_expire_and_pen_up_restores_speed_fallback() {
        let now = Instant::now();
        let pen = PenSample {
            pressure: 0.35,
            tilt: (20., -15.),
            down: true,
            at: now,
        };
        let active = active_sample(Some(pen), now + Duration::from_millis(119)).unwrap();
        assert_eq!((active.pressure, active.tilt), (0.35, (20., -15.)));
        assert!(active_sample(Some(pen), now + FRESH).is_none());
        assert!(active_sample(Some(PenSample { down: false, ..pen }), now).is_none());
        assert!(active_sample(None, now).is_none());
        assert!(
            active_sample(
                Some(PenSample {
                    pressure: f32::NAN,
                    ..pen
                }),
                now
            )
            .is_none()
        );
    }
}

/// What the status line says about pressure.
pub fn status() -> String {
    if let Some(error) = state().lock().ok().and_then(|s| s.backend_error.clone()) {
        return format!("pen input unavailable ({error}); pressure: speed");
    }
    let s = state()
        .lock()
        .map(|s| (s.devices.clone(), s.denied.clone(), s.scanned))
        .unwrap_or_default();
    match s {
        (d, _, _) if !d.is_empty() => format!("pen: {}", d.join(", ")),
        (_, denied, _) if !denied.is_empty() => format!(
            "pen found but unreadable ({}); add yourself to the input group",
            denied.join(", ")
        ),
        (_, _, true) => "no pen; speed stands in for pressure".into(),
        _ => "pressure: speed".into(),
    }
}

/// Names of the devices opened as pens.
pub fn devices() -> Vec<String> {
    state()
        .lock()
        .map(|s| s.devices.clone())
        .unwrap_or_default()
}

/// Start (once) the threads that follow the tablets. Safe to call often;
/// later calls rescan for newly plugged devices no more than every few
/// seconds.
pub fn start() {
    #[cfg(target_os = "linux")]
    linux::start();
    #[cfg(target_os = "macos")]
    macos::start();
    #[cfg(target_os = "windows")]
    windows::start();
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        if let Ok(mut s) = state().lock() {
            s.scanned = true;
        }
    }
}

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use block2::RcBlock;
    use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::{NSEvent, NSEventButtonMask, NSEventMask, NSEventSubtype, NSEventType};
    use std::cell::RefCell;
    use std::ptr::NonNull;

    thread_local! {
        static MONITOR: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    }

    pub(super) fn start() {
        if MainThreadMarker::new().is_none() {
            return;
        }
        MONITOR.with(|monitor| {
            if monitor.borrow().is_some() {
                return;
            }
            let callback = RcBlock::new(|event: NonNull<NSEvent>| {
                // SAFETY: AppKit supplies a live NSEvent for the duration of
                // the callback. Returning this exact pointer never swallows
                // or replaces events intended for GPUI and text inputs.
                let e = unsafe { event.as_ref() };
                observe(e);
                event.as_ptr()
            });
            let mask = NSEventMask::TabletPoint
                | NSEventMask::TabletProximity
                | NSEventMask::LeftMouseDown
                | NSEventMask::LeftMouseUp
                | NSEventMask::LeftMouseDragged
                | NSEventMask::MouseMoved
                | NSEventMask::RightMouseDown
                | NSEventMask::RightMouseUp
                | NSEventMask::RightMouseDragged
                | NSEventMask::OtherMouseDown
                | NSEventMask::OtherMouseUp
                | NSEventMask::OtherMouseDragged;
            // SAFETY: installed on the main thread. AppKit copies the block;
            // its return value is always the original valid event pointer.
            *monitor.borrow_mut() =
                unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &callback) };
            if let Ok(mut s) = state().lock() {
                s.scanned = true;
            }
        });
    }

    fn observe(e: &NSEvent) {
        let kind = e.r#type();
        let subtype = e.subtype();
        if kind == NSEventType::TabletProximity || subtype == NSEventSubtype::TabletProximity {
            if let Ok(mut s) = state().lock() {
                if e.isEnteringProximity() {
                    if s.devices.is_empty() {
                        s.devices.push("macOS tablet".into());
                    }
                } else {
                    s.latest = None;
                }
            }
            return;
        }
        if kind == NSEventType::TabletPoint || subtype == NSEventSubtype::TabletPoint {
            let pressure = e.pressure().clamp(0.0, 1.0);
            let tilt = e.tilt();
            let up = matches!(
                kind,
                NSEventType::LeftMouseUp | NSEventType::RightMouseUp | NSEventType::OtherMouseUp
            );
            let down =
                !up && (pressure > 0.0 || e.buttonMask().contains(NSEventButtonMask::PenTip));
            if let Ok(mut s) = state().lock() {
                if s.devices.is_empty() {
                    s.devices.push("macOS tablet".into());
                }
                s.latest = Some(PenSample {
                    pressure,
                    // AppKit normalizes each tilt axis to -1..1 at ±90°.
                    tilt: (
                        (tilt.x as f32).clamp(-1.0, 1.0) * 90.0,
                        (tilt.y as f32).clamp(-1.0, 1.0) * 90.0,
                    ),
                    down,
                    at: Instant::now(),
                });
            }
        } else if let Ok(mut s) = state().lock() {
            // Switching straight from pen to mouse must not reuse a fresh
            // pen sample during the 120 ms fallback interval.
            s.latest = None;
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use evdev::{AbsoluteAxisCode, Device, EventSummary, KeyCode};
    use std::collections::HashSet;

    fn last_scan() -> &'static Mutex<Option<Instant>> {
        static L: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
        L.get_or_init(|| Mutex::new(None))
    }

    fn opened() -> &'static Mutex<HashSet<std::path::PathBuf>> {
        static O: OnceLock<Mutex<HashSet<std::path::PathBuf>>> = OnceLock::new();
        O.get_or_init(|| Mutex::new(HashSet::new()))
    }

    pub fn start() {
        {
            let mut l = last_scan().lock().unwrap_or_else(|e| e.into_inner());
            if l.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
                return;
            }
            *l = Some(Instant::now());
        }
        std::thread::Builder::new()
            .name("emulsion-tablet-scan".into())
            .spawn(scan)
            .ok();
    }

    fn looks_like_pen(d: &Device) -> bool {
        let abs = d.supported_absolute_axes();
        let keys = d.supported_keys();
        abs.is_some_and(|a| a.contains(AbsoluteAxisCode::ABS_PRESSURE))
            && keys.is_some_and(|k| {
                k.contains(KeyCode::BTN_TOOL_PEN) || k.contains(KeyCode::BTN_STYLUS)
            })
    }

    fn scan() {
        let Ok(dir) = std::fs::read_dir("/dev/input") else {
            mark_scanned();
            return;
        };
        for entry in dir.flatten() {
            let path = entry.path();
            if !path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("event"))
            {
                continue;
            }
            if opened().lock().map(|o| o.contains(&path)).unwrap_or(true) {
                continue;
            }
            match Device::open(&path) {
                Ok(d) if looks_like_pen(&d) => {
                    let name = d.name().unwrap_or("pen").to_string();
                    if let Ok(mut s) = state().lock() {
                        s.devices.push(name.clone());
                    }
                    if let Ok(mut o) = opened().lock() {
                        o.insert(path.clone());
                    }
                    std::thread::Builder::new()
                        .name(format!("emulsion-tablet {name}"))
                        .spawn(move || follow(d, name, path))
                        .ok();
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    // Cannot inspect it; report it only when the udev name says tablet.
                    if let Some(n) = sysfs_name(&path)
                        && is_tablet_name(&n)
                        && let Ok(mut s) = state().lock()
                        && !s.denied.contains(&n)
                    {
                        s.denied.push(n);
                    }
                }
                Err(_) => {}
            }
        }
        mark_scanned();
    }

    fn mark_scanned() {
        if let Ok(mut s) = state().lock() {
            s.scanned = true;
        }
    }

    fn sysfs_name(dev: &std::path::Path) -> Option<String> {
        let n = dev.file_name()?.to_string_lossy().into_owned();
        std::fs::read_to_string(format!("/sys/class/input/{n}/device/name"))
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn is_tablet_name(n: &str) -> bool {
        let l = n.to_lowercase();
        [
            "pen", "stylus", "wacom", "tablet", "huion", "xp-pen", "gaomon", "veikk",
        ]
        .iter()
        .any(|k| l.contains(k))
    }

    /// Follow one pen device until it goes away.
    fn follow(mut d: Device, name: String, path: std::path::PathBuf) {
        let (pmin, pmax) = d
            .get_abs_state()
            .ok()
            .and_then(|st| {
                let info = st.get(AbsoluteAxisCode::ABS_PRESSURE.0 as usize)?;
                Some((
                    info.minimum as f32,
                    info.maximum.max(info.minimum + 1) as f32,
                ))
            })
            .unwrap_or((0.0, 4095.0));
        let tilt_max = d
            .get_abs_state()
            .ok()
            .and_then(|st| {
                st.get(AbsoluteAxisCode::ABS_TILT_X.0 as usize)
                    .map(|i| i.maximum.max(1) as f32)
            })
            .unwrap_or(64.0);
        let mut cur = PenSample {
            pressure: 0.0,
            tilt: (0.0, 0.0),
            down: false,
            at: Instant::now(),
        };
        loop {
            let events = match d.fetch_events() {
                Ok(e) => e,
                Err(_) => break,
            };
            for ev in events {
                match ev.destructure() {
                    EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_PRESSURE, v) => {
                        cur.pressure = ((v as f32 - pmin) / (pmax - pmin)).clamp(0.0, 1.0);
                        // Many pens never send BTN_TOUCH cleanly; pressure is the truth.
                        cur.down = cur.pressure > 0.0;
                    }
                    EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_TILT_X, v) => {
                        cur.tilt.0 = v as f32 / tilt_max * 60.0
                    }
                    EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_TILT_Y, v) => {
                        cur.tilt.1 = v as f32 / tilt_max * 60.0
                    }
                    EventSummary::Key(_, KeyCode::BTN_TOUCH, v) => {
                        cur.down = v != 0 || cur.pressure > 0.0
                    }
                    EventSummary::Synchronization(..) => {
                        cur.at = Instant::now();
                        if let Ok(mut s) = state().lock() {
                            s.latest = Some(cur);
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Ok(mut s) = state().lock() {
            s.devices.retain(|n| *n != name);
            s.latest = None;
        }
        if let Ok(mut o) = opened().lock() {
            o.remove(&path);
        }
    }
}
