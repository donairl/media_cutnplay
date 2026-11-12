//! Material Design 3 (dark scheme) tokens and widget styles.
//!
//! Colors follow the M3 baseline dark palette; shapes, state layers and
//! type sizes follow the M3 component specs as closely as iced allows.

use iced::font::Weight;
use iced::theme::Palette;
use crate::ui::icons;
use iced::widget::{button, container, progress_bar, row, scrollable, slider, text, Row, Text};
use iced::{
    border, widget, Alignment, Background, Border, Color, Element, Font, Shadow, Theme, Vector,
};

// ---- Color roles -----------------------------------------------------------

pub const PRIMARY: Color = Color::from_rgb8(0xD0, 0xBC, 0xFF);
pub const ON_PRIMARY: Color = Color::from_rgb8(0x38, 0x1E, 0x72);
pub const PRIMARY_CONTAINER: Color = Color::from_rgb8(0x4F, 0x37, 0x8B);
pub const ON_PRIMARY_CONTAINER: Color = Color::from_rgb8(0xEA, 0xDD, 0xFF);
pub const SECONDARY_CONTAINER: Color = Color::from_rgb8(0x4A, 0x44, 0x58);
pub const ON_SECONDARY_CONTAINER: Color = Color::from_rgb8(0xE8, 0xDE, 0xF8);
pub const TERTIARY: Color = Color::from_rgb8(0xEF, 0xB8, 0xC8);
pub const ERROR: Color = Color::from_rgb8(0xF2, 0xB8, 0xB5);
pub const SUCCESS: Color = Color::from_rgb8(0xA8, 0xDA, 0xB5);

pub const SURFACE: Color = Color::from_rgb8(0x14, 0x12, 0x18);
pub const SURFACE_CONTAINER_LOW: Color = Color::from_rgb8(0x1D, 0x1B, 0x20);
pub const SURFACE_CONTAINER: Color = Color::from_rgb8(0x21, 0x1F, 0x26);
pub const SURFACE_CONTAINER_HIGH: Color = Color::from_rgb8(0x2B, 0x29, 0x30);
pub const SURFACE_CONTAINER_HIGHEST: Color = Color::from_rgb8(0x36, 0x34, 0x3B);
pub const ON_SURFACE: Color = Color::from_rgb8(0xE6, 0xE0, 0xE9);
pub const ON_SURFACE_VARIANT: Color = Color::from_rgb8(0xCA, 0xC4, 0xD0);
pub const INVERSE_SURFACE: Color = Color::from_rgb8(0xE6, 0xE0, 0xE9);
pub const INVERSE_ON_SURFACE: Color = Color::from_rgb8(0x32, 0x2F, 0x35);
pub const OUTLINE: Color = Color::from_rgb8(0x93, 0x8F, 0x99);
pub const OUTLINE_VARIANT: Color = Color::from_rgb8(0x49, 0x45, 0x4F);

// ---- Shape -----------------------------------------------------------------

pub const RADIUS_SMALL: f32 = 8.0;
pub const RADIUS_MEDIUM: f32 = 12.0;
pub const RADIUS_LARGE: f32 = 16.0;
pub const RADIUS_FULL: f32 = 999.0;

/// Gap between panes and outer window margin.
pub const GUTTER: f32 = 12.0;

// ---- State layers ----------------------------------------------------------

const HOVER: f32 = 0.08;
const PRESSED: f32 = 0.12;
const DISABLED_CONTAINER: f32 = 0.12;
const DISABLED_CONTENT: f32 = 0.38;

fn mix(base: Color, overlay: Color, amount: f32) -> Color {
    Color {
        r: base.r + (overlay.r - base.r) * amount,
        g: base.g + (overlay.g - base.g) * amount,
        b: base.b + (overlay.b - base.b) * amount,
        a: base.a,
    }
}

/// Container color with the content color laid over it as a state layer.
fn state_layer(container: Option<Color>, content: Color, amount: f32) -> Color {
    match container {
        Some(base) => mix(base, content, amount),
        None => content.scale_alpha(amount),
    }
}

// ---- Application theme -----------------------------------------------------

pub fn app_theme() -> Theme {
    Theme::custom(
        "Material 3 Dark",
        Palette {
            background: SURFACE,
            text: ON_SURFACE,
            primary: PRIMARY,
            success: SUCCESS,
            warning: TERTIARY,
            danger: ERROR,
        },
    )
}

// ---- Typography ------------------------------------------------------------

const MEDIUM: Font = Font {
    weight: Weight::Medium,
    ..Font::DEFAULT
};

/// Title medium: pane headers.
pub fn title<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(16).font(MEDIUM).color(ON_SURFACE)
}

