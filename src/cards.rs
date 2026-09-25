//! Pictures of cards, shared by every screen that picks a commander.
//!
//! Two different pictures for two different jobs. A saved deck shows the art
//! crop its player stares at all game, so their deck list reads as their
//! table. A search result shows the whole card, because the frame and the
//! type line are what tell four similarly-named legends apart.
//!
//! The tiles take their press message from the caller, so setup and the
//! player manager can show the same cards and mean different things by a
//! tap.
//!
//! # Why every image here is [`ContentFit::Contain`]
//!
//! Never `Cover`, and never a box that doesn't match its picture's shape.
//! When a fitted image spills past its bounds, `image::draw` wraps it in a
//! clip layer of its own - and in iced 0.13 `layer::Stack::push_clip`
//! *replaces* the clip in force rather than intersecting with it. Inside a
//! scrollable that clip is then placed at the scrolled position with
//! nothing holding it to the viewport, so the art gets drawn over whatever
//! sits above the list, or disappears entirely while the list is moving.
//! `Contain` scales by the smaller of the two ratios, so it can never
//! overflow and that layer is never pushed. The boxes below are cut to
//! their picture's own proportions, which is also why nothing here needs
//! cropping.

use std::collections::HashMap;

use iced::widget::{button, column, container, image, row, scrollable, stack, text};
use iced::{Alignment, ContentFit, Element, Length};

use crate::model::{Commander, SavedDeck};
use crate::scryfall::ScryfallCard;
use crate::style;

/// Every image the app has already fetched, keyed by its URL.
pub type Images = HashMap<String, image::Handle>;

/// Scryfall's `art_crop`, 626x457.
const ART_ASPECT: f32 = 457.0 / 626.0;
/// Scryfall's `small` card image, 146x204.
const CARD_ASPECT: f32 = 204.0 / 146.0;

/// A deck tile is a square. One shape repeated is what makes a grid of ten
/// decks read as a rack of cards rather than as ten unrelated panels, and a
/// square is the shape a thumb finds without aiming.
/// Sized to fill the row rather than to a round number: five of these plus
/// their gaps span the table's screen almost exactly, so ten decks - the
/// most anyone here keeps - land in two full rows with nothing left over.
/// Big enough, too, to know a commander by its art from across the table
/// instead of by reading the name under it.
pub const TILE: f32 = 350.0;

/// The tile's inside, after its padding.
const INNER_W: f32 = TILE - style::GAP_SM as f32 * 2.0;
/// The art fills that width at its own proportions - see the note above on
/// why it is never cropped to fit.
const ART_H: f32 = INNER_W * ART_ASPECT;
/// Whatever is left of the square: two lines of [`style::T_LABEL`] at
/// iced's 1.3x line height, give or take a pixel. Deriving it this way is
/// what makes the tile exactly square rather than nearly so.
const NAME_H: f32 = TILE - style::GAP_SM as f32 * 2.0 - ART_H - style::GAP_XS as f32;

/// A whole card, close enough to the size Scryfall serves it at that it
/// stays crisp instead of being blown up. Search results keep the card's
/// own proportions: a card is a portrait rectangle, and squaring one off
/// would cut away the type line that tells two printings apart.
pub const CARD_W: f32 = 246.0;
const CARD_H: f32 = CARD_W * CARD_ASPECT;
const CARD_NAME_H: f32 = style::T_CAPTION as f32 * 1.3 * 2.0;
const BADGE_H: f32 = style::T_CAPTION as f32 * 1.3 + style::GAP_XS as f32;
/// The whole result tile, so a grid of them lands on one baseline.
const CARD_TILE_H: f32 =
    CARD_H + CARD_NAME_H + BADGE_H + style::GAP_SM as f32 * 2.0 + style::GAP_XS as f32 * 2.0;

/// What a linked Moxfield deck turned out to be. Both halves are optional:
/// a deck can be linked and not yet analysed, and an analysis that failed
/// leaves the link behind.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DeckMeta {
    pub bracket: Option<u8>,
    pub salt: Option<f64>,
}

impl DeckMeta {
    fn is_empty(&self) -> bool {
        self.bracket.is_none() && self.salt.is_none()
    }
}

/// One mana pip. A pale disc with its letter in dark ink, which is how the
/// game itself prints them.
pub const PIP: f32 = 30.0;

