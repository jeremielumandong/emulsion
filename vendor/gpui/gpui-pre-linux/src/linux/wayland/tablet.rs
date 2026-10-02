//! Translate tablet-v2 frames into the same pointer callbacks used on other
//! platforms, keeping pressure and tilt paired with their surface coordinates.
// Emulsion addition, distributed under this package's Apache-2.0 license.

use super::{client::*, serial::SerialKind, to_shape, window::WaylandWindowStatePtr};
use gpui::{
    CursorStyle, Modifiers, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent,
    MouseUpEvent, PenInput, Pixels, PlatformInput, Point, point, px, with_pen_input,
};
use wayland_backend::protocol::WEnum;
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, delegate_noop, event_created_child,
};
use wayland_protocols::wp::{
    cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1,
    tablet::zv2::client::{
        zwp_tablet_manager_v2::ZwpTabletManagerV2,
        zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2},
        zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
        zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
        zwp_tablet_pad_v2::{self, ZwpTabletPadV2},
        zwp_tablet_seat_v2::{self, ZwpTabletSeatV2},
        zwp_tablet_tool_v2::{self, ZwpTabletToolV2},
        zwp_tablet_v2::{self, ZwpTabletV2},
    },
};

#[derive(Clone, Copy)]
enum Action {
    Move(Option<MouseButton>, bool),
    Down(MouseButton),
    Up(MouseButton),
    Exit,
}

#[derive(Default)]
pub(super) struct TabletToolState {
    pub window: Option<WaylandWindowStatePtr>,
    tool: Option<ZwpTabletToolV2>,
    cursor: Option<WpCursorShapeDeviceV1>,
    enter_serial: u32,
    position: Point<Pixels>,
    pressure: Option<f32>,
    tilt: (f32, f32),
    down: bool,
    side_button: Option<MouseButton>,
    pending: Vec<Action>,
}

impl TabletToolState {
    pub fn reset(&mut self) {
        self.window = None;
        self.pressure = self.pressure.map(|_| 0.);
        self.tilt = (0., 0.);
        self.down = false;
        self.side_button = None;
        self.pending.clear();
    }

    pub fn destroy(self) {
        if let Some(cursor) = self.cursor {
            cursor.destroy();
        }
        if let Some(tool) = self.tool {
            tool.destroy();
        }
    }

    pub fn set_cursor_style(&self, style: CursorStyle) {
        if self.window.is_some() {
            if let Some(cursor) = &self.cursor {
                cursor.set_shape(self.enter_serial, to_shape(style));
            }
        }
    }

    fn leave(&mut self) {
        // Some compositors/tools disappear without a final tip-up.
        if self.down {
            self.pending.push(Action::Up(MouseButton::Left));
        }
        if let Some(button) = self.side_button.take() {
            self.pending.push(Action::Up(button));
        }
        self.down = false;
        self.pending.push(Action::Exit);
    }

    fn frame(&mut self, modifiers: Modifiers) -> Vec<(PlatformInput, PenInput)> {
        let pen = PenInput::new(self.pressure, self.tilt, self.down);
        std::mem::take(&mut self.pending)
            .into_iter()
            .map(|action| {
                let mut sample = pen;
                let event = match action {
                    Action::Move(pressed_button, down) => {
                        sample.down = down;
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: self.position,
                            pressed_button,
                            modifiers,
                        })
                    }
                    Action::Down(button) => {
                        sample.down = button == MouseButton::Left;
                        PlatformInput::MouseDown(MouseDownEvent {
                            button,
                            position: self.position,
                            modifiers,
                            click_count: 1,
                            first_mouse: false,
                        })
                    }
                    Action::Up(button) => {
                        sample.down = false;
                        PlatformInput::MouseUp(MouseUpEvent {
                            button,
                            position: self.position,
                            modifiers,
                            click_count: 1,
                        })
                    }
                    Action::Exit => {
                        sample.down = false;
                        PlatformInput::MouseExited(MouseExitEvent {
                            position: self.position,
                            pressed_button: None,
                            modifiers,
                        })
                    }
                };
                (event, sample)
            })
            .collect()
    }
}

delegate_noop!(WaylandClientStatePtr: ignore ZwpTabletManagerV2);
delegate_noop!(WaylandClientStatePtr: ignore ZwpTabletPadRingV2);
delegate_noop!(WaylandClientStatePtr: ignore ZwpTabletPadStripV2);