/// Title small in the primary color: section headers.
pub fn section<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(14).font(MEDIUM).color(PRIMARY)
}

/// Label large: button labels. Color comes from the button style.
pub fn label<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(14).font(MEDIUM)
}

/// Leading icon followed by a button label.
pub fn icon_label<'a, Message: 'a>(glyph: char, content: &'a str) -> Row<'a, Message> {
    row![icons::icon(glyph, 18), label(content)]
        .spacing(8)
        .align_y(Alignment::Center)
}

/// Body medium.
pub fn body<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(14).color(ON_SURFACE)
}

/// Body small in the variant color: supporting text.
pub fn supporting<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(12).color(ON_SURFACE_VARIANT)
}

// ---- Containers ------------------------------------------------------------

fn filled_container(color: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(color)),
        border: border::rounded(radius),
        ..container::Style::default()
    }
}

/// Window background.
pub fn surface(_theme: &Theme) -> container::Style {
    filled_container(SURFACE, 0.0)
}

/// A pane: large-shape filled card.
pub fn card(_theme: &Theme) -> container::Style {
    filled_container(SURFACE_CONTAINER_LOW, RADIUS_LARGE)
}

/// A card nested inside a pane.
pub fn inner_card(_theme: &Theme) -> container::Style {
    filled_container(SURFACE_CONTAINER_HIGH, RADIUS_MEDIUM)
}

/// Placeholder shown where the video will appear.
pub fn placeholder(_theme: &Theme) -> container::Style {
    filled_container(SURFACE_CONTAINER, RADIUS_MEDIUM)
}

pub fn video_backdrop(_theme: &Theme) -> container::Style {
    filled_container(Color::BLACK, 0.0)
}

/// Transport bar hovering over fullscreen video.
pub fn floating_bar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color { a: 0.92, ..SURFACE_CONTAINER_HIGH }.into()),
        shadow: iced::Shadow {
            color: Color { a: 0.5, ..Color::BLACK },
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 24.0,
        },
        ..filled_container(SURFACE_CONTAINER_HIGH, RADIUS_LARGE)
    }
}

pub fn status_bar(_theme: &Theme) -> container::Style {
    filled_container(SURFACE_CONTAINER, RADIUS_FULL)
}

/// Outlined chip used for read-only values such as timecodes.
pub fn chip(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(ON_SURFACE_VARIANT),
        border: Border {
            color: OUTLINE_VARIANT,
            width: 1.0,
            radius: RADIUS_SMALL.into(),
        },
        ..container::Style::default()
    }
}

/// Plain tooltip: inverse surface, extra-small shape.
pub fn tooltip(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(INVERSE_ON_SURFACE),
        ..filled_container(INVERSE_SURFACE, 4.0)
    }
}

// ---- Buttons ---------------------------------------------------------------

/// Padding for common (pill) buttons: 40px tall with a 14px label.
pub const BUTTON_PADDING: [f32; 2] = [10.0, 24.0];
/// Padding for icon-only buttons holding a 20px icon (40px target).
pub const ICON_PADDING: f32 = 10.0;
/// Padding for compact text buttons.
pub const COMPACT_PADDING: [f32; 2] = [8.0, 12.0];

fn button_style(
    container: Option<Color>,
    content: Color,
    outline: Option<Color>,
    radius: f32,
    status: button::Status,
) -> button::Style {
    let (background, text_color, outline) = match status {
        button::Status::Active => (container, content, outline),
        button::Status::Hovered => (
            Some(state_layer(container, content, HOVER)),
            content,
            outline,
        ),
        button::Status::Pressed => (
            Some(state_layer(container, content, PRESSED)),
            content,
            outline,
        ),
        button::Status::Disabled => (
            container.map(|_| ON_SURFACE.scale_alpha(DISABLED_CONTAINER)),
            ON_SURFACE.scale_alpha(DISABLED_CONTENT),
            outline.map(|_| ON_SURFACE.scale_alpha(DISABLED_CONTAINER)),
        ),
    };

    button::Style {
        background: background.map(Background::Color),
        text_color,
        border: Border {
            color: outline.unwrap_or(Color::TRANSPARENT),
            width: if outline.is_some() { 1.0 } else { 0.0 },
            radius: radius.into(),
        },
        ..button::Style::default()
    }
}

/// Filled button: the single most important action on screen.
pub fn filled(_theme: &Theme, status: button::Status) -> button::Style {
    button_style(Some(PRIMARY), ON_PRIMARY, None, RADIUS_FULL, status)
}

