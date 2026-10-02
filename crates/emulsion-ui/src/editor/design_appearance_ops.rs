//! UI input validation; geometry operations are shared with MCP.
pub(super) use emulsion_core::design_formatting::*;

pub(super) fn number(value: &str, min: f32, max: f32) -> Result<f32, String> {
    value
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite() && *v >= min && *v <= max)
        .ok_or_else(|| {
            t!(
                "editor.design_appearance_ops.number_range",
                min = min,
                max = max
            )
            .into_owned()
        })
}
