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

use crate::salt::{Analysis, CardScore, Criterion};
use crate::style;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Summary,
    Bracket,
    Salt,
    Synergy,
    WinConditions,
    Power,
    Interaction,
    Consistency,
    Efficiency,
}
impl Section {
    pub fn from_label(label: &str) -> Self {
        match label {
            "BRACKET" => Self::Bracket,
            "SALT" => Self::Salt,
            "SYNERGY" => Self::Synergy,
            "WIN CON" => Self::WinConditions,
            "POWER" => Self::Power,
            "INTERACTION" => Self::Interaction,
            _ => Self::Summary,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Bracket => "Bracket",
            Self::Salt => "Salt",
            Self::Synergy => "Synergy",
            Self::WinConditions => "Win conditions",
            Self::Power => "Power level",
            Self::Interaction => "Interaction",
            Self::Consistency => "Consistency",
            Self::Efficiency => "Efficiency",
        }
    }
}
/// Deck-local disclosure state. Opening a different deck starts collapsed.
#[derive(Debug, Default)]
pub struct Details {
    open: std::collections::BTreeSet<String>,
    visible: std::collections::BTreeMap<String, usize>,
}
impl Details {
    pub fn toggle(&mut self, key: String) {
        if !self.open.remove(&key) {
            self.open.insert(key);
        }
    }
    pub fn collapse_all(&mut self) {
        self.open.clear();
        self.visible.clear();
    }
    pub fn show_more(&mut self, key: String) {
        *self.visible.entry(key).or_insert(12) += 12;
    }
    pub(crate) fn is_open(&self, key: &str) -> bool {
        self.open.contains(key)
    }
    pub(crate) fn limit(&self, key: &str) -> usize {
        self.visible.get(key).copied().unwrap_or(12)
    }
}

pub fn scroll_id() -> scrollable::Id {
    scrollable::Id::new("deck-score-section")
}

pub fn view<'a>(
    analysis: &'a Analysis,
    selected: Section,
    details: &'a Details,
) -> Element<'a, crate::app::Message> {
    let mut navigation = row![].spacing(style::GAP_XS);
    for section in [
        Section::Summary,
        Section::Power,
        Section::Bracket,
        Section::Salt,
        Section::Synergy,
        Section::WinConditions,
        Section::Interaction,
        Section::Consistency,
        Section::Efficiency,
    ] {
        navigation = navigation.push(
            style::touch_button(section.label(), style::T_LABEL)
                .width(Length::Fixed(
                    (section.label().len() as f32 * 11. + 32.).max(112.),
                ))
                .style(if selected == section {
                    style::primary
                } else {
                    style::secondary
                })
                .on_press(crate::app::Message::ScoreSection(section)),
        );
    }
    let content = if selected == Section::Summary {
        body(analysis)
    } else {
        let mut content =
            column![text(selected.label()).size(style::T_HEADING)].spacing(style::GAP);
        if analysis.has_local() {
            if selected == Section::Bracket {
                content = content
                    .push(bracket_calculation(analysis, true))
                    .push(criteria_section(analysis));
            } else if selected == Section::Salt {
                content = content.push(score_hero(format!("{:.2}", analysis.salt_total), "points", "Local salt",
                    "Category totals include every copy. Expand a category to see its card table.".into()))
                    .push(row![text("Salt categories").size(style::T_SUBHEAD).width(Length::Fill), collapse_control()].align_y(Alignment::Center));
                for category in &analysis.categories {
                    let key = format!("local:Salt:{:?}", category.kind);
                    content = content.push(disclosure(
                        &key,
                        category.kind.label(),
                        &format!(
                            "{} entries | {:.2} total points",
                            category.cards.len(),
                            category.score
                        ),
                        details,
                        || salt_table(&category.cards, &key, details, false),
                    ));
                }
                let stacked = analysis.stacked_offenders();
                if !stacked.is_empty() {
                    content = content.push(if stacked.len() > 6 {
                        disclosure(
                            "local:Salt:stacked",
                            "Cards in multiple categories",
                            &format!("{} cards with overlapping contributions", stacked.len()),
                            details,
                            || stacked_section(&stacked),
                        )
                    } else {
                        stacked_section(&stacked)
                    });
                }
            } else if let Some(a) = &analysis.assessment {
                content = content.push(if selected == Section::Power {
                    power_details(a)
                } else {
                    metric_page(a, selected, details)
                });
                if selected == Section::WinConditions && !analysis.combos.is_empty() {
                    content = content.push(if analysis.combos.len() > 4 {
                        disclosure(
                            "local:Win conditions:combos",
                            "Detailed combo lines and prerequisites",
                            &format!(
                                "{} detected variants — starting mana and setup requirements",
                                analysis.combos.len()
                            ),
                            details,
                            || combos_section(analysis),
                        )
                    } else {
                        combos_section(analysis)
                    });
                }
            } else {
                content = content.push(text("Refresh the local calculation to see this score's formula and contributing cards."));
            }
        }

        if let Some(report) = &analysis.report {
            if selected == Section::Bracket {
                let mut rows = vec![text(format!("Reported bracket: {}", report.bracket))
                    .size(style::T_SUBHEAD)
                    .into()];
                rows.extend(
                    report
                        .rationale
                        .iter()
                        .map(|reason| text(reason).size(style::T_BODY).into()),
                );
                content = content.push(section(
                    "CommanderSalt bracket",
                    Some("Saved report result, separate from the local calculation."),
                    rows,
                ));
            } else if selected == Section::Salt {
                content = content.push(
                    text(format!(
                        "CommanderSalt snapshot | {:.2} points",
                        report.salt_total
                    ))
                    .size(style::T_SUBHEAD),
                );
                for category in &report.categories {
                    let key = format!("report:Salt:{}", category.label);
                    content = content.push(disclosure(
                        &key,
                        &category.label,
                        &format!(
                            "{} entries | {:.2} reported points",
                            category.cards.len(),
                            category.score
                        ),
                        details,
                        || salt_table(&category.cards, &key, details, true),
                    ));
                }
            } else {
                let value = match selected {
                    Section::Power => Some(report.power),
                    Section::Synergy => Some(report.synergy),
                    Section::WinConditions => Some(report.win_conditions),
                    Section::Interaction => Some(report.interaction),
                    Section::Consistency => report.consistency,
                    Section::Efficiency => report.efficiency,
                    _ => None,
                };
                let key = format!("report:{}:snapshot", selected.label());
                let note = value
                    .map(|v| {
                        format!(
                            "{v:.2}{}",
                            if selected == Section::Power {
                                " / 10"
                            } else {
                                " reported points"
                            }
                        )
                    })
                    .unwrap_or_else(|| "Not supplied in saved report".into());
                let mut rows: Vec<Element<'a, crate::app::Message>> = vec![
                    text("Saved report result. CommanderSalt uses a separate calculation; its snapshot does not include the full calculation for this metric.").size(style::T_BODY).color(style::TEXT_MUTED).into()
                ];
                if selected == Section::Interaction {
                    if let Some(cards) = &report.interaction_cards {
                        rows.push(evidence_list(
                            "Reported interaction cards",
                            cards,
                            &key,
                            details,
                        ));
                    }
                }
                if selected == Section::WinConditions {
                    rows.push(
                        text(format!(
                            "{} combo entries | {} independent winning lines",
                            report.combo_count, report.independent_lines
                        ))
                        .size(style::T_BODY)
                        .into(),
                    );
                }
                content = content.push(section("CommanderSalt snapshot", Some(&note), rows));
            }
        }
        let page = container(content).padding(style::GAP).width(Length::Fill);
        scrollable(page).id(scroll_id()).height(Length::Fill).into()
    };
    column![navigation.wrap(), content]
        .spacing(style::GAP_SM)
        .height(Length::Fill)
        .into()
}

