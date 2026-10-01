//! Import an existing public CommanderSalt report. Scores retain their source
//! and snapshot date; fetching a report does not submit or reanalyse a deck.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub url: String,
    pub scored_at: String,
    #[serde(default)]
    pub saved_at: String,
    #[serde(default)]
    pub salt_total: f64,
    #[serde(default)]
    pub bracket: u8,
    #[serde(default)]
    pub card_count: u32,
    #[serde(default)]
    pub consistency: Option<f64>,
    #[serde(default)]
    pub efficiency: Option<f64>,
    #[serde(default)]
    pub synergy_coverage: Option<f64>,
    #[serde(default)]
    pub interaction_cards: Option<Vec<String>>,
    pub power: f64,
    pub synergy: f64,
    pub interaction: f64,
    pub win_conditions: f64,
    pub combo_count: u32,
    pub independent_lines: u32,
    pub categories: Vec<Contribution>,
    pub rationale: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contribution {
    pub label: String,
    pub score: f64,
    pub cards: Vec<crate::salt::CardScore>,
}

pub fn parse_ref(input: &str) -> Option<String> {
    let url = url::Url::parse(input.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !matches!(
            url.host_str(),
            Some("commandersalt.com" | "www.commandersalt.com")
        )
    {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    let id = path.strip_prefix("/details/deck/")?;
    (id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit())).then(|| id.to_owned())
}

fn number(value: &Value, path: &str) -> Result<f64, String> {
    value
        .pointer(path)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.)
        .ok_or_else(|| format!("CommanderSalt report is missing a valid {path}."))
}

fn count(value: &Value, path: &str) -> Result<u32, String> {
    value
        .pointer(path)
        .and_then(Value::as_u64)
        .and_then(|n| n.try_into().ok())
        .ok_or_else(|| format!("CommanderSalt report is missing a valid {path}."))
}

pub(crate) fn decode(value: Value, id: &str) -> Result<crate::salt::Analysis, String> {
    if value["id"].as_str() != Some(id) || value["isPrivate"].as_bool() == Some(true) {
        return Err("CommanderSalt returned a different or private report.".into());
    }
    let source = value["url"]
        .as_str()
        .ok_or("Report has no source deck URL.")?;
    let source_url = url::Url::parse(source).map_err(|_| "Invalid source deck URL.")?;
    if !matches!(
        source_url.host_str(),
        Some("moxfield.com" | "www.moxfield.com")
    ) {
        return Err("This report must describe a Moxfield deck.".into());
    }
    let public_id = crate::moxfield::parse_ref(source).ok_or("Invalid Moxfield deck URL.")?;
    let bracket = count(&value, "/details/brackets/displayBracket")?;
    if !(1..=5).contains(&bracket) {
        return Err("Invalid reported bracket.".into());
    }
    let power = number(&value, "/powerLevelRating")?;
    if power > 11. {
        return Err("Invalid reported power level.".into());
    }
    let stamp = value
        .pointer("/ingestDate/ingestDate")
        .and_then(Value::as_i64)
        .and_then(chrono::DateTime::from_timestamp_millis)
        .ok_or("Report has no valid analysis date.")?
        .to_rfc3339();
    let scoring = value
        .pointer("/details/salt/scoring")
        .and_then(Value::as_object)
        .ok_or("Report has no salt breakdown.")?;
    let mut categories = Vec::new();
    for (key, category) in scoring {
        let score = number(category, "/score")?;
        if score == 0. {
            continue;
        }
        let mut cards = Vec::new();
        if let Some(list) = category["list"].as_object() {
            for (card_id, entry) in list {
                let name = entry["name"]
                    .as_str()
                    .or_else(|| value["cards"][card_id]["name"].as_str())
                    .unwrap_or(card_id);
                cards.push(crate::salt::CardScore {
                    name: name.into(),
                    quantity: None,
                    score: number(entry, "/score")?,
                });
            }
        }
        cards.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.name.cmp(&b.name))
        });
        categories.push(Contribution {
            label: category["label"].as_str().unwrap_or(key).into(),
            score,
            cards,
        });
    }
    categories.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.label.cmp(&b.label))
    });
    let salt_total = number(&value, "/saltRating")?;
    if (categories.iter().map(|c| c.score).sum::<f64>() - salt_total).abs() > 0.05 {
        return Err("CommanderSalt's salt total does not match its breakdown.".into());
    }
    let rationale = value
        .pointer("/details/brackets/profile/rationale")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| r["why"].as_str().map(str::to_owned))
        .collect();
    let report = Report {
        url: format!("https://www.commandersalt.com/details/deck/{id}"),
        scored_at: stamp,
        saved_at: chrono::Local::now().to_rfc3339(),
        salt_total,
        bracket: bracket as u8,
        card_count: count(&value, "/_cardCount")?,
        consistency: number(&value, "/details/powerLevel/scoring/consistency/score").ok(),
        efficiency: number(&value, "/details/powerLevel/scoring/efficiency/score").ok(),
        synergy_coverage: number(&value, "/details/synergy/profile/coverage")
            .ok()
            .filter(|v| *v <= 1.)
            .map(|v| v * 100.),
        interaction_cards: interaction_cards(&value),
        power,
        synergy: number(&value, "/synergyRating")?,
        interaction: number(&value, "/details/powerLevel/scoring/interaction/score")?,
        win_conditions: number(
            &value,
            "/details/powerLevel/spikeScorePieces/winConditions/total",
        )?,
        combo_count: count(&value, "/details/combos/profile/count")?,
        independent_lines: count(&value, "/details/combos/profile/effectiveWinconLines")?,
        categories,
        rationale,
    };
    Ok(crate::salt::Analysis {
        local_available: false,
        assessment: None,
        scoring_version: crate::salt::SCORING_VERSION,
        deck_name: value["name"]
            .as_str()
            .ok_or("Report has no deck name.")?
            .into(),
        public_id: public_id.clone(),
        url: format!("https://moxfield.com/decks/{public_id}"),
        card_count: count(&value, "/_cardCount")?,
        bracket: bracket as u8,
        criteria: Vec::new(),
        owner_bracket: None,
        auto_bracket: None,
        salt_total,
        categories: Vec::new(),
        combos: Vec::new(),
        unscored: Vec::new(),
        analysed_at: chrono::Local::now().to_rfc3339(),
        report: Some(report),
    })
}

