//! The app's design system.
//!
//! Screens are meant to say *what a thing is* - a panel, the primary action,
//! a seat that needs attention - and get its look from here. Nothing outside
//! this module should invent a colour, a corner radius or a shadow.
//!
//! Neutral charcoal surfaces keep commander art in the foreground. Purple is
//! reserved for actions, selection and the current turn. See STYLE_GUIDE.md.

use iced::border::Radius;
use iced::theme::palette;
use iced::widget::{button, column, container, row, text, text_input, Button};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Theme, Vector};

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

/// The neutral surface stack, darkest first. Every opaque background in the app is
/// one of these four - a fifth shade of near-black is always a mistake.
///
/// `0` is the page, `1` is anything resting on it (panels, rows, cards),
/// `2` is anything raised off that (hover, selection, plain buttons), and
/// `3` is reserved for hairlines and borders rather than fills.
pub const SURFACE_0: Color = hex(0x101114);
pub const SURFACE_1: Color = hex(0x191B20);
pub const SURFACE_2: Color = hex(0x24272E);
pub const SURFACE_3: Color = hex(0x363A44);

/// The accent ramp. [`ACCENT`] is the house purple; [`ACCENT_DEEP`] is for
/// large tinted fills where full saturation would shout, and
/// [`ACCENT_BRIGHT`] for hover and focus, where it has to visibly lift.
pub const ACCENT_DEEP: Color = hex(0x2B2340);
pub const ACCENT: Color = hex(0x8B5CF6);
pub const ACCENT_BRIGHT: Color = hex(0xB69AFF);

/// Ink. [`TEXT_MUTED`] is for captions and secondary lines and is the
/// dimmest thing allowed to carry words - anything fainter is decoration.
pub const TEXT: Color = hex(0xF3F4F7);
pub const TEXT_MUTED: Color = hex(0xADB1BD);
/// Ink for text sitting on an accent fill.
pub const TEXT_ON_ACCENT: Color = hex(0xF8F5FF);
/// Status colours. Deliberately desaturated next to the accent so a win or a
/// lethal seat reads as *status*, not as a second brand colour.
pub const SUCCESS: Color = hex(0x35C48A);
pub const DANGER: Color = hex(0xE5484D);

/// Hairline that separates without drawing a line you actually notice.
pub const HAIRLINE: Color = Color {
    a: 0.55,
    ..SURFACE_3
};

// ---------------------------------------------------------------------------
// Shape, spacing and type
// ---------------------------------------------------------------------------

/// Corner radii. Bigger than a desktop app would use, on purpose: this runs
/// on a touchscreen where soft, chunky shapes read as tappable.
pub const R_SM: f32 = 12.0;
pub const R_MD: f32 = 16.0;
pub const R_LG: f32 = 20.0;
/// Fully rounded - chips, pills and the round counter glyphs.
pub const R_PILL: f32 = 999.0;

// Sizing tuned for the machine this runs on: a 3000x2000 panel at 1.6x
// scale, so ~1875x1210 logical px, ~6.5 logical px per mm. A finger needs
// roughly 9mm, hence the 60px floor on anything tappable.
/// Standard tappable row/button height.
pub const TOUCH_H: f32 = 72.0;
/// Primary call-to-action height.
pub const TOUCH_H_LG: f32 = 88.0;
/// Page padding and the gap between major blocks.
pub const GAP: u16 = 16;
/// Spacing sub-units, for inside a block rather than between blocks.
/// Use these steps and their multiples for shared component spacing.
pub const GAP_SM: u16 = GAP / 2;
pub const GAP_XS: u16 = GAP / 4;

/// Vertical padding that lands a text field exactly on [`TOUCH_H`], given a
/// [`T_SUBHEAD`] value inside it. Derived rather than eyeballed so every
/// field in the app is the same height as every button beside it.
pub const FIELD_PAD: f32 = (TOUCH_H - T_SUBHEAD as f32 * 1.3) / 2.0;