fn disclosure<'a>(
    key: &str,
    title: &str,
    note: &str,
    details: &Details,
    build: impl FnOnce() -> Element<'a, crate::app::Message>,
) -> Element<'a, crate::app::Message> {
    let open = details.is_open(key);
    let header = iced::widget::button(container(
        row![
            column![
                text(title.to_owned()).size(style::T_SUBHEAD),
                text(note.to_owned())
                    .size(style::T_CAPTION)
                    .color(style::TEXT_MUTED)
            ]
            .spacing(style::GAP_XS)
            .width(Length::Fill),
            text(if open { "Hide" } else { "Show" })
                .size(style::T_LABEL)
                .color(style::ACCENT_BRIGHT),
        ]
        .spacing(style::GAP)
        .align_y(Alignment::Center),
    ))
    .padding(style::GAP)
    .width(Length::Fill)
    .style(style::secondary)
    .on_press(crate::app::Message::ToggleScoreGroup(key.to_owned()));
    let mut body = column![header].spacing(style::GAP_SM);
    if open {
        body = body.push(
            container(
                column![
                    build(),
                    style::touch_button("Hide these details", style::T_LABEL)
                        .width(Length::Fill)
                        .style(style::ghost)
                        .on_press(crate::app::Message::ToggleScoreGroup(key.to_owned()))
                ]
                .spacing(style::GAP),
            )
            .padding(style::GAP)
            .width(Length::Fill)
            .style(style::panel),
        );
    }
    body.into()
}

fn score_hero<'a>(
    value: String,
    unit: &str,
    caption: &str,
    supporting: String,
) -> Element<'a, crate::app::Message> {
    container(
        column![
            text(caption.to_owned())
                .size(style::T_LABEL)
                .color(style::TEXT_MUTED),
            row![
                text(value)
                    .size(style::T_DISPLAY)
                    .color(style::ACCENT_BRIGHT),
                text(unit.to_owned())
                    .size(style::T_SUBHEAD)
                    .color(style::TEXT_MUTED)
            ]
            .spacing(style::GAP_SM)
            .align_y(Alignment::Center),
            text(supporting)
                .size(style::T_BODY)
                .color(style::TEXT_MUTED),
        ]
        .spacing(style::GAP_SM),
    )
    .padding(style::GAP)
    .width(Length::Fill)
    .style(style::panel)
    .into()
}

/// Small evidence lists read as content; large collections can be opened on demand.
fn evidence_list<'a>(
    title: &str,
    cards: &'a [String],
    key: &str,
    details: &Details,
) -> Element<'a, crate::app::Message> {
    if cards.len() > 6 {
        disclosure(
            key,
            title,
            &format!("{} listed cards", cards.len()),
            details,
            || paged_cards(cards, key, details),
        )
    } else {
        let mut names: Vec<_> = cards.iter().collect();
        names.sort_by_cached_key(|name| name.to_lowercase());
        let note = (cards.len() > 1).then(|| format!("{} listed cards", cards.len()));
        section(
            title,
            note.as_deref(),
            names
                .into_iter()
                .map(|name| {
                    container(text(name).size(style::T_BODY))
                        .padding(style::GAP_SM)
                        .width(Length::Fill)
                        .style(style::table_row)
                        .into()
                })
                .collect(),
        )
    }
}

fn paged_cards<'a>(
    cards: &'a [String],
    key: &str,
    details: &Details,
) -> Element<'a, crate::app::Message> {
    let mut names: Vec<_> = cards.iter().collect();
    names.sort_by_cached_key(|name| name.to_lowercase());
    let limit = details.limit(key);
    let shown = limit.min(names.len());
    let mut rows = column![container(row![
        text("Card")
            .size(style::T_CAPTION)
            .color(style::TEXT_MUTED)
            .width(Length::Fill),
        table_number("Copies".into(), 75., style::TEXT_MUTED, style::T_CAPTION)
    ])
    .padding([8, 12])]
    .spacing(style::GAP_XS);
    for (index, entry) in names.into_iter().take(limit).enumerate() {
        let (quantity, name) = entry
            .split_once("× ")
            .and_then(|(q, n)| q.trim().parse::<u32>().ok().map(|q| (q, n)))
            .unwrap_or((1, entry.as_str()));
        rows = rows.push(
            container(
                row![
                    text(name).size(style::T_BODY).width(Length::Fill),
                    table_number(quantity.to_string(), 75., style::TEXT_MUTED, style::T_BODY)
                ]
                .spacing(style::GAP),
            )
            .padding([12, 12])
            .width(Length::Fill)
            .style(move |theme| {
                if index % 2 == 0 {
                    table_band(theme)
                } else {
                    container::Style::default()
                }
            }),
        );
    }
    rows = rows.push(
        text(format!("Showing {shown} of {} listed cards", cards.len()))
            .size(style::T_CAPTION)
            .color(style::TEXT_MUTED),
    );
    if shown < cards.len() {
        rows = rows.push(
            style::touch_button("Show 12 more cards", style::T_LABEL)
                .width(Length::Fill)
                .style(style::secondary)
                .on_press(crate::app::Message::MoreScoreCards(key.to_owned())),
        );
    }
    rows.into()
}