fn interaction_cards(value: &Value) -> Option<Vec<String>> {
    let scoring = value.pointer("/details/powerLevel/scoring")?;
    let roles = scoring.pointer("/interaction/subCategories")?.as_array()?;
    let mut names = std::collections::BTreeSet::new();
    for role in roles {
        let cards = scoring.get(role.as_str()?)?.get("list")?.as_object()?;
        for id in cards.keys() {
            let card = value.get("cards")?.get(id)?;
            names.insert(card.get("name")?.as_str()?.to_owned());
        }
    }
    Some(names.into_iter().collect())
}

pub async fn fetch(id: String) -> Result<crate::salt::Analysis, String> {
    let client = reqwest::Client::builder()
        .user_agent("commander_pod/0.1 (local Commander pod tracker)")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get("https://api.commandersalt.com/decks")
        .query(&[("id", &id)])
        .send()
        .await
        .map_err(|e| format!("Couldn't load CommanderSalt report: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Couldn't load CommanderSalt report: {e}"))?;
    let value = response
        .json()
        .await
        .map_err(|e| format!("Couldn't read CommanderSalt report: {e}"))?;
    decode(value, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "bf9dad6c497fcac34ff0933f6ad5ac06";
    fn fixture() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/magda-commandersalt.json")).unwrap()
    }
    #[tokio::test]
    #[ignore = "reads a public CommanderSalt report over the network"]
    async fn live_public_report_import() {
        let a = fetch(ID.into()).await.unwrap();
        assert_eq!(a.public_id, "mCMqeU3Ydk2PeTLx47ijkw");
        assert_eq!(a.card_count, 100);
        assert!(a.report.as_ref().unwrap().power > 0.);
        println!(
            "Imported {}: salt {:.2}, power {:.1}, bracket {}",
            a.deck_name,
            a.salt_total,
            a.report.unwrap().power,
            a.bracket
        );
    }

    #[test]
    fn legacy_import_migrates_report_totals_without_inventing_local_results() {
        let original = decode(fixture(), ID).unwrap();
        let mut json = serde_json::to_value(&original).unwrap();
        for key in [
            "salt_total",
            "bracket",
            "card_count",
            "saved_at",
            "consistency",
            "efficiency",
            "synergy_coverage",
        ] {
            json["report"].as_object_mut().unwrap().remove(key);
        }
        json.as_object_mut().unwrap().remove("local_available");
        let restored = serde_json::from_value::<crate::salt::Analysis>(json)
            .unwrap()
            .review_saved();
        assert!(!restored.has_local());
        let report = restored.report.unwrap();
        assert_eq!(report.salt_total, original.salt_total);
        assert_eq!(report.bracket, original.bracket);
        assert_eq!(report.saved_at, original.analysed_at);
        assert!(report.consistency.is_none());
    }

    #[test]
    fn imports_actual_magda_scores_and_survives_offline_roundtrip() {
        let a = decode(fixture(), ID).unwrap();
        assert_eq!(a.public_id, "mCMqeU3Ydk2PeTLx47ijkw");
        assert_eq!(a.bracket, 4);
        assert!((a.salt_total - 85.52179401780548).abs() < 1e-9);
        let r = a.report.as_ref().unwrap();
        assert!((r.power - 8.58773741828711).abs() < 1e-9);
        assert_eq!(r.interaction, 105.);
        assert_eq!(r.consistency, Some(170.));
        assert_eq!(r.efficiency, Some(335.));
        assert_eq!(r.synergy_coverage, Some(85.3));
        assert!(r
            .interaction_cards
            .as_ref()
            .unwrap()
            .contains(&"Barbarian Ring".into()));
        assert!((r.synergy - 1438.4).abs() < 1e-9);
        assert_eq!(r.combo_count, 10);
        assert_eq!(r.independent_lines, 2);
        assert!(r
            .categories
            .iter()
            .any(|c| c.label == "Board wipes" && c.score == 21.));
        let copy: crate::salt::Analysis =
            serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(copy.review_saved(), a);
    }
    #[test]
    fn rejects_wrong_reports_missing_metrics_and_inconsistent_totals() {
        assert!(decode(fixture(), "wrong-id").is_err());
        for path in [
            "/powerLevelRating",
            "/synergyRating",
            "/details/powerLevel/scoring/interaction/score",
        ] {
            let mut data = fixture();
            *data.pointer_mut(path).unwrap() = Value::Null;
            assert!(decode(data, ID).is_err(), "{path}");
        }
        let mut data = fixture();
        data["saltRating"] = 999.into();
        assert!(decode(data, ID).is_err());
    }
    #[test]
    fn only_accepts_public_report_links() {
        assert_eq!(
            parse_ref(&format!(
                "https://commandersalt.com/details/deck/{ID}#brackets"
            )),
            Some(ID.into())
        );
        for url in [
            format!("https://evil.example/details/deck/{ID}"),
            format!("https://commandersalt.com.evil.example/details/deck/{ID}"),
            "https://commandersalt.com/algorithm".into(),
        ] {
            assert!(parse_ref(&url).is_none());
        }
    }
}
