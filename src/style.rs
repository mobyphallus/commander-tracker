//! The app's design system.
//!
//! Screens are meant to say *what a thing is* - a panel, the primary action,
//! a seat that needs attention - and get its look from here. Nothing outside
//! this module should invent a colour, a corner radius or a shadow.
//!
//! The house style: a near-black surface stack with a violet cast, lit by a
//! single dark-purple accent. Colour is a scarce resource - the accent marks
//! the one thing on a screen that matters (the active seat, the primary
//! action) and nothing else. Depth comes from four flat surface steps plus
//! hairline borders rather than from heavy shadows, which is what keeps it
//! reading as sleek instead of as a stack of grey boxes.

use iced::border::Radius;
use iced::theme::palette;
use iced::widget::{button, column, container, text, text_input, Button};
use iced::{
    gradient, Alignment, Background, Border, Color, Degrees, Gradient, Length, Shadow, Theme,
    Vector,
};

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

/// `0xRRGGBB` to a [`Color`], usable in a `const`.
const fn hex(rgb: u32) -> Color {
    Color::from_rgb(
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
    )
}

/// The surface stack, darkest first. Every opaque background in the app is
/// one of these four - a fifth shade of near-black is always a mistake.
///
/// `0` is the page, `1` is anything resting on it (panels, rows, cards),
/// `2` is anything raised off that (hover, selection, plain buttons), and
/// `3` is reserved for hairlines and borders rather than fills.
pub const SURFACE_0: Color = hex(0x0B0910);
pub const SURFACE_1: Color = hex(0x151120);
pub const SURFACE_2: Color = hex(0x1F1830);
pub const SURFACE_3: Color = hex(0x2E2542);

/// The accent ramp. [`ACCENT`] is the house purple; [`ACCENT_DEEP`] is for
/// large tinted fills where full saturation would shout, and
/// [`ACCENT_BRIGHT`] for hover and focus, where it has to visibly lift.
pub const ACCENT_DEEP: Color = hex(0x281A45);
pub const ACCENT: Color = hex(0x7C3AED);
pub const ACCENT_BRIGHT: Color = hex(0x9F74FF);

/// Ink. [`TEXT_MUTED`] is for captions and secondary lines and is the
/// dimmest thing allowed to carry words - anything fainter is decoration.
pub const TEXT: Color = hex(0xEDE9F7);
pub const TEXT_MUTED: Color = hex(0xA79CC0);
/// Ink for text sitting on an accent fill.
pub const TEXT_ON_ACCENT: Color = hex(0xF8F5FF);

/// Status colours. Deliberately desaturated next to the accent so a win or a
/// lethal seat reads as *status*, not as a second brand colour.
pub const SUCCESS: Color = hex(0x35C48A);
pub const DANGER: Color = hex(0xE5484D);

/// Hairline that separates without drawing a line you actually notice.
pub const HAIRLINE: Color = Color { a: 0.55, ..SURFACE_3 };

// ---------------------------------------------------------------------------
// Shape, spacing and type
// ---------------------------------------------------------------------------

/// Corner radii. Bigger than a desktop app would use, on purpose: this runs
/// on a touchscreen where soft, chunky shapes read as tappable.
pub const R_SM: f32 = 10.0;
pub const R_MD: f32 = 16.0;
pub const R_LG: f32 = 22.0;
/// Fully rounded - chips, pills and the round counter glyphs.
pub const R_PILL: f32 = 999.0;

// Sizing tuned for the machine this runs on: a 3000x2000 panel at 1.6x
// scale, so ~1875x1210 logical px, ~6.5 logical px per mm. A finger needs
// roughly 9mm, hence the 60px floor on anything tappable.
/// Standard tappable row/button height.
pub const TOUCH_H: f32 = 76.0;
/// Primary call-to-action height.
pub const TOUCH_H_LG: f32 = 104.0;
/// Page padding and the gap between major blocks.
pub const GAP: u16 = 18;