fn salt_table<'a>(
    cards: &[CardScore],
    key: &str,
    details: &Details,
    report: bool,
) -> Element<'a, crate::app::Message> {
    let mut cards = cards.to_vec();
    if !report {
        crate::salt::sort_card_scores(&mut cards);
    }
    let shown = details.limit(key).min(cards.len());
    let mut head = row![text("Card / quantity")
        .size(style::T_CAPTION)
        .width(Length::Fill)]
    .spacing(style::GAP_SM);
    if !report {
        head = head.push(table_number(
            "Per copy".into(),
            100.,
            style::TEXT_MUTED,
            style::T_CAPTION,
        ));
    }
    head = head.push(table_number(
        if report { "Reported" } else { "Total points" }.into(),
        100.,
        style::TEXT_MUTED,
        style::T_CAPTION,
    ));
    let mut rows = column![container(head).padding([8, 12])].spacing(style::GAP_XS);
    for (index, card) in cards.iter().take(shown).enumerate() {
        let mut cells = row![text(card.display_name())
            .size(style::T_BODY)
            .width(Length::Fill)]
        .spacing(style::GAP_SM);
        if !report {
            cells = cells.push(table_number(
                card.unit_score()
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "Unknown".into()),
                100.,
                style::TEXT_MUTED,
                style::T_BODY,
            ));
        }
        cells = cells.push(table_number(
            format!("{:.2}", card.score),
            100.,
            style::ACCENT_BRIGHT,
            style::T_BODY,
        ));
        rows = rows.push(
            container(cells)
                .padding([12, 12])
                .width(Length::Fill)
                .style(move |theme| {
                    if index % 2 == 0 {
                        table_band(theme)
                    } else {
                        container::Style::default()
                    }
                }),
        );
    }
    rows = rows.push(
        text(format!("Showing {shown} of {} entries", cards.len()))
            .size(style::T_CAPTION)
            .color(style::TEXT_MUTED),
    );
    if shown < cards.len() {
        rows = rows.push(
            style::touch_button("Show 12 more entries", style::T_LABEL)
                .width(Length::Fill)
                .style(style::secondary)
                .on_press(crate::app::Message::MoreScoreCards(key.to_owned())),
        );
    }
    if !report && cards.iter().any(|c| c.quantity.is_none()) {
        rows = rows.push(text("Refresh the local calculation to recover per-copy ratings for this older snapshot.").size(style::T_BODY).color(style::ACCENT_BRIGHT));
    }
    rows.into()
}

fn collapse_control<'a>() -> Element<'a, crate::app::Message> {
    style::touch_button("Collapse all details", style::T_LABEL)
        .width(230)
        .style(style::ghost)
        .on_press(crate::app::Message::CollapseScoreGroups)
        .into()
}

fn table_number<'a>(
    value: String,
    width: f32,
    tone: iced::Color,
    size: u16,
) -> Element<'a, crate::app::Message> {
    container(text(value).size(size).color(tone))
        .width(width)
        .align_x(Alignment::End)
        .into()
}

fn table_band(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(style::SURFACE_2.into()),
        border: iced::Border {
            radius: 8.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn pill<'a>(label: &str, tone: iced::Color) -> Element<'a, crate::app::Message> {
    container(text(label.to_owned()).size(style::T_CAPTION).color(tone))
        .padding([8, 12])
        .style(move |_| container::Style {
            background: Some(iced::Color { a: 0.09, ..tone }.into()),
            border: iced::Border {
                color: iced::Color { a: 0.20, ..tone },
                width: 1.,
                radius: 10.into(),
            },
            ..Default::default()
        })
        .into()
}

fn calculation_step<'a>(
    number: &str,
    label: &str,
    value: &str,
    note: &str,
    tone: iced::Color,
) -> Element<'a, crate::app::Message> {
    container(
        column![
            text(number.to_owned())
                .size(style::T_CAPTION)
                .color(style::ACCENT_BRIGHT),
            text(label.to_owned()).size(style::T_LABEL),
            text(value.to_owned()).size(style::T_HEADING).color(tone),
            text(note.to_owned())
                .size(style::T_CAPTION)
                .color(style::TEXT_MUTED),
        ]
        .spacing(style::GAP_SM),
    )
    .padding(style::GAP)
    .width(310)
    .style(table_band)
    .into()
}

