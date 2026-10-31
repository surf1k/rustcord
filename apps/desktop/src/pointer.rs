use eframe::egui::{Event, PointerButton, Pos2, RawInput, pos2};
use ui::scroll::{Middle, SidePress};
use winit::window::Window;

/// Middle and side-button edges stripped from egui input for one frame.
pub struct Intercepted {
	pub middle: Middle,
	pub side: SidePress,
}

/// The window's pointer, translated for Rustcord.
///
/// Holds the two facts `RawInput` cannot carry across frames: whether the middle button is
/// still down after a frame with no events, and the last position the cursor was seen at.
#[derive(Default)]
pub struct Pointer {
	down: bool,
	last: Option<Pos2>,
}

impl Pointer {
	/// Remove middle / Extra1 / Extra2 from `events` and return what scroll and side-nav need.
	/// With `keep_buttons` their presses also stay in `events`, for bindings that use them.
	/// Releases always stay, so egui cannot hold a button whose keep flag changed mid-press.
	/// When `track`, append the OS cursor as a `PointerMoved` so a cursor that has left the
	/// window keeps reporting its distance from the drive origin.
	pub fn intercept(
		&mut self,
		raw: &mut RawInput,
		window: &Window,
		pixels_per_point: f32,
		track: bool,
		keep_buttons: bool,
	) -> Intercepted {
		let intercepted = self.intercept_events(raw, keep_buttons);
		if track
			&& raw.focused
			&& let Some(at) = Self::client_cursor(window, pixels_per_point)
			&& self.last != Some(at)
		{
			// Appended last so a same-batch `PointerGone` from `CursorLeft` is restored.
			self.last = Some(at);
			raw.events.push(Event::PointerMoved(at));
		}
		intercepted
	}

	fn intercept_events(&mut self, raw: &mut RawInput, keep_buttons: bool) -> Intercepted {
		let mut middle = Middle::default();
		let mut side = SidePress::default();
		raw.events.retain(|event| match event {
			// Rustcord uses middle-click for autoscroll, including over text edits.
			Event::MiddleClickPaste { .. } => false,
			Event::PointerButton {
				pos,
				button: PointerButton::Middle,
				pressed,
				..
			} => {
				if *pressed {
					middle.pressed.get_or_insert(*pos);
				}
				self.down = *pressed;
				keep_buttons || !pressed
			}
			Event::PointerButton {
				button: PointerButton::Extra1,
				pressed,
				..
			} => {
				if *pressed {
					side.back = true;
				}
				keep_buttons || !pressed
			}
			Event::PointerButton {
				button: PointerButton::Extra2,
				pressed,
				..
			} => {
				if *pressed {
					side.forward = true;
				}
				keep_buttons || !pressed
			}
			Event::PointerMoved(pos) => {
				self.last = Some(*pos);
				true
			}
			Event::PointerGone | Event::WindowFocused(false) => {
				self.down = false;
				self.last = None;
				middle.pressed = None;
				true
			}
			_ => true,
		});
		if !raw.focused {
			self.down = false;
			middle.pressed = None;
		}
		middle.down = self.down;
		Intercepted { middle, side }
	}

	fn client_cursor(window: &Window, pixels_per_point: f32) -> Option<Pos2> {
		let (x, y) = platform::cursor_position()?;
		let origin = window.inner_position().ok()?;
		Some(pos2(
			(x - f64::from(origin.x)) as f32 / pixels_per_point,
			(y - f64::from(origin.y)) as f32 / pixels_per_point,
		))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn leaving_window_cannot_keep_middle_held_after_a_missed_release() {
		let mut pointer = Pointer::default();
		let press = Event::PointerButton {
			pos: pos2(24.0, 36.0),
			button: PointerButton::Middle,
			pressed: true,
			modifiers: eframe::egui::Modifiers::NONE,
		};
		let mut raw = RawInput {
			focused: true,
			events: vec![press.clone()],
			..Default::default()
		};
		assert!(pointer.intercept_events(&mut raw, false).middle.down);
		assert!(
			raw.events.is_empty(),
			"middle press must not start egui text selection"
		);
		raw.events = vec![Event::PointerGone];
		assert!(!pointer.intercept_events(&mut raw, false).middle.down);
		raw.events.clear();
		assert!(!pointer.intercept_events(&mut raw, false).middle.down);
		// Focus loss may arrive without CursorLeft; it must have the same cancellation behavior.
		raw.events = vec![press];
		assert!(pointer.intercept_events(&mut raw, true).middle.down);
		assert_eq!(
			raw.events.len(),
			1,
			"a bound middle button remains available to keybindings"
		);
		raw.focused = false;
		raw.events.clear();
		assert!(!pointer.intercept_events(&mut raw, true).middle.down);
	}
}
