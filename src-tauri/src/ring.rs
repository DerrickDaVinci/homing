//! Detects "the user is dragging files while holding the trigger keys" so the
//! homing ring can open under the cursor. (Adapted from Peekless.)
//!
//! No Accessibility permission and no event tap: we poll three global
//! values the OS exposes to every app, about 30 times a second:
//!   - which mouse buttons are down       (NSEvent.pressedMouseButtons)
//!   - which modifier keys are held       (NSEvent.modifierFlags)
//!   - the drag pasteboard's change count (bumped when any drag starts)
//! A drag that started after the button went down and carries file URLs,
//! with exactly the trigger keys held, is what we are looking for. "Exactly"
//! lets Homing share drags with other tools (Filee on ⌘, Peekless on ⌥).

use serde::{Deserialize, Serialize};
use std::path::Path;

/// A modifier key that can be part of the trigger.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modifier {
    Control,
    Option,
    Shift,
    Command,
}

/// The trigger keys, sorted and without duplicates; never empty.
pub fn normalize(mut keys: Vec<Modifier>) -> Vec<Modifier> {
    keys.sort();
    keys.dedup();
    if keys.is_empty() {
        keys.push(Modifier::Shift);
    }
    keys
}

/// Another app that also opens something when you drag with keys held.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct OtherRing {
    pub name: &'static str,
    pub keys: Vec<Modifier>,
}

/// Drag gestures of other tools on this machine, read from their settings.
pub fn other_rings(config_dir: &Path) -> Vec<OtherRing> {
    let read = |p: &str| std::fs::read(config_dir.join(p)).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    let mut out = Vec::new();
    // Peekless: {"ring": true, "ringModifier": "option"}
    if let Some(v) = read("app.peekless.desktop/settings.json") {
        if v["ring"].as_bool().unwrap_or(true) {
            let m = serde_json::from_value(v["ringModifier"].clone()).unwrap_or(Modifier::Option);
            out.push(OtherRing { name: "Peekless", keys: vec![m] });
        }
    }
    // Filee: {"triggers": [{"kind": "Drag", "enabled": true, "modifiers": "Meta"}]}
    if let Some(v) = read("Filee/settings.json") {
        for t in v["triggers"].as_array().into_iter().flatten() {
            if t["kind"] == "Drag" && t["enabled"].as_bool().unwrap_or(true) {
                let keys = filee_keys(t["modifiers"].as_str().unwrap_or(""));
                if !keys.is_empty() {
                    out.push(OtherRing { name: "Filee", keys });
                }
            }
        }
    }
    out
}

/// "Ctrl, Alt" -> [Control, Option] (Filee's names, Meta being ⌘ on macOS).
fn filee_keys(s: &str) -> Vec<Modifier> {
    let keys = s.split(',').filter_map(|k| match k.trim() {
        "Ctrl" | "Control" => Some(Modifier::Control),
        "Alt" | "Option" => Some(Modifier::Option),
        "Shift" => Some(Modifier::Shift),
        "Meta" | "Cmd" | "Command" | "Win" => Some(Modifier::Command),
        _ => None,
    });
    let mut v: Vec<_> = keys.collect();
    v.sort();
    v.dedup();
    v
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
    E: Fn() -> Option<Vec<Modifier>> + Send + 'static,
{
    use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSPasteboard, NSPasteboardNameDrag};
    let flag = |m: Modifier| match m {
        Modifier::Option => NSEventModifierFlags::Option,
        Modifier::Command => NSEventModifierFlags::Command,
        Modifier::Control => NSEventModifierFlags::Control,
        Modifier::Shift => NSEventModifierFlags::Shift,
    };
    let all = NSEventModifierFlags::Option | NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Shift;
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
                    let held = config().is_some_and(|keys| {
                        let want = keys.into_iter().fold(NSEventModifierFlags::empty(), |acc, m| acc | flag(m));
                        NSEvent::modifierFlags_class() & all == want
                    });
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
/// ring is macOS-only and Windows users drop folders on the window.
#[cfg(not(target_os = "macos"))]
pub fn spawn<F, E>(_on: F, _config: E)
where
    F: Fn(Signal) + Send + 'static,
    E: Fn() -> Option<Vec<Modifier>> + Send + 'static,
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use Modifier::*;

    #[test]
    fn keys_are_sorted_unique_and_never_empty() {
        assert_eq!(normalize(vec![Shift, Control, Shift]), vec![Control, Shift]);
        assert_eq!(normalize(vec![]), vec![Shift]);
    }

    #[test]
    fn reads_other_tools_settings() {
        let dir = std::env::temp_dir().join(format!("homing-rings-{}", std::process::id()));
        let put = |p: &str, s: &str| {
            let f = dir.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, s).unwrap();
        };
        put("app.peekless.desktop/settings.json", r#"{"ring":true,"ringModifier":"option"}"#);
        put(
            "Filee/settings.json",
            r#"{"triggers":[{"kind":"Drag","enabled":true,"modifiers":"Meta"},{"kind":"KeyChord","modifiers":"Ctrl, Alt"},{"kind":"Drag","enabled":false,"modifiers":"Shift"}]}"#,
        );
        let found = other_rings(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found, vec![OtherRing { name: "Peekless", keys: vec![Option] }, OtherRing { name: "Filee", keys: vec![Command] }]);
        assert_eq!(filee_keys("Ctrl, Alt"), vec![Control, Option]);
    }
}
