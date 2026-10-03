//! Values that change over time: easings, bezier ease curves and sampling a
//! keyframe track, plus applying animated transforms to layers. Neutral:
//! Design keeps its properties and storage, Storyboard its own; both
//! interpolate and apply motion through here.
use crate::{Document, NodeId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Step,
}
impl Easing {
    pub const ALL: [Self; 5] = [
        Self::Linear,
        Self::EaseIn,
        Self::EaseOut,
        Self::EaseInOut,
        Self::Step,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::EaseIn => "Ease in",
            Self::EaseOut => "Ease out",
            Self::EaseInOut => "Ease in and out",
            Self::Step => "Hold",
        }
    }
    pub fn sample(self, t: f64) -> f64 {
        let t = t.clamp(0., 1.);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t,
            Self::EaseOut => 1. - (1. - t) * (1. - t),
            Self::EaseInOut => t * t * (3. - 2. * t),
            Self::Step => {
                if t < 1. {
                    0.
                } else {
                    1.
                }
            }
        }
    }
}

/// A cubic bezier ease from (0, 0) to (1, 1) through two handles, as in CSS
/// `cubic-bezier`. `x1` and `x2` stay within 0–1 so time never runs back.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl Curve {
    pub fn validate(&self) -> Result<(), String> {
        let finite = [self.x1, self.y1, self.x2, self.y2]
            .iter()
            .all(|v| v.is_finite());
        if !finite
            || !(0. ..=1.).contains(&self.x1)
            || !(0. ..=1.).contains(&self.x2)
            || self.y1.abs() > 10.
            || self.y2.abs() > 10.
        {
            return Err("Ease curve handles need x from 0 to 1 and y from -10 to 10.".into());
        }
        Ok(())
    }

    /// Eased progress at time fraction `t`.
    pub fn sample(&self, t: f64) -> f64 {
        let t = t.clamp(0., 1.);
        let bezier = |a: f64, b: f64, s: f64| {
            let u = 1. - s;
            3. * u * u * s * a + 3. * u * s * s * b + s * s * s
        };
        // Find the curve parameter whose x is `t` (x is monotonic), then y.
        let (mut lo, mut hi) = (0., 1.);
        for _ in 0..40 {
            let mid = (lo + hi) / 2.;
            if bezier(self.x1, self.x2, mid) < t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        bezier(self.y1, self.y2, (lo + hi) / 2.)
    }
}

/// Eased progress through a segment: the curve when given, else the easing.
pub fn ease(easing: Easing, curve: Option<Curve>, t: f64) -> f64 {
    match curve {
        Some(curve) => curve.sample(t),
        None => easing.sample(t),
    }
}

/// A key as seen by `sample_by`: its time, value, and how the segment that
/// starts at it eases.
pub struct KeyView {
    pub time: f64,
    pub value: f64,
    pub easing: Easing,
    pub curve: Option<Curve>,
}

/// The value at `time` of keys in time order: held before the first and
/// after the last key, eased between neighbours. `None` without keys.
pub fn sample_by<K>(keys: &[K], time: f64, view: impl Fn(&K) -> KeyView) -> Option<f64> {
    let first = view(keys.first()?);
    if time <= first.time {
        return Some(first.value);
    }
    let mut a = first;
    for key in &keys[1..] {
        let b = view(key);
        if time <= b.time {
            let span = b.time - a.time;
            if span <= 0. {
                return Some(b.value);
            }
            let t = (time - a.time) / span;
            return Some(a.value + (b.value - a.value) * ease(a.easing, a.curve, t));
        }
        a = b;
    }
    Some(a.value)
}

/// Layers in parent-first order, so a parent's motion applies before its
/// children's.
pub fn parents_first(doc: &Document, ids: impl IntoIterator<Item = NodeId>) -> Vec<NodeId> {
    let mut ids: Vec<NodeId> = ids.into_iter().collect();
    ids.sort_by_key(|id| {
        let mut depth = 0;
        let mut current = doc.node(*id).and_then(|n| n.parent);
        while let Some(id) = current {
            depth += 1;
            current = doc.node(id).and_then(|n| n.parent);
        }
        depth
    });
    ids
}

/// Run `apply` on `doc` with every layer lock lifted, then put the locks
/// back: animation moves locked layers too, without changing the locks.
pub fn with_layers_unlocked<T>(
    doc: &mut Document,
    apply: impl FnOnce(&mut Document) -> Result<T, String>,
) -> Result<T, String> {
    let locks: Vec<_> = doc
        .nodes
        .iter()
        .map(|n| (n.id, n.locked, n.locks, n.link_group))
        .collect();
    for node in &mut doc.nodes {
        node.locked = false;
        node.locks = Default::default();
        node.link_group = None;
    }
    let result = apply(doc);
    for (id, locked, granular, links) in locks {
        if let Some(node) = doc.node_mut(id) {
            node.locked = locked;
            node.locks = granular;
            node.link_group = links;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct K(f64, f64, Easing);
    fn view(k: &K) -> KeyView {
        KeyView {
            time: k.0,
            value: k.1,
            easing: k.2,
            curve: None,
        }
    }

    #[test]
    fn keys_hold_at_the_ends_and_ease_between() {
        let keys = [K(10., 0., Easing::Linear), K(20., 100., Easing::Step)];
        assert_eq!(sample_by(&keys, 0., view), Some(0.));
        assert_eq!(sample_by(&keys, 15., view), Some(50.));
        assert_eq!(sample_by(&keys, 30., view), Some(100.));
        assert_eq!(sample_by::<K>(&[], 5., view), None);
        let held = [K(0., 1., Easing::Step), K(10., 2., Easing::Linear)];
        assert_eq!(sample_by(&held, 9.9, view), Some(1.));
    }

    #[test]
    fn bezier_curves_match_their_shapes() {
        let linear = Curve {
            x1: 0.25,
            y1: 0.25,
            x2: 0.75,
            y2: 0.75,
        };
        assert!((linear.sample(0.3) - 0.3).abs() < 1e-6);
        let ease = Curve {
            x1: 0.42,
            y1: 0.,
            x2: 0.58,
            y2: 1.,
        };
        assert!(ease.sample(0.2) < 0.2 && ease.sample(0.8) > 0.8);
        assert!((ease.sample(0.5) - 0.5).abs() < 1e-6);
        let overshoot = Curve {
            x1: 0.3,
            y1: 1.6,
            x2: 0.7,
            y2: 1.,
        };
        assert!(overshoot.sample(0.5) > 1.);
        assert!(Curve { x1: 1.5, ..linear }.validate().is_err());
        assert_eq!(
            super::ease(Easing::EaseIn, Some(linear), 0.5),
            linear.sample(0.5)
        );
    }

    #[test]
    fn locks_come_back_after_animated_edits() {
        let mut doc = Document::new(8, 8);
        let mut node = crate::Node::new(1, "Hero", crate::NodeKind::Fill { rgba: [1; 4] });
        node.locked = true;
        doc.nodes.push(node);
        let seen = with_layers_unlocked(&mut doc, |d| Ok(d.nodes[0].locked)).unwrap();
        assert!(!seen && doc.nodes[0].locked);
    }
}