/// Filled tonal button: secondary actions.
pub fn tonal(_theme: &Theme, status: button::Status) -> button::Style {
    button_style(
        Some(SECONDARY_CONTAINER),
        ON_SECONDARY_CONTAINER,
        None,
        RADIUS_FULL,
        status,
    )
}

/// Outlined button: medium-emphasis actions.
pub fn outlined(_theme: &Theme, status: button::Status) -> button::Style {
    button_style(None, PRIMARY, Some(OUTLINE), RADIUS_FULL, status)
}

/// Text button: low-emphasis actions.
pub fn text_button(_theme: &Theme, status: button::Status) -> button::Style {
    button_style(None, PRIMARY, None, RADIUS_FULL, status)
}

/// Toggle icon button: tonal container while selected.
pub fn icon_toggle(selected: bool, status: button::Status) -> button::Style {
    if selected {
        button_style(
            Some(SECONDARY_CONTAINER),
            ON_SECONDARY_CONTAINER,
            None,
            RADIUS_FULL,
            status,
        )
    } else {
        button_style(None, ON_SURFACE_VARIANT, None, RADIUS_FULL, status)
    }
}

/// Standard icon button.
pub fn icon_button(_theme: &Theme, status: button::Status) -> button::Style {
    button_style(None, ON_SURFACE_VARIANT, None, RADIUS_FULL, status)
}

/// Extended FAB: the play / pause control.
pub fn fab(_theme: &Theme, status: button::Status) -> button::Style {
    let style = button_style(
        Some(PRIMARY_CONTAINER),
        ON_PRIMARY_CONTAINER,
        None,
        RADIUS_LARGE,
        status,
    );

    if status == button::Status::Disabled {
        return style;
    }

    button::Style {
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.3),
            offset: Vector::new(0.0, 2.0),
            blur_radius: if status == button::Status::Hovered { 10.0 } else { 6.0 },
        },
        ..style
    }
}

/// List item; the selected one carries the secondary container color.
pub fn list_item(selected: bool, status: button::Status) -> button::Style {
    if selected {
        button_style(
            Some(SECONDARY_CONTAINER),
            ON_SECONDARY_CONTAINER,
            None,
            RADIUS_MEDIUM,
            status,
        )
    } else {
        button_style(None, ON_SURFACE, None, RADIUS_MEDIUM, status)
    }
}

// ---- Slider ----------------------------------------------------------------

/// Height the scrubber must be given so the handle matches the M3 spec.
pub const SLIDER_HEIGHT: f32 = 44.0;

/// M3 slider: thick rounded track and a thin bar handle separated from the
/// track by a gap. The gap is the handle's border, painted in the color of
/// the surface the slider sits on (a pane card).
pub fn scrubber(_theme: &Theme, status: slider::Status) -> slider::Style {
    // The bar narrows while it is being dragged.
    let gap = if status == slider::Status::Dragged { 5.0 } else { 4.0 };

    slider::Style {
        rail: slider::Rail {
            backgrounds: (PRIMARY.into(), SECONDARY_CONTAINER.into()),
            width: 16.0,
            border: border::rounded(RADIUS_SMALL),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Rectangle {
                width: 12,
                border_radius: 6.0.into(),
            },
            background: PRIMARY.into(),
            border_width: gap,
            border_color: SURFACE_CONTAINER_LOW,
        },
    }
}

// ---- Progress --------------------------------------------------------------

pub const PROGRESS_GIRTH: f32 = 4.0;

pub fn progress(_theme: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: SECONDARY_CONTAINER.into(),
        bar: PRIMARY.into(),
        border: border::rounded(PROGRESS_GIRTH / 2.0),
    }
}

// ---- Scrollable ------------------------------------------------------------

pub fn scroll(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let active = matches!(
        status,
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered: true,
            ..
        } | scrollable::Status::Dragged {
            is_vertical_scrollbar_dragged: true,
            ..
        }
    );

    let rail = scrollable::Rail {
        background: None,
        border: border::rounded(RADIUS_FULL),
        scroller: scrollable::Scroller {
            background: if active { ON_SURFACE_VARIANT } else { OUTLINE_VARIANT }.into(),
            border: border::rounded(RADIUS_FULL),
        },
    };

    scrollable::Style {
        vertical_rail: rail,
        horizontal_rail: rail,
        ..scrollable::default(theme, status)
    }
}

// ---- Shared pieces ---------------------------------------------------------

/// Wraps an icon-only control with a plain tooltip naming its action.
pub fn with_tooltip<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    hint: &'a str,
    position: widget::tooltip::Position,
) -> Element<'a, Message> {
    widget::tooltip(
        content,
        container(text(hint).size(12))
            .padding([4.0, 8.0])
            .style(tooltip),
        position,
    )
    .gap(4)
    .into()
}
