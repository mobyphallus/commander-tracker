use iced::widget::container;
use iced::{Border, Color, Shadow, Theme, Vector};

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
