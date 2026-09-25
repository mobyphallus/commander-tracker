//! The app's own on-screen keyboard.
//!
//! This runs on a tablet lying flat on the table with nothing plugged into
//! it, and iced 0.13 has neither IME support nor any way to ask the
//! compositor for a keyboard - so a text field here is untypable unless the
//! app draws one itself.
//!
//! It types straight into the `String` behind a field rather than
//! synthesising key events, which means a screen only has to say *which* of
//! its fields is being edited. The field stays an ordinary `text_input`, so
//! a physical keyboard still works alongside this one.

use iced::widget::{button, column, container, row, Space};
use iced::{Element, Length};

use crate::style;

/// A key, as the keyboard reports it. Everything that isn't a character is
/// an edit or a mode change, so the screen owning the field never has to
/// know which key was hit - only what it meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Space,
    Backspace,
    Clear,
    Shift,
    /// Flip between the letters and the numbers/punctuation layer.
    Symbols,
    /// The field's own action - what its Enter key does.
    Submit,
    /// Put the keyboard away.
    Done,
}

/// What the screen should do once a key has been applied to its field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Text or a modifier changed. Nothing further to do.
    Typed,
    /// Run the field's action, the same as its Enter key.
    Submit,
    /// The keyboard has closed itself.
    Done,
}

/// Which field the keyboard is typing into, and the state of its modifiers.
/// Generic over the owning screen's field id so each screen names its own
/// fields.
#[derive(Debug, Clone)]
pub struct Keyboard<F> {
    field: Option<F>,
    shift: bool,
    symbols: bool,
}

impl<F> Default for Keyboard<F> {
    fn default() -> Self {
        Self {
            field: None,
            shift: false,
            symbols: false,
        }
    }
}

impl<F: Copy + PartialEq> Keyboard<F> {
    /// Opens the keyboard on `field`, whose text is currently `value`. An
    /// empty field starts shifted: every name this app asks for - a
    /// player's, a commander's - is a proper noun.
    pub fn open(&mut self, field: F, value: &str) {
        if self.field != Some(field) {
            self.symbols = false;
        }
        self.field = Some(field);
        self.shift = value.is_empty();
    }

    pub fn close(&mut self) {
        self.field = None;
        self.shift = false;
        self.symbols = false;
    }

    pub fn field(&self) -> Option<F> {
        self.field
    }

    /// Applies `key` to `value` and reports what the screen should do next.
    ///
    /// Everything is appended at the end of the text. The caret in the
    /// `text_input` is iced's own and this never moves it, so typing lands
    /// where the eye expects even after the value has been replaced from
    /// under the widget.
    pub fn press(&mut self, key: Key, value: &mut String) -> Outcome {
        match key {
            Key::Char(c) => {
                if self.shift {
                    value.extend(c.to_uppercase());
                    self.shift = false;
                } else {
                    value.push(c);
                }
                Outcome::Typed
            }
            Key::Space => {
                value.push(' ');
                self.shift = false;
                Outcome::Typed
            }
            Key::Backspace => {
                value.pop();
                Outcome::Typed
            }
            Key::Clear => {
                value.clear();
                self.shift = true;
                Outcome::Typed
            }
            Key::Shift => {
                self.shift = !self.shift;
                Outcome::Typed
            }
            Key::Symbols => {
                self.symbols = !self.symbols;
                Outcome::Typed
            }
            Key::Submit => Outcome::Submit,
            Key::Done => {
                self.close();
                Outcome::Done
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Layout
//
// Rows are laid out in twentieths rather than in pixels, so every letter is
// the same width whether its row holds ten of them or seven, and the whole
// block stretches to whatever screen it's on.
// ---------------------------------------------------------------------------

const ROW_TOP: &str = "qwertyuiop";
const ROW_HOME: &str = "asdfghjkl";
const ROW_BOTTOM: &str = "zxcvbnm";

/// The punctuation that actually turns up in what this app asks for:
/// apostrophes and commas in card names, `//` in a double-faced one,
/// hyphens in both names and cards. The underscore is here for Moxfield deck
/// links - their ids are base64url, so a deck whose id happens to contain one
/// would otherwise be impossible to type.
const SYM_TOP: &str = "1234567890";
const SYM_HOME: &str = "-_',./&";
const SYM_BOTTOM: &str = "!?\":;+";

/// Width of one letter, in twentieths of the keyboard.
const KEY: u16 = 2;

/// The keyboard panel. `on_key` turns a key into the owning screen's
/// message; `submit` labels the field's action key, or is `None` when the
/// field has nothing to submit to.
pub fn view<'a, F, Message>(
    keyboard: &Keyboard<F>,
    on_key: impl Fn(Key) -> Message + Copy + 'a,
    submit: Option<&'a str>,
) -> Element<'a, Message>
where
    F: Copy + PartialEq,
    Message: Clone + 'a,
{
    let (top, home, bottom) = if keyboard.symbols {
        (SYM_TOP, SYM_HOME, SYM_BOTTOM)
    } else {
        (ROW_TOP, ROW_HOME, ROW_BOTTOM)
    };

    // Row two is inset by half a key and row three carries shift and
    // backspace on its ends, which is where a hand expects to find them.
    let inset = (20 - home.chars().count() as u16 * KEY) / 2;
    let mut home_row = vec![spacer(inset)];
    home_row.extend(letters(home, keyboard.shift, on_key));
    home_row.push(spacer(inset));

    let shift_style = if keyboard.shift {
        style::key_modifier_on
    } else {
        style::key_modifier
    };
    // Shift and Delete are spelled out rather than drawn as the usual
    // arrows: the app ships no icon font, and U+21E7 and U+232B come out of
    // the default one as empty boxes.
    let mut bottom_row = vec![modifier("Shift", 3, shift_style, on_key(Key::Shift))];
    bottom_row.extend(letters(bottom, keyboard.shift, on_key));
    bottom_row.push(modifier(
        "Delete",
        20 - 3 - bottom.chars().count() as u16 * KEY,
        style::key_modifier,
        on_key(Key::Backspace),
    ));

    let layer_style = if keyboard.symbols {
        style::key_modifier_on
    } else {
        style::key_modifier
    };
    let layer_label = if keyboard.symbols { "ABC" } else { "123" };

    let mut space_row = vec![
        modifier(layer_label, 3, layer_style, on_key(Key::Symbols)),
        modifier("Clear", 3, style::key_modifier, on_key(Key::Clear)),
    ];
    match submit {
        Some(label) => {
            space_row.push(modifier("space", 7, style::key, on_key(Key::Space)));
            space_row.push(modifier("Done", 3, style::key_modifier, on_key(Key::Done)));
            space_row.push(cta(label, 4, on_key(Key::Submit)));
        }
        None => {
            space_row.push(modifier("space", 11, style::key, on_key(Key::Space)));
            space_row.push(modifier("Done", 3, style::key_modifier, on_key(Key::Done)));
        }
    }

    container(
        column![
            row(letters(top, keyboard.shift, on_key)).spacing(style::GAP_SM),
            row(home_row).spacing(style::GAP_SM),
            row(bottom_row).spacing(style::GAP_SM),
            row(space_row).spacing(style::GAP_SM),
        ]
        .spacing(style::GAP_SM),
    )
    .padding(style::GAP_SM)
    .width(Length::Fill)
    .style(style::panel)
    .into()
}

fn letters<'a, Message: Clone + 'a>(
    chars: &str,
    shift: bool,
    on_key: impl Fn(Key) -> Message,
) -> Vec<Element<'a, Message>> {
    chars
        .chars()
        .map(|c| {
            let label: String = if shift {
                c.to_uppercase().collect()
            } else {
                c.to_string()
            };
            style::touch_button(label, style::T_LEAD)
                .width(Length::FillPortion(KEY))
                .style(style::key)
                .on_press(on_key(Key::Char(c)))
                .into()
        })
        .collect()
}

fn modifier<'a, Message: Clone + 'a>(
    label: &'a str,
    portion: u16,
    style: fn(&iced::Theme, button::Status) -> button::Style,
    message: Message,
) -> Element<'a, Message> {
    style::touch_button(label, style::T_LABEL)
        .width(Length::FillPortion(portion))
        .style(style)
        .on_press(message)
        .into()
}

fn cta<'a, Message: Clone + 'a>(
    label: &'a str,
    portion: u16,
    message: Message,
) -> Element<'a, Message> {
    style::touch_button(label, style::T_ACTION)
        .width(Length::FillPortion(portion))
        .style(style::primary)
        .on_press(message)
        .into()
}

