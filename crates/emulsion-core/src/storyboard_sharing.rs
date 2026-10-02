//! Shared storyboards: advisory scene claims and the cloud revision a
//! board last merged.
//!
//! A **claim** says who is working on a scene ("I'm on scenes 4–6"). It is
//! advisory: anyone can still edit a claimed scene, and the editor only
//! warns. Claims are board data, so they are saved, travel with every cloud
//! revision and take part in Undo. Releasing a claim keeps a released
//! record with the release time, so when two copies of the board are merged
//! ([`merge_claims`]) the latest word on each scene wins, whether that is a
//! claim or a release.
//!
//! `merged_revision` names the other artist's cloud revision that the last
//! shared-project merge brought in. The next upload of this board records
//! it as the revision's second parent, so both heads are superseded.
use crate::storyboard::{GroupId, Storyboard};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_CLAIMS: usize = 10_000;
const MAX_DEVICE_CHARS: usize = 64;

/// One scene's claim, or its release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneClaim {
    pub scene: GroupId,
    /// The artist's name (Settings › Storyboard › Your name).
    pub claimant: String,
    /// The installation that made the claim (its cloud device ID).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device: String,
    /// Seconds since the Unix epoch, of the claim or its release.
    pub time: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub released: bool,
}

impl SceneClaim {
    /// Whether this claim belongs to someone other than `claimant` on
    /// `device`: another name, or the same (empty) name on another device.
    pub fn is_other(&self, claimant: &str, device: &str) -> bool {
        let (mine, theirs) = (claimant.trim(), self.claimant.trim());
        if !mine.is_empty() && !theirs.is_empty() {
            return !mine.eq_ignore_ascii_case(theirs);
        }
        !self.device.is_empty() && self.device != device
    }

    /// Latest wins; ties break the same way on every machine.
    fn key(&self) -> (u64, bool, &str, &str) {
        (self.time, self.released, &self.claimant, &self.device)
    }
}

/// Sharing data kept with the board.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sharing {
    /// At most one record per scene, in scene order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claims: Vec<SceneClaim>,
    /// The cloud revision (UUID) the last shared-project merge took in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_revision: Option<String>,
}

impl Sharing {
    pub fn is_empty(&self) -> bool {
        self.claims.is_empty() && self.merged_revision.is_none()
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.claims.len() > MAX_CLAIMS {
            return Err(format!("A storyboard keeps at most {MAX_CLAIMS} claims."));
        }
        let mut scenes = std::collections::HashSet::new();
        for claim in &self.claims {
            check_claimant(&claim.claimant)?;
            if claim.device.chars().count() > MAX_DEVICE_CHARS
                || claim.device.chars().any(char::is_control)
            {
                return Err("A claim's device ID is invalid.".into());
            }
            if !scenes.insert(claim.scene) {
                return Err("A scene has one claim record at most.".into());
            }
        }
        if let Some(id) = &self.merged_revision
            && (id.len() != 36 || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
        {
            return Err("The merged revision must be a cloud revision ID.".into());
        }
        Ok(())
    }
}

fn check_claimant(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > crate::storyboard_review::MAX_AUTHOR_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(format!(
            "Claims need a name of 1–{} characters: set yours in Settings › Storyboard.",
            crate::storyboard_review::MAX_AUTHOR_CHARS
        ));
    }
    Ok(())
}

/// Every scene's latest record from both copies of a board, in scene order.
pub fn merge_claims(ours: &[SceneClaim], theirs: &[SceneClaim]) -> Vec<SceneClaim> {
    let mut latest: BTreeMap<GroupId, &SceneClaim> = BTreeMap::new();
    for claim in ours.iter().chain(theirs) {
        match latest.get(&claim.scene) {
            Some(kept) if kept.key() >= claim.key() => {}
            _ => {
                latest.insert(claim.scene, claim);
            }
        }
    }
    latest.into_values().cloned().collect()
}

impl Storyboard {
    /// The scene's active claim, if any.
    pub fn claim(&self, scene: GroupId) -> Option<&SceneClaim> {
        self.sharing
            .claims
            .iter()
            .find(|c| c.scene == scene && !c.released)
    }

    /// Active claims in board order of their scenes.
    pub fn active_claims(&self, layout: &[crate::project::PageId]) -> Vec<&SceneClaim> {
        self.outline(layout)
            .iter()
            .filter_map(|s| self.claim(s.scene))
            .collect()
    }

    fn set_claim(&mut self, claim: SceneClaim) {
        let claims = &mut self.sharing.claims;
        match claims.iter_mut().find(|c| c.scene == claim.scene) {
            Some(existing) => *existing = claim,
            None => {
                let at = claims.partition_point(|c| c.scene < claim.scene);
                claims.insert(at, claim);
            }
        }
    }

