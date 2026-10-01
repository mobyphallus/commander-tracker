//! Persisted score-display choices shared by collection and pregame views.
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{app::Message, cards::DeckMeta, db, salt::Analysis, style};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Primary {
    Power,
    Bracket,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub primary: Primary,
    pub commander_list: Option<Primary>,
    pub salt: bool,
    pub win_conditions: bool,
    pub synergy: bool,
    pub interaction: bool,
    pub prefer_report: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            primary: Primary::Bracket,
            commander_list: None,
            salt: true,
            win_conditions: false,
            synergy: false,
            interaction: false,
            prefer_report: false,
        }
    }
}

impl Preferences {
    pub fn load(conn: &Connection) -> Result<Self, String> {
        db::setting(conn, "score_preferences")
            .map_err(|e| e.to_string())?
            .map(|raw| {
                serde_json::from_str(&raw)
                    .map_err(|e| format!("Couldn't read score preferences: {e}"))
            })
            .unwrap_or_else(|| Ok(Self::default()))
    }

    pub fn save(&self, conn: &Connection) -> Result<(), String> {
        let raw = serde_json::to_string(self).map_err(|e| e.to_string())?;
        db::set_setting(conn, "score_preferences", &raw).map_err(|e| e.to_string())
    }

    pub fn scores(&self, a: &Analysis, commander_list: bool) -> DeckMeta {
        let primary = if commander_list {
            self.commander_list.unwrap_or(self.primary)
        } else {
            self.primary
        };
        let report = a
            .report
            .as_ref()
            .filter(|_| self.prefer_report || !a.has_local());
        let mut meta = if let Some(r) = report {
            DeckMeta {
                bracket: Some(r.bracket),
                power: Some(r.power),
                salt: Some(r.salt_total),
                win_conditions: Some(r.win_conditions),
                synergy: Some(r.synergy),
                interaction: Some(r.interaction),
                report: true,
            }
        } else {
            let assessment = a.assessment.as_ref();
            DeckMeta {
                bracket: Some(a.calculated_bracket()),
                power: assessment.and_then(|p| p.power),
                salt: Some(a.salt_total),
                win_conditions: assessment.map(|p| p.win_conditions.score),
                synergy: assessment.map(|p| p.synergy.score),
                interaction: assessment.map(|p| p.interaction.score),
                report: false,
            }
        };
        // Keep one source's supporting units together. A missing local power
        // may use the report's power without mixing the supporting point scales.
        if meta.power.is_none() {
            meta.power = a.report.as_ref().map(|r| r.power);
        }
        match primary {
            Primary::Power => meta.bracket = None,
            Primary::Bracket => meta.power = None,
        }
        if !self.salt {
            meta.salt = None;
        }
        if !self.win_conditions {
            meta.win_conditions = None;
        }
        if !self.synergy {
            meta.synergy = None;
        }
        if !self.interaction {
            meta.interaction = None;
        }
        meta
    }
}