/// The type scale. These are the sizes the app already settled on, pinned
/// down so screens stop inventing neighbours two pixels apart. A size not on
/// this list should be rare and deliberate.
///
/// [`T_ACTION`] is the default for button labels; [`T_COUNTER`] is the life
/// total on a seat tile and belongs to nothing else.
pub const T_MICRO: u16 = 13;
pub const T_CAPTION: u16 = 16;
pub const T_BODY: u16 = 18;
pub const T_LABEL: u16 = 20;
pub const T_ACTION: u16 = 22;
pub const T_SUBHEAD: u16 = 24;
pub const T_LEAD: u16 = 30;
pub const T_HEADING: u16 = 32;
pub const T_TITLE: u16 = 36;
pub const T_DISPLAY: u16 = 48;
pub const T_COUNTER: u16 = 76;
/// Player identity stays distinct from action labels on the board.
pub const T_PLAYER_NAME: u16 = 28;

/// Elevation. Shadows are neutral and reserved for over-art elements.
fn shadow(blur: f32, y: f32, alpha: f32) -> Shadow {
    Shadow {
        color: Color {
            a: alpha,
            ..hex(0x050608)
        },
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
/// neutral surfaces and purple action states consistent.
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
            strong: pair(hex(0x737986), TEXT),
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

/// A name you pick out of a short list - a player taking a seat.
///
/// Deliberately much bigger than a row or a button. At setup the screen
/// holds five names and nothing else, and a control sized for a dense list
/// leaves most of the table empty while making the one thing on it hard to
/// hit from a chair.
pub const NAME_TILE_W: f32 = 280.0;
pub const NAME_TILE_H: f32 = 112.0;

pub fn name_tile<'a, Msg: 'a>(label: impl text::IntoFragment<'a>) -> Button<'a, Msg> {
    sized_button(label, T_TITLE, NAME_TILE_H).width(Length::Fixed(NAME_TILE_W))
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
        border: Border {
            color: style.border.color.scale_alpha(0.4),
            ..style.border
        },
        shadow: shadow(0.0, 0.0, 0.0),
    }
}

/// A restrained solid accent with high-contrast ink, reserved for the next action.
pub fn primary(_theme: &Theme, status: button::Status) -> button::Style {
    let fill = match status {
        button::Status::Hovered => hex(0x7C4BDF),
        button::Status::Pressed => ACCENT_DEEP,
        _ => hex(0x7040CF),
    };
    let base = button::Style {
        background: Some(fill.into()),
        text_color: TEXT_ON_ACCENT,
        ..button_base(R_MD)
    };
    if matches!(status, button::Status::Disabled) {
        dim(base)
    } else {
        base
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
        border: Border {
            color: line,
            ..button_base(R_MD).border
        },
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
        border: Border {
            color: line,
            ..button_base(R_MD).border
        },
        ..button_base(R_MD)
    };
    match status {
        button::Status::Disabled => dim(base),
        _ => base,
    }
}

/// A tappable row in a list. Panel-shaped and flat: a history entry or a
/// player row is something you pick, not a button you press, and a column of
/// lifted slabs reads as noise.
pub fn row_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (fill, line) = match status {
        button::Status::Hovered => (SURFACE_2, ACCENT.scale_alpha(0.45)),
        button::Status::Pressed => (SURFACE_0, ACCENT.scale_alpha(0.6)),
        _ => (SURFACE_1, HAIRLINE),
    };
    let base = button::Style {
        background: Some(fill.into()),
        text_color: TEXT,
        border: Border {
            color: line,
            ..button_base(R_MD).border
        },
        ..button_base(R_MD)
    };
    match status {
        button::Status::Disabled => dim(base),
        _ => base,
    }
}

/// A destructive action at rest. Red enough to be recognised, light enough
/// that it doesn't shout from a row of harmless controls - it only fills in
/// when the finger is already on it. The solid [`danger`] fill is for the
/// moment of consequence: the confirm step, not the way into it.
pub fn danger_ghost(_theme: &Theme, status: button::Status) -> button::Style {
    let (fill, line, ink) = match status {
        button::Status::Hovered => (DANGER.scale_alpha(0.16), DANGER, DANGER),
        button::Status::Pressed => (DANGER.scale_alpha(0.3), DANGER, TEXT),
        _ => (
            Color::TRANSPARENT,
            DANGER.scale_alpha(0.35),
            DANGER.scale_alpha(0.85),
        ),
    };
    let base = button::Style {
        background: Some(Background::Color(fill)),
        text_color: ink,
        border: Border {
            color: line,
            ..button_base(R_MD).border
        },
        ..button_base(R_MD)
    };
    match status {
        button::Status::Disabled => dim(base),
        _ => base,
    }
}

/// A picture tile picked out of a grid, with its actions now showing
/// elsewhere. Lit at the edge rather than filled: the card art is the
/// content, and an accent wash over it would repaint the card's own
/// colours.
pub fn tile_selected(_theme: &Theme, status: button::Status) -> button::Style {
    let line = match status {
        button::Status::Pressed => ACCENT,
        _ => ACCENT_BRIGHT,
    };
    button::Style {
        background: Some(SURFACE_2.into()),
        text_color: TEXT,
        border: Border {
            color: line,
            width: 2.0,
            radius: Radius::from(R_MD),
        },
        shadow: Shadow::default(),
    }
}

/// One letter on the app's own keyboard. Flatter and tighter-cornered than
/// a [`secondary`] button on purpose: thirty slabs, each with its own
/// hairline and 16px corner, read as thirty separate objects instead of one
/// keyboard. The accent only appears under the finger, which on a block
/// this dense is the only feedback that lands.
pub fn key(_theme: &Theme, status: button::Status) -> button::Style {
    let (fill, ink) = match status {
        button::Status::Hovered => (SURFACE_3, TEXT),
        button::Status::Pressed => (ACCENT, TEXT_ON_ACCENT),
        _ => (SURFACE_2, TEXT),
    };
    button::Style {
        background: Some(fill.into()),
        text_color: ink,
        border: Border {
            color: HAIRLINE,
            ..button_base(R_SM).border
        },
        ..button_base(R_SM)
    }
}

/// A key that changes what the other keys do, or acts on the whole field:
/// shift, delete, the symbol layer. Sunk a step below the letters so the
/// alphabet stays the foreground - but only a step. SURFACE_1 on a
/// near-black page is very nearly the page, so the hairline is what keeps
/// these reading as keys rather than as words floating under the keyboard.
pub fn key_modifier(_theme: &Theme, status: button::Status) -> button::Style {
    let (fill, ink) = match status {
        button::Status::Hovered => (SURFACE_2, TEXT),
        button::Status::Pressed => (ACCENT, TEXT_ON_ACCENT),
        _ => (SURFACE_1, TEXT_MUTED),
    };
    button::Style {
        background: Some(fill.into()),
        text_color: ink,
        border: Border {
            color: HAIRLINE,
            ..button_base(R_SM).border
        },
        ..button_base(R_SM)
    }
}

/// A modifier that's currently doing something - shift held down, the
/// symbol layer showing. Lit so its state is readable at a glance rather
/// than inferred from the letters.
pub fn key_modifier_on(_theme: &Theme, status: button::Status) -> button::Style {
    let fill = match status {
        button::Status::Pressed => ACCENT_DEEP,
        _ => ACCENT,
    };
    button::Style {
        background: Some(fill.into()),
        text_color: TEXT_ON_ACCENT,
        ..button_base(R_SM)
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
        border: Border {
            color: line,
            ..button_base(R_MD).border
        },
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
        button::Status::Hovered | button::Status::Pressed => (0.96, ACCENT_BRIGHT.scale_alpha(0.6)),
        _ => (0.9, Color { a: 0.22, ..TEXT }),
    };
    button::Style {
        background: Some(Color { a: bg, ..SCRIM }.into()),
        text_color: TEXT,
        border: Border {
            color: line,
            width: 1.0,
            radius: Radius::from(R_LG),
        },
        shadow: shadow(0.0, 0.0, 0.0),
    }
}

/// Scores need a near-opaque surface to stay readable over pale card art.
pub fn score_button(theme: &Theme, status: button::Status) -> button::Style {
    let mut paint = glass_button(theme, status);
    paint.background = Some(match status {
        button::Status::Pressed => ACCENT_DEEP.into(),
        button::Status::Hovered => SURFACE_2.into(),
        _ => Color {
            a: 0.94,
            ..SURFACE_1
        }
        .into(),
    });
    paint.border.radius = R_MD.into();
    paint
}

// ---------------------------------------------------------------------------
// Containers
// ---------------------------------------------------------------------------

/// The page background, and every mid-level panel that sits on it.
pub fn panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_1.into()),
        text_color: Some(TEXT),
        border: Border {
            color: HAIRLINE,
            width: 1.0,
            radius: Radius::from(R_MD),
        },
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// Same as [`panel`], but lit by the accent to call out the active seat or
/// the selected item. A clear border allows the state to
/// remain visible at arm's length across a table.
pub fn panel_active(theme: &Theme) -> container::Style {
    container::Style {
        border: Border {
            color: ACCENT,
            width: 3.0,
            radius: Radius::from(R_MD),
        },
        shadow: Shadow::default(),
        ..panel(theme)
    }
}

