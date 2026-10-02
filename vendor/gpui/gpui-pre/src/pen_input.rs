//! Native pen data accompanying the pointer event currently being dispatched.
// Emulsion addition, distributed under this package's Apache-2.0 license.

use std::cell::Cell;

/// Pressure and tilt from the same native event as a pointer position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenInput {
    /// Normalized pressure, or `None` when the tool does not report pressure.
    pub pressure: Option<f32>,
    /// Tilt in degrees from vertical, on the x and y axes.
    pub tilt: (f32, f32),
    /// Whether the pen tip is touching the drawing surface.
    pub down: bool,
}

impl PenInput {
    /// Normalize device data before exposing it to drawing code.
    pub fn new(pressure: Option<f32>, tilt: (f32, f32), down: bool) -> Self {
        let axis = |v: f32| {
            if v.is_finite() {
                v.clamp(-90., 90.)
            } else {
                0.
            }
        };
        Self {
            pressure: pressure.filter(|v| v.is_finite()).map(|v| v.clamp(0., 1.)),
            tilt: (axis(tilt.0), axis(tilt.1)),
            down,
        }
    }
}

thread_local! {
    static CURRENT_PEN: Cell<Option<PenInput>> = const { Cell::new(None) };
}

/// Native pen data for the input callback currently running on this thread.
/// This is cleared after dispatch; hover and pen-up report `down == false`.
pub fn current_pen_input() -> Option<PenInput> {
    CURRENT_PEN.with(Cell::get)
}

/// Dispatch a pointer event with its native pen data available to listeners.
/// Nested dispatch and unwinding restore the previous event's data.
pub fn with_pen_input<R>(pen: PenInput, dispatch: impl FnOnce() -> R) -> R {
    struct Restore(Option<PenInput>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT_PEN.with(|current| current.set(self.0));
        }
    }
    let pen = PenInput::new(pen.pressure, pen.tilt, pen.down);
    let _restore = Restore(CURRENT_PEN.with(|current| current.replace(Some(pen))));
    dispatch()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pen_data_is_scoped_to_its_event_and_survives_nested_dispatch() {
        let down = PenInput::new(Some(0.25), (15., -30.), true);
        let up = PenInput::new(Some(0.), (0., 0.), false);
        assert_eq!(current_pen_input(), None);
        with_pen_input(down, || {
            assert_eq!(current_pen_input(), Some(down));
            with_pen_input(up, || assert_eq!(current_pen_input(), Some(up)));
            assert_eq!(current_pen_input(), Some(down));
        });
        assert_eq!(current_pen_input(), None);
    }

    #[test]
    fn invalid_native_data_cannot_reach_brush_dynamics() {
        let pen = PenInput::new(Some(f32::NAN), (f32::INFINITY, -120.), true);
        assert_eq!(pen.pressure, None);
        assert_eq!(pen.tilt, (0., -90.));
        assert_eq!(PenInput::new(Some(2.), (0., 0.), true).pressure, Some(1.));
        assert_eq!(PenInput::new(Some(0.), (0., 0.), true).pressure, Some(0.));
    }

    #[test]
    fn panicking_listener_does_not_leave_pen_data_on_mouse_events() {
        let _ = std::panic::catch_unwind(|| {
            with_pen_input(PenInput::new(Some(1.), (0., 0.), true), || {
                panic!("listener")
            });
        });
        assert_eq!(current_pen_input(), None);
    }
}
