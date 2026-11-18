//! Logical-pixel normalization for one GPUI-hosted retained viewport. This owns
//! neither window callbacks nor focus/capture; hosts install those explicitly.
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct GpuiInputAdapter {
    pub bounds: Rect,
    /// Logical pixels per platform wheel line; pixel deltas remain unchanged.
    pub wheel_line_height: f32,
}
impl GpuiInputAdapter {
    pub fn new(bounds: Rect, wheel_line_height: f32) -> Result<Self, Error> {
        if [
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            wheel_line_height,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || bounds.width < 0.0
            || bounds.height < 0.0
            || wheel_line_height <= 0.0
        {
            return Err(Error::InvalidViewport);
        }
        Ok(Self {
            bounds,
            wheel_line_height,
        })
    }
    pub fn position(&self, position: gpui::Point<gpui::Pixels>) -> [f32; 2] {
        [
            f32::from(position.x) - self.bounds.x,
            f32::from(position.y) - self.bounds.y,
        ]
    }
    pub fn contains(&self, position: gpui::Point<gpui::Pixels>) -> bool {
        let [x, y] = self.position(position);
        x >= 0.0 && y >= 0.0 && x < self.bounds.width && y < self.bounds.height
    }
    pub fn mouse_move(&self, event: &gpui::MouseMoveEvent, captured: bool) -> UiInput {
        if captured || self.contains(event.position) {
            UiInput::PointerMove(self.position(event.position))
        } else {
            UiInput::PointerLeave
        }
    }
    pub fn mouse_down(&self, event: &gpui::MouseDownEvent) -> Option<UiInput> {
        (event.button == gpui::MouseButton::Left && self.contains(event.position))
            .then(|| UiInput::PrimaryDown(self.position(event.position)))
    }
    pub fn mouse_up(&self, event: &gpui::MouseUpEvent, captured: bool) -> Option<UiInput> {
        (event.button == gpui::MouseButton::Left && (captured || self.contains(event.position)))
            .then(|| UiInput::PrimaryUp(self.position(event.position)))
    }
    pub fn wheel(&self, event: &gpui::ScrollEvent) -> Option<UiInput> {
        if !self.contains(event.position) {
            return None;
        }
        let delta = event.delta.pixel_delta(gpui::px(self.wheel_line_height));
        Some(UiInput::Wheel {
            position: self.position(event.position),
            delta: [-f32::from(delta.x), -f32::from(delta.y)],
        })
    }
    pub fn key_down(event: &gpui::KeyDownEvent) -> Option<UiInput> {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return None;
        }
        match event.keystroke.key.as_str() {
            "tab" if !event.is_held => Some(UiInput::Tab {
                backwards: modifiers.shift,
            }),
            "enter" if !modifiers.shift => Some(UiInput::KeyDown {
                key: UiActivationKey::Enter,
                repeat: event.is_held,
            }),
            "space" | " " if !modifiers.shift => Some(UiInput::KeyDown {
                key: UiActivationKey::Space,
                repeat: event.is_held,
            }),
            "escape" if !modifiers.shift => Some(UiInput::Cancel),
            _ => None,
        }
    }
    pub fn key_up(event: &gpui::KeyUpEvent) -> Option<UiInput> {
        // Release an already accepted activation even if modifiers changed
        // while held. Router ignores unmatched releases; shortcuts stay host-owned.
        match event.keystroke.key.as_str() {
            "enter" => Some(UiInput::KeyUp(UiActivationKey::Enter)),
            "space" | " " => Some(UiInput::KeyUp(UiActivationKey::Space)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gpui_host_normalizes_origin_capture_wheel_units_and_keys() {
        let adapter = GpuiInputAdapter::new(
            Rect {
                x: 20.0,
                y: 30.0,
                width: 100.0,
                height: 60.0,
            },
            16.0,
        )
        .unwrap();
        let point = gpui::point(gpui::px(25.0), gpui::px(35.0));
        assert_eq!(adapter.position(point), [5.0; 2]);
        let up = gpui::MouseUpEvent {
            position: gpui::point(gpui::px(200.0), gpui::px(35.0)),
            button: gpui::MouseButton::Left,
            ..Default::default()
        };
        assert!(adapter.mouse_up(&up, false).is_none());
        assert_eq!(
            adapter.mouse_up(&up, true),
            Some(UiInput::PrimaryUp([180.0, 5.0]))
        );
        let wheel = gpui::ScrollEvent {
            position: point,
            delta: gpui::ScrollDelta::Lines(gpui::point(0.0, -2.0)),
            ..Default::default()
        };
        assert_eq!(
            adapter.wheel(&wheel),
            Some(UiInput::Wheel {
                position: [5.0; 2],
                delta: [0.0, 32.0]
            })
        );
        let event = gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse("shift-tab").unwrap(),
            is_held: false,
            prefer_character_input: false,
        };
        assert_eq!(
            GpuiInputAdapter::key_down(&event),
            Some(UiInput::Tab { backwards: true })
        );
        let event = gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse("ctrl-enter").unwrap(),
            ..event
        };
        assert_eq!(GpuiInputAdapter::key_down(&event), None);
        assert!(
            GpuiInputAdapter::new(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: f32::NAN,
                    height: 0.0
                },
                16.0
            )
            .is_err()
        );
    }
}
