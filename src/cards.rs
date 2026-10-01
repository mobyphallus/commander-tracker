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

use iced::widget::{button, column, container, image, responsive, row, scrollable, text};
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

/// Preferred tile width; grids derive their actual width from the viewport.
pub const TILE: f32 = 350.0;
const NAME_H: f32 = style::T_LABEL as f32 * 1.3 * 2.0;

/// Fit whole columns into the viewport, reserving space for the scrollbar.
fn tile_width(available: f32) -> f32 {
    let usable = (available - style::GAP as f32).max(1.0);
    let columns = ((usable + style::GAP as f32) / (320.0 + style::GAP as f32))
        .floor()
        .max(1.0);
    ((usable - (columns - 1.0) * style::GAP as f32) / columns).min(TILE)
}

/// Each tile gets the same measured width. The column inside `grid` keeps
/// the scrollable's minimum height from stretching individual cards.
pub fn adaptive_grid<'a, Message: 'a>(
    count: usize,
    make: impl Fn(usize, f32) -> Element<'a, Message> + 'a,
) -> Element<'a, Message> {
    responsive(move |size| {
        let width = tile_width(size.width);
        grid((0..count).map(|i| make(i, width)).collect())
    })
    .into()
}

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
    pub power: Option<f64>,
    pub salt: Option<f64>,
    pub win_conditions: Option<f64>,
    pub synergy: Option<f64>,
    pub interaction: Option<f64>,
    pub report: bool,
}

impl DeckMeta {
    pub fn is_empty(&self) -> bool {
        self.entries().is_empty()
    }

    pub fn entries(&self) -> Vec<(&'static str, String)> {
        let mut entries = Vec::new();
        if let Some(v) = self.bracket {
            entries.push(("BRACKET", v.to_string()));
        }
        if let Some(v) = self.power {
            entries.push(("POWER", format!("{v:.1}")));
        }
        if let Some(v) = self.salt {
            entries.push(("SALT", format!("{v:.0}")));
        }
        for (label, value) in [
            ("WIN CON", self.win_conditions),
            ("SYNERGY", self.synergy),
            ("INTERACTION", self.interaction),
        ] {
            if let Some(v) = value {
                entries.push((
                    label,
                    if self.report {
                        format!("{v:.0} pts")
                    } else {
                        format!("{v:.0}/100")
                    },
                ));
            }
        }
        entries
    }

    pub fn summary(&self) -> String {
        self.entries()
            .iter()
            .map(|(label, value)| format!("{label} {value}"))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// Mana symbols use a pale disc and a recognizable dark vector silhouette.
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
    crate::icon::mana(symbol, PIP)
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
/// Partners share one selectable tile and preserve both images' proportions.
pub fn deck_tile<'a>(
    deck: &'a SavedDeck,
    selected: bool,
    images: &'a Images,
    meta: DeckMeta,
    on_press: crate::app::Message,
    on_deck_list: crate::app::Message,
    on_summary: crate::app::Message,
    tile_width: f32,
) -> Element<'a, crate::app::Message> {
    use crate::app::Message;
    let width = tile_width - style::GAP_SM as f32 * 2.0;
    // Partners share a single tile; neither the selectable area nor the grid
    // changes width. Both full-card portraits keep their original proportions.
    let art: Element<Message> = match &deck.partner {
        Some(partner) => {
            let half = (width - style::GAP_XS as f32) / 2.0;
            container(
                row![
                    portrait(&deck.commander, images, half),
                    portrait(partner, images, half)
                ]
                .spacing(style::GAP_XS),
            )
            .center_y(width * ART_ASPECT)
            .style(style::panel)
            .into()
        }
        None => portrait(&deck.commander, images, width),
    };
    let art_height = width * ART_ASPECT;

