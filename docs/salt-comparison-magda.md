# Magda scoring comparison

Investigated 2026-09-28 using the production database read-only and the public CommanderSalt result. No production scores or scoring code were changed.

- Moxfield: https://moxfield.com/decks/mCMqeU3Ydk2PeTLx47ijkw
- CommanderSalt: https://www.commandersalt.com/details/deck/bf9dad6c497fcac34ff0933f6ad5ac06
- Public result JSON: https://api.commandersalt.com/decks?id=bf9dad6c497fcac34ff0933f6ad5ac06
- Local saved analysis: 2026-09-28T18:53:06-05:00, scoring version 2, 100 cards.
- CommanderSalt result ingest: 2026-09-28T23:29:14Z, 100 cards.
- Direct Moxfield API access during this investigation returned HTTP 403. Card text and prices below are from the locally saved deck; CommanderSalt is an earlier snapshot, not a simultaneous rescore.

## Salt reconciliation

| Category | Local | CommanderSalt | Remote minus local |
| --- | ---: | ---: | ---: |
| Community salt | 30.077524 | 29.673394 | -0.404130 |
| Board wipes | 0 | 21 | +21 |
| Infinite combos | 24 | 30 | +6 |
| Theft | 7 | 0 | -7 |
| Extra combats | 5 | 3 | -2 |
| Meta staples | 0 | 0.5 | +0.5 |
| Card price | 0 | 1.3484 | +1.3484 |
| **Total** | **66.077524** | **85.521794** | **+19.444270** |

These are different models and data snapshots, not merely different rounding.

## Confirmed local gaps

1. `src/salt.rs::categories_for` recognizes only a few creature-wipe phrases. It misses Bloodfire Dwarf's damage to each creature without flying and Vandalblast's overload artifact destruction. CommanderSalt scores each at 7.
2. `src/moxfield.rs::CardData` imports only top-level oracle text. The saved Fast // Furious and Sundering Eruption // Volcanic Fissure have empty oracle text. Furious accounts for another missing 7-point wipe. Face data must be retained or enriched before categorization; counting faces must not double the deck's quantity.
3. Extra-combat salt is explicitly an unverified guess in our code (5); this example assigns Great Train Heist 3.
4. No meta-staple category exists locally. This result assigns Sol Ring and Springleaf Drum 0.25 each. Those memberships should come from a maintained source, not this single deck.
5. Six cards lack local EDHREC scores. The ASCII-only slug function drops accented letters instead of transliterating them (Glóin becomes gl-in), and taking only the front name of every ` // ` card conflates split cards with double-faced cards. Missing-score messaging currently suggests new cards even though request failures and name normalization can also cause it.
6. The analysis model contains no synergy, interaction, win-condition strength, mana-efficiency, consistency, or 1–10 power rating. A bracket screening result cannot substitute for these.

## Differences that should not be blindly patched to match

- Hellkite Tyrant genuinely steals artifacts. Our 7-point theft classification matches its rules text; CommanderSalt gives no theft contribution in this result. Removing it solely to match would discard valid information.
- CommanderSalt has 10 combo entries; we have 8 relevant Spellbook entries. Six Clock of Omens variants and the Dwarven Bloodboiler variant are shared. CommanderSalt additionally expands the Battered Golem / Magda / Maskwood Nexus engine with Hoard Hauler, Lifecraft Engine, and Unlicensed Hearse. Our result instead has the generic three-card engine. Net difference: two entries, 6 salt. Template expansion and line deduplication need an explicit policy.
- CommanderSalt prices Smaug the Magnificent at $67.42, producing 1.3484 price salt. The saved Moxfield price is $20.79, below our $25 threshold. This is a source/snapshot difference, not demonstrated arithmetic failure.
- Community-score sources, missing cards, and face handling differ. The exact aggregate difference is -0.404130; it is not evidence that all six missing local scores explain the total gap.

## Power and bracket findings

CommanderSalt's result contains:

- Overall power: 8.58773741828711 (approximately 8.6/10; integer display field is 8).
- Synergy: 1438.4.
- Interaction: 105 (the frontend reads `details.powerLevel.scoring.interaction.score`).
- Win-condition rating: 672.5246710915425; top-level rounded value 672.5.
- Ten combo entries, grouped into two independent winning lines.
- Display bracket: 4; its separate WotC-criteria screening field is 2.