/// A small inline label - a count, a colour identity, a status word. Sits on
/// a surface rather than on art, which is what separates it from [`glass`].
// ---------------------------------------------------------------------------
// Mana
// ---------------------------------------------------------------------------

/// The five colours as Magic itself prints them: a pale disc with the
/// symbol in dark ink.
///
/// Deliberately the card-face colours rather than anything from the palette
/// above - this is the one place the house style gives way. A player reads a
/// mana symbol by its colour and silhouette, and tinting these
/// violet to match the app would cost exactly the recognition they exist
/// for. Anything that isn't WUBRG is colourless.
pub fn mana_color(symbol: char) -> Color {
    match symbol {
        'W' => hex(0xFFFBD5),
        'U' => hex(0xAAE0FA),
        'B' => hex(0xCBC2BF),
        'R' => hex(0xF9AA8F),
        'G' => hex(0x9BD3AE),
        _ => hex(0xCAC5C0),
    }
}

/// Ink for the symbol inside a pip. Near-black rather than the app's text
/// colour, which would vanish on these pale discs.
pub const MANA_INK: Color = hex(0x1B1410);

pub fn badge(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_2.into()),
        text_color: Some(TEXT_MUTED),
        border: Border {
            color: HAIRLINE,
            width: 1.0,
            radius: Radius::from(R_PILL),
        },
        ..container::Style::default()
    }
}

