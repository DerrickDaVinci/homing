//! Detects "the user is dragging files while holding ⌥" so the homing ring
//! can open under the cursor. (Shared with Peekless.)
//!
//! No Accessibility permission and no event tap: we poll three global
//! values the OS exposes to every app, about 30 times a second:
//!   - which mouse buttons are down       (NSEvent.pressedMouseButtons)
//!   - which modifier keys are held       (NSEvent.modifierFlags)
//!   - the drag pasteboard's change count (bumped when any drag starts)
//! A drag that started after the button went down and carries file URLs,
//! with ⌥ held, is what we are looking for.

/// Which modifier key opens the ring.
#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modifier {
    Option,
    Command,
    Control,
    Shift,
}

pub enum Signal {
    /// Open the ring at the cursor.
    Show,
    /// The mouse button was released while the ring was open.
    Release,
}

#[cfg(target_os = "macos")]
pub fn spawn<F, E>(on: F, config: E)
where
    F: Fn(Signal) + Send + 'static,
    E: Fn() -> Option<Modifier> + Send + 'static,
{
    use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSPasteboard, NSPasteboardNameDrag};
    let flag = |m: Modifier| match m {
        Modifier::Option => NSEventModifierFlags::Option,
        Modifier::Command => NSEventModifierFlags::Command,
        Modifier::Control => NSEventModifierFlags::Control,
        Modifier::Shift => NSEventModifierFlags::Shift,
    };
    use std::time::Duration;

    std::thread::spawn(move || {
        let drag = NSPasteboard::pasteboardWithName(unsafe { NSPasteboardNameDrag });
        let has_files = |pb: &NSPasteboard| {
            pb.types().is_some_and(|types| types.iter().any(|t| matches!(t.to_string().as_str(), "public.file-url" | "NSFilenamesPboardType")))
        };
        #[derive(PartialEq)]
        enum State {
            Idle,
            Pressed(isize),
            Shown,
        }
        let mut state = State::Idle;
        loop {
            std::thread::sleep(Duration::from_millis(33));
            let left_down = NSEvent::pressedMouseButtons() & 1 == 1;
            state = match state {
                State::Idle if left_down => State::Pressed(drag.changeCount()),
                State::Idle => State::Idle,
                State::Pressed(_) if !left_down => State::Idle,
                State::Pressed(at_press) => {
                    let held = config().is_some_and(|m| NSEvent::modifierFlags_class().contains(flag(m)));
                    if held && drag.changeCount() != at_press && has_files(&drag) {
                        on(Signal::Show);
                        State::Shown
                    } else {
                        State::Pressed(at_press)
                    }
                }
                State::Shown if !left_down => {
                    on(Signal::Release);
                    State::Idle
                }
                State::Shown => State::Shown,
            };
        }
    });
}

/// Windows needs a different approach (OLE drag detection); until then the
/// ring is macOS-only and Windows users use the window's drop zone.
#[cfg(not(target_os = "macos"))]
pub fn spawn<F, E>(_on: F, _config: E)
where
    F: Fn(Signal) + Send + 'static,
    E: Fn() -> Option<Modifier> + Send + 'static,
{
}
