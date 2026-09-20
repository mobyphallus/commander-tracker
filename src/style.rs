use iced::widget::{button, container, text, Button};
use iced::{Border, Color, Length, Shadow, Theme, Vector};

// Sizing tuned for the machine this runs on: a 3000x2000 panel at 1.6x
// scale, so ~1875x1210 logical px, ~6.5 logical px per mm. A finger needs
// roughly 9mm, hence the 60px floor on anything tappable.
/// Standard tappable row/button height.
pub const TOUCH_H: f32 = 76.0;
/// Primary call-to-action height.
pub const TOUCH_H_LG: f32 = 104.0;
/// Page padding and the gap between major blocks.
pub const GAP: u16 = 18;

/// A button sized for fingers, with its label centered. Callers still set
/// width, style and `on_press`.
pub fn touch_button<'a, Msg: 'a>(label: &'a str, size: u16) -> Button<'a, Msg> {
    sized_button(label, size, TOUCH_H)
}

/// A taller button for the main action on a screen.
pub fn cta_button<'a, Msg: 'a>(label: &'a str, size: u16) -> Button<'a, Msg> {
    sized_button(label, size, TOUCH_H_LG)
}

fn sized_button<'a, Msg: 'a>(label: &'a str, size: u16, height: f32) -> Button<'a, Msg> {
    button(
        container(text(label).size(size))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .padding(0)
    .height(Length::Fixed(height))
}

pub fn app_theme() -> Theme {
    let palette = iced::theme::Palette {
        background: Color::from_rgb8(0x15, 0x13, 0x1a),
        text: Color::from_rgb8(0xEE, 0xE9, 0xF4),
        primary: Color::from_rgb8(0xCE, 0xA2, 0x4B),
        success: Color::from_rgb8(0x5E, 0xB9, 0x7B),
        danger: Color::from_rgb8(0xD9, 0x6B, 0x5E),
    };
    Theme::custom("Commander Pod".to_string(), palette)
}

/// The page background, and every mid-level panel that sits on it.
pub fn panel(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(p.background.weak.color.into()),
        text_color: Some(p.background.weak.text),
        border: Border {
            color: p.background.strong.color,
            width: 1.0,
            radius: 14.0.into(),
        },
        shadow: Shadow {
            color: Color::BLACK,
            offset: Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
        ..container::Style::default()
    }
}

/// Same as `panel`, but with a bright accent border to call out the active
/// seat / selected item.
pub fn panel_active(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        border: Border {
            color: p.primary.base.color,
            width: 3.0,
            radius: 14.0.into(),
        },
        ..panel(theme)
    }
}

/// A panel that flags something needs attention (e.g. a seat at lethal damage).
pub fn panel_danger(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        border: Border {
            color: p.danger.base.color,
            width: 3.0,
            radius: 14.0.into(),
        },
        ..panel(theme)
    }
}

/// A translucent dark "frosted" chip. iced can't do a real backdrop blur
/// without a custom shader, so this is the stand-in that keeps text legible
/// over bright commander art.
pub fn glass(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.62).into()),
        text_color: Some(Color::WHITE),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.25),
            width: 1.0,
            radius: 18.0.into(),
        },
        ..container::Style::default()
    }
}

/// Heavier version for the big life number, which sits directly on the art.
pub fn glass_strong(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.72).into()),
        text_color: Some(Color::WHITE),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.3),
            width: 1.0,
            radius: 26.0.into(),
        },
        ..container::Style::default()
    }
}

/// Small round chip behind the +/- glyphs.
pub fn glass_round(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
        text_color: Some(Color::WHITE),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.28),
            width: 1.0,
            radius: 44.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn header(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(p.primary.weak.color.into()),
        text_color: Some(p.primary.weak.text),
        border: Border {
            radius: 14.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}