    /// Claim `scenes` for `claimant` on `device` at `now`, replacing any
    /// earlier claim on them (claims are advisory).
    pub fn claim_scenes(
        &mut self,
        scenes: &[GroupId],
        claimant: &str,
        device: &str,
        now: u64,
    ) -> Result<(), String> {
        check_claimant(claimant)?;
        if scenes.is_empty() {
            return Err("Choose scenes to claim.".into());
        }
        for scene in scenes {
            if !self.scenes.contains_key(scene) {
                return Err(format!("No scene has ID {scene}."));
            }
        }
        for &scene in scenes {
            self.set_claim(SceneClaim {
                scene,
                claimant: claimant.trim().into(),
                device: device.chars().take(MAX_DEVICE_CHARS).collect(),
                time: now,
                released: false,
            });
        }
        Ok(())
    }

    /// Release the claims on `scenes` (anyone's: claims are advisory).
    /// Returns how many were released.
    pub fn release_scenes(&mut self, scenes: &[GroupId], now: u64) -> Result<usize, String> {
        let mut released = 0;
        for &scene in scenes {
            if let Some(claim) = self.claim(scene).cloned() {
                self.set_claim(SceneClaim {
                    time: now.max(claim.time),
                    released: true,
                    ..claim
                });
                released += 1;
            }
        }
        if released == 0 {
            return Err("None of those scenes is claimed.".into());
        }
        Ok(released)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storyboard::{Level, Settings};

    fn board() -> (Storyboard, Vec<GroupId>) {
        let ids = [1, 2, 3];
        let mut b = Storyboard::new(Settings::new(32, 18), &ids);
        let s2 = b.split(&ids, 2, Level::Scene, Some("2")).unwrap();
        let s3 = b.split(&ids, 3, Level::Scene, Some("3")).unwrap();
        let s1 = b.panels[&1].scene;
        (b, vec![s1, s2, s3])
    }

    #[test]
    fn claims_merge_as_a_union_where_the_latest_word_wins() {
        let (mut ours, s) = board();
        let mut theirs = ours.clone();
        ours.claim_scenes(&[s[0], s[1]], "Maya", "dev-a", 100)
            .unwrap();
        theirs
            .claim_scenes(&[s[1], s[2]], "Ravi", "dev-b", 200)
            .unwrap();
        let merged = merge_claims(&ours.sharing.claims, &theirs.sharing.claims);
        let who: Vec<_> = merged
            .iter()
            .map(|c| (c.scene, c.claimant.as_str()))
            .collect();
        assert_eq!(who, [(s[0], "Maya"), (s[1], "Ravi"), (s[2], "Ravi")]);
        // Same inputs, either order: same result.
        assert_eq!(
            merge_claims(&theirs.sharing.claims, &ours.sharing.claims),
            merged
        );

        // A later release beats an earlier claim; a later claim beats it.
        ours.sharing.claims = merged.clone();
        ours.release_scenes(&[s[2]], 300).unwrap();
        let after = merge_claims(&ours.sharing.claims, &theirs.sharing.claims);
        ours.sharing.claims = after;
        assert!(ours.claim(s[2]).is_none());
        theirs.claim_scenes(&[s[2]], "Ravi", "dev-b", 400).unwrap();
        ours.sharing.claims = merge_claims(&ours.sharing.claims, &theirs.sharing.claims);
        assert_eq!(ours.claim(s[2]).unwrap().claimant, "Ravi");
        ours.validate(&[1, 2, 3]).unwrap();
    }

    #[test]
    fn claims_need_a_name_and_real_scenes_and_know_whose_they_are() {
        let (mut b, s) = board();
        assert!(b.claim_scenes(&[s[0]], " ", "d", 1).is_err());
        assert!(b.claim_scenes(&[999], "Maya", "d", 1).is_err());
        assert!(b.release_scenes(&[s[0]], 2).is_err());
        b.claim_scenes(&[s[0]], "Maya", "dev-a", 1).unwrap();
        let claim = b.claim(s[0]).unwrap();
        assert!(!claim.is_other("maya", "dev-z"));
        assert!(claim.is_other("Ravi", "dev-a"));
        assert!(!claim.is_other("", "dev-a"));
        assert!(claim.is_other("", "dev-b"));
        assert_eq!(b.active_claims(&[1, 2, 3]).len(), 1);
        // Removing the scene drops its claim.
        b.reconcile(&[2, 3]);
        assert!(b.sharing.claims.iter().all(|c| c.scene != s[0]));
    }
}
