// Added by Emulsion. Inspired by AgentOps' Apache-2.0 GPUI software-rendering fixes.
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn needs_presentation(
    required: bool,
    pending: bool,
    high_rate_input: bool,
    software: bool,
) -> bool {
    required || pending || (high_rate_input && !software)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_skips_idle_keepalive_but_never_drops_required_or_pending_frames() {
        assert!(!needs_presentation(false, false, true, true));
        assert!(needs_presentation(false, false, true, false));
        for software in [false, true] {
            for high_rate in [false, true] {
                assert!(needs_presentation(true, false, high_rate, software));
                assert!(needs_presentation(false, true, high_rate, software));
            }
            assert!(!needs_presentation(false, false, false, software));
        }
    }
}
