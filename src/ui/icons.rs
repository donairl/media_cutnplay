//! Material Symbols (Rounded, filled) icons.
//!
//! The bundled font is a subset holding only the glyphs listed here. To add
//! an icon, re-download the subset with the new name in `icon_names`
//! (see `assets/fonts/README.md`) and add its codepoint below.

use iced::widget::{text, Text};
use iced::{Font, Pixels};

/// Font file contents; register with `iced::application(..).font(..)`.
pub const FONT_BYTES: &[u8] =
    include_bytes!("../../assets/fonts/MaterialSymbolsRounded-subset.ttf");

const FONT: Font = Font::with_name("Material Symbols Rounded Filled");

pub const ADD: char = '\u{e145}';
pub const CHECK_CIRCLE: char = '\u{e86c}';
pub const CLOSE: char = '\u{e5cd}';
pub const CONTENT_CUT: char = '\u{e14e}';
pub const DELETE: char = '\u{e872}';
pub const ERROR: char = '\u{e000}';
pub const FAST_FORWARD: char = '\u{e01f}';
pub const FAST_REWIND: char = '\u{e020}';
pub const FOLDER_OPEN: char = '\u{e2c8}';
pub const FULLSCREEN: char = '\u{e5d0}';
pub const FULLSCREEN_EXIT: char = '\u{e5d1}';
pub const LEFT_PANEL_CLOSE: char = '\u{f717}';
pub const LEFT_PANEL_OPEN: char = '\u{f716}';
pub const PAUSE: char = '\u{e034}';
pub const PLAY_ARROW: char = '\u{e037}';
pub const RIGHT_PANEL_CLOSE: char = '\u{f705}';
pub const RIGHT_PANEL_OPEN: char = '\u{f704}';
pub const SKIP_NEXT: char = '\u{e044}';
pub const SKIP_PREVIOUS: char = '\u{e045}';

/// An icon glyph. Its color is inherited from the surrounding widget
/// unless set explicitly.
pub fn icon<'a>(glyph: char, size: impl Into<Pixels>) -> Text<'a> {
    text(glyph.to_string())
        .font(FONT)
        .size(size)
        .line_height(1.0)
}