fn power_details<'a>(a: &'a crate::power::Assessment) -> Element<'a, crate::app::Message> {
    let metrics = [
        (Section::WinConditions, &a.win_conditions, 0.25),
        (Section::Consistency, &a.consistency, 0.20),
        (Section::Interaction, &a.interaction, 0.20),
        (Section::Efficiency, &a.efficiency, 0.20),
        (Section::Synergy, &a.synergy, 0.15),
    ];
    let weighted: f64 = metrics.iter().map(|(_, m, w)| m.score * w).sum();
    let mut body = column![
        container(
            row![
                column![
                    text("Local power estimate")
                        .size(style::T_LABEL)
                        .color(style::TEXT_MUTED),
                    row![
                        text(
                            a.power
                                .map(|p| format!("{p:.2}"))
                                .unwrap_or_else(|| "—".into())
                        )
                        .size(style::T_DISPLAY)
                        .color(style::ACCENT_BRIGHT),
                        text("/ 10").size(style::T_SUBHEAD).color(style::TEXT_MUTED)
                    ]
                    .spacing(style::GAP_SM)
                    .align_y(Alignment::Center)
                ]
                .spacing(style::GAP_SM)
                .width(Length::Fill),
                column![
                    text("Weighted score")
                        .size(style::T_LABEL)
                        .color(style::TEXT_MUTED),
                    text(format!("{weighted:.2} / 100")).size(style::T_HEADING),
                    text(if a.power.is_some() {
                        "Five contributing metrics"
                    } else {
                        "Power needs a complete deck"
                    })
                    .size(style::T_CAPTION)
                    .color(style::TEXT_MUTED)
                ]
                .spacing(style::GAP_SM)
                .width(Length::Fill),
            ]
            .spacing(style::GAP)
            .align_y(Alignment::Center)
        )
        .padding(style::GAP)
        .width(Length::Fill)
        .style(style::panel),
        text("Score contributions").size(style::T_SUBHEAD),
        text("Tap a metric to explore its calculation and contributing cards.")
            .size(style::T_BODY)
            .color(style::TEXT_MUTED),
    ]
    .spacing(style::GAP);
    let tones = [
        style::ACCENT,
        style::SUCCESS,
        iced::Color::from_rgb8(93, 168, 239),
        iced::Color::from_rgb8(239, 183, 91),
        iced::Color::from_rgb8(225, 127, 182),
    ];
    for (index, (target, metric, weight)) in metrics.into_iter().enumerate() {
        body = body.push(
            iced::widget::button(
                column![
                    row![
                        column![
                            text(target.label()).size(style::T_SUBHEAD),
                            text(format!("{:.0}% of power", weight * 100.))
                                .size(style::T_CAPTION)
                                .color(style::TEXT_MUTED)
                        ]
                        .spacing(style::GAP_XS)
                        .width(Length::Fill),
                        column![
                            text(format!("{:.1} / 100", metric.score)).size(style::T_SUBHEAD),
                            text(format!("{:.2} weighted points", metric.score * weight))
                                .size(style::T_CAPTION)
                                .color(style::TEXT_MUTED)
                        ]
                        .spacing(style::GAP_XS)
                        .align_x(Alignment::End),
                    ]
                    .spacing(style::GAP)
                    .align_y(Alignment::Center),
                    bar(metric.score / 100., Length::Fill, tones[index])
                ]
                .spacing(style::GAP_SM),
            )
            .padding(style::GAP)
            .width(Length::Fill)
            .style(style::secondary)
            .on_press(crate::app::Message::ScoreSection(target)),
        );
    }
    let mut composition = row![].spacing(3);
    let mut legend = row![].spacing(style::GAP_SM);
    for (index, (target, metric, weight)) in metrics.iter().enumerate() {
        let points = metric.score * weight;
        if points > 0. {
            let tone = tones[index];
            composition = composition.push(
                container(Space::new(Length::Fill, 26))
                    .width(Length::FillPortion((points * 100.).round().max(1.) as u16))
                    .style(move |_| container::Style {
                        background: Some(tone.into()),
                        border: iced::Border {
                            radius: 5.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            );
        }
        legend = legend.push(pill(
            &format!("{}  {:.2}", target.label(), points),
            tones[index],
        ));
    }
    let stages = row![
        calculation_step(
            "01",
            "Weight each metric",
            "Score × its share",
            "25% · 20% · 20% · 20% · 15%",
            style::TEXT
        ),
        calculation_step(
            "02",
            "Add the contributions",
            &format!("{weighted:.2}"),
            "Weighted points out of 100",
            style::ACCENT_BRIGHT
        ),
        calculation_step(
            "03",
            "Convert to power",
            &a.power
                .map(|v| format!("{v:.2} / 10"))
                .unwrap_or_else(|| "Pending".into()),
            &format!("1 + 9 × ({weighted:.2} ÷ 100)"),
            style::ACCENT_BRIGHT
        ),
    ]
    .spacing(style::GAP_SM)
    .wrap();
    body = body.push(container(column![
        text("How the score comes together").size(style::T_LEAD),
        text("Each color is one metric’s contribution to the weighted total.").size(style::T_BODY).color(style::TEXT_MUTED),
        composition,
        legend.wrap(),
        stages,
        text("Power requires 100 cards and known mana values for every nonland card. Salt, prices and declared brackets do not contribute.").size(style::T_CAPTION).color(style::TEXT_MUTED),
    ].spacing(style::GAP)).padding(style::GAP).width(Length::Fill).style(style::panel));
    body.into()
}

fn metric_page<'a>(
    a: &'a crate::power::Assessment,
    selected: Section,
    details: &Details,
) -> Element<'a, crate::app::Message> {
    let (metric, caption) = match selected {
        Section::Synergy => (&a.synergy, "Coverage of connected engines"),
        Section::WinConditions => (&a.win_conditions, "Strength of detected winning plans"),
        Section::Interaction => (&a.interaction, "Answers, disruption and protection"),
        Section::Consistency => (&a.consistency, "Card access and engine consistency"),
        _ => (&a.efficiency, "Curve and mana development"),
    };
    let groups: Vec<_> = metric
        .evidence
        .iter()
        .filter(|g| !g.cards.is_empty())
        .collect();
    let long_groups = groups
        .iter()
        .any(|g| g.cards.len() > 6 && !g.label.starts_with("Combo family "));
    let mut calculation = column![].spacing(style::GAP_SM);
    if selected == Section::Efficiency {
        calculation = calculation.push(
            text(format!(
                "Average nonland mana value: {} | {} lands",
                a.average_mana_value
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "Unknown".into()),
                a.land_count
            ))
            .size(style::T_BODY),
        );
    }
    if selected == Section::WinConditions {
        for (label, rule) in [
            ("Combo plans", "Winning family: 65. Treasure engine with a matching conditional payoff: 55. Lock: 45. Resource engine needing an outlet: 35. Up to two additional families add 10 each."),
            ("Alternate wins", "Start at 30. Up to two additional alternate-win cards add 5 each."),
            ("Combat", "Start at 20 with at least 15 creatures. Add 5 for up to six finishers."),
            ("Combining plans", "Use the strongest plan. Up to two additional detected plans add 5 each. Cap at 100.")
        ] {
            calculation = calculation.push(column![text(label).size(style::T_LABEL), text(rule).size(style::T_BODY).color(style::TEXT_MUTED)].spacing(style::GAP_XS));
        }
    }
    let mut body = column![score_hero(
        format!("{:.1}", metric.score),
        "/ 100",
        caption,
        metric.explanation.clone()
    ),]
    .spacing(style::GAP);
    let families: Vec<_> = groups
        .iter()
        .filter(|g| g.label.starts_with("Combo family "))
        .collect();
    if !families.is_empty() {
        let mut rows = Vec::new();
        for (index, family) in families.into_iter().enumerate() {
            let (_, outcome) = family
                .label
                .split_once(" · ")
                .unwrap_or((&family.label, ""));
            let pieces: Vec<Vec<&str>> = family
                .cards
                .iter()
                .map(|line| line.split(" + ").collect())
                .collect();
            let common: Vec<&str> = pieces
                .first()
                .into_iter()
                .flatten()
                .copied()
                .filter(|card| pieces.iter().all(|line| line.contains(card)))
                .collect();
            let mut entry = column![
                row![
                    text(format!("FAMILY {:02}", index + 1))
                        .size(style::T_CAPTION)
                        .color(style::ACCENT_BRIGHT)
                        .width(Length::Fill),
                    text(format!(
                        "{} variant{}",
                        pieces.len(),
                        if pieces.len() == 1 { "" } else { "s" }
                    ))
                    .size(style::T_CAPTION)
                    .color(style::TEXT_MUTED),
                ]
                .spacing(style::GAP_SM),
                text(if common.is_empty() {
                    family.cards[0].clone()
                } else {
                    common.join(" + ")
                })
                .size(style::T_SUBHEAD),
                pill(
                    outcome,
                    if outcome == "winning outcome" {
                        style::SUCCESS
                    } else {
                        style::ACCENT_BRIGHT
                    }
                ),
            ]
            .spacing(style::GAP_SM);
            if common.len() >= 2 && pieces.len() > 1 {
                entry = entry.push(
                    text("Pair this core with one of these pieces")
                        .size(style::T_CAPTION)
                        .color(style::TEXT_MUTED),
                );
                let mut choices = row![].spacing(style::GAP_SM);
                for line in pieces {
                    let extra: Vec<_> = line
                        .into_iter()
                        .filter(|card| !common.contains(card))
                        .collect();
                    choices = choices.push(pill(
                        &if extra.is_empty() {
                            "Core alone".into()
                        } else {
                            extra.join(" + ")
                        },
                        style::TEXT,
                    ));
                }
                entry = entry.push(choices.wrap());
            } else if pieces.len() > 1 {
                // Some transitive families have no universal core; preserve every complete line.
                for line in &family.cards {
                    entry = entry.push(
                        container(text(line).size(style::T_BODY))
                            .padding(style::GAP_SM)
                            .width(Length::Fill)
                            .style(table_band),
                    );
                }
            }
            if index > 0 {
                rows.push(iced::widget::horizontal_rule(1).into());
            }
            rows.push(
                container(entry)
                    .padding(style::GAP)
                    .width(Length::Fill)
                    .into(),
            );
        }
        body = body.push(section(
            "Combos",
            Some("Shared pieces form one plan. Variants show different ways to complete it."),
            rows,
        ));
    }
    let other_groups: Vec<_> = groups
        .into_iter()
        .filter(|g| !g.label.starts_with("Combo family "))
        .collect();
    if !other_groups.is_empty() {
        let mut heading = row![text("Contributing cards")
            .size(style::T_SUBHEAD)
            .width(Length::Fill)];
        if long_groups {
            heading = heading.push(collapse_control());
        }
        body = body.push(heading.align_y(Alignment::Center));
        for group in other_groups {
            let key = format!("local:{}:{}", selected.label(), group.label);
            body = body.push(evidence_list(&group.label, &group.cards, &key, details));
        }
    }
    // Keep explanations visible; only large evidence collections need disclosure.
    // The score hero already supplies the short explanation on other metric pages.
    if matches!(selected, Section::WinConditions | Section::Efficiency) {
        body = body.push(section(
            "How this score is calculated",
            None,
            vec![calculation.into()],
        ));
    }
    body.into()
}