    let tile = button(
        column![
            art,
            container(
                text(deck.label())
                    .size(style::T_LABEL)
                    .color(style::TEXT)
                    .width(Length::Fixed(width)),
            )
            .height(Length::Fixed(NAME_H))
            .clip(true),
            mana_row(&identity(deck)),
        ]
        .spacing(style::GAP_XS),
    )
    .padding(style::GAP_SM)
    .width(Length::Fixed(width + style::GAP_SM as f32 * 2.0))
    .height(Length::Fixed(
        art_height + NAME_H + PIP + style::GAP_XS as f32 * 2.0 + style::GAP_SM as f32 * 2.0,
    ))
    .style(style::ghost)
    .on_press(on_press);

    // One outline and background ties the score footer to its commander.
    let mut content = column![tile];
    if !meta.is_empty() {
        content = content
            .push(iced::widget::horizontal_rule(1))
            .push(container(score_buttons(meta, on_deck_list, on_summary)).padding(style::GAP_SM));
    }
    container(content)
        .width(Length::Fixed(width + style::GAP_SM as f32 * 2.0))
        .style(if selected {
            style::panel_active
        } else {
            style::panel
        })
        .clip(true)
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
    width: f32,
) -> Element<'a, Message> {
    match commander.portrait_url().and_then(|u| images.get(u)) {
        Some(handle) => picture(handle, width, width * ART_ASPECT),
        None => waiting(&commander.name, width, width * ART_ASPECT),
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

/// An integrated footer: primary score at left, supporting metrics at right.
pub fn score_buttons<'a>(
    meta: DeckMeta,
    on_deck_list: crate::app::Message,
    on_breakdown: crate::app::Message,
) -> Element<'a, crate::app::Message> {
    if meta.is_empty() {
        return iced::widget::horizontal_space()
            .width(Length::Shrink)
            .into();
    }
    let entries = meta.entries();
    let primary = entries
        .iter()
        .find(|(label, _)| matches!(*label, "BRACKET" | "POWER"));
    let extras: Vec<_> = entries
        .iter()
        .filter(|(label, _)| !matches!(*label, "BRACKET" | "POWER"))
        .collect();
    let mut supporting = column![].spacing(0);
    for pair in extras.chunks(2) {
        let mut line = row![].spacing(style::GAP_XS);
        for (label, value) in pair {
            line = line.push(
                button(score_value(label, value, false))
                    .padding(style::GAP_XS)
                    .width(Length::Fill)
                    .height(style::TOUCH_H)
                    .style(style::ghost)
                    .on_press(crate::app::Message::OpenScoreSection(
                        Box::new(on_breakdown.clone()),
                        crate::screens::breakdown::Section::from_label(label),
                    )),
            );
        }
        if pair.len() == 1 && extras.len() > 1 {
            line = line.push(iced::widget::horizontal_space());
        }
        supporting = supporting.push(line);
    }
    let mut footer = row![].spacing(style::GAP_SM).align_y(Alignment::Center);
    if let Some((label, value)) = primary {
        footer = footer.push(
            button(score_value(label, value, true))
                .width(88)
                .height(if extras.len() > 2 {
                    style::TOUCH_H * 2.
                } else {
                    style::TOUCH_H
                })
                .padding(style::GAP_XS)
                .style(style::ghost)
                .on_press(if *label == "BRACKET" {
                    on_deck_list
                } else {
                    crate::app::Message::OpenScoreSection(
                        Box::new(on_breakdown),
                        crate::screens::breakdown::Section::Power,
                    )
                }),
        );
    }
    if !extras.is_empty() {
        footer = footer.push(supporting.width(Length::Fill));
    }
    footer.into()
}

