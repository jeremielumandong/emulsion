// Added by Emulsion. Software-only scheduling adapted from AgentOps' vendored
// Apache-2.0 GPUI rendering_policy.rs and platform/windows/vsync.rs.
// SPDX-License-Identifier: Apache-2.0
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static SOFTWARE_RENDERING: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_software_rendering(enabled: bool) {
    SOFTWARE_RENDERING.store(enabled, Ordering::Relaxed);
}

pub(crate) fn software_rendering() -> bool {
    SOFTWARE_RENDERING.load(Ordering::Relaxed)
}

pub(crate) fn is_software_adapter(flag: bool, name: &str) -> bool {
    flag || [
        "Microsoft Basic Render Driver",
        "Microsoft Basic Display Adapter",
    ]
    .iter()
    .any(|software| name.trim().eq_ignore_ascii_case(software))
}

pub(crate) fn frame_interval(software: bool) -> Option<Duration> {
    software.then_some(Duration::from_nanos(33_333_334))
}

/// Hardware enumeration remains lazy: forced software does not probe hardware,
/// failed candidates continue, and fallback runs only after hardware is exhausted.
pub(crate) fn select_adapter<A, T, E>(
    force_software: bool,
    adapters: impl IntoIterator<Item = A>,
    mut try_hardware: impl FnMut(A) -> Option<T>,
    fallback: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    if !force_software {
        for adapter in adapters {
            if let Some(device) = try_hardware(adapter) {
                return Ok(device);
            }
        }
    }
    fallback()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_detection_does_not_misclassify_virtual_or_integrated_hardware() {
        for name in ["VMware SVGA 3D", "Intel UHD Graphics", "NVIDIA A10"] {
            assert!(!is_software_adapter(false, name));
        }
        assert!(is_software_adapter(true, "Unknown software adapter"));
        assert!(is_software_adapter(false, "Microsoft Basic Render Driver"));
        assert!(is_software_adapter(
            false,
            " microsoft basic display adapter "
        ));
    }

    #[test]
    fn software_pacing_is_capped_at_thirty_hertz_and_hardware_is_unchanged() {
        assert_eq!(frame_interval(false), None);
        let interval = frame_interval(true).unwrap();
        assert!(interval.as_secs_f64() >= 1.0 / 30.0);
        assert!(interval < Duration::from_millis(34));
    }

    #[test]
    fn recovery_can_restore_hardware_pacing() {
        set_software_rendering(true);
        assert!(software_rendering());
        set_software_rendering(false);
        assert!(!software_rendering());
    }
    #[test]
    fn selection_retries_failed_hardware_and_never_probes_after_success() {
        let mut attempts = Vec::new();
        let chosen = select_adapter(
            false,
            [1, 2, 3],
            |adapter| {
                attempts.push(adapter);
                (adapter == 2).then_some(adapter)
            },
            || -> Result<i32, ()> { panic!("hardware succeeds before WARP") },
        );
        assert_eq!(chosen, Ok(2));
        assert_eq!(attempts, [1, 2]);
    }

    #[test]
    fn empty_or_failed_hardware_reaches_fallback_and_preserves_its_error() {
        let mut attempted = 0;
        let chosen = select_adapter(
            false,
            [1, 2],
            |_| {
                attempted += 1;
                None::<i32>
            },
            || Err("WARP unavailable"),
        );
        assert_eq!(chosen, Err("WARP unavailable"));
        assert_eq!(attempted, 2);
        assert_eq!(
            select_adapter(false, [], |_: i32| None, || Ok::<_, ()>(7)),
            Ok(7)
        );
    }

    #[test]
    fn explicit_software_skips_even_hardware_enumeration() {
        let hardware = std::iter::from_fn(|| -> Option<i32> { panic!("must not enumerate") });
        assert_eq!(
            select_adapter(true, hardware, Some, || Ok::<_, ()>(7)),
            Ok(7)
        );
    }
}