/// Width of the count and floor cells in the criteria table, so the columns
/// line up down the screen regardless of label length.
const W_COUNT: f32 = 70.0;
const W_FLOOR: f32 = 110.0;
const W_SCORE: f32 = 96.0;
const BAR_H: f32 = 14.0;
/// How many cards to name per salt category before falling back to a count.
const CARDS_SHOWN: usize = 6;

/// The whole breakdown, as one scrollable body.
///
/// Deliberately chrome-free: no header, no back button, no refresh. The
/// screen that owns the deck supplies those, which keeps navigation in one
/// place and lets this render for any message type at all.
pub fn body(analysis: &Analysis) -> Element<'_, crate::app::Message> {
    let mut body = column![].spacing(style::GAP);
    if let Some(report) = &analysis.report {
        if !analysis.has_local() {
            return scrollable(
                container(report_sections(analysis, report, false)).padding(style::GAP),
            )
            .height(Length::Fill)
            .into();
        }
    }
    body = body.push(hero(analysis));
    if let Some(assessment) = &analysis.assessment {
        body = body.push(local_overview(assessment, analysis.salt_total));
    } else {
        body = body.push(container(text("Re-check this Moxfield deck to calculate power, synergy, interaction and win conditions.")
            .size(style::T_BODY).color(style::ACCENT_BRIGHT)).padding(style::GAP).style(style::panel));
    }
    if let Some(report) = &analysis.report {
        body = body.push(comparison(analysis, report));
    }
    body = body.push(bracket_calculation(analysis, false));
    body = body.push(criteria_section(analysis));
    if let Some(a) = &analysis.assessment {
        let mut rows = vec![
            text(format!(
                "Win conditions: {:.0} / 100",
                a.win_conditions.score
            ))
            .size(style::T_SUBHEAD)
            .into(),
            text(&a.win_conditions.explanation)
                .size(style::T_BODY)
                .color(style::TEXT_MUTED)
                .into(),
        ];
        for (label, prefix) in [
            ("Alternate-win cards", "Alternate-win"),
            ("Combat and damage finishers", "Combat finishers"),
        ] {
            if let Some(group) = a
                .win_conditions
                .evidence
                .iter()
                .find(|g| g.label.starts_with(prefix) && !g.cards.is_empty())
            {
                rows.push(
                    column![
                        text(format!("{label} · {} listed", group.cards.len()))
                            .size(style::T_LABEL),
                        text(name_list(&group.cards, 3))
                            .size(style::T_BODY)
                            .color(style::TEXT_MUTED)
                    ]
                    .spacing(style::GAP_XS)
                    .into(),
                );
            }
        }
        if a.combo_families > 0 {
            rows.push(text(format!("{} detected combo families — see the combo lines below for pieces and prerequisites.", a.combo_families)).size(style::T_BODY).into());
        }
        rows.push(
            style::touch_button("View winning plans and contributing cards", style::T_LABEL)
                .style(style::secondary)
                .on_press(crate::app::Message::ScoreSection(Section::WinConditions))
                .into(),
        );
        body = body.push(section("Winning plans", Some("A quick overview. The Win conditions section contains the full calculation and card lists."), rows));
    }

    if !analysis.combos.is_empty() {
        body = body.push(combos_section(analysis));
    }

    if analysis.scoring_version < crate::salt::SCORING_VERSION {
        body = body.push(text("Saved with an older scoring method. Refresh the local calculation for per-copy salt and corrected card-face lookups.").size(style::T_BODY).color(style::ACCENT_BRIGHT));
    }
    body = body.push(footnotes(analysis));
    if let Some(report) = &analysis.report {
        body = body.push(report_sections(analysis, report, false));
    }

    scrollable(container(body).padding(iced::Padding {
        right: 16.,
        ..iced::Padding::new(style::GAP_SM as f32)
    }))
    .id(scroll_id())
    .height(Length::Fill)
    .into()
}

