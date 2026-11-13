//! Keyboard shortcuts for transport, cut markers and window control.

use crate::state::messages::Message;
use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

/// Seconds skipped by Shift+Left / Shift+Right.
pub const SHORT_JUMP_SECS: f64 = 5.0;
/// Seconds skipped by J / L.
pub const LONG_JUMP_SECS: f64 = 10.0;

/// Maps a key press to its action, or `None` when the key is unbound.
///
/// `repeat` is true for auto-repeated presses of a held key: seeking keys
/// keep firing, toggles and markers fire once.
pub fn message_for(key: &Key, modifiers: Modifiers, repeat: bool) -> Option<Message> {
    if modifiers.alt() || modifiers.logo() {
        return None;
    }

    if modifiers.command() {
        return match key.as_ref() {
            Key::Character(c) if c.eq_ignore_ascii_case("o") && !repeat => {
                Some(Message::OpenFilePicker)
            }
            _ => None,
        };
    }

    let shift = modifiers.shift();

    // Held keys: frame stepping and jumps repeat.
    let seeking = match key.as_ref() {
        Key::Named(Named::ArrowLeft) if shift => Some(Message::SeekBy(-SHORT_JUMP_SECS)),
        Key::Named(Named::ArrowRight) if shift => Some(Message::SeekBy(SHORT_JUMP_SECS)),
        Key::Named(Named::ArrowLeft) => Some(Message::StepFrame(-1)),
        Key::Named(Named::ArrowRight) => Some(Message::StepFrame(1)),
        Key::Character(c) if c.eq_ignore_ascii_case("j") => {
            Some(Message::SeekBy(-LONG_JUMP_SECS))
        }
        Key::Character(c) if c.eq_ignore_ascii_case("l") => Some(Message::SeekBy(LONG_JUMP_SECS)),
        _ => None,
    };
    if seeking.is_some() {
        return seeking;
    }

    if repeat {
        return None;
    }

    match key.as_ref() {
        Key::Named(Named::Space) => Some(Message::TogglePlay),
        Key::Named(Named::Enter) => Some(Message::AddSegment),
        Key::Named(Named::Home) => Some(Message::Seek(0.0)),
        // `Seek` clamps to the duration.
        Key::Named(Named::End) => Some(Message::Seek(f64::MAX)),
        Key::Character(c) => match c.to_ascii_lowercase().as_str() {
            "k" => Some(Message::TogglePlay),
            "i" => Some(Message::SetInPoint),
            "o" => Some(Message::SetOutPoint),
            "x" => Some(Message::ClearPoints),
            "f" => Some(Message::ToggleFullscreen),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    fn press(key: Key) -> Option<Message> {
        message_for(&key, Modifiers::empty(), false)
    }

    #[test]
    fn transport_keys() {
        assert!(matches!(press(Key::Named(Named::Space)), Some(Message::TogglePlay)));
        assert!(matches!(press(ch("k")), Some(Message::TogglePlay)));
        assert!(matches!(press(Key::Named(Named::ArrowLeft)), Some(Message::StepFrame(-1))));
        assert!(matches!(press(Key::Named(Named::ArrowRight)), Some(Message::StepFrame(1))));
        assert!(matches!(press(ch("j")), Some(Message::SeekBy(s)) if s == -LONG_JUMP_SECS));
        assert!(matches!(press(ch("l")), Some(Message::SeekBy(s)) if s == LONG_JUMP_SECS));
        assert!(matches!(press(Key::Named(Named::Home)), Some(Message::Seek(s)) if s == 0.0));
    }

    #[test]
    fn shift_arrows_jump() {
        let jump = message_for(&Key::Named(Named::ArrowRight), Modifiers::SHIFT, false);
        assert!(matches!(jump, Some(Message::SeekBy(s)) if s == SHORT_JUMP_SECS));
    }

    #[test]
    fn marker_and_window_keys_ignore_case() {
        assert!(matches!(press(ch("i")), Some(Message::SetInPoint)));
        assert!(matches!(press(ch("O")), Some(Message::SetOutPoint)));
        assert!(matches!(press(ch("x")), Some(Message::ClearPoints)));
        assert!(matches!(press(Key::Named(Named::Enter)), Some(Message::AddSegment)));
        assert!(matches!(press(ch("F")), Some(Message::ToggleFullscreen)));
    }

    #[test]
    fn repeat_fires_seeking_only() {
        let held = |key: Key| message_for(&key, Modifiers::empty(), true);
        assert!(held(Key::Named(Named::ArrowRight)).is_some());
        assert!(held(ch("l")).is_some());
        assert!(held(Key::Named(Named::Space)).is_none());
        assert!(held(ch("i")).is_none());
        assert!(held(ch("f")).is_none());
    }

    #[test]
    fn modifiers() {
        assert!(matches!(
            message_for(&ch("o"), Modifiers::COMMAND, false),
            Some(Message::OpenFilePicker)
        ));
        assert!(message_for(&ch("i"), Modifiers::COMMAND, false).is_none());
        assert!(message_for(&Key::Named(Named::Space), Modifiers::ALT, false).is_none());
        assert!(press(ch("q")).is_none());
    }
}