Its power assessment includes consistency, interaction, efficiency, win conditions, mana base, and additional adjustments. Synergy contributes to consistency. These are separate from its additive salt categories. A 1–10 scale alone does not establish greater accuracy, but this model incorporates important information our bracket screening omits.

Our bracket is 3 because Spellbook flags six lines as definitely-two-card while their displayed card lists include Magda plus two other pieces. Our UI does disclose that commanders/setup may be involved. CommanderSalt counts actual two-card entries differently and raises its displayed bracket using the broader power assessment.

## Implementation direction

First fix face import, board-wipe detection, and score lookup normalization with regression fixtures. Version the scoring method and clearly mark saved analyses that need refreshing. Then add separate, attributed power/synergy/interaction/win-condition results, either by explicitly linking a CommanderSalt report or by building and validating a transparent local model. Do not derive power by scaling salt or changing constants until this one deck matches. Preserve the distinction between owner-declared bracket, card-based screening, and a broader power assessment.

## Algorithm documentation cross-check

Additional references supplied by the user:

- https://commandersalt.com/algorithm
- Embedded pipeline: https://commandersalt.com/resources/ingestion_flowchart.html

The page is rendered with JavaScript; its text was read from the site's public frontend bundle and the embedded flowchart HTML. It describes nonlinear compression into the power scale and says its weights evolve. The flowchart separately processes card categories, combos, synergy, mana base, salt, archetypes, power, and brackets. Its power branch combines consistency, interaction, efficiency, and win conditions, with mana-base and commander adjustments. This supports implementing separate metrics rather than converting salt to power. The live deck result remains the reference for exact values: the diagram is explanatory documentation, not a complete, versioned implementation or proof that all illustrated thresholds match the current engine.

## Implemented fixes

Scoring version 3 retains all Moxfield face text, recognizes damage and overload wipes, assigns extra combats 3 points, normalizes common accented card names, tries full split-card names before front-face fallback, and stops caching failed HTTP requests as missing EDHREC scores. Existing negative cache entries are retried. Saved older local analyses show a refresh notice.

The Salt & bracket link field now also accepts a public `https://www.commandersalt.com/details/deck/<id>` report URL. This imports the service's actual power, synergy, interaction, win-condition, salt and bracket results, source timestamp, bracket explanations, and salt contributions. It retains the underlying Moxfield URL for card-list access. Re-check reloads the published report; deck changes must first be reanalysed on CommanderSalt. The application does not submit decks to that service or claim that its local estimates are the service's formula.

Results persist in the existing analysis JSON and work offline. Failed imports preserve the prior saved analysis. Late responses after unlinking or switching player/page cannot repopulate that deck. The live Magda report import was verified at 85.52 salt, 8.6 power, bracket 4.

## Local assessment follow-up

Normal Moxfield checks now calculate their own power, synergy, interaction, win-condition, consistency and efficiency estimates. CommanderSalt import is optional. The versioned local method and its limits are documented in `local-power-scoring.md`; its values are not expected to equal CommanderSalt's proprietary scores.

A live Moxfield check after these changes produced 7.7 estimated power, 93 synergy, 58 interaction, 60 win conditions and 85.07 local salt. Supporting scores use a local 0–100 scale. Imported-report screens provide a Calculate from Moxfield action to switch to local scoring without copying the source URL.

## Retained comparisons and second local model

Both sources are now retained for the same Moxfield deck. Local refresh preserves the published report; report refresh preserves the local calculation. The comparison displays the original score units and source dates, including CommanderSalt consistency (170), efficiency (335) and synergy coverage (85.3%) when available. Older report snapshots remain readable and can be refreshed to obtain those additional fields.

Local model version 2 recognizes additional interaction from Barbarian Ring, Chaos Warp, Summon: Bahamut, Season of the Bold, Hellkite Tyrant, Glóin and Unlicensed Hearse. Infinite-Treasure engines also get credit for matching conditional attack/upkeep payoffs actually present in the deck. The cached full-deck regression now returns approximately 8.4 power, 82.5 interaction and 75 win conditions. Supporting indices remain distinct from CommanderSalt's native points; remaining scale and formula differences are documented in `local-power-scoring.md`.