pub(crate) fn local_overview<'a>(
    assessment: &'a crate::power::Assessment,
    salt_total: f64,
) -> Element<'a, crate::app::Message> {
    let tile = |label: &str, value: String| -> Element<'a, crate::app::Message> {
        iced::widget::button(
            column![
                text(value)
                    .size(style::T_HEADING)
                    .color(style::ACCENT_BRIGHT),
                text(label.to_owned())
                    .size(style::T_BODY)
                    .color(style::TEXT_MUTED)
            ]
            .spacing(style::GAP_XS),
        )
        .padding(style::GAP)
        .width(Length::Fill)
        .style(style::secondary)
        .on_press(crate::app::Message::ScoreSection(
            match label.split(" / ").next().unwrap_or(label) {
                "Salt" => Section::Salt,
                "Synergy" => Section::Synergy,
                "Interaction" => Section::Interaction,
                "Win conditions" => Section::WinConditions,
                "Consistency" => Section::Consistency,
                "Efficiency" => Section::Efficiency,
                _ => Section::Summary,
            },
        ))
        .into()
    };
    let mut overview = column![
        text("Local deck assessment").size(style::T_HEADING),
        row![tile("Salt", format!("{salt_total:.2}")),
            tile("Synergy / 100", format!("{:.0}", assessment.synergy.score))].spacing(style::GAP),
        row![tile("Interaction / 100", format!("{:.0}", assessment.interaction.score)),
            tile("Win conditions / 100", format!("{:.0}", assessment.win_conditions.score))].spacing(style::GAP),
        row![tile("Consistency / 100", format!("{:.0}", assessment.consistency.score)),
            tile("Efficiency / 100", format!("{:.0}", assessment.efficiency.score))].spacing(style::GAP),
        text("Calculated from this Moxfield list and known combos. These are local estimates, separate from salt and CommanderSalt's scores; timing and pilot decisions aren't simulated.")
            .size(style::T_BODY).color(style::TEXT_MUTED),
    ].spacing(style::GAP_SM);
    if assessment.power.is_none() {
        overview = overview.push(text("Power needs a complete 100-card list with mana values for every nonland card. Re-check to fetch missing data.")
            .size(style::T_BODY).color(style::ACCENT_BRIGHT));
    }
    overview.into()
}

fn comparison<'a, Msg: 'a>(
    analysis: &'a Analysis,
    report: &'a crate::commander_salt::Report,
) -> Element<'a, Msg> {
    let cell = |value: String| text(value).size(style::T_BODY).width(Length::Fill);
    let mut rows = column![
        text("Local and CommanderSalt comparison").size(style::T_HEADING),
        row![
            cell("Metric".into()),
            cell("Local".into()),
            cell("CommanderSalt".into())
        ]
        .spacing(style::GAP),
    ]
    .spacing(style::GAP_SM);
    let mut values = vec![
        (
            "Salt",
            format!("{:.2}", analysis.salt_total),
            format!("{:.2}", report.salt_total),
        ),
        (
            "Bracket",
            format!("{} (calculated)", analysis.calculated_bracket()),
            report.bracket.to_string(),
        ),
        (
            "Cards",
            analysis.card_count.to_string(),
            report.card_count.to_string(),
        ),
    ];
    if let Some(a) = &analysis.assessment {
        values.insert(
            0,
            (
                "Power / 10",
                a.power
                    .map(|p| format!("{p:.2}"))
                    .unwrap_or_else(|| "Unavailable".into()),
                format!("{:.2}", report.power),
            ),
        );
        values.push((
            "Synergy coverage",
            format!("{:.1}%", a.synergy.score),
            report
                .synergy_coverage
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "Not in saved report".into()),
        ));
        for (label, local, remote) in [
            ("Synergy", a.synergy.score, Some(report.synergy)),
            ("Interaction", a.interaction.score, Some(report.interaction)),
            (
                "Win conditions",
                a.win_conditions.score,
                Some(report.win_conditions),
            ),
            ("Consistency", a.consistency.score, report.consistency),
            ("Efficiency", a.efficiency.score, report.efficiency),
        ] {
            values.push((
                label,
                format!("{local:.1} / 100"),
                remote
                    .map(|v| format!("{v:.1} pts"))
                    .unwrap_or_else(|| "Not in saved report".into()),
            ));
        }
    }
    for (label, local, remote) in values {
        rows = rows.push(row![cell(label.into()), cell(local), cell(remote)].spacing(style::GAP));
    }
    let timestamp = |value: &str| {
        chrono::DateTime::parse_from_rfc3339(value)
            .map(|date| date.format("%Y-%m-%d %H:%M %:z").to_string())
            .unwrap_or_else(|_| value.to_owned())
    };
    rows = rows.push(text(format!("Local calculated: {}\nCommanderSalt scored: {}\nReport imported: {}", timestamp(&analysis.analysed_at), timestamp(&report.scored_at), timestamp(&report.saved_at))).size(style::T_CAPTION).color(style::TEXT_MUTED))
        .push(text("Each source is saved separately. Reports may describe an older deck revision. Supporting scores currently use different formulas and scales; the calculated bracket also considers local power.").size(style::T_CAPTION).color(style::TEXT_MUTED));
    if let (Some(a), Some(remote_cards)) = (&analysis.assessment, &report.interaction_cards) {
        let local: std::collections::BTreeSet<_> = a
            .interaction
            .evidence
            .iter()
            .flat_map(|g| &g.cards)
            .map(String::as_str)
            .collect();
        // Report entries may use one face's name, while the local list keeps
        // the full physical card name. Compare both faces before claiming a miss.
        let missing: Vec<_> = remote_cards
            .iter()
            .filter(|name| {
                !local.iter().any(|local| {
                    local
                        .split_once("× ")
                        .map_or(*local, |(_, name)| name)
                        .split(" // ")
                        .any(|face| name.split(" // ").any(|remote| face == remote))
                })
            })
            .cloned()
            .collect();
        rows = rows.push(
            text(format!(
                "CommanderSalt interaction cards not recognized locally: {}",
                if missing.is_empty() {
                    "None".into()
                } else {
                    name_list(&missing, 30)
                }
            ))
            .size(style::T_CAPTION)
            .color(style::TEXT_MUTED),
        );
    }
    container(rows)
        .padding(style::GAP)
        .style(style::panel)
        .into()
}