/// A deck's colour identity as pips rather than as a run of letters.
/// "WUBG" is four things to decode; four coloured discs is one glance.
pub fn mana_row<'a, Message: 'a>(identity: &str) -> Element<'a, Message> {
    // A colourless deck still gets a pip - an empty space would read as
    // "not loaded yet" rather than as "no colours".
    let symbols: Vec<char> = if identity.is_empty() {
        vec!['C']
    } else {
        identity.chars().collect()
    };
    row(symbols
        .into_iter()
        .map(pip)
        .collect::<Vec<Element<Message>>>())
    .spacing(style::GAP_XS / 2)
    .into()
}

fn pip<'a, Message: 'a>(symbol: char) -> Element<'a, Message> {
    container(
        text(symbol.to_string())
            .size(style::T_CAPTION)
            .color(style::MANA_INK),
    )
    .center_x(Length::Fixed(PIP))
    .center_y(Length::Fixed(PIP))
    .style(style::mana_pip(symbol))
    .into()
}

/// How many search results get their picture fetched. Scryfall answers a
/// loose name with up to 175 cards, and nobody scrolls past the first
/// screenful of a search they're about to narrow anyway.
pub const PREFETCH: usize = 40;

/// A scrolling grid of tiles.
///
/// The wrapping row sits inside a `column!` on purpose, and the grid breaks
/// without it. `scrollable::layout` builds its content's limits from its own
/// `limits.min()`, and a wrapping row hands those same limits to every child
/// - while `Limits::resolve` clamps a `Fixed` (or `Shrink`) height *up* to
/// `min.height`. So inside a full-height scrollable every tile is silently
/// stretched to the height of the whole viewport, and one row of them fills
/// the screen. A column re-issues its children's limits from `Size::ZERO`,
/// which hands each tile back the height it asked for.
pub fn grid<'a, Message: 'a>(tiles: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    scrollable(column![row(tiles).spacing(style::GAP).wrap()])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// One saved deck: its art, its name, and its colours.
///
/// A partner pair takes two squares' worth of width and shows both cards at
/// full size rather than squeezing two arts into one square. It's one deck
/// either way - the tile is one button - but a deck with two commanders
/// looks like one at a glance, and neither art has to be cropped to say so.
pub fn deck_tile<'a, Message: Clone + 'a>(
    deck: &'a SavedDeck,
    selected: bool,
    images: &'a Images,
    meta: DeckMeta,
    on_press: Message,
) -> Element<'a, Message> {
    let (art, width): (Element<Message>, f32) = match &deck.partner {
        Some(partner) => (
            row![portrait(&deck.commander, images), portrait(partner, images)]
                .spacing(style::GAP_XS)
                .into(),
            INNER_W * 2.0 + style::GAP_XS as f32,
        ),
        None => (portrait(&deck.commander, images), INNER_W),
    };

    button(
        column![
            // The colours and the deck's numbers ride on the art instead of
            // taking rows of their own, which is what keeps the tile square.
            stack![
                art,
                container(
                    row![
                        mana_row(&identity(deck)),
                        iced::widget::horizontal_space(),
                        meta_chips(meta),
                    ]
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(Alignment::End)
                .padding(style::GAP_XS),
            ],
            container(
                text(deck.label())
                    .size(style::T_LABEL)
                    .color(style::TEXT)
                    .width(Length::Fixed(width)),
            )
            .height(Length::Fixed(NAME_H)),
        ]
        .spacing(style::GAP_XS),
    )
    .padding(style::GAP_SM)
    .width(Length::Fixed(width + style::GAP_SM as f32 * 2.0))
    .height(Length::Fixed(TILE))
    .style(if selected {
        style::tile_selected
    } else {
        style::row_button
    })
    .on_press(on_press)
    .into()
}

/// One card in a set of search results, with its name underneath in a fixed
/// slot so a two-line name doesn't shove its neighbours down the grid.
pub fn card_tile<'a, Message: Clone + 'a>(
    card: &'a ScryfallCard,
    images: &'a Images,
    on_press: Message,
) -> Element<'a, Message> {
    let face: Element<Message> = match card
        .small_url
        .as_deref()
        .or(card.image_url.as_deref())
        .and_then(|u| images.get(u))
    {
        Some(handle) => picture(handle, CARD_W, CARD_H),
        None => waiting(&card.name, CARD_W, CARD_H),
    };

    button(
        column![
            face,
            container(
                text(&card.name)
                    .size(style::T_CAPTION)
                    .color(style::TEXT)
                    .width(Length::Fixed(CARD_W)),
            )
            .height(Length::Fixed(CARD_NAME_H)),
            mana_row(&ordered(&card.color_identity)),
        ]
        .spacing(style::GAP_XS),
    )
    .padding(style::GAP_SM)
    .width(Length::Fixed(CARD_W + style::GAP_SM as f32 * 2.0))
    .height(Length::Fixed(CARD_TILE_H))
    .style(style::row_button)
    .on_press(on_press)
    .into()
}