fn score_value<'a, Message: 'a>(label: &str, value: &str, primary: bool) -> Element<'a, Message> {
    let (number, unit) = if let Some(number) = value.strip_suffix("/100") {
        (number, " /100")
    } else if let Some(number) = value.strip_suffix(" pts") {
        (number, " pts")
    } else {
        (value, "")
    };
    let label = match label {
        "BRACKET" => "Bracket",
        "POWER" => "Power",
        "SALT" => "Salt",
        "WIN CON" => "Win con",
        "SYNERGY" => "Synergy",
        _ => "Interaction",
    };
    container(
        column![
            row![
                text(number.to_owned())
                    .size(if primary {
                        style::T_HEADING
                    } else {
                        style::T_LABEL
                    })
                    .color(if primary {
                        style::ACCENT_BRIGHT
                    } else {
                        style::TEXT
                    }),
                text(unit.to_owned())
                    .size(style::T_MICRO)
                    .color(style::TEXT_MUTED)
            ]
            .align_y(Alignment::Center),
            text(label).size(style::T_MICRO).color(style::TEXT_MUTED),
        ]
        .spacing(2),
    )
    .center_y(Length::Fill)
    .into()
}

/// Responsive score band with a clear primary value and quieter supporting metrics.
pub fn score_strip<'a>(
    meta: DeckMeta,
    on_deck_list: crate::app::Message,
    on_breakdown: crate::app::Message,
) -> Element<'a, crate::app::Message> {
    container(responsive(move |size| {
        let entries = meta.entries();
        let wide = size.width / entries.len().max(1) as f32 >= 105.0;
        let mut strip = row![].spacing(style::GAP_SM).align_y(Alignment::Center);
        for (index, (label, value)) in entries.iter().enumerate() {
            let primary = matches!(*label, "POWER" | "BRACKET");
            let (number, unit) = if let Some(n) = value.strip_suffix("/100") {
                (n, "/100")
            } else if let Some(n) = value.strip_suffix(" pts") {
                (n, "pts")
            } else {
                (value.as_str(), "")
            };
            let caption = match *label {
                "POWER" => "Power",
                "BRACKET" => "Bracket",
                "SALT" => "Salt",
                "WIN CON" if wide => "Win conditions",
                "WIN CON" => "Win",
                "SYNERGY" if wide => "Synergy",
                "SYNERGY" => "Syn",
                _ if wide => "Interaction",
                _ => "Int",
            };
            let mut metric = column![
                text(caption).size(style::T_MICRO).color(style::TEXT_MUTED),
                text(number.to_owned())
                    .size(if primary {
                        style::T_HEADING
                    } else {
                        style::T_SUBHEAD
                    })
                    .color(if primary {
                        style::ACCENT_BRIGHT
                    } else {
                        style::TEXT
                    }),
            ]
            .spacing(2)
            .align_x(Alignment::Center);
            if !primary {
                metric = metric.push(text(unit).size(style::T_MICRO).color(style::TEXT_MUTED));
            }
            strip = strip.push(
                button(
                    container(metric)
                        .center_x(Length::Fill)
                        .center_y(style::TOUCH_H),
                )
                .padding(0)
                .width(Length::Fill)
                .height(style::TOUCH_H)
                .style(style::ghost)
                .on_press(if *label == "BRACKET" {
                    on_deck_list.clone()
                } else {
                    crate::app::Message::OpenScoreSection(
                        Box::new(on_breakdown.clone()),
                        crate::screens::breakdown::Section::from_label(label),
                    )
                }),
            );
            if primary && index + 1 < entries.len() {
                strip = strip.push(container(iced::widget::vertical_rule(1)).height(48));
            }
        }
        strip.into()
    }))
    .height(style::TOUCH_H)
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
    fn adaptive_tiles_fit_tablet_widths() {
        for available in [736.0, 1216.0, 1811.0] {
            let width = tile_width(available);
            assert!((280.0..=TILE).contains(&width));
            let columns = ((available - style::GAP as f32 + style::GAP as f32)
                / (320.0 + style::GAP as f32))
                .floor();
            assert!(
                columns * width + (columns - 1.0) * style::GAP as f32
                    <= available - style::GAP as f32 + 0.01
            );
            let partner_half = (width - 2.0 * style::GAP_SM as f32 - style::GAP_XS as f32) / 2.0;
            assert!(partner_half * 2.0 + (style::GAP_XS as f32) < width);
        }
    }

    #[test]
    fn every_art_box_matches_the_shape_of_the_art_in_it() {
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
