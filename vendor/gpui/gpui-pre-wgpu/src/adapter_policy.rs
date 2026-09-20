// Added by Emulsion for hardware-first selection with an explicit software override.
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn priority(
    software: bool,
    user_match: bool,
    compositor_match: bool,
    device_priority: u8,
    backend_priority: u8,
) -> (u8, u8, u8, u8, u8) {
    (
        u8::from(!user_match),
        u8::from(software),
        u8::from(!compositor_match),
        device_priority,
        backend_priority,
    )
}

pub(crate) fn allowed(software: bool, force_software: bool, reject_software: bool) -> bool {
    if force_software {
        software
    } else {
        !reject_software || !software
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_precedes_software_even_when_compositor_uses_software() {
        assert!(priority(false, false, false, 3, 1) < priority(true, false, true, 4, 0));
        assert!(priority(false, false, true, 1, 0) < priority(false, false, false, 0, 0));
    }

    #[test]
    fn explicit_device_override_retains_priority() {
        assert!(priority(true, true, false, 4, 0) < priority(false, false, true, 0, 0));
    }

    #[test]
    fn default_and_recovery_allow_software_but_forced_mode_never_uses_hardware() {
        assert!(allowed(false, false, false));
        assert!(allowed(true, false, false));
        assert!(!allowed(true, false, true));
        assert!(!allowed(false, true, false));
        assert!(!allowed(false, true, true));
        assert!(allowed(true, true, false));
        assert!(allowed(true, true, true));
    }
}