fn spacer<'a, Message: 'a>(portion: u16) -> Element<'a, Message> {
    Space::new(Length::FillPortion(portion), Length::Shrink).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Field {
        Name,
        Search,
    }

    fn typing(value: &str) -> (Keyboard<Field>, String) {
        let mut kb = Keyboard::default();
        kb.open(Field::Name, value);
        (kb, value.to_string())
    }

    #[test]
    fn an_empty_field_starts_shifted_and_shift_is_one_shot() {
        let (mut kb, mut value) = typing("");
        kb.press(Key::Char('a'), &mut value);
        kb.press(Key::Char('l'), &mut value);
        kb.press(Key::Char('e'), &mut value);
        kb.press(Key::Char('x'), &mut value);
        assert_eq!(value, "Alex");
    }

    #[test]
    fn a_field_with_text_in_it_keeps_typing_in_lower_case() {
        let (mut kb, mut value) = typing("Atraxa");
        kb.press(Key::Space, &mut value);
        kb.press(Key::Char('p'), &mut value);
        assert_eq!(value, "Atraxa p");
    }

    #[test]
    fn backspace_takes_the_last_character_only() {
        let (mut kb, mut value) = typing("Tymna");
        kb.press(Key::Backspace, &mut value);
        assert_eq!(value, "Tymn");
    }

    #[test]
    fn clear_empties_the_field_and_re_shifts_it() {
        let (mut kb, mut value) = typing("Tymna");
        kb.press(Key::Clear, &mut value);
        kb.press(Key::Char('k'), &mut value);
        assert_eq!(value, "K");
    }

    #[test]
    fn submit_and_done_leave_the_text_alone() {
        let (mut kb, mut value) = typing("Thrasios");
        assert_eq!(kb.press(Key::Submit, &mut value), Outcome::Submit);
        assert_eq!(kb.press(Key::Done, &mut value), Outcome::Done);
        assert_eq!(value, "Thrasios");
        assert_eq!(kb.field(), None);
    }

    #[test]
    fn moving_to_another_field_drops_the_symbol_layer() {
        let mut kb: Keyboard<Field> = Keyboard::default();
        kb.open(Field::Search, "");
        kb.press(Key::Symbols, &mut String::new());
        kb.open(Field::Name, "");
        let mut value = String::new();
        kb.press(Key::Char('q'), &mut value);
        assert_eq!(value, "Q");
        assert_eq!(kb.field(), Some(Field::Name));
    }
}
