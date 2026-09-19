//! Recipe bundles: many recipes in one TOML file, for sharing a set
//! (a photographer's looks, a club's presets) in a single download.
//! The file is plain TOML with a small header and a `[[recipes]]` table
//! per recipe, so it stays readable and diffable.

use crate::{Recipe, RecipeError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Bundle {
    /// What the set is called.
    pub name: String,
    pub author: String,
    pub notes: String,
    /// Bundle format version, for forward compatibility.
    pub version: u32,
    pub recipes: Vec<Recipe>,
}

pub const VERSION: u32 = 1;

impl Bundle {
    pub fn new(name: impl Into<String>, recipes: Vec<Recipe>) -> Self {
        Self {
            name: name.into(),
            version: VERSION,
            recipes,
            ..Default::default()
        }
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// Parse and validate every recipe; a bad one fails the whole bundle
    /// so nothing half-imports.
    pub fn from_toml(text: &str) -> Result<Bundle, RecipeError> {
        let b: Bundle = toml::from_str(text)?;
        for r in &b.recipes {
            r.validate()?;
        }
        Ok(b)
    }
}

/// Does this TOML text look like a bundle rather than one recipe?
pub fn is_bundle(text: &str) -> bool {
    text.lines().any(|l| l.trim() == "[[recipes]]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_round_trips_and_is_told_from_a_recipe() {
        let mut a = Recipe::default();
        a.name = "Warm Street".into();
        a.tags = vec!["warm".into()];
        let mut b = Recipe::default();
        b.name = "Cool Night".into();
        let bundle = Bundle::new("Test set", vec![a.clone(), b.clone()]);
        let text = bundle.to_toml();
        assert!(is_bundle(&text));
        assert!(!is_bundle(&a.to_toml()));
        let back = Bundle::from_toml(&text).unwrap();
        assert_eq!(back.name, "Test set");
        assert_eq!(back.version, VERSION);
        assert_eq!(back.recipes.len(), 2);
        assert_eq!(back.recipes[0].name, "Warm Street");
        assert_eq!(back.recipes[1], b);
    }
}