impl Dispatch<ZwpTabletSeatV2, ()> for WaylandClientStatePtr {
    event_created_child!(WaylandClientStatePtr, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
    ]);

    fn event(
        this: &mut Self,
        _: &ZwpTabletSeatV2,
        event: zwp_tablet_seat_v2::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let zwp_tablet_seat_v2::Event::ToolAdded { id } = event {
            let client = this.get_client();
            let mut state = client.borrow_mut();
            let cursor = state
                .globals
                .cursor_shape_manager
                .as_ref()
                .map(|manager| manager.get_tablet_tool_v2(&id, qh, ()));
            state.tablet_tools.insert(
                id.id(),
                TabletToolState {
                    tool: Some(id),
                    cursor,
                    ..Default::default()
                },
            );
        }
    }
}

impl Dispatch<ZwpTabletV2, ()> for WaylandClientStatePtr {
    fn event(
        _: &mut Self,
        tablet: &ZwpTabletV2,
        event: zwp_tablet_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, zwp_tablet_v2::Event::Removed) {
            tablet.destroy();
        }
    }
}

impl Dispatch<ZwpTabletPadV2, ()> for WaylandClientStatePtr {
    event_created_child!(WaylandClientStatePtr, ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
    ]);
    fn event(
        _: &mut Self,
        pad: &ZwpTabletPadV2,
        event: zwp_tablet_pad_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, zwp_tablet_pad_v2::Event::Removed) {
            pad.destroy();
        }
    }
}

