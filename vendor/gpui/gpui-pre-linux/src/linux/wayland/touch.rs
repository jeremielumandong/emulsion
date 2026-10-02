//! Native touchscreen contacts, including Sunshine's virtual touchscreen.
// Emulsion addition, distributed under this package's Apache-2.0 license.
use super::{client::*, serial::SerialKind, window::WaylandWindowStatePtr};
use gpui::{Pixels, PlatformInput, Point, TouchEvent, TouchId, TouchPhase, point, px};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, protocol::wl_touch};

pub(super) struct Contact {
    pub window: WaylandWindowStatePtr,
    id: TouchId,
    position: Point<Pixels>,
}

impl Contact {
    pub fn dispatch(&self, phase: TouchPhase) {
        self.window.handle_input(PlatformInput::Touch(TouchEvent {
            id: self.id,
            phase,
            position: self.position,
            predicted_position: None,
            force: None,
        }));
    }
}

impl Dispatch<wl_touch::WlTouch, ()> for WaylandClientStatePtr {
    fn event(
        this: &mut Self,
        _: &wl_touch::WlTouch,
        event: wl_touch::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let client = this.get_client();
        let mut state = client.borrow_mut();
        match event {
            wl_touch::Event::Down {
                id,
                surface,
                x,
                y,
                serial,
                ..
            } => {
                let Some(window) = get_window(&mut state, &surface.id()) else {
                    return;
                };
                state.serial_tracker.update(SerialKind::MousePress, serial);
                let contact = Contact {
                    window,
                    id: TouchId(state.next_touch_id),
                    position: point(px(x as f32), px(y as f32)),
                };
                state.next_touch_id = state.next_touch_id.wrapping_add(1);
                let previous = state.touch_contacts.insert(id, contact);
                let contact = state.touch_contacts.get(&id).unwrap();
                let window = contact.window.clone();
                let input = TouchEvent {
                    id: contact.id,
                    phase: TouchPhase::Started,
                    position: contact.position,
                    ..Default::default()
                };
                drop(state);
                if let Some(previous) = previous {
                    previous.dispatch(TouchPhase::Cancelled);
                }
                window.handle_input(PlatformInput::Touch(input));
            }
            wl_touch::Event::Motion { id, x, y, .. } => {
                let Some(contact) = state.touch_contacts.get_mut(&id) else {
                    return;
                };
                contact.position = point(px(x as f32), px(y as f32));
                let window = contact.window.clone();
                let input = TouchEvent {
                    id: contact.id,
                    phase: TouchPhase::Moved,
                    position: contact.position,
                    ..Default::default()
                };
                drop(state);
                window.handle_input(PlatformInput::Touch(input));
            }
            wl_touch::Event::Up { id, .. } => {
                let contact = state.touch_contacts.remove(&id);
                drop(state);
                if let Some(contact) = contact {
                    contact.dispatch(TouchPhase::Ended);
                }
            }
            wl_touch::Event::Cancel => {
                let contacts: Vec<_> = state
                    .touch_contacts
                    .drain()
                    .map(|(_, contact)| contact)
                    .collect();
                drop(state);
                for contact in contacts {
                    contact.dispatch(TouchPhase::Cancelled);
                }
            }
            _ => {}
        }
    }
}