fn report_sections<'a, Msg: 'a>(
    analysis: &'a Analysis,
    report: &'a crate::commander_salt::Report,
    include_contributions: bool,
) -> iced::widget::Column<'a, Msg> {
    let metric = |label: &str, value: String| -> Element<'a, Msg> {
        container(
            column![
                text(value)
                    .size(style::T_HEADING)
                    .color(style::ACCENT_BRIGHT),
                text(label.to_owned())
                    .size(style::T_BODY)
                    .color(style::TEXT_MUTED),
            ]
            .spacing(style::GAP_XS),
        )
        .padding(style::GAP)
        .width(Length::Fill)
        .style(style::panel)
        .into()
    };
    let mut body = column![
        text("CommanderSalt assessment").size(style::T_HEADING),
        text(&analysis.deck_name).size(style::T_SUBHEAD),
        text(format!("{} cards · Report scored {} · Saved {}", report.card_count,
            short_date(&report.scored_at), short_date(&report.saved_at)))
            .size(style::T_CAPTION).color(style::TEXT_MUTED),
        row![metric("Power / 10", format!("{:.1}", report.power)),
             metric("CommanderSalt bracket", report.bracket.to_string())].spacing(style::GAP),
        row![metric("Salt", format!("{:.2}", report.salt_total)), metric("Synergy", format!("{:.1}", report.synergy))].spacing(style::GAP),
        row![metric("Interaction", format!("{:.1}", report.interaction)), metric("Win conditions", format!("{:.1}", report.win_conditions))].spacing(style::GAP),
        text("Power combines consistency, efficiency, interaction, win conditions and mana quality. Synergy supports that assessment; these scores are separate from salt.")
            .size(style::T_BODY).color(style::TEXT_MUTED),
        text(format!("{} combo entries · {} independent winning lines", report.combo_count, report.independent_lines))
            .size(style::T_SUBHEAD),
    ].spacing(style::GAP);
    if !report.rationale.is_empty() {
        body = body.push(section(
            "Why this bracket",
            None,
            report
                .rationale
                .iter()
                .map(|reason| text(reason).size(style::T_BODY).into())
                .collect(),
        ));
    }
    for category in report.categories.iter().filter(|_| include_contributions) {
        let cards = category
            .cards
            .iter()
            .map(|card| {
                container(
                    column![
                        row![
                            text(&card.name).size(style::T_LABEL).width(Length::Fill),
                            column![
                                text(format!("{:.2}", card.score))
                                    .size(style::T_LABEL)
                                    .color(style::ACCENT_BRIGHT),
                                text("reported points")
                                    .size(style::T_CAPTION)
                                    .color(style::TEXT_MUTED)
                            ]
                            .align_x(Alignment::End),
                        ]
                        .spacing(style::GAP)
                        .align_y(Alignment::Center),
                        iced::widget::horizontal_rule(1)
                    ]
                    .spacing(style::GAP_SM),
                )
                .padding(style::GAP_SM)
                .into()
            })
            .collect();
        body = body.push(section(
            &format!("{} · {:.2}", category.label, category.score),
            Some(
                "Reported card or combo contributions; category totals may also apply quantities.",
            ),
            cards,
        ));
    }
    body = body.push(text(&report.url).size(style::T_CAPTION).color(style::ACCENT_BRIGHT))
        .push(text("This is the published report snapshot. If you edit the deck, refresh its analysis on CommanderSalt, then paste the updated report link here.")
            .size(style::T_CAPTION).color(style::TEXT_MUTED));
    body
}

/// The two headline numbers, side by side, with the one-line reason for each.
fn hero(analysis: &Analysis) -> Element<'_, crate::app::Message> {
    let value = |label: &'static str, number: String, target| {
        iced::widget::button(
            column![
                text(number)
                    .size(style::T_DISPLAY)
                    .color(style::ACCENT_BRIGHT),
                text(label).size(style::T_BODY).color(style::TEXT_MUTED),
            ]
            .spacing(style::GAP_XS),
        )
        .padding(style::GAP)
        .width(225)
        .style(style::secondary)
        .on_press(crate::app::Message::ScoreSection(target))
    };
    let power = analysis
        .assessment
        .as_ref()
        .and_then(|a| a.power)
        .map(|v| format!("{v:.2}"))
        .unwrap_or_else(|| "—".into());
    let mut headline = row![
        value("Power level / 10", power, Section::Power),
        value(
            "Calculated bracket",
            analysis.calculated_bracket().to_string(),
            Section::Bracket
        ),
        value(
            "Card-based bracket",
            analysis.bracket.to_string(),
            Section::Bracket
        ),
    ]
    .spacing(style::GAP_SM);
    if let Some(report) = &analysis.report {
        headline = headline.push(value(
            "CommanderSalt bracket",
            report.bracket.to_string(),
            Section::Bracket,
        ));
    }
    column![headline.wrap(), text(claims(analysis)).size(style::T_BODY).color(style::TEXT_MUTED),
        text("Calculated bracket uses the higher of the card-based bracket and the local power bracket.")
            .size(style::T_BODY).color(style::TEXT_MUTED)]
        .spacing(style::GAP_SM).into()
}