impl Dispatch<ZwpTabletPadGroupV2, ()> for WaylandClientStatePtr {
    event_created_child!(WaylandClientStatePtr, ZwpTabletPadGroupV2, [
        zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
        zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
    ]);
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadGroupV2,
        _: zwp_tablet_pad_group_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletToolV2, ()> for WaylandClientStatePtr {
    fn event(
        this: &mut Self,
        tool: &ZwpTabletToolV2,
        event: zwp_tablet_tool_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let client = this.get_client();
        let mut state = client.borrow_mut();
        // Resolve the target before borrowing this tool's mutable frame state.
        let entered_window = if let zwp_tablet_tool_v2::Event::ProximityIn {
            surface, serial, ..
        } = &event
        {
            state.serial_tracker.update(SerialKind::MouseEnter, *serial);
            get_window(&mut state, &surface.id())
        } else {
            None
        };
        if let zwp_tablet_tool_v2::Event::Down { serial } = &event {
            state.serial_tracker.update(SerialKind::MousePress, *serial);
        }
        let style = state.cursor_style.unwrap_or(CursorStyle::Arrow);
        let modifiers = state.modifiers;
        let Some(pen) = state.tablet_tools.get_mut(&tool.id()) else {
            return;
        };
        match event {
            zwp_tablet_tool_v2::Event::Capability {
                capability: WEnum::Value(zwp_tablet_tool_v2::Capability::Pressure),
            } => {
                pen.pressure = Some(0.);
            }
            zwp_tablet_tool_v2::Event::ProximityIn { serial, .. } => {
                pen.reset();
                pen.window = entered_window;
                pen.enter_serial = serial;
                pen.set_cursor_style(style);
                pen.pending.push(Action::Move(None, false));
            }
            zwp_tablet_tool_v2::Event::Motion { x, y } => {
                pen.position = point(px(x as f32), px(y as f32));
                let button = if pen.down {
                    Some(MouseButton::Left)
                } else {
                    pen.side_button
                };
                pen.pending.push(Action::Move(button, pen.down));
            }
            zwp_tablet_tool_v2::Event::Pressure { pressure } => {
                pen.pressure = Some(pressure as f32 / 65535.);
                if pen.pending.is_empty() {
                    let button = if pen.down {
                        Some(MouseButton::Left)
                    } else {
                        pen.side_button
                    };
                    pen.pending.push(Action::Move(button, pen.down));
                }
            }
            zwp_tablet_tool_v2::Event::Tilt { tilt_x, tilt_y } => {
                pen.tilt = (tilt_x as f32, tilt_y as f32);
                if pen.pending.is_empty() {
                    let button = if pen.down {
                        Some(MouseButton::Left)
                    } else {
                        pen.side_button
                    };
                    pen.pending.push(Action::Move(button, pen.down));
                }
            }
            zwp_tablet_tool_v2::Event::Down { .. } => {
                pen.down = true;
                pen.pending.push(Action::Down(MouseButton::Left));
            }
            zwp_tablet_tool_v2::Event::Up => {
                pen.down = false;
                pen.pending.push(Action::Up(MouseButton::Left));
            }
            zwp_tablet_tool_v2::Event::Button {
                button,
                state: WEnum::Value(button_state),
                ..
            } => {
                // BTN_STYLUS/BTN_STYLUS2 map to the usual pen side buttons.
                let button = match button {
                    0x14b => MouseButton::Right,
                    0x14c => MouseButton::Middle,
                    _ => return,
                };
                if button_state == zwp_tablet_tool_v2::ButtonState::Pressed {
                    pen.side_button = Some(button);
                    pen.pending.push(Action::Down(button));
                } else {
                    pen.side_button = None;
                    pen.pending.push(Action::Up(button));
                }
            }
            zwp_tablet_tool_v2::Event::ProximityOut => pen.leave(),
            zwp_tablet_tool_v2::Event::Frame { .. } | zwp_tablet_tool_v2::Event::Removed => {
                let removed = matches!(event, zwp_tablet_tool_v2::Event::Removed);
                if removed {
                    pen.leave();
                }
                let window = pen.window.clone();
                let events = pen.frame(modifiers);
                let exited = events
                    .iter()
                    .any(|(event, _)| matches!(event, PlatformInput::MouseExited(_)));
                let position = pen.position;
                if exited {
                    pen.reset();
                }
                if removed {
                    if let Some(pen) = state.tablet_tools.remove(&tool.id()) {
                        pen.destroy();
                    }
                }
                state.mouse_location = Some(position);
                drop(state);
                if let Some(window) = window {
                    window.set_hovered(true);
                    for (input, sample) in events {
                        with_pen_input(sample, || window.handle_input(input));
                    }
                    if exited {
                        window.set_hovered(false);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_pairs_tip_down_and_motion_with_pressure_and_surface_coordinates() {
        let mut tool = TabletToolState {
            down: true,
            ..Default::default()
        };
        tool.pending.push(Action::Down(MouseButton::Left));
        tool.pending
            .push(Action::Move(Some(MouseButton::Left), true));
        // Pressure/tilt commonly follow motion, before the frame delimiter.
        tool.position = point(px(123.5), px(42.));
        tool.pressure = Some(0.3);
        tool.tilt = (20., -15.);
        let events = tool.frame(Modifiers::default());
        assert_eq!(events.len(), 2);
        for (event, sample) in events {
            assert_eq!(sample, PenInput::new(Some(0.3), (20., -15.), true));
            match event {
                PlatformInput::MouseDown(event) => assert_eq!(event.position, tool.position),
                PlatformInput::MouseMove(event) => {
                    assert_eq!(event.position, tool.position);
                    assert_eq!(event.pressed_button, Some(MouseButton::Left));
                }
                _ => panic!("unexpected event"),
            }
        }
        assert!(tool.frame(Modifiers::default()).is_empty());
    }

    #[test]
    fn proximity_loss_releases_the_tip_and_side_button_before_exit() {
        let mut tool = TabletToolState {
            down: true,
            pressure: Some(0.8),
            side_button: Some(MouseButton::Right),
            ..Default::default()
        };
        tool.leave();
        let events = tool.frame(Modifiers::default());
        assert!(
            matches!(&events[0].0, PlatformInput::MouseUp(event) if event.button == MouseButton::Left)
        );
        assert!(
            matches!(&events[1].0, PlatformInput::MouseUp(event) if event.button == MouseButton::Right)
        );
        assert!(matches!(&events[2].0, PlatformInput::MouseExited(_)));
        assert!(events.iter().all(|(_, sample)| !sample.down));
        tool.reset();
        assert_eq!(tool.pressure, Some(0.));
        assert_eq!(tool.tilt, (0., 0.));
        assert!(tool.pending.is_empty());
    }

    #[test]
    fn hover_does_not_become_a_stroke_and_missing_pressure_stays_optional() {
        let mut tool = TabletToolState::default();
        tool.pending.push(Action::Move(None, false));
        let events = tool.frame(Modifiers::default());
        assert!(!events[0].1.down);
        assert_eq!(events[0].1.pressure, None);
        assert!(
            matches!(&events[0].0, PlatformInput::MouseMove(event) if event.pressed_button.is_none())
        );
    }
}
