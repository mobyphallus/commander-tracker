# Scoring regression fixtures

Captured 2026-09-28 from the public Magda deck supplied for the investigation:

- `magda-commandersalt.json`: selected fields from https://api.commandersalt.com/decks?id=bf9dad6c497fcac34ff0933f6ad5ac06. Preserves report totals, category entries, source, timestamp, and bracket explanations. Excludes unrelated service metadata.
- `magda-moxfield-cards.json`: selected cards and import fields from https://api2.moxfield.com/v3/decks/all/mCMqeU3Ydk2PeTLx47ijkw. Covers split cards, modal double-faced cards, damage wipes, overload, and extra combat. This is deliberately a partial deck.

Offline tests must use these snapshots rather than depend on changing service results. Live report verification is an explicitly ignored network test.

- `magda-local-deck.json`: the full normalized Moxfield list from the same captured response, including printed mana values and all face text, for local assessment regressions.
- `magda-local-combos.json`: the relevant Commander Spellbook lines in the previously saved Magda analysis. Used to check engine-family grouping and duplicate-variant invariance.

The CommanderSalt fixture also retains interaction-category card lists, consistency, efficiency and synergy coverage for side-by-side comparison regressions. These fields were added from the same public report after verifying its power and synergy totals matched the original capture.
