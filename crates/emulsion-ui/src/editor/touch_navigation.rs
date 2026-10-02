//! Finger navigation shares the desktop viewport; the pen remains a drawing tool.
use crate::viewport::View;
use gpui_kit::{Bounds, Pixels, TouchEvent, TouchId, TouchPhase};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct TouchNavigation {
    contacts: BTreeMap<TouchId, (f64, f64)>,
    previous: Option<((f64, f64), f64)>,
}

impl TouchNavigation {
    pub fn update(
        &mut self,
        event: &TouchEvent,
        view: &mut View,
        bounds: &Bounds<Pixels>,
        enabled: bool,
    ) -> bool {
        let position = (
            f32::from(event.position.x) as f64,
            f32::from(event.position.y) as f64,
        );
        if !position.0.is_finite() || !position.1.is_finite() {
            return false;
        }
        let changed_contacts = match event.phase {
            TouchPhase::Started => self.contacts.insert(event.id, position).is_none(),
            TouchPhase::Moved => {
                let Some(contact) = self.contacts.get_mut(&event.id) else {
                    return false;
                };
                *contact = position;
                false
            }
            TouchPhase::Ended | TouchPhase::Cancelled => self.contacts.remove(&event.id).is_some(),
        };
        let mut contacts = self.contacts.values();
        let current = contacts.next().copied().map(|first| match contacts.next() {
            Some(second) => (
                ((first.0 + second.0) / 2., (first.1 + second.1) / 2.),
                (first.0 - second.0).hypot(first.1 - second.1),
            ),
            None => (first, 0.),
        });
        let previous = self.previous;
        self.previous = current;
        if !enabled || changed_contacts {
            return false;
        }
        let (Some((old_center, old_distance)), Some((center, distance))) = (previous, current)
        else {
            return false;
        };
        if old_distance >= 1. && distance >= 1. {
            view.zoom_at(distance / old_distance, old_center, bounds);
        }
        view.pan(center.0 - old_center.0, center.1 - old_center.1);
        center != old_center || distance != old_distance
    }
}