/// The type scale. These are the sizes the app already settled on, pinned
/// down so screens stop inventing neighbours two pixels apart. A size not on
/// this list should be rare and deliberate.
///
/// [`T_ACTION`] is the default for button labels; [`T_COUNTER`] is the life
/// total on a seat tile and belongs to nothing else.
pub const T_MICRO: u16 = 11;
pub const T_CAPTION: u16 = 16;
pub const T_BODY: u16 = 18;
pub const T_LABEL: u16 = 20;
pub const T_ACTION: u16 = 22;
pub const T_SUBHEAD: u16 = 26;
pub const T_LEAD: u16 = 30;
pub const T_HEADING: u16 = 34;
pub const T_TITLE: u16 = 40;
pub const T_DISPLAY: u16 = 54;
pub const T_COUNTER: u16 = 76;

/// Elevation. Shadows are violet-black rather than neutral black so they
/// tint the surface underneath instead of dirtying it.
fn shadow(blur: f32, y: f32, alpha: f32) -> Shadow {
    Shadow {
        color: Color { a: alpha, ..hex(0x05030A) },
        offset: Vector::new(0.0, y),
        blur_radius: blur,
    }
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

/// The app theme.
///
/// The extended palette is written out by hand rather than derived from a
/// five-colour [`palette::Palette`]. iced's generator mixes `secondary` out
/// of the background and the text colour, which produces a neutral grey -
/// and since `secondary` is what every plain button in the app lands on,
/// that grey is most of what you'd see. Spelling the ramps out keeps the
/// violet cast in the surfaces, where it does the work.
pub fn app_theme() -> Theme {
    Theme::custom_with_fn("Commander Pod".to_string(), base_palette(), |_| extended())
}

fn base_palette() -> palette::Palette {
    palette::Palette {
        background: SURFACE_0,
        text: TEXT,
        primary: ACCENT,
        success: SUCCESS,
        danger: DANGER,
    }
}

fn pair(color: Color, text: Color) -> palette::Pair {
    palette::Pair { color, text }
}

fn extended() -> palette::Extended {
    palette::Extended {
        background: palette::Background {
            base: pair(SURFACE_0, TEXT),
            weak: pair(SURFACE_1, TEXT),
            // Deliberately lighter than SURFACE_2: iced spends
            // `background.strong` on text-input placeholders and scrollbar
            // thumbs, which have to be legible rather than structural.
            strong: pair(hex(0x6E6288), TEXT),
        },
        primary: palette::Primary {
            base: pair(ACCENT, TEXT_ON_ACCENT),
            weak: pair(ACCENT_DEEP, TEXT),
            strong: pair(ACCENT_BRIGHT, hex(0x140A24)),
        },
        secondary: palette::Secondary {
            base: pair(SURFACE_2, TEXT),
            weak: pair(SURFACE_1, TEXT_MUTED),
            strong: pair(SURFACE_3, TEXT),
        },
        success: palette::Success {
            base: pair(SUCCESS, hex(0x052015)),
            weak: pair(hex(0x123528), TEXT),
            strong: pair(hex(0x4EE0A4), hex(0x052015)),
        },
        danger: palette::Danger {
            base: pair(DANGER, hex(0x2A0709)),
            weak: pair(hex(0x3A1518), TEXT),
            strong: pair(hex(0xFF6168), hex(0x2A0709)),
        },
        is_dark: true,
    }
}

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

/// A button sized for fingers, with its label centered. Callers still set
/// width, style and `on_press`. The label takes anything `text` accepts, so
/// a computed `String` (a countdown, a player's name) works as well as a
/// literal.
pub fn touch_button<'a, Msg: 'a>(label: impl text::IntoFragment<'a>, size: u16) -> Button<'a, Msg> {
    sized_button(label, size, TOUCH_H)
}

/// The edge of a square choice tile. Big enough to hold its own on a
/// full-screen question and to be an easy target from across a table, while
/// a whole scale of them still fits one row.
pub const TILE: f32 = 230.0;

/// One option in a set you pick from by tapping: a value, and a word saying
/// what the value means.
///
/// Deliberately [`secondary`]. A screen that asks you to choose between
/// equal options has no single most-important one, so none of them takes
/// the accent - it stays on the header. Filling every tile with the accent
/// would make the whole screen shout and say nothing.
pub fn choice_tile<'a, Msg: 'a>(
    value: impl text::IntoFragment<'a>,
    label: impl text::IntoFragment<'a>,
) -> Button<'a, Msg> {
    button(
        container(
            column![
                text(value).size(T_DISPLAY),
                text(label).size(T_CAPTION).color(TEXT_MUTED),
            ]
            .spacing(2)
            .align_x(Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .padding(0)
    .width(Length::Fixed(TILE))
    .height(Length::Fixed(TILE))
    .style(secondary)
}

/// A taller button for the main action on a screen.
pub fn cta_button<'a, Msg: 'a>(label: impl text::IntoFragment<'a>, size: u16) -> Button<'a, Msg> {
    sized_button(label, size, TOUCH_H_LG)
}

fn sized_button<'a, Msg: 'a>(
    label: impl text::IntoFragment<'a>,
    size: u16,
    height: f32,
) -> Button<'a, Msg> {
    button(
        container(text(label).size(size))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .padding(0)
    .height(Length::Fixed(height))
}

fn button_base(radius: f32) -> button::Style {
    button::Style {
        background: None,
        text_color: TEXT,
        border: Border {
            color: Color::TRANSPARENT,
            width: 1.0,
            radius: Radius::from(radius),
        },
        shadow: shadow(0.0, 0.0, 0.0),
    }
}

fn dim(style: button::Style) -> button::Style {
    button::Style {
        background: style.background.map(|b| b.scale_alpha(0.4)),
        text_color: style.text_color.scale_alpha(0.4),
        border: Border { color: style.border.color.scale_alpha(0.4), ..style.border },
        shadow: shadow(0.0, 0.0, 0.0),
    }
}

/// The one action on a screen you actually want tapped. Accent-filled, with
/// a top-to-bottom wash so it reads as a lit surface rather than a flat
/// swatch, and the only button carrying a shadow.
pub fn primary(_theme: &Theme, status: button::Status) -> button::Style {
    let (top, bottom, lift) = match status {
        button::Status::Hovered => (ACCENT_BRIGHT, ACCENT, 14.0),
        button::Status::Pressed => (ACCENT_DEEP, ACCENT, 4.0),
        _ => (ACCENT, hex(0x5B27B8), 10.0),
    };
    let base = button::Style {
        background: Some(wash(top, bottom)),
        text_color: TEXT_ON_ACCENT,
        shadow: shadow(lift * 1.6, lift * 0.4, 0.45),
        ..button_base(R_MD)
    };
    match status {
        button::Status::Disabled => dim(base),
        _ => base,
    }
}

/// Everything else. A raised surface with a hairline - quiet enough that a
/// screen full of them doesn't compete with the one accent button on it.
pub fn secondary(_theme: &Theme, status: button::Status) -> button::Style {
    let (fill, line) = match status {
        button::Status::Hovered => (SURFACE_3, ACCENT_BRIGHT.scale_alpha(0.55)),
        button::Status::Pressed => (SURFACE_1, ACCENT.scale_alpha(0.7)),
        _ => (SURFACE_2, HAIRLINE),
    };
    let base = button::Style {
        background: Some(fill.into()),
        text_color: TEXT,
        border: Border { color: line, ..button_base(R_MD).border },
        ..button_base(R_MD)
    };
    match status {
        button::Status::Disabled => dim(base),
        _ => base,
    }
}

/// Destructive or "they're out" actions. Tinted rather than filled, so a
/// mis-tap is less likely than with a solid red slab.
pub fn danger(_theme: &Theme, status: button::Status) -> button::Style {
    tinted(DANGER, hex(0x3A1518), status)
}

/// Confirmations - declaring a winner, ending a game.
pub fn success(_theme: &Theme, status: button::Status) -> button::Style {
    tinted(SUCCESS, hex(0x123528), status)
}

fn tinted(accent: Color, fill: Color, status: button::Status) -> button::Style {
    let (bg, line, ink) = match status {
        button::Status::Hovered => (accent.scale_alpha(0.28), accent, accent),
        button::Status::Pressed => (accent.scale_alpha(0.4), accent, TEXT),
        _ => (fill.into(), accent.scale_alpha(0.55), accent),
    };
    let base = button::Style {
        background: Some(bg.into()),
        text_color: ink,
        border: Border { color: line, ..button_base(R_MD).border },
        ..button_base(R_MD)
    };
    match status {
        button::Status::Disabled => dim(base),
        _ => base,
    }
}

/// No fill at all - for "Cancel", "Back" and other ways out, which should be
/// findable without being the loudest thing on the screen.
pub fn ghost(_theme: &Theme, status: button::Status) -> button::Style {
    let (fill, line, ink) = match status {
        button::Status::Hovered => (SURFACE_2.into(), HAIRLINE, TEXT),
        button::Status::Pressed => (SURFACE_1.into(), HAIRLINE, TEXT),
        _ => (Color::TRANSPARENT, Color::TRANSPARENT, TEXT_MUTED),
    };
    let base = button::Style {
        background: Some(Background::Color(fill)),
        text_color: ink,
        border: Border { color: line, ..button_base(R_MD).border },
        ..button_base(R_MD)
    };
    match status {
        button::Status::Disabled => dim(base),
        _ => base,
    }
}

/// A button that matches the frosted chips used for the life total and the
/// player name, so on-tile controls all read as one family.
pub fn glass_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (bg, line) = match status {
        button::Status::Hovered | button::Status::Pressed => (0.78, ACCENT_BRIGHT.scale_alpha(0.6)),
        _ => (0.62, Color { a: 0.22, ..TEXT }),
    };
    button::Style {
        background: Some(Color { a: bg, ..SCRIM }.into()),
        text_color: TEXT,
        border: Border { color: line, width: 1.0, radius: Radius::from(R_LG) },
        shadow: shadow(0.0, 0.0, 0.0),
    }
}

/// A vertical wash between two colours. The whole app's gradients are this
/// shape - top-lit, never diagonal, never more than two stops.
fn wash(top: Color, bottom: Color) -> Background {
    Background::Gradient(Gradient::Linear(
        gradient::Linear::new(Degrees(180.0))
            .add_stop(0.0, top)
            .add_stop(1.0, bottom),
    ))
}

// ---------------------------------------------------------------------------
// Containers
// ---------------------------------------------------------------------------

/// The page background, and every mid-level panel that sits on it.
pub fn panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_1.into()),
        text_color: Some(TEXT),
        border: Border { color: HAIRLINE, width: 1.0, radius: Radius::from(R_MD) },
        shadow: shadow(14.0, 4.0, 0.35),
        ..container::Style::default()
    }
}

/// Same as [`panel`], but lit by the accent to call out the active seat or
/// the selected item. The glow does the work - the border is there to
/// survive being seen on a screen at arm's length across a table.
pub fn panel_active(theme: &Theme) -> container::Style {
    container::Style {
        border: Border { color: ACCENT, width: 3.0, radius: Radius::from(R_MD) },
        shadow: Shadow {
            color: Color { a: 0.5, ..ACCENT },
            offset: Vector::new(0.0, 0.0),
            blur_radius: 26.0,
        },
        ..panel(theme)
    }
}

/// A panel that flags something needs attention (e.g. a seat at lethal damage).
pub fn panel_danger(theme: &Theme) -> container::Style {
    container::Style {
        border: Border { color: DANGER, width: 3.0, radius: Radius::from(R_MD) },
        shadow: Shadow {
            color: Color { a: 0.45, ..DANGER },
            offset: Vector::new(0.0, 0.0),
            blur_radius: 24.0,
        },
        ..panel(theme)
    }
}

/// The title block at the top of a screen. The only large accent-tinted
/// surface in the app, which is what makes it read as the top of the page
/// without needing a rule under it.
pub fn header(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(wash(ACCENT_DEEP, SURFACE_1)),
        text_color: Some(TEXT),
        border: Border { color: HAIRLINE, width: 1.0, radius: Radius::from(R_LG) },
        shadow: shadow(18.0, 6.0, 0.3),
        ..container::Style::default()
    }
}