fn bracket_calculation<'a, Msg: 'a>(analysis: &'a Analysis, show_scale: bool) -> Element<'a, Msg> {
    let power = analysis.assessment.as_ref().and_then(|a| a.power);
    let band = analysis.power_bracket();
    let mut rows = vec![
        text(format!("1. Card-based bracket: {}", analysis.bracket))
            .size(style::T_SUBHEAD)
            .into(),
        text("The detected card and combo criteria below set this floor.")
            .size(style::T_BODY)
            .color(style::TEXT_MUTED)
            .into(),
        text(match (power, band) {
            (Some(power), Some(band)) => {
                format!("2. Local power: {power:.2} / 10 gives bracket {band}")
            }
            _ => "2. Local power is unavailable".into(),
        })
        .size(style::T_SUBHEAD)
        .into(),
        text(match band {
            Some(band) => format!(
                "3. Higher of bracket {} and bracket {band}: calculated bracket {}",
                analysis.bracket,
                analysis.calculated_bracket()
            ),
            None => format!(
                "3. Use the card-based bracket: calculated bracket {}",
                analysis.calculated_bracket()
            ),
        })
        .size(style::T_SUBHEAD)
        .color(style::ACCENT_BRIGHT)
        .into(),
    ];
    if show_scale {
        rows.push(text("Local power bands").size(style::T_SUBHEAD).into());
        for (range, bracket) in [
            ("Below 3", 1),
            ("3 up to 5", 2),
            ("5 up to 7", 3),
            ("7 up to 9", 4),
            ("9 and above", 5),
        ] {
            rows.push(
                container(
                    row![
                        text(range).size(style::T_BODY).width(Length::Fill),
                        text(format!("Bracket {bracket}")).size(style::T_BODY)
                    ]
                    .spacing(style::GAP),
                )
                .padding(style::GAP_SM)
                .style(if band == Some(bracket) {
                    style::panel_active
                } else {
                    style::table_row
                })
                .into(),
            );
        }
        rows.push(text("Each 'up to' range excludes its upper value. Missing power uses the card-based bracket. Salt and declared brackets do not enter this calculation.")
            .size(style::T_BODY).color(style::TEXT_MUTED).into());
    }
    section(
        "How the calculated bracket is determined",
        Some("Local estimate for your pregame conversation."),
        rows,
    )
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
    let mut rows = vec![row![
        text("Criterion").size(style::T_CAPTION).width(Length::Fill),
        text("Matches").size(style::T_CAPTION).width(W_COUNT),
        text("Bracket floor").size(style::T_CAPTION).width(W_FLOOR)
    ]
    .spacing(style::GAP)
    .into()];
    rows.extend(
        analysis
            .criteria
            .iter()
            .map(|c| criterion_row(c, analysis.bracket)),
    );

    section(
        "Bracket criteria",
        Some("Local estimates for a pregame conversation, not an official rating. Card counts and combo costs cannot predict how consistently a deck wins early."),
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
            text(format!("Bracket {}", criterion.floor))
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
                tags.push("low cost from hand".to_string());
            }
            if combo.lock {
                tags.push("lock".to_string());
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

            let mut details = column![
                text(combo.label()).size(style::T_LABEL).color(style::TEXT),
                text(produces)
                    .size(style::T_CAPTION)
                    .color(style::TEXT_MUTED),
                text(combo.mana_label())
                    .size(style::T_CAPTION)
                    .color(style::TEXT),
            ]
            .spacing(style::GAP_XS)
            .width(Length::Fill);
            if !tags.is_empty() {
                details = details.push(text(tags.join(" · ")).size(style::T_CAPTION).color(tone));
            }
            if !combo.setup_cost_checked {
                details = details.push(
                    text("Setup cost not established · review prerequisites before judging speed")
                        .size(style::T_CAPTION)
                        .color(style::TEXT_MUTED),
                );
            }
            if !combo.mana_needed.is_empty() {
                details = details.push(
                    text(format!("Mana requirement: {}", combo.mana_needed))
                        .size(style::T_CAPTION)
                        .color(style::TEXT_MUTED),
                );
            }
            if !combo.prerequisites.is_empty() {
                details = details.push(
                    text(format!("Prerequisites: {}", combo.prerequisites))
                        .size(style::T_CAPTION)
                        .color(style::TEXT_MUTED),
                );
            }
            container(details)
                .padding(style::GAP_SM)
                .width(Length::Fill)
                .style(style::table_row)
                .into()
        })
        .collect::<Vec<_>>();

    section(
        &format!("Combo lines ({})", analysis.combos.len()),
        Some("Starting mana assumes the combo is already set up; it excludes the cost of pieces already in play. Zero means no extra mana after setup, not free cards. Only fully specified lines starting from hand can trigger the local low-cost estimate. Other lines need a setup and speed review."),
        rows,
    )
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
                "Community scores unavailable for {}. Missing data or failed lookups count as zero, so the total may read low.",
                name_list(&analysis.unscored, 8)
            ))
            .size(style::T_CAPTION)
            .color(style::TEXT_MUTED),
        );
    }

    lines = lines.push(
        text(format!(
            "Local bracket screening uses Commander Spellbook. Local salt combines EDHREC scores and category weights. Checked {}.",
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
fn bar<'a, Msg: 'a>(frac: f64, width: Length, tone: iced::Color) -> Element<'a, Msg> {
    // FillPortion splits by ratio, so both halves have to stay non-zero; a
    // thousandth of the track is far under a pixel either way.
    let filled = (frac.clamp(0.0, 1.0) * 1000.0).round().clamp(1.0, 999.0) as u16;
    container(
        row![
            container(Space::new(Length::Fill, Length::Fill))
                .width(Length::FillPortion(filled))
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(tone.into()),
                    ..Default::default()
                }),
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