/// A panel that flags something needs attention (e.g. a seat at lethal damage).
pub fn panel_danger(theme: &Theme) -> container::Style {
    container::Style {
        border: Border {
            color: DANGER,
            width: 3.0,
            radius: Radius::from(R_MD),
        },
        shadow: Shadow::default(),
        ..panel(theme)
    }
}

/// Quiet page chrome. Purple belongs on the actionable content, not every header.
pub fn header(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT),
        ..Default::default()
    }
}

/// Shared title and back control for non-game screens.
pub fn page_header<'a, Msg: Clone + 'a>(
    title: impl text::IntoFragment<'a>,
    subtitle: impl text::IntoFragment<'a>,
    back: Msg,
) -> Element<'a, Msg> {
    container(
        row![
            icon_button(crate::icon::Glyph::Back, "Back", T_LABEL)
                .width(120)
                .style(ghost)
                .on_press(back),
            column![
                text(title).size(T_TITLE),
                text(subtitle).size(T_CAPTION).color(TEXT_MUTED)
            ]
            .spacing(GAP_XS)
            .width(Length::Fill),
        ]
        .spacing(GAP)
        .align_y(Alignment::Center),
    )
    .padding([GAP_SM, 0])
    .width(Length::Fill)
    .style(header)
    .into()
}