pub struct State {
    pub preferences: Option<Preferences>,
    pub error: Option<String>,
}
impl State {
    pub fn load(conn: &Connection) -> Self {
        match Preferences::load(conn) {
            Ok(preferences) => Self {
                preferences: Some(preferences),
                error: None,
            },
            Err(error) => Self {
                preferences: None,
                error: Some(error),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub enum Change {
    Primary(Primary),
    CommanderList(Option<Primary>),
    Salt(bool),
    WinConditions(bool),
    Synergy(bool),
    Interaction(bool),
    PreferReport(bool),
    Retry,
}

pub fn update(state: &mut State, conn: &Connection, change: Change) {
    if matches!(change, Change::Retry) {
        *state = State::load(conn);
        return;
    }
    let Some(mut next) = state.preferences else {
        return;
    };
    match change {
        Change::Primary(v) => next.primary = v,
        Change::CommanderList(v) => next.commander_list = v,
        Change::Salt(v) => next.salt = v,
        Change::WinConditions(v) => next.win_conditions = v,
        Change::Synergy(v) => next.synergy = v,
        Change::Interaction(v) => next.interaction = v,
        Change::PreferReport(v) => next.prefer_report = v,
        Change::Retry => unreachable!(),
    }
    match next.save(conn) {
        Ok(()) => {
            state.preferences = Some(next);
            state.error = None;
        }
        Err(e) => state.error = Some(format!("Couldn't save preferences: {e}")),
    }
}

pub fn view(state: &State) -> Element<'_, Message> {
    let mut body = column![row![
        style::touch_button("Back", style::T_LABEL)
            .width(120)
            .style(style::ghost)
            .on_press(Message::GoHome),
        text("Settings · Preferences").size(style::T_HEADING)
    ]
    .spacing(style::GAP),]
    .spacing(style::GAP);
    if let Some(p) = state.preferences {
        let choice = |label: &'static str, selected: bool, change| {
            style::touch_button(label, style::T_LABEL)
                .width(210)
                .style(if selected {
                    style::primary
                } else {
                    style::secondary
                })
                .on_press(Message::Preferences(change))
        };
        body = body
            .push(text("Default score").size(style::T_SUBHEAD))
            .push(
                text("Shown on the table before the game starts.")
                    .size(style::T_BODY)
                    .color(style::TEXT_MUTED),
            )
            .push(
                row![
                    choice(
                        "Power level",
                        p.primary == Primary::Power,
                        Change::Primary(Primary::Power)
                    ),
                    choice(
                        "Bracket",
                        p.primary == Primary::Bracket,
                        Change::Primary(Primary::Bracket)
                    )
                ]
                .spacing(style::GAP)
                .wrap(),
            )
            .push(text("Commander list").size(style::T_SUBHEAD))
            .push(
                row![
                    choice(
                        "Follow default",
                        p.commander_list.is_none(),
                        Change::CommanderList(None)
                    ),
                    choice(
                        "Power level",
                        p.commander_list == Some(Primary::Power),
                        Change::CommanderList(Some(Primary::Power))
                    ),
                    choice(
                        "Bracket",
                        p.commander_list == Some(Primary::Bracket),
                        Change::CommanderList(Some(Primary::Bracket))
                    )
                ]
                .spacing(style::GAP)
                .wrap(),
            )
            .push(text("Additional scores").size(style::T_SUBHEAD))
            .push(
                text("Show these in the commander list and pregame setup.")
                    .size(style::T_BODY)
                    .color(style::TEXT_MUTED),
            );
        for (label, checked, change) in [
            ("Salt score", p.salt, Change::Salt as fn(bool) -> Change),
            ("Win conditions", p.win_conditions, Change::WinConditions),
            ("Synergy", p.synergy, Change::Synergy),
            ("Interaction", p.interaction, Change::Interaction),
        ] {
            body = body.push(
                container(
                    button(
                        row![
                            text(label).size(style::T_LABEL).width(Length::Fill),
                            text(if checked { "On" } else { "Off" }).size(style::T_LABEL),
                        ]
                        .spacing(style::GAP)
                        .align_y(iced::Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(style::TOUCH_H)
                    .padding(style::GAP)
                    .style(if checked {
                        style::primary
                    } else {
                        style::secondary
                    })
                    .on_press(Message::Preferences(change(!checked))),
                )
                .max_width(660),
            );
        }
        body = body.push(text("Preferred source").size(style::T_SUBHEAD))
            .push(row![choice("Local calculation", !p.prefer_report, Change::PreferReport(false)),
                choice("CommanderSalt", p.prefer_report, Change::PreferReport(true))].spacing(style::GAP).wrap())
            .push(text("Uses the other source when the preferred analysis is unavailable. Detailed comparisons always retain both. Changes are saved automatically.").size(style::T_BODY).color(style::TEXT_MUTED));
    } else {
        body = body.push(
            style::touch_button("Retry loading preferences", style::T_LABEL)
                .on_press(Message::Preferences(Change::Retry)),
        );
    }
    if let Some(error) = &state.error {
        body = body.push(text(error).color(style::DANGER));
    }
    container(scrollable(body))
        .padding(style::GAP)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysed() -> Analysis {
        let mut a = crate::commander_salt::decode(
            serde_json::from_str(include_str!("../tests/fixtures/magda-commandersalt.json"))
                .unwrap(),
            "bf9dad6c497fcac34ff0933f6ad5ac06",
        )
        .unwrap();
        a.local_available = true;
        a.bracket = 3;
        a.salt_total = 85.07;
        a.assessment = Some(crate::power::assess(
            &serde_json::from_str(include_str!("../tests/fixtures/magda-local-deck.json")).unwrap(),
            &serde_json::from_str::<Vec<crate::salt::ComboLine>>(include_str!(
                "../tests/fixtures/magda-local-combos.json"
            ))
            .unwrap(),
        ));
        a
    }

    #[test]
    fn collection_override_and_each_optional_metric_are_independent() {
        let a = analysed();
        for primary in [Primary::Power, Primary::Bracket] {
            for commander_list in [None, Some(Primary::Power), Some(Primary::Bracket)] {
                for enabled in 0..16 {
                    let p = Preferences {
                        primary,
                        commander_list,
                        salt: enabled & 1 != 0,
                        win_conditions: enabled & 2 != 0,
                        synergy: enabled & 4 != 0,
                        interaction: enabled & 8 != 0,
                        prefer_report: false,
                    };
                    let table = p.scores(&a, false);
                    let list = p.scores(&a, true);
                    assert_eq!(table.power.is_some(), primary == Primary::Power);
                    assert_eq!(
                        list.power.is_some(),
                        commander_list.unwrap_or(primary) == Primary::Power
                    );
                    for meta in [table, list] {
                        assert_ne!(meta.power.is_some(), meta.bracket.is_some());
                        assert_eq!(meta.salt.is_some(), p.salt);
                        assert_eq!(meta.win_conditions.is_some(), p.win_conditions);
                        assert_eq!(meta.synergy.is_some(), p.synergy);
                        assert_eq!(meta.interaction.is_some(), p.interaction);
                    }
                }
            }
        }
    }

    #[test]
    fn calculated_bracket_uses_power_and_respects_the_card_floor() {
        let mut a = analysed();
        assert_eq!(a.calculated_bracket(), 4);
        for (power, expected) in [(2., 3), (4., 3), (6., 3), (8., 4), (9., 5)] {
            a.assessment.as_mut().unwrap().power = Some(power);
            assert_eq!(a.calculated_bracket(), expected);
        }
        for (power, band) in [
            (2.99, 1),
            (3., 2),
            (4.99, 2),
            (5., 3),
            (6.99, 3),
            (7., 4),
            (8.99, 4),
            (9., 5),
        ] {
            a.assessment.as_mut().unwrap().power = Some(power);
            assert_eq!(a.power_bracket(), Some(band));
            assert_eq!(a.calculated_bracket(), 3_u8.max(band));
        }
        a.assessment.as_mut().unwrap().power = None;
        assert_eq!(a.power_bracket(), None);
        assert_eq!(a.calculated_bracket(), 3);
    }

    #[test]
    fn preferred_source_and_fallback_keep_native_supporting_units() {
        let mut a = analysed();
        let mut p = Preferences {
            synergy: true,
            ..Default::default()
        };
        assert!(!p.scores(&a, false).report);
        p.prefer_report = true;
        let remote = p.scores(&a, false);
        assert!(remote.report);
        assert_eq!(remote.synergy, Some(1438.4));
        a.report = None;
        assert!(!p.scores(&a, false).report);
        assert_eq!(
            p.scores(&a, false).synergy,
            Some(a.assessment.unwrap().synergy.score)
        );
    }

    #[test]
    fn choices_persist_and_failed_writes_keep_previous_preferences() {
        let conn = Connection::open_in_memory().unwrap();
        db::init(&conn).unwrap();
        let mut state = State::load(&conn);
        update(&mut state, &conn, Change::Primary(Primary::Power));
        update(
            &mut state,
            &conn,
            Change::CommanderList(Some(Primary::Bracket)),
        );
        update(&mut state, &conn, Change::Synergy(true));
        let saved = state.preferences.unwrap();
        assert_eq!(Preferences::load(&conn).unwrap(), saved);
        conn.pragma_update(None, "query_only", true).unwrap();
        update(&mut state, &conn, Change::Interaction(true));
        assert_eq!(state.preferences, Some(saved));
        assert!(state.error.is_some());
        assert_eq!(Preferences::load(&conn).unwrap(), saved);
    }

    #[test]
    fn loading_preferences_does_not_change_stored_deck_results() {
        let (conn, game) = crate::session::tests::fixture();
        let seat = &game.seats[0];
        let a = analysed();
        db::save_deck_analysis(&conn, seat.player.id, seat.commander.id, &a.public_id, &a).unwrap();
        let p = Preferences {
            primary: Primary::Power,
            commander_list: Some(Primary::Bracket),
            salt: false,
            synergy: true,
            ..Default::default()
        };
        p.save(&conn).unwrap();
        let links = db::deck_links(&conn, seat.player.id).unwrap();
        assert!(links[&seat.commander.id].table_scores.power.is_some());
        assert_eq!(links[&seat.commander.id].list_scores.bracket, Some(4));
        assert!(links[&seat.commander.id].list_scores.salt.is_none());
        assert_eq!(
            db::deck_breakdown(&conn, seat.player.id, seat.commander.id),
            Some(a)
        );
    }
}