// --- Over-art surfaces -----------------------------------------------------
//
// Commander art is arbitrary and often bright, so anything laid over it gets
// a dark scrim instead of a theme surface. iced can't do a real backdrop blur
// without a custom shader, so these translucent chips are the stand-in.

/// The base for every over-art scrim: violet-black rather than pure black,
/// so a tile that's half art and half chip still reads as one surface.
const SCRIM: Color = hex(0x08060E);

fn frosted(alpha: f32, radius: f32, border_alpha: f32) -> container::Style {
    container::Style {
        background: Some(Color { a: alpha, ..SCRIM }.into()),
        text_color: Some(TEXT),
        border: Border {
            color: Color { a: border_alpha, ..TEXT },
            width: 1.0,
            radius: Radius::from(radius),
        },
        ..container::Style::default()
    }
}

/// The paint of a frosted chip, for the seat labels that have to be *drawn*
/// rather than laid out. Those rotate to face the player sitting opposite
/// or side-on, which no container style can do, so they draw themselves -
/// and take their colours from here rather than inventing any.
pub struct ChipPaint {
    pub background: Color,
    pub border: Color,
    pub radius: f32,
}

/// Paint matching [`glass`].
pub fn glass_paint() -> ChipPaint {
    ChipPaint {
        background: Color { a: 0.62, ..SCRIM },
        border: Color { a: 0.22, ..TEXT },
        radius: R_LG,
    }
}

