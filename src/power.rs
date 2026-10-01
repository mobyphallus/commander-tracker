//! Transparent local deck screening, independent of salt and card prices.
//! This is a versioned heuristic, not CommanderSalt's formula or a calibrated
//! prediction of win rate. See docs/local-power-scoring.md for weights/limits.
use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use crate::{
    moxfield::{Card, Deck},
    salt::ComboLine,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assessment {
    pub version: u8,
    pub power: Option<f64>,
    pub synergy: Metric,
    pub interaction: Metric,
    pub win_conditions: Metric,
    pub consistency: Metric,
    pub efficiency: Metric,
    pub average_mana_value: Option<f64>,
    pub land_count: u32,
    pub combo_families: u32,
    pub missing_mana: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    pub score: f64,
    pub explanation: String,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub label: String,
    pub cards: Vec<String>,
}

fn metric(score: f64, explanation: impl Into<String>, evidence: Vec<Evidence>) -> Metric {
    Metric {
        score: score.clamp(0., 100.),
        explanation: explanation.into(),
        evidence,
    }
}

fn has(text: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|p| text.contains(p))
}

fn is_land(card: &Card) -> bool {
    // Spell/land MDFCs stay in the spell curve. Their optional land faces do
    // not count as guaranteed mana sources in this conservative screen.
    card.type_line
        .split(" // ")
        .next()
        .unwrap_or_default()
        .contains("Land")
}

#[derive(Default)]
struct Roles {
    answer: Option<(&'static str, f64)>,
    ramp: bool,
    draw: bool,
    tutor: bool,
    recursion: bool,
    finisher: bool,
    alternate_win: bool,
}

struct Facts<'a> {
    card: &'a Card,
    text: String,
    types: String,
    roles: Roles,
}

impl<'a> Facts<'a> {
    fn new(card: &'a Card) -> Self {
        let text = card.oracle_text.to_lowercase().replace('’', "'");
        let types = card.type_line.to_lowercase();
        let mut roles = Roles::default();
        {
            // Highest applicable role wins: a removal spell with overload is
            // one interaction card, not one point for every matching keyword.
            roles.answer = if crate::salt::is_board_wipe(&text) {
                Some(("Board wipes", 6.))
            } else if text.contains("counter target") && has(&text, &["spell", "ability"]) {
                Some(("Counterspells / abilities", 6.))
            } else if has(
                &text,
                &[
                    "opponents can't cast",
                    "players can't cast",
                    "opponents can't search",
                    "players can't search",
                    "opponents can't draw",
                    "lands don't untap",
                    "cost {1} more to cast",
                    "cost {2} more to cast",
                ],
            ) {
                Some(("Restrictions and taxes", 5.))
            } else if (has(
                &text,
                &[
                    "destroy target",
                    "exile target",
                    "destroy up to one target",
                    "exile up to one target",
                ],
            ) || (text.contains("return target") && text.contains("owner's hand")))
                && has(
                    &text,
                    &[
                        "creature",
                        "artifact",
                        "enchantment",
                        "permanent",
                        "planeswalker",
                        "land",
                    ],
                )
                && !has(
                    &text,
                    &["from your graveyard", "from a graveyard", "from graveyards"],
                )
            {
                Some(("Targeted removal", 4.))
            } else if text.contains("owner of target permanent shuffles") {
                Some(("Targeted removal", 4.))
            } else if text.contains("gain control of")
                && has(&text, &["target", "all artifacts", "all creatures"])
            {
                Some(("Theft / control", 4.))
            } else if has(&text, &["goad target", "goad each", "goad all"]) {
                Some(("Combat control", 3.))
            } else if text.contains("deals")
                && has(
                    &text,
                    &[
                        "damage to any target",
                        "damage to target creature",
                        "damage to target permanent",
                        "damage to up to one target creature",
                    ],
                )
            {
                Some(("Targeted damage", 4.))
            } else if text.contains("exile")
                && has(
                    &text,
                    &[
                        "from a graveyard",
                        "from graveyards",
                        "from a single graveyard",
                        "all graveyards",
                        "target player's graveyard",
                    ],
                )
            {
                Some(("Graveyard answers", 3.))
            } else if has(
                &text,
                &["hexproof", "indestructible", "shroud", "protection from"],
            ) && has(
                &text,
                &["you control", "target creature", "equipped creature"],
            ) {
                Some(("Protection", 3.))
            } else {
                None
            };
        }
        if !is_land(card) {
            roles.ramp = has(
                &text,
                &[
                    "add {",
                    "add one mana",
                    "add two mana",
                    "add three mana",
                    "add that much",
                    "add an amount of",
                    "add mana",
                ],
            ) || (text.contains("create") && text.contains("treasure"))
                || (text.contains("search your library")
                    && text.contains("land")
                    && text.contains("battlefield"))
                || text.contains("additional land")
                || (has(
                    &text,
                    &["spells you cast cost", "creature spells you cast cost"],
                ) && text.contains("less to cast"));
            roles.draw = has(
                &text,
                &[
                    "draw a card",
                    "draw two cards",
                    "draw three cards",
                    "draw four cards",
                    "draw x cards",
                    "draw that many cards",
                    "draw cards",
                ],
            ) || (text.contains("exile")
                && has(&text, &["you may play", "you may cast"])
                && text.contains("your library"));
            roles.tutor = text.contains("search your library")
                && !has(
                    &text,
                    &[
                        "basic land",
                        "land card",
                        "a plains",
                        "an island",
                        "a swamp",
                        "a mountain",
                        "a forest",
                    ],
                );
            roles.recursion = text.contains("graveyard")
                && text.contains("return")
                && has(&text, &["to your hand", "to the battlefield"]);
            roles.alternate_win = text.contains("you win the game");
            roles.finisher = has(
                &text,
                &[
                    "creatures you control get +",
                    "additional combat phase",
                    "double the damage",
                    "triple the damage",
                ],
            ) || (text.contains("each opponent")
                && has(&text, &["deals", "loses"]))
                || (types.contains("creature")
                    && card.mana_value.is_some_and(|v| v >= 5.)
                    && has(&text, &["flying", "trample", "double strike"]));
        }
        Self {
            card,
            text,
            types,
            roles,
        }
    }
}