/// Labeled touch action. Icons are decorative and never the only cue.
pub fn icon_button<'a, Msg: 'a>(
    glyph: crate::icon::Glyph,
    label: impl text::IntoFragment<'a>,
    size: u16,
) -> Button<'a, Msg> {
    button(
        container(
            row![crate::icon::view(glyph, 24.0, TEXT), text(label).size(size)]
                .spacing(GAP_SM)
                .align_y(Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .padding([0, GAP])
    .height(TOUCH_H)
}

/// A row in a table you read rather than tap. Flatter than [`panel`]: ten
/// bordered, shadowed slabs down a page read as ten objects instead of one
/// list.
pub fn table_row(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_1.into()),
        text_color: Some(TEXT),
        border: Border {
            color: HAIRLINE,
            width: 1.0,
            radius: Radius::from(R_SM),
        },
        ..container::Style::default()
    }
}

/// The track and the fill of a comparison bar. Two figures are far easier to
/// compare as lengths than as numbers, so these exist to be used together.
pub fn meter_track(_theme: &Theme) -> container::Style {
    meter(SURFACE_2)
}

pub fn meter_fill(_theme: &Theme) -> container::Style {
    meter(ACCENT)
}

fn meter(fill: Color) -> container::Style {
    container::Style {
        background: Some(fill.into()),
        border: Border {
            radius: Radius::from(R_PILL),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

// --- Over-art surfaces -----------------------------------------------------
//
// Commander art is arbitrary and often bright, so anything laid over it gets
// a dark scrim instead of a theme surface. iced can't do a real backdrop blur
// without a custom shader, so these translucent chips are the stand-in.

/// The neutral-black base for every over-art scrim,
/// so a tile that's half art and half chip still reads as one surface.
const SCRIM: Color = hex(0x080A0D);

fn frosted(alpha: f32, radius: f32, border_alpha: f32) -> container::Style {
    container::Style {
        background: Some(Color { a: alpha, ..SCRIM }.into()),
        text_color: Some(TEXT),
        border: Border {
            color: Color {
                a: border_alpha,
                ..TEXT
            },
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
        background: Color { a: 0.90, ..SCRIM },
        border: Color { a: 0.22, ..TEXT },
        radius: R_LG,
    }
}

/// Paint matching [`glass_strong`].
pub fn glass_strong_paint() -> ChipPaint {
    ChipPaint {
        background: Color { a: 0.94, ..SCRIM },
        border: Color { a: 0.26, ..TEXT },
        radius: R_LG + 4.0,
    }
}

/// Paint for a chip that has to be found at a glance rather than read past:
/// the end-turn control on whichever seat's turn it is. The one piece of
/// accent that appears on the board itself.
pub fn accent_chip_paint() -> ChipPaint {
    ChipPaint {
        background: Color {
            a: 0.98,
            ..ACCENT_DEEP
        },
        border: Color {
            a: 0.95,
            ..ACCENT_BRIGHT
        },
        radius: R_LG,
    }
}

/// A translucent chip for labels sitting over commander art.
pub fn glass(_theme: &Theme) -> container::Style {
    frosted(0.90, R_LG, 0.22)
}

/// Heavier version for the big life number, which sits directly on the art.
pub fn glass_strong(_theme: &Theme) -> container::Style {
    frosted(0.94, R_LG + 4.0, 0.26)
}

/// Small round chip behind the +/- glyphs.
pub fn glass_round(_theme: &Theme) -> container::Style {
    frosted(0.82, R_PILL, 0.24)
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
        border: Border {
            color: line,
            width: 1.0,
            radius: Radius::from(R_SM),
        },
        icon: TEXT_MUTED,
        placeholder: TEXT_MUTED,
        value: TEXT,
        selection: Color { a: 0.45, ..ACCENT },
    }
}