/// A commander's portrait at deck-tile size, or a placeholder holding its
/// exact space while the picture is on its way - so a grid never reflows
/// under the finger as art arrives.
pub fn portrait<'a, Message: 'a>(
    commander: &'a Commander,
    images: &'a Images,
) -> Element<'a, Message> {
    match commander.portrait_url().and_then(|u| images.get(u)) {
        Some(handle) => picture(handle, INNER_W, ART_H),
        None => waiting(&commander.name, INNER_W, ART_H),
    }
}

/// A commander's portrait for a row rather than a grid.
pub fn thumbnail<'a, Message: 'a>(
    commander: &'a Commander,
    images: &'a Images,
) -> Element<'a, Message> {
    let width = style::TOUCH_H / ART_ASPECT;
    match commander.portrait_url().and_then(|u| images.get(u)) {
        Some(handle) => picture(handle, width, style::TOUCH_H),
        None => waiting(&commander.name, width, style::TOUCH_H),
    }
}

/// A whole card at a given width, in a box cut to match it.
pub fn card_picture<'a, Message: 'a>(handle: &image::Handle, width: f32) -> Element<'a, Message> {
    picture(handle, width, width * CARD_ASPECT)
}

/// An image in a box cut to its own proportions.
pub fn picture<'a, Message: 'a>(
    handle: &image::Handle,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    image(handle.clone())
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .content_fit(ContentFit::Contain)
        .into()
}

/// The hole a picture will fill, with the card's name in it meanwhile -
/// still enough to pick by if the network is slow.
fn waiting<'a, Message: 'a>(name: &str, width: f32, height: f32) -> Element<'a, Message> {
    container(
        text(first_word(name))
            .size(style::T_CAPTION)
            .color(style::TEXT_MUTED),
    )
    .padding(style::GAP_XS)
    .center_x(Length::Fixed(width))
    .center_y(Length::Fixed(height))
    .style(style::art_inset)
    .into()
}

/// The colour-identity chip.
fn badge<'a, Message: 'a>(label: String) -> Element<'a, Message> {
    container(text(label).size(style::T_CAPTION).color(style::TEXT_MUTED))
        .padding([0, style::GAP_XS])
        .height(Length::Fixed(BADGE_H))
        .align_y(Alignment::Center)
        .style(style::badge)
        .into()
}

/// A readable touch target over commander art, using the shared glass surface.
pub fn score_button<'a, Message: Clone + 'a>(
    meta: DeckMeta,
    on_press: Message,
) -> Element<'a, Message> {
    if meta.is_empty() {
        return iced::widget::horizontal_space()
            .width(Length::Shrink)
            .into();
    }
    let mut scores = row![].spacing(style::GAP).align_y(Alignment::Center);
    if let Some(bracket) = meta.bracket {
        scores = scores.push(
            column![
                text(bracket.to_string()).size(style::T_HEADING),
                text("BRACKET").size(style::T_MICRO)
            ]
            .align_x(Alignment::Center),
        );
    }
    if let Some(salt) = meta.salt {
        scores = scores.push(
            column![
                text(format!("{salt:.0}")).size(style::T_HEADING),
                text("SALT").size(style::T_MICRO)
            ]
            .align_x(Alignment::Center),
        );
    }
    button(
        column![scores, text("View summary").size(style::T_CAPTION)]
            .spacing(style::GAP_XS)
            .align_x(Alignment::Center),
    )
    .padding(style::GAP_SM)
    .style(style::glass_button)
    .on_press(on_press)
    .into()
}

/// The name a commander actually goes by at the table: "Thrasios", not
/// "Thrasios, Triton Hero".
pub fn first_word(name: &str) -> String {
    name.split([',', ' ']).next().unwrap_or(name).to_string()
}