fn evidence(label: &str, cards: impl Iterator<Item = String>) -> Evidence {
    Evidence {
        label: label.into(),
        cards: cards.collect::<BTreeSet<_>>().into_iter().collect(),
    }
}
fn card_label(c: &Card) -> String {
    if c.quantity > 1 {
        format!("{}× {}", c.quantity, c.name)
    } else {
        c.name.clone()
    }
}
fn role_evidence(
    facts: &[Facts<'_>],
    label: &str,
    predicate: impl Fn(&Facts<'_>) -> bool,
) -> Evidence {
    evidence(
        label,
        facts
            .iter()
            .filter(|c| predicate(c))
            .map(|c| card_label(c.card)),
    )
}
fn quantity(facts: &[Facts<'_>], predicate: impl Fn(&Facts<'_>) -> bool) -> u32 {
    facts
        .iter()
        .filter(|c| predicate(c))
        .map(|c| c.card.quantity)
        .sum()
}

/// Count coverage of actual supporter/payoff pairs, not keyword mentions.
/// Lands are excluded from the denominator so 35 basics cannot inflate synergy.
fn synergy(facts: &[Facts<'_>], combos: &[ComboLine]) -> Metric {
    let spells: Vec<_> = facts.iter().filter(|f| !is_land(f.card)).collect();
    let mut linked = HashSet::new();
    let mut groups = Vec::new();
    let mut theme =
        |name: &str, supports: &dyn Fn(&Facts<'_>) -> bool, payoff: &dyn Fn(&Facts<'_>) -> bool| {
            let mut members = BTreeSet::new();
            for a in &spells {
                if !payoff(a) {
                    continue;
                }
                for b in &spells {
                    if a.card.name != b.card.name && supports(b) {
                        members.insert(a.card.name.clone());
                        members.insert(b.card.name.clone());
                    }
                }
            }
            if !members.is_empty() {
                linked.extend(members.iter().cloned());
                groups.push(Evidence {
                    label: name.into(),
                    cards: members.into_iter().collect(),
                });
            }
        };
    theme(
        "Artifacts and artifact tokens",
        &|c| {
            c.types.contains("artifact")
                || (c.text.contains("create")
                    && has(
                        &c.text,
                        &["treasure", "clue", "food", "blood token", "artifact token"],
                    ))
        },
        &|c| {
            has(
                &c.text,
                &[
                    "artifacts you control",
                    "artifact you control",
                    "artifact enters",
                    "artifact spells",
                    "sacrifice an artifact",
                    "untap target artifact",
                    "tap two untapped artifacts",
                ],
            )
        },
    );
    theme(
        "Treasure engines",
        &|c| c.text.contains("create") && c.text.contains("treasure"),
        &|c| {
            has(
                &c.text,
                &[
                    "sacrifice five treasures",
                    "sacrifice a treasure",
                    "number of treasures",
                    "for each treasure",
                    "treasures you control",
                ],
            )
        },
    );
    theme(
        "Tokens and anthems",
        &|c| c.text.contains("create") && c.text.contains("token"),
        &|c| {
            has(
                &c.text,
                &[
                    "tokens you control",
                    "one or more tokens would be created",
                    "creature tokens",
                    "creatures you control get +",
                    "populate",
                ],
            ) && !(c.text.contains("create")
                && !has(&c.text, &["you control", "would be created", "populate"]))
        },
    );
    theme(
        "Spell casting",
        &|c| has(&c.types, &["instant", "sorcery"]),
        &|c| {
            has(
                &c.text,
                &[
                    "whenever you cast an instant",
                    "whenever you cast a sorcery",
                    "whenever you cast a noncreature",
                    "magecraft",
                    "prowess",
                ],
            )
        },
    );
    theme(
        "Sacrifice and death triggers",
        &|c| {
            c.text.contains("sacrifice")
                && has(&c.text, &[":", "additional cost"])
                && !c.text.contains("each opponent sacrifices")
        },
        &|c| c.text.contains("whenever") && has(&c.text, &["dies", "you sacrifice"]),
    );
    theme(
        "Graveyard engines",
        &|c| {
            has(
                &c.text,
                &[
                    "mill",
                    "discard a card",
                    "sacrifice a creature",
                    "sacrifice another creature",
                ],
            )
        },
        &|c| c.roles.recursion || has(&c.text, &["flashback", "escape—", "escape —", "delve"]),
    );
    theme(
        "Life gain",
        &|c| {
            (c.text.contains("gain") && c.text.contains("life") && !c.text.contains("can't gain"))
                || c.text.contains("lifelink")
        },
        &|c| {
            has(
                &c.text,
                &[
                    "whenever you gain life",
                    "if you gained life",
                    "life you gained",
                ],
            )
        },
    );
    theme(
        "Counters",
        &|c| c.text.contains("+1/+1 counter") || c.text.contains("proliferate"),
        &|c| {
            has(
                &c.text,
                &[
                    "with a +1/+1 counter",
                    "for each counter",
                    "modified creatures",
                    "one or more +1/+1 counters would be put",
                ],
            )
        },
    );
    theme(
        "Landfall and extra lands",
        &|c| c.roles.ramp && has(&c.text, &["land", "lands"]),
        &|c| {
            has(
                &c.text,
                &["landfall", "whenever a land", "whenever you play a land"],
            )
        },
    );
    for (singular, plural) in [
        ("dwarf", "dwarves"),
        ("elf", "elves"),
        ("dragon", "dragons"),
        ("goblin", "goblins"),
        ("zombie", "zombies"),
        ("vampire", "vampires"),
        ("human", "humans"),
        ("sliver", "slivers"),
        ("wizard", "wizards"),
        ("merfolk", "merfolk"),
        ("soldier", "soldiers"),
        ("dinosaur", "dinosaurs"),
        ("angel", "angels"),
        ("spirit", "spirits"),
        ("rat", "rats"),
        ("knight", "knights"),
        ("faerie", "faeries"),
        ("cat", "cats"),
    ] {
        theme(
            &format!("{plural} support"),
            &|c| {
                c.types
                    .split(|ch: char| !ch.is_alphabetic())
                    .any(|w| w == singular)
                    || c.text.contains("changeling")
                    || (c.text.contains("create")
                        && c.text.contains("token")
                        && c.text.split_whitespace().any(|w| w == singular))
            },
            &|c| {
                c.text.contains(&format!("{plural} you control"))
                    || c.text.contains(&format!("{singular} you control"))
                    || c.text.contains(&format!("{singular} enters"))
                    || c.text.contains(&format!("{singular} spells"))
            },
        );
    }
    let names: HashSet<_> = spells.iter().map(|c| c.card.name.as_str()).collect();
    let combo_cards: BTreeSet<_> = combos
        .iter()
        .flat_map(|c| c.cards.iter())
        .filter(|n| names.contains(n.as_str()))
        .cloned()
        .collect();
    if combo_cards.len() >= 2 {
        linked.extend(combo_cards.iter().cloned());
        groups.push(Evidence {
            label: "Known combo pieces".into(),
            cards: combo_cards.into_iter().collect(),
        });
    }
    let total: u32 = spells.iter().map(|c| c.card.quantity).sum();
    let supported: u32 = spells
        .iter()
        .filter(|c| linked.contains(&c.card.name))
        .map(|c| c.card.quantity)
        .sum();
    metric(if total > 0 { 100. * supported as f64 / total as f64 } else { 0. },
        format!("{supported} of {total} nonland cards connect to another card through a detected engine or known combo. Each card counts once."), groups)
}

/// Conservative families: variants sharing two or more pieces are connected.
/// This is not a claim of independent deterministic wins or of assembly speed.
fn combo_families(combos: &[ComboLine]) -> Vec<Vec<&ComboLine>> {
    let mut groups: Vec<Vec<&ComboLine>> = Vec::new();
    for combo in combos.iter().filter(|c| c.cards.len() >= 2) {
        let names: HashSet<_> = combo.cards.iter().collect();
        let mut new_group = vec![combo];
        let mut i = 0;
        while i < groups.len() {
            if groups[i].iter().any(|other| {
                other
                    .cards
                    .iter()
                    .collect::<HashSet<_>>()
                    .intersection(&names)
                    .count()
                    >= 2
            }) {
                new_group.extend(groups.remove(i));
            } else {
                i += 1;
            }
        }
        groups.push(new_group);
    }
    groups
}

pub fn assess(deck: &Deck, combos: &[ComboLine]) -> Assessment {
    let facts: Vec<_> = deck.all_cards().map(Facts::new).collect();
    let synergy = synergy(&facts, combos);
    let mut answers = std::collections::BTreeMap::<&str, Vec<String>>::new();
    let mut interaction_points = 0.;
    for fact in &facts {
        if let Some((label, base)) = fact.roles.answer {
            let points = match fact.card.mana_value {
                Some(v) if v <= 2. && !is_land(fact.card) => base + 1.,
                Some(v) if v >= 5. => base * 0.75,
                _ => base,
            };
            interaction_points += points * fact.card.quantity as f64;
            answers
                .entry(label)
                .or_default()
                .push(card_label(fact.card));
        }
    }
    let interaction = metric(interaction_points,
        "Answers score by role and mana cost: wipes/counters 6, restrictions 5, removal/control 4, protection/graveyard answers 3; +1 for nonlands at mana value ≤2, ×0.75 at ≥5. Utility-land answers count without the cheap-spell bonus. Each card uses its strongest role.",
        answers.into_iter().map(|(label, cards)| evidence(label, cards.into_iter())).collect());
    let ramp = quantity(&facts, |c| c.roles.ramp) as f64;
    let draw = quantity(&facts, |c| c.roles.draw) as f64;
    let tutors = quantity(&facts, |c| c.roles.tutor) as f64;
    let recursion = quantity(&facts, |c| c.roles.recursion) as f64;
    let consistency = metric(draw.min(12.) * 4. + tutors.min(4.) * 10. + recursion.min(5.) * 3. + synergy.score * 0.2,
        "Draw/access cards ×4 (up to 12), nonland tutors ×10 (up to 4), recursion ×3 (up to 5), plus 20% of synergy; capped at 100.",
        vec![role_evidence(&facts, "Draw and card access", |c| c.roles.draw),
             role_evidence(&facts, "Nonland tutors", |c| c.roles.tutor),
             role_evidence(&facts, "Recursion", |c| c.roles.recursion)]);
    let groups = combo_families(combos);
    let mut combo_score: f64 = 0.;
    let mut win_evidence = Vec::new();
    for (i, group) in groups.iter().enumerate() {
        let wins = group.iter().any(|line| {
            line.produces.iter().any(|outcome| {
                has(
                    &outcome.to_lowercase(),
                    &[
                        "win the game",
                        "loses the game",
                        "infinite damage",
                        "infinite loss of life",
                        "infinite combat",
                        "infinite turns",
                        "infinite mill",
                    ],
                )
            })
        });
        let locks = group.iter().any(|c| c.lock);
        let mut conditional_payoffs = BTreeSet::new();
        for line in group {
            let treasure_engine = line.produces.iter().any(|p| {
                let p = p.to_lowercase();
                p.contains("infinite") && p.contains("treasure")
            });
            if treasure_engine {
                for fact in &facts {
                    // A specific resource/output match, not an assumption that
                    // any infinite engine wins. Attack/upkeep timing still applies.
                    if (fact
                        .text
                        .contains("damage equal to the number of treasures you control")
                        && fact.text.contains("any target"))
                        || (fact.roles.alternate_win
                            && fact.text.contains("twenty or more artifacts"))
                    {
                        conditional_payoffs.insert(fact.card.name.clone());
                    }
                }
            }
        }
        combo_score = combo_score.max(if wins {
            65.
        } else if !conditional_payoffs.is_empty() {
            55.
        } else if locks {
            45.
        } else {
            35.
        });
        win_evidence.push(evidence(
            &format!(
                "Combo family {} · {}",
                i + 1,
                if wins {
                    "winning outcome"
                } else if !conditional_payoffs.is_empty() {
                    "resource engine with conditional payoff in this deck"
                } else if locks {
                    "lock"
                } else {
                    "resource engine; needs an outlet"
                }
            ),
            group.iter().map(|line| line.label()),
        ));
        if !conditional_payoffs.is_empty() {
            win_evidence.push(Evidence {
                label: "Treasure-engine payoffs (must assemble; attack/upkeep timing applies)"
                    .into(),
                cards: conditional_payoffs.into_iter().collect(),
            });
        }
    }
    if !groups.is_empty() {
        combo_score += (groups.len().saturating_sub(1).min(2) as f64) * 10.;
    }
    let alternate = quantity(&facts, |c| c.roles.alternate_win);
    let finishers = quantity(&facts, |c| c.roles.finisher);
    let creatures = quantity(&facts, |c| !is_land(c.card) && c.types.contains("creature"));
    let combat: f64 = if creatures >= 15 {
        20. + (finishers.min(6) as f64) * 5.
    } else {
        (finishers.min(6) as f64) * 5.
    };
    let alt_score: f64 = if alternate > 0 {
        30. + alternate.saturating_sub(1).min(2) as f64 * 5.
    } else {
        0.
    };
    let routes = [combo_score, combat, alt_score]
        .iter()
        .filter(|&&v| v > 0.)
        .count();
    let win_points =
        combo_score.max(combat).max(alt_score) + routes.saturating_sub(1).min(2) as f64 * 5.;
    win_evidence.push(role_evidence(
        &facts,
        "Alternate-win text (conditions still apply)",
        |c| c.roles.alternate_win,
    ));
    win_evidence.push(role_evidence(
        &facts,
        "Combat finishers / damage payoffs",
        |c| c.roles.finisher,
    ));
    let win_conditions = metric(win_points,
        format!("{} combo families, {alternate} alternate-win cards, {finishers} potential finishers and {creatures} creatures. Strongest plan sets the base; extra families and plans add limited redundancy. Setup and outlets still matter.", groups.len()), win_evidence);
    let land_count = quantity(&facts, |c| is_land(c.card));
    let mut sum = 0.;
    let mut known = 0;
    let mut missing_mana = Vec::new();
    for fact in facts.iter().filter(|c| !is_land(c.card)) {
        if let Some(mv) = fact.card.mana_value.filter(|v| v.is_finite() && *v >= 0.) {
            sum += mv * fact.card.quantity as f64;
            known += fact.card.quantity;
        } else {
            missing_mana.push(fact.card.name.clone());
        }
    }
    let average_mana_value = (known > 0).then(|| sum / known as f64);
    let cheap_ramp = quantity(&facts, |c| {
        c.roles.ramp && c.card.mana_value.is_some_and(|mv| mv <= 2.)
    });
    let land_points = if (30..=40).contains(&land_count) {
        10.
    } else {
        (10. - (land_count as f64 - 35.).abs()).max(0.)
    };
    let curve_points = average_mana_value
        .map(|avg| ((5. - avg) * 15.).clamp(0., 45.))
        .unwrap_or(0.);
    let efficiency = metric(curve_points + (ramp * 3.).min(30.) + (cheap_ramp as f64 * 5.).min(15.) + land_points,
        format!("Curve contributes up to 45 points, ramp up to 30, low-cost ramp up to 15 and land supply up to 10. {land_count} front-face lands; spell/land cards stay in the spell curve. Fixing and actual speed are not simulated."),
        vec![role_evidence(&facts, "Ramp / cost reduction", |c| c.roles.ramp),
             role_evidence(&facts, "Ramp at mana value ≤2", |c| c.roles.ramp && c.card.mana_value.is_some_and(|mv| mv <= 2.))]);
    // Missing mana values should not make an old cache look like a weak deck.
    // Small/partial lists can show their role breakdown without a power claim.
    let power = (missing_mana.is_empty() && known > 0 && deck.card_count() == 100).then(|| {
        let weighted = win_conditions.score * 0.25
            + consistency.score * 0.20
            + interaction.score * 0.20
            + efficiency.score * 0.20
            + synergy.score * 0.15;
        (1. + 9. * weighted / 100.).clamp(1., 10.)
    });
    Assessment {
        version: 2,
        power,
        synergy,
        interaction,
        win_conditions,
        consistency,
        efficiency,
        average_mana_value,
        land_count,
        combo_families: groups.len() as u32,
        missing_mana,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Deck, Vec<ComboLine>) {
        (
            serde_json::from_str(include_str!("../tests/fixtures/magda-local-deck.json")).unwrap(),
            serde_json::from_str(include_str!("../tests/fixtures/magda-local-combos.json"))
                .unwrap(),
        )
    }
    fn vanilla() -> Deck {
        let card = |name: &str, quantity, types: &str, mv| Card {
            name: name.into(),
            quantity,
            type_line: types.into(),
            layout: String::new(),
            oracle_text: String::new(),
            scryfall_id: String::new(),
            color_identity: String::new(),
            usd: None,
            reserved: false,
            mana_value: Some(mv),
        };
        Deck {
            public_id: "local-test".into(),
            name: "Vanilla".into(),
            url: String::new(),
            owner_bracket: None,
            auto_bracket: None,
            commanders: vec![card("Leader", 1, "Legendary Creature — Bear", 2.)],
            mainboard: vec![
                card("Forest", 35, "Basic Land — Forest", 0.),
                card("Bears", 64, "Creature — Bear", 2.),
            ],
        }
    }
    #[test]
    fn magda_has_supported_engines_answers_and_distinct_combo_families() {
        let (deck, combos) = fixture();
        let a = assess(&deck, &combos);
        assert!(a.power.is_some());
        assert_eq!(a.combo_families, 2);
        assert!(a
            .synergy
            .evidence
            .iter()
            .any(|g| g.label == "dwarves support"
                && g.cards.contains(&"Magda, Brazen Outlaw".into())));
        let wipes = a
            .interaction
            .evidence
            .iter()
            .find(|g| g.label == "Board wipes")
            .unwrap();
        for name in ["Bloodfire Dwarf", "Fast // Furious", "Vandalblast"] {
            assert!(wipes.cards.contains(&name.into()), "{name}");
        }
        assert!(a
            .win_conditions
            .evidence
            .iter()
            .any(|g| g.cards.contains(&"Hellkite Tyrant".into())));
        assert!(a.synergy.score > assess(&vanilla(), &[]).synergy.score);
        assert!(a.power.unwrap() > assess(&vanilla(), &[]).power.unwrap());
        println!("Magda local: power {:.1}, synergy {:.0}, interaction {:.0}, win conditions {:.0}, consistency {:.0}, efficiency {:.0}",
            a.power.unwrap(), a.synergy.score, a.interaction.score, a.win_conditions.score, a.consistency.score, a.efficiency.score);
    }
    #[test]
    fn interaction_covers_utility_lands_flexible_removal_and_control() {
        let (deck, _) = fixture();
        for name in [
            "Barbarian Ring",
            "Chaos Warp",
            "Summon: Bahamut",
            "Season of the Bold",
            "Hellkite Tyrant",
            "Unlicensed Hearse",
            "Glóin, Dwarf Emissary",
        ] {
            let card = deck.all_cards().find(|c| c.name == name).unwrap();
            assert!(Facts::new(card).roles.answer.is_some(), "{name}");
        }
        let mut card = deck.commanders[0].clone();
        for text in [
            "Exile target card from your graveyard. You may cast it this turn.",
            "Return target creature card from your graveyard to your hand.",
            "This creature has hexproof.",
        ] {
            card.oracle_text = text.into();
            assert!(Facts::new(&card).roles.answer.is_none(), "{text}");
        }
    }

    #[test]
    fn treasure_engine_needs_an_actual_matching_payoff() {
        let (mut deck, combos) = fixture();
        let with_payoffs = assess(&deck, &combos);
        assert!(with_payoffs
            .win_conditions
            .evidence
            .iter()
            .any(|g| g.label.starts_with("Treasure-engine payoffs")));
        for card in &mut deck.mainboard {
            if ["Smaug the Magnificent", "Hellkite Tyrant"].contains(&card.name.as_str()) {
                card.oracle_text.clear();
            }
        }
        let without = assess(&deck, &combos);
        assert!(!without
            .win_conditions
            .evidence
            .iter()
            .any(|g| g.label.starts_with("Treasure-engine payoffs")));
        assert!(with_payoffs.win_conditions.score > without.win_conditions.score);
    }

    #[test]
    fn duplicate_combo_variants_do_not_inflate_power() {
        let (deck, mut combos) = fixture();
        let original = assess(&deck, &combos);
        combos.extend(combos.clone());
        combos.reverse();
        let repeated = assess(&deck, &combos);
        assert_eq!(original.combo_families, repeated.combo_families);
        assert_eq!(original.win_conditions.score, repeated.win_conditions.score);
        assert_eq!(original.power, repeated.power);
    }
    #[test]
    fn prices_and_owner_bracket_do_not_influence_power() {
        let (mut deck, combos) = fixture();
        let expected = assess(&deck, &combos);
        deck.owner_bracket = Some(5);
        deck.auto_bracket = Some(5);
        for c in deck.commanders.iter_mut().chain(deck.mainboard.iter_mut()) {
            c.usd = Some(9999.);
            c.reserved = true;
        }
        assert_eq!(assess(&deck, &combos), expected);
    }
    #[test]
    fn incomplete_or_old_lists_do_not_get_a_misleading_power_number() {
        let mut deck = vanilla();
        deck.commanders[0].mana_value = None;
        let a = assess(&deck, &[]);
        assert!(a.power.is_none());
        assert_eq!(a.missing_mana, ["Leader"]);
        deck.commanders[0].mana_value = Some(2.);
        deck.mainboard[1].quantity = 1;
        assert!(assess(&deck, &[]).power.is_none());
    }
    #[test]
    fn interaction_additions_help_without_counting_counter_markers() {
        let mut deck = vanilla();
        let before = assess(&deck, &[]);
        deck.mainboard[1].quantity -= 1;
        let mut answer = deck.commanders[0].clone();
        answer.name = "Counterspell".into();
        answer.type_line = "Instant".into();
        answer.oracle_text = "Counter target spell.".into();
        deck.mainboard.push(answer);
        let after = assess(&deck, &[]);
        assert!(after.interaction.score > before.interaction.score);
        assert!(after.power.unwrap() > before.power.unwrap());
        deck.mainboard.last_mut().unwrap().oracle_text =
            "Put a +1/+1 counter on target creature.".into();
        assert_eq!(assess(&deck, &[]).interaction.score, 0.);
    }
    #[test]
    fn artifacts_are_not_synergistic_just_because_removal_mentions_them() {
        let mut deck = vanilla();
        deck.mainboard[1].type_line = "Artifact Creature — Bear".into();
        deck.commanders[0].oracle_text = "Destroy target artifact.".into();
        assert_eq!(assess(&deck, &[]).synergy.score, 0.);
    }
    #[test]
    fn local_metrics_round_trip_for_offline_use() {
        let (deck, combos) = fixture();
        let a = assess(&deck, &combos);
        let b: Assessment = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(a, b);
        for score in [
            a.synergy.score,
            a.interaction.score,
            a.win_conditions.score,
            a.consistency.score,
            a.efficiency.score,
        ] {
            assert!((0. ..=100.).contains(&score));
        }
    }
}
