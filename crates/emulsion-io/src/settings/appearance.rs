//! Named appearance choices from the editor handoff. Unknown future values
//! fall back without discarding the rest of a user's settings.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    Halo,
    Tide,
    Moss,
    Amber,
    Rose,
    #[default]
    #[serde(other)]
    Ember,
}

impl Accent {
    pub const ALL: [Self; 6] = [
        Self::Ember,
        Self::Halo,
        Self::Tide,
        Self::Moss,
        Self::Amber,
        Self::Rose,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ember => "Ember",
            Self::Halo => "Halo",
            Self::Tide => "Tide",
            Self::Moss => "Moss",
            Self::Amber => "Amber",
            Self::Rose => "Rose",
        }
    }

    pub fn rgb(self) -> u32 {
        match self {
            Self::Ember => 0xD93A1E,
            Self::Halo => 0x7A5CF5,
            Self::Tide => 0x1E8FD9,
            Self::Moss => 0x3E9E5B,
            Self::Amber => 0xE5A81B,
            Self::Rose => 0xD9457E,
        }
    }

    pub fn foreground(self) -> u32 {
        if self == Self::Amber {
            0x1A1206
        } else {
            0xFFFFFF
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corners {
    Square,
    Round,
    #[default]
    #[serde(other)]
    Soft,
}

impl Corners {
    pub const ALL: [Self; 3] = [Self::Square, Self::Soft, Self::Round];

    pub fn label(self) -> &'static str {
        match self {
            Self::Square => "Square",
            Self::Soft => "Soft",
            Self::Round => "Round",
        }
    }

    pub fn radius(self) -> f32 {
        match self {
            Self::Square => 3.,
            Self::Soft => 8.,
            Self::Round => 12.,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn old_settings_and_unknown_appearance_values_preserve_other_preferences() {
        let old: Settings =
            serde_json::from_str(r#"{"light_mode":true,"draw_mode":true}"#).unwrap();
        assert_eq!(old.accent, Accent::Ember);
        assert_eq!(old.corners, Corners::Soft);
        assert!(old.canvas_presets.is_empty());
        let future: Settings = serde_json::from_str(
            r#"{"accent":"future","corners":"future","light_mode":true,"draw_mode":true}"#,
        )
        .unwrap();
        assert_eq!(future, old);
        let settings = Settings {
            accent: Accent::Amber,
            corners: Corners::Round,
            ..old
        };
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<Settings>(&encoded).unwrap(),
            settings
        );
    }
}