/// Paint matching [`glass_strong`].
pub fn glass_strong_paint() -> ChipPaint {
    ChipPaint {
        background: Color { a: 0.72, ..SCRIM },
        border: Color { a: 0.26, ..TEXT },
        radius: R_LG + 4.0,
    }
}

/// Paint for a chip that has to be found at a glance rather than read past:
/// the end-turn control on whichever seat's turn it is. The one piece of
/// accent that appears on the board itself.
pub fn accent_chip_paint() -> ChipPaint {
    ChipPaint {
        background: Color { a: 0.88, ..ACCENT },
        border: Color { a: 0.95, ..ACCENT_BRIGHT },
        radius: R_LG,
    }
}

/// A translucent chip for labels sitting over commander art.
pub fn glass(_theme: &Theme) -> container::Style {
    frosted(0.62, R_LG, 0.22)
}

/// Heavier version for the big life number, which sits directly on the art.
pub fn glass_strong(_theme: &Theme) -> container::Style {
    frosted(0.72, R_LG + 4.0, 0.26)
}

/// Small round chip behind the +/- glyphs.
pub fn glass_round(_theme: &Theme) -> container::Style {
    frosted(0.55, R_PILL, 0.24)
}

/// A pill for the game timer floating in the middle of the table.
pub fn glass_pill(_theme: &Theme) -> container::Style {
    frosted(0.55, R_PILL, 0.24)
}

/// A near-opaque cover over a whole tile - the seat action menu, and the
/// "eliminated" state. Dark enough to read against any art underneath.
pub fn scrim(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color { a: 0.88, ..SCRIM }.into()),
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

/// Frame around the partner commander's inset art, so the second commander
/// reads as a deliberate inset rather than a rendering glitch.
pub fn art_inset(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color { a: 0.5, ..SCRIM }.into()),
        border: Border {
            color: Color { a: 0.6, ..TEXT },
            width: 2.0,
            radius: Radius::from(R_SM),
        },
        shadow: shadow(12.0, 2.0, 0.6),
        ..container::Style::default()
    }
}

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// Text fields, matched to the button shapes. iced's default is a 2px-radius
/// box that looks nothing like the rest of this app.
pub fn input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let line = match status {
        text_input::Status::Focused => ACCENT_BRIGHT,
        text_input::Status::Hovered => SURFACE_3,
        _ => HAIRLINE,
    };
    text_input::Style {
        background: SURFACE_1.into(),
        border: Border { color: line, width: 1.0, radius: Radius::from(R_SM) },
        icon: TEXT_MUTED,
        placeholder: TEXT_MUTED,
        value: TEXT,
        selection: Color { a: 0.45, ..ACCENT },
    }
}