/// A deck's colour identity, in the order the game says them, and counting
/// both halves of a partner pair - that pair is the identity the deck was
/// built to.
pub fn identity(deck: &SavedDeck) -> String {
    let mut letters: String = deck.commander.color_identity.clone();
    if let Some(partner) = &deck.partner {
        for c in partner.color_identity.chars() {
            if !letters.contains(c) {
                letters.push(c);
            }
        }
    }
    ordered(&letters)
}

/// Colour letters in the order the game says them. Empty stays empty -
/// a colourless deck is drawn as one grey pip, not as a word.
fn ordered(identity: &str) -> String {
    let mut letters: Vec<char> = identity.chars().collect();
    letters.sort_by_key(|c| "WUBRG".find(*c).unwrap_or(9));
    letters.into_iter().collect()
}

/// The bracket a linked deck plays at and how salty its list is, as two
/// small chips. Nothing at all when the deck has no Moxfield link - an
/// empty pair of chips would suggest the numbers were zero rather than
/// unknown.
pub fn meta_chips<'a, Message: 'a>(meta: DeckMeta) -> Element<'a, Message> {
    if meta.is_empty() {
        return iced::widget::horizontal_space()
            .width(Length::Shrink)
            .into();
    }
    let mut chips = row![].spacing(style::GAP_XS / 2);
    if let Some(bracket) = meta.bracket {
        chips = chips.push(badge(format!("B{bracket}")));
    }
    if let Some(salt) = meta.salt {
        chips = chips.push(badge(format!("{salt:.0} SALT")));
    }
    chips.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ArtFraming;

    fn commander(name: &str, identity: &str) -> Commander {
        Commander {
            id: 1,
            oracle_id: name.to_string(),
            name: name.to_string(),
            image_url: None,
            art_crop_url: None,
            color_identity: identity.to_string(),
            framing: ArtFraming::default(),
        }
    }

    #[test]
    fn a_deck_tile_is_square() {
        let height = style::GAP_SM as f32 * 2.0 + ART_H + style::GAP_XS as f32 + NAME_H;
        assert!(
            (height - TILE).abs() < 0.5,
            "tile is {TILE} wide but {height} tall"
        );
    }

    /// The whole reason every box here is cut to its picture's shape: a box
    /// that isn't lets the image overflow, and an image that overflows
    /// inside a scrollable gets drawn outside it.
    #[test]
    fn every_art_box_matches_the_shape_of_the_art_in_it() {
        assert!((ART_H / INNER_W - ART_ASPECT).abs() < 0.001);
        assert!((CARD_H / CARD_W - CARD_ASPECT).abs() < 0.001);
        let thumb_w = style::TOUCH_H / ART_ASPECT;
        assert!((style::TOUCH_H / thumb_w - ART_ASPECT).abs() < 0.001);
    }

    #[test]
    fn a_pair_carries_both_halves_colours() {
        let deck = SavedDeck {
            commander: commander("Thrasios, Triton Hero", "GU"),
            partner: Some(commander("Tymna the Weaver", "WB")),
        };
        assert_eq!(identity(&deck), "WUBG");
    }

    #[test]
    fn colours_come_out_in_the_order_the_game_says_them() {
        let deck = SavedDeck {
            commander: commander("Atraxa, Praetors' Voice", "GWUB"),
            partner: None,
        };
        assert_eq!(identity(&deck), "WUBG");
    }

    /// A colourless deck has no letters, and [`mana_row`] is what turns
    /// that into the one grey pip a player actually sees - an empty row
    /// would read as "not loaded yet" rather than as "no colours".
    #[test]
    fn a_colourless_commander_has_no_colour_letters() {
        let deck = SavedDeck {
            commander: commander("Kozilek, the Great Distortion", ""),
            partner: None,
        };
        assert_eq!(identity(&deck), "");
    }

    #[test]
    fn colour_letters_are_ordered_wherever_they_come_from() {
        assert_eq!(ordered("GWUB"), "WUBG");
        assert_eq!(ordered("RW"), "WR");
        assert_eq!(ordered(""), "");
    }

    #[test]
    fn a_commander_goes_by_the_name_before_its_comma() {
        assert_eq!(first_word("Thrasios, Triton Hero"), "Thrasios");
        assert_eq!(first_word("Winota, Joiner of Forces"), "Winota");
        assert_eq!(first_word("Hearthhull"), "Hearthhull");
    }
}
