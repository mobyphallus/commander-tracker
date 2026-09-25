//! The full salt and bracket breakdown for one linked deck.
//!
//! This is the screen a pod actually reads during rule 0, so it's built
//! around one idea: never show a number without showing what produced it.
//! The bracket names the criteria that set it, every salt category can be
//! opened down to the cards that fed it, and anything the analysis couldn't
//! score is listed rather than quietly rounded away.
//!
//! It's generic over the message type instead of reaching for
//! `players::PlayersMessage`, so the screen that owns the deck keeps control
//! of navigation and this file stays pure rendering.

use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{Alignment, Element, Length};

use crate::salt::{Analysis, Category, CategoryKind, Criterion};
use crate::style;

/// Width of the count and floor cells in the criteria table, so the columns
/// line up down the screen regardless of label length.
const W_COUNT: f32 = 70.0;
const W_FLOOR: f32 = 86.0;
const W_SCORE: f32 = 96.0;
/// The proportional bars in the salt table.
const W_BAR: f32 = 160.0;
const BAR_H: f32 = 14.0;
/// The big bracket digit's box, wide enough for the label beneath it.
const W_HERO: f32 = 144.0;
/// How many cards to name per salt category before falling back to a count.
const CARDS_SHOWN: usize = 6;

/// The whole breakdown, as one scrollable body.
///
/// Deliberately chrome-free: no header, no back button, no refresh. The
/// screen that owns the deck supplies those, which keeps navigation in one
/// place and lets this render for any message type at all.
pub fn body<'a, Msg: 'a>(analysis: &'a Analysis) -> Element<'a, Msg> {
    let mut body = column![hero(analysis), criteria_section(analysis)].spacing(style::GAP);

    if !analysis.combos.is_empty() {
        body = body.push(combos_section(analysis));
    }
    body = body.push(salt_section(analysis));

    let stacked = analysis.stacked_offenders();
    if !stacked.is_empty() {
        body = body.push(stacked_section(&stacked));
    }
    body = body.push(footnotes(analysis));

    scrollable(body.padding(style::GAP_SM))
        .height(Length::Fill)
        .into()
}

/// The two headline numbers, side by side, with the one-line reason for each.
fn hero<'a, Msg: 'a>(analysis: &'a Analysis) -> Element<'a, Msg> {
    let because = match analysis.deciding().as_slice() {
        // Nothing tripped: the deck floors at the bottom of the scale.
        [] => "Nothing in the list pushes it up".to_string(),
        deciding => format!(
            "Because: {}",
            deciding
                .iter()
                .map(|c| c.kind.label())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };

    let bracket_block = column![
        text(analysis.bracket.to_string())
            .size(style::T_DISPLAY)
            .color(style::TEXT),
        text("BRACKET")
            .size(style::T_MICRO)
            .color(style::TEXT_MUTED),
    ]
    .spacing(style::GAP_XS)
    .align_x(Alignment::Center)
    .width(Length::Fixed(W_HERO));

    let salt_block = column![
        row![
            text(format!("{:.0}", analysis.salt_total))
                .size(style::T_DISPLAY)
                .color(style::TEXT),
            text(analysis.salt_grade())
                .size(style::T_SUBHEAD)
                .color(style::ACCENT_BRIGHT),
        ]
        .spacing(style::GAP_SM)
        .align_y(Alignment::Center),
        text(format!("SALT - {}", analysis.salt_band().to_uppercase()))
            .size(style::T_MICRO)
            .color(style::TEXT_MUTED),
    ]
    .spacing(style::GAP_XS)
    .align_x(Alignment::Center)
    .width(Length::Fixed(W_HERO));

    let mut detail = column![
        text(because).size(style::T_SUBHEAD).color(style::TEXT),
        text(claims(analysis))
            .size(style::T_BODY)
            .color(style::TEXT_MUTED),
        bar(analysis.salt_percent() / 100.0, Length::Fill),
    ]
    .spacing(style::GAP_SM)
    .width(Length::Fill);

    // The single most useful sentence on the screen: the list is stronger
    // than its owner is claiming. Said plainly, without accusing anybody -
    // people mislabel decks by accident far more often than on purpose.
    if analysis.understated() {
        detail = detail.push(
            text(format!(
                "Listed on Moxfield as bracket {}, but the cards support {}.",
                analysis.owner_bracket.unwrap_or(analysis.bracket),
                analysis.bracket
            ))
            .size(style::T_BODY)
            .color(style::DANGER),
        );
    }

    container(
        column![row![bracket_block, salt_block].spacing(style::GAP), detail].spacing(style::GAP),
    )
    .padding(style::GAP)
    .width(Length::Fill)
    .style(style::panel)
    .into()
}

/// What everyone else thinks the bracket is, for comparison with ours.
fn claims(analysis: &Analysis) -> String {
    let mut parts = Vec::new();
    if let Some(owner) = analysis.owner_bracket {
        parts.push(format!("owner says {owner}"));
    }
    if let Some(auto) = analysis.auto_bracket {
        parts.push(format!("Moxfield says {auto}"));
    }
    if parts.is_empty() {
        return "Nobody else has put a number on this deck".to_string();
    }
    parts.join("  -  ")
}

fn criteria_section<'a, Msg: 'a>(analysis: &'a Analysis) -> Element<'a, Msg> {
    let rows = analysis
        .criteria
        .iter()
        .map(|c| criterion_row(c, analysis.bracket))
        .collect::<Vec<_>>();

    section(
        "Bracket criteria",
        Some("The five things Wizards' guidelines measure. The deck lands on the highest floor any of them sets."),
        rows,
    )
}

fn criterion_row<'a, Msg: 'a>(criterion: &'a Criterion, bracket: u8) -> Element<'a, Msg> {
    let deciding = criterion.is_deciding(bracket);
    // Colour carries the same information as the floor number so the shape
    // of the table is readable before any of it is read.
    let tone = match criterion.floor {
        f if f >= 4 && criterion.count > 0 => style::DANGER,
        3 if criterion.count > 0 => style::ACCENT_BRIGHT,
        _ => style::TEXT_MUTED,
    };

    let mut left =
        column![text(criterion.kind.label())
            .size(style::T_SUBHEAD)
            .color(if deciding {
                style::TEXT
            } else {
                style::TEXT_MUTED
            })]
        .spacing(style::GAP_XS);

    // The cards responsible, when there are any. This is the bit that turns
    // "bracket 4" into a conversation instead of an argument.
    if criterion.culprits.is_empty() {
        left = left.push(
            text(criterion.kind.help())
                .size(style::T_CAPTION)
                .color(style::TEXT_MUTED),
        );
    } else {
        left = left.push(
            text(name_list(&criterion.culprits, CARDS_SHOWN))
                .size(style::T_CAPTION)
                .color(style::TEXT_MUTED),
        );
    }

    let line = row![
        left.width(Length::Fill),
        figure(criterion.count.to_string(), W_COUNT, tone),
        container(
            text(format!("B{}", criterion.floor))
                .size(style::T_LABEL)
                .color(tone)
        )
        .width(Length::Fixed(W_FLOOR))
        .align_x(Alignment::Center),
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center);

    container(line)
        .padding(style::GAP_SM)
        .width(Length::Fill)
        .style(if deciding {
            style::panel_active
        } else {
            style::table_row
        })
        .into()
}

fn combos_section<'a, Msg: 'a>(analysis: &'a Analysis) -> Element<'a, Msg> {
    let rows = analysis
        .combos
        .iter()
        .map(|combo| {
            // Tags rather than prose: at a table people scan these.
            let mut tags = Vec::new();
            if combo.two_card {
                tags.push("two-card".to_string());
            }
            if combo.early {
                tags.push("early".to_string());
            }
            if combo.lock {
                tags.push("lock".to_string());
            }
            if let Some(mana) = combo.mana_value {
                tags.push(format!("{mana} mana"));
            }

            let produces = if combo.produces.is_empty() {
                "combo line".to_string()
            } else {
                combo.produces.join(", ")
            };

            let tone = if combo.early || combo.lock {
                style::DANGER
            } else {
                style::TEXT_MUTED
            };

            container(
                row![
                    column![
                        text(combo.label()).size(style::T_LABEL).color(style::TEXT),
                        text(produces)
                            .size(style::T_CAPTION)
                            .color(style::TEXT_MUTED),
                    ]
                    .spacing(style::GAP_XS)
                    .width(Length::Fill),
                    text(tags.join(" - ")).size(style::T_CAPTION).color(tone),
                ]
                .spacing(style::GAP)
                .align_y(Alignment::Center),
            )
            .padding(style::GAP_SM)
            .width(Length::Fill)
            .style(style::table_row)
            .into()
        })
        .collect::<Vec<_>>();

    section(
        &format!("Combo lines ({})", analysis.combos.len()),
        Some("Every line Commander Spellbook can find in this list. Two-card lines that win for seven mana or less are what force bracket 4."),
        rows,
    )
}

fn salt_section<'a, Msg: 'a>(analysis: &'a Analysis) -> Element<'a, Msg> {
    // Bars are relative to the biggest category, not to the total: it makes
    // the shape of a deck's saltiness legible even when one category
    // dominates, which with community salt it usually does.
    let largest = analysis
        .categories
        .first()
        .map(|c| c.score)
        .unwrap_or(1.0)
        .max(0.000_1);

    let rows = analysis
        .categories
        .iter()
        .map(|c| category_row(c, largest))
        .collect::<Vec<_>>();

    if rows.is_empty() {
        return section(
            "Where the salt comes from",
            None,
            vec![text("Nothing in this deck scored any salt at all.")
                .size(style::T_BODY)
                .color(style::TEXT_MUTED)
                .into()],
        );
    }

    section(
        "Where the salt comes from",
        Some("Each category is worth the same per card in every deck, so these are comparable across the pod."),
        rows,
    )
}

fn category_row<'a, Msg: 'a>(category: &'a Category, largest: f64) -> Element<'a, Msg> {
    let names: Vec<String> = category.cards.iter().map(|c| c.name.clone()).collect();
    // The community-salt category is every card in the deck, so listing its
    // contents is noise; the top few by score is the useful part.
    let detail = if category.kind == CategoryKind::Edhrec {
        format!("Saltiest: {}", name_list(&names, 4.min(names.len().max(1))))
    } else {
        name_list(&names, CARDS_SHOWN)
    };

    container(
        row![
            column![
                text(category.kind.label())
                    .size(style::T_LABEL)
                    .color(style::TEXT),
                text(detail).size(style::T_CAPTION).color(style::TEXT_MUTED),
            ]
            .spacing(style::GAP_XS)
            .width(Length::Fill),
            bar(category.score / largest, Length::Fixed(W_BAR)),
            figure(
                format!("{:.1}", category.score),
                W_SCORE,
                style::ACCENT_BRIGHT
            ),
        ]
        .spacing(style::GAP)
        .align_y(Alignment::Center),
    )
    .padding(style::GAP_SM)
    .width(Length::Fill)
    .style(style::table_row)
    .into()
}

/// Cards earning salt for more than one reason. These are the first things
/// to cut, and the ones worth mentioning before the game starts.
fn stacked_section<'a, Msg: 'a>(stacked: &[(String, usize, f64)]) -> Element<'a, Msg> {
    let rows = stacked
        .iter()
        .take(8)
        .map(|(name, count, score)| {
            container(
                row![
                    // Owned rather than borrowed: the list is built fresh in
                    // `view`, so nothing outlives it to borrow from.
                    text(name.clone()).size(style::T_LABEL).color(style::TEXT),
                    Space::new(Length::Fill, Length::Shrink),
                    text(format!("{count} categories"))
                        .size(style::T_CAPTION)
                        .color(style::TEXT_MUTED),
                    figure(format!("{score:.1}"), W_SCORE, style::DANGER),
                ]
                .spacing(style::GAP)
                .align_y(Alignment::Center),
            )
            .padding(style::GAP_SM)
            .width(Length::Fill)
            .style(style::table_row)
            .into()
        })
        .collect::<Vec<_>>();

    section(
        "Repeat offenders",
        Some("Cards that annoy people in more than one way at once."),
        rows,
    )
}

/// The small print: what the analysis couldn't see, and when it ran. Kept
/// visible rather than tucked away, because an unscored card is the one
/// thing that can make the total read low.
fn footnotes<'a, Msg: 'a>(analysis: &'a Analysis) -> Element<'a, Msg> {
    let mut lines = column![].spacing(style::GAP_XS);

    if !analysis.unscored.is_empty() {
        lines = lines.push(
            text(format!(
                "No community salt score yet for {} - usually too new. Their salt is counted as zero, so the total may read low.",
                name_list(&analysis.unscored, 8)
            ))
            .size(style::T_CAPTION)
            .color(style::TEXT_MUTED),
        );
    }

    lines = lines.push(
        text(format!(
            "Bracket from Wizards' criteria via Commander Spellbook. Salt from EDHREC community scores. Checked {}.",
            short_date(&analysis.analysed_at)
        ))
        .size(style::T_MICRO)
        .color(style::TEXT_MUTED),
    );

    container(lines)
        .padding(style::GAP_SM)
        .width(Length::Fill)
        .into()
}

// ---------------------------------------------------------------------------
// Small shared pieces
// ---------------------------------------------------------------------------

fn section<'a, Msg: 'a>(
    title: &str,
    note: Option<&str>,
    rows: Vec<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let mut head = column![text(title.to_string())
        .size(style::T_LEAD)
        .color(style::TEXT)]
    .spacing(style::GAP_XS);
    if let Some(note) = note {
        head = head.push(
            text(note.to_string())
                .size(style::T_CAPTION)
                .color(style::TEXT_MUTED),
        );
    }

    let mut body = column![head].spacing(style::GAP_SM);
    for r in rows {
        body = body.push(r);
    }

    container(body.padding(style::GAP_SM))
        .width(Length::Fill)
        .style(style::panel)
        .into()
}

/// A right-aligned number in a fixed-width cell, so columns line up.
fn figure<'a, Msg: 'a>(value: String, width: f32, colour: iced::Color) -> Element<'a, Msg> {
    container(text(value).size(style::T_LABEL).color(colour))
        .width(Length::Fixed(width))
        .align_x(Alignment::End)
        .into()
}

/// A proportional bar. `frac` is clamped into the track, never past either
/// end of it.
fn bar<'a, Msg: 'a>(frac: f64, width: Length) -> Element<'a, Msg> {
    // FillPortion splits by ratio, so both halves have to stay non-zero; a
    // thousandth of the track is far under a pixel either way.
    let filled = (frac.clamp(0.0, 1.0) * 1000.0).round().clamp(1.0, 999.0) as u16;
    container(
        row![
            container(Space::new(Length::Fill, Length::Fill))
                .width(Length::FillPortion(filled))
                .height(Length::Fill)
                .style(style::meter_fill),
            Space::new(Length::FillPortion(1000 - filled), Length::Fill),
        ]
        .height(Length::Fill),
    )
    .width(width)
    .height(Length::Fixed(BAR_H))
    .clip(true)
    .style(style::meter_track)
    .into()
}

/// "A, B and 4 more" - a list that can't overflow a row however long it is.
fn name_list(names: &[String], shown: usize) -> String {
    if names.is_empty() {
        return String::new();
    }
    if names.len() <= shown {
        return names.join(", ");
    }
    format!(
        "{} and {} more",
        names[..shown].join(", "),
        names.len() - shown
    )
}

/// The date out of an RFC 3339 timestamp. Nobody needs the seconds.
fn short_date(timestamp: &str) -> &str {
    timestamp.split('T').next().unwrap_or(timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_lists_never_run_away() {
        let names: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert_eq!(name_list(&[], 3), "");
        assert_eq!(name_list(&names[..2], 3), "a, b");
        assert_eq!(name_list(&names[..3], 3), "a, b, c");
        assert_eq!(name_list(&names, 3), "a, b, c and 1 more");
    }

    #[test]
    fn dates_lose_their_clock() {
        assert_eq!(short_date("2026-09-24T21:15:03+01:00"), "2026-09-24");
        // Anything unexpected comes back untouched rather than truncated.
        assert_eq!(short_date("whenever"), "whenever");
    }
}
