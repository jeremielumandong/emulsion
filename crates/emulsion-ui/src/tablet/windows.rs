//! Thread-local observers leave every native message untouched, including
//! DefWindowProc's pen-to-mouse promotion used by GPUI. Queued and sent messages
//! need separate hooks; both are released when their owning UI thread exits.

use super::{PenSample, state};
use std::{cell::RefCell, ptr::null_mut, time::Instant};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{Input::Pointer::*, WindowsAndMessaging::*},
};

struct Monitor {
    queued: HHOOK,
    sent: HHOOK,
}

impl Drop for Monitor {
    fn drop(&mut self) {
        // SAFETY: these handles belong to this thread and are unhooked once.
        unsafe {
            UnhookWindowsHookEx(self.queued);
            UnhookWindowsHookEx(self.sent);
        }
    }
}

thread_local! {
    static MONITOR: RefCell<Option<Monitor>> = const { RefCell::new(None) };
}

pub(super) fn start() {
    MONITOR.with(|slot| {
        if slot.borrow().is_some() {
            return;
        }
        // SAFETY: callbacks live for the process lifetime. A nonzero thread ID
        // scopes observation to the calling UI thread, never another process.
        let (queued, sent) = unsafe {
            let thread = GetCurrentThreadId();
            (
                SetWindowsHookExW(WH_GETMESSAGE, Some(queued_message), null_mut(), thread),
                SetWindowsHookExW(WH_CALLWNDPROC, Some(sent_message), null_mut(), thread),
            )
        };
        if queued.is_null() || sent.is_null() {
            let error = std::io::Error::last_os_error().to_string();
            // SAFETY: roll back whichever hook installed successfully.
            unsafe {
                if !queued.is_null() {
                    UnhookWindowsHookEx(queued);
                }
                if !sent.is_null() {
                    UnhookWindowsHookEx(sent);
                }
            }
            if let Ok(mut s) = state().lock() {
                s.backend_error = Some(error);
            }
            return;
        }
        *slot.borrow_mut() = Some(Monitor { queued, sent });
        if let Ok(mut s) = state().lock() {
            s.scanned = true;
            s.backend_error = None;
        }
    });
}

unsafe extern "system" fn queued_message(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam == PM_REMOVE as usize && lparam != 0 {
        // SAFETY: WH_GETMESSAGE supplies a valid MSG during this callback.
        let msg = unsafe { &*(lparam as *const MSG) };
        observe(msg.message, msg.wParam);
    }
    // SAFETY: preserve the hook chain with exactly the supplied arguments.
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn sent_message(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && lparam != 0 {
        // SAFETY: WH_CALLWNDPROC supplies a valid CWPSTRUCT during this callback.
        let msg = unsafe { &*(lparam as *const CWPSTRUCT) };
        observe(msg.message, msg.wParam);
    }
    // SAFETY: observation never consumes or changes messages.
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

fn clear() {
    if let Ok(mut s) = state().lock() {
        s.latest = None;
    }
}

fn observe(message: u32, wparam: WPARAM) {
    match message {
        WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP => {
            let mut info = POINTER_PEN_INFO::default();
            // SAFETY: queried on the receiving thread while the pointer message
            // is current; Windows writes to a correctly sized initialized struct.
            if unsafe { GetPointerPenInfo((wparam & 0xffff) as u32, &mut info) } == 0 {
                clear();
                return;
            }
            if let Ok(mut s) = state().lock() {
                if s.devices.is_empty() {
                    s.devices.push("Windows Ink pen".into());
                }
                s.latest = normalized_sample(&info, message == WM_POINTERUP, Instant::now());
            }
        }
        WM_POINTERLEAVE | WM_POINTERCAPTURECHANGED | WM_KILLFOCUS | WM_CANCELMODE => clear(),
        WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            // SAFETY: reads metadata for the message currently being dispatched.
            let extra = unsafe { GetMessageExtraInfo() } as usize;
            if !is_promoted_pen(extra) {
                clear();
            }
        }
        _ => {}
    }
}

fn is_promoted_pen(extra: usize) -> bool {
    // Microsoft tablet signature; bit 7 differentiates touch from pen.
    extra & 0xffff_ff80 == 0xff51_5700
}

fn normalized_sample(info: &POINTER_PEN_INFO, up: bool, at: Instant) -> Option<PenSample> {
    // Missing pressure capability uses the existing speed fallback. Unreported
    // tilt is neutral, never arbitrary bytes from a driver or a previous sample.
    if info.penMask & PEN_MASK_PRESSURE == 0 {
        return None;
    }
    Some(PenSample {
        pressure: (info.pressure as f32 / 1024.0).clamp(0.0, 1.0),
        tilt: (
            if info.penMask & PEN_MASK_TILT_X != 0 {
                info.tiltX.clamp(-90, 90) as f32
            } else {
                0.0
            },
            if info.penMask & PEN_MASK_TILT_Y != 0 {
                info.tiltY.clamp(-90, 90) as f32
            } else {
                0.0
            },
        ),
        down: !up
            && info.pointerInfo.pointerFlags & POINTER_FLAG_INCONTACT != 0
            && info.pointerInfo.pointerFlags & POINTER_FLAG_CANCELED == 0,
        at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_pressure_capabilities_and_tilt_ranges() {
        let mut info = POINTER_PEN_INFO::default();
        let now = Instant::now();
        assert!(normalized_sample(&info, false, now).is_none());
        info.penMask = PEN_MASK_PRESSURE | PEN_MASK_TILT_X;
        info.pressure = 512;
        info.tiltX = -120;
        info.tiltY = 40;
        info.pointerInfo.pointerFlags = POINTER_FLAG_INCONTACT;
        let sample = normalized_sample(&info, false, now).unwrap();
        assert_eq!(sample.pressure, 0.5);
        assert_eq!(sample.tilt, (-90.0, 0.0));
        assert!(sample.down);
        info.pressure = 2000;
        assert_eq!(normalized_sample(&info, false, now).unwrap().pressure, 1.0);
        assert!(!normalized_sample(&info, true, now).unwrap().down);
        info.pointerInfo.pointerFlags |= POINTER_FLAG_CANCELED;
        assert!(!normalized_sample(&info, false, now).unwrap().down);
        info.pointerInfo.pointerFlags = POINTER_FLAG_INRANGE;
        assert!(!normalized_sample(&info, false, now).unwrap().down);
    }

    #[test]
    fn mouse_and_touch_never_reuse_pen_pressure() {
        assert!(is_promoted_pen(0xff51_5701));
        assert!(is_promoted_pen(0xff51_577f));
        assert!(!is_promoted_pen(0xff51_5781));
        assert!(!is_promoted_pen(0));
    }

    #[test]
    fn lost_capture_leave_and_focus_clear_the_active_sample() {
        for message in [
            WM_POINTERCAPTURECHANGED,
            WM_POINTERLEAVE,
            WM_KILLFOCUS,
            WM_CANCELMODE,
        ] {
            state().lock().unwrap().latest = Some(PenSample {
                pressure: 0.7,
                tilt: (15.0, -20.0),
                down: true,
                at: Instant::now(),
            });
            observe(message, 0);
            assert!(super::super::sample().is_none(), "message {message:#x}");
        }
    }
}
