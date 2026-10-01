# Local power assessment, version 2

The normal Moxfield check calculates this assessment from the fetched card list and the relevant Commander Spellbook combo lines. It does not require a CommanderSalt report. Salt, prices, reserved status and the owner's claimed bracket do not enter this model.

These are transparent screening heuristics, not a reproduction of CommanderSalt, calibrated win-rate predictions or official bracket rules. Scores persist with the analysis and are available offline. Local scoring schema version 6 also preserves per-card quantities for salt presentation and corrects double-faced card lookups. Power weights remain unchanged. Older analyses prompt for a re-check.

## Headline scores

Synergy, interaction, win conditions, consistency and efficiency each range from 0 to 100. Estimated power is `1 + 9 * weighted_score / 100`, with weights:

| Dimension | Weight |
| --- | ---: |
| Win conditions | 25% |
| Consistency | 20% |
| Interaction | 20% |
| Efficiency | 20% |
| Synergy | 15% |

The estimate is withheld unless the list contains 100 cards and every nonland card has a known, finite, nonnegative mana value. Supporting evidence is still shown. This is a completeness check, not a full Commander legality validator. Power does not override the separate card-based bracket screen or claim competitive intent.

## Synergy

Score is the percentage of nonland cards connected to another card through a detected supporter/payoff pair or a known combo. A card counts once even if it participates in several engines; quantities are respected. Cards never support themselves. A removal spell mentioning artifacts is not an artifact payoff.

Detected themes include artifacts, Treasure, tokens/anthems, instant/sorcery casting, sacrifice/death, graveyard use, life gain, counters, landfall with ramp/extra lands, and selected creature types. Changelings support those creature types. The screen lists the participating cards. This is coverage, not a measure of how quickly or consistently the engines operate; themes outside the supported patterns may be missed.

## Interaction

Each card takes its strongest detected role: wipes/counters 6, restrictions/taxes 5, targeted removal/damage/control 4, protection/graveyard answers 3. For nonland cards, mana value 2 or less adds 1; mana value 5 or more multiplies the base by 0.75. Quantities apply; total is capped at 100. Utility-land answers count without a cheap-spell bonus. Flexible target wording, permanent shuffling, theft, goad and graveyard exile are included. Personal defensive keywords alone are not counted as team protection.

## Win conditions

Relevant combo variants sharing at least two card names are grouped into connected families. Repeating a variant does not create another family. These are conservative families, not a claim of independent wins.

The strongest family starts at 65 for a recognized winning output, 55 for a Treasure engine with a matching attack/upkeep payoff in the deck, 45 for a lock or 35 for a resource engine that still needs an outlet. The conditional-payoff rule currently recognizes Treasure-count damage to any target and the twenty-artifact alternate-win condition. It does not treat those as immediate wins or assume the payoff is already in play. Up to two additional families add 10 each. Starting activation mana does not imply cheap assembly.

An alternate-win card starts its plan at 30, with up to two additional cards adding 5 each. Creature combat starts at 20 with at least 15 creatures; up to six potential finishers add 5 each. Finishers include mass pumps, additional combat, selected damage payoffs and evasive creatures of mana value at least 5. These are candidate finishers, not guaranteed victories.

The strongest of those three plans supplies the score, with up to two additional detected plans adding 5 each. No threshold claims a particular winning turn. Setup, timing, disruption, a suitable outlet and actual execution remain important.

## Consistency

Draw/card-access cards contribute 4 each (maximum 12 cards); nonland tutors contribute 10 each (maximum 4); recursion contributes 3 each (maximum 5). Add 20% of synergy and cap at 100. Overlapping roles may contribute here because they provide distinct forms of access; the evidence shows the cards.

## Efficiency

- Curve: `clamp((5 - average_nonland_mana_value) * 15, 0, 45)`.
- Ramp/cost reduction: 3 per card, capped at 30.
- Ramp with mana value at most 2: 5 per card, capped at 15. This is low-cost ramp, not a claim that every card is fast mana.
- Land supply: 10 with 30–40 front-face lands; otherwise `max(0, 10 - abs(lands - 35))`.

Spell/land MDFCs stay in the spell curve. Printed combined mana value is used for split cards. Color fixing, alternate costs and actual opening hands are not simulated. Land-light competitive lists and unusual architectures may be underestimated.

## Validation

Regression tests use the supplied public Magda list and cached Spellbook lines, plus controlled counterexamples. They check recognized engines and wipes, duplicate-combo invariance, price independence, missing-data behavior, improvement from added interaction, keyword false positives and offline serialization. These checks validate implementation behavior, not predictive accuracy; the weights need broader deck and match-result validation before making stronger claims.

## Comparison with CommanderSalt

A deck now retains its local calculation and imported CommanderSalt report together. Either source can be refreshed independently. The comparison shows each timestamp, card count, power, salt, bracket and supporting scores, followed by both detailed breakdowns. The deck tiles use the local salt/bracket when available. Re-linking to a different Moxfield deck discards the old comparison; importing a report for a different linked deck is rejected. Importing does not request a new analysis from CommanderSalt.

The supporting scales are **not interchangeable**. Our synergy is coverage and our other supporting metrics are capped heuristic indices. CommanderSalt exposes weighted point totals; its report has a separate synergy-coverage percentage. Both original units are shown, and coverage is compared separately using percentages. There is no justified constant that converts the old local index into the service's weighted points. Exact native-point parity remains unimplemented: the public flowchart omits current category biases, parser rules, caps and final power adjustments. Do not multiply local scores to make the example match.

The saved Magda fixture now estimates 8.4 power, interaction 82.5/100 and win conditions 75/100, compared with the imported report's 8.6 power, 105 interaction points and 672.5 win-condition points. This is evidence of corrected detections on one list, not broad calibration. Matching interaction-card names and synergy coverage help diagnose genuine coverage gaps separately from score units.

Reference: [CommanderSalt algorithm](https://www.commandersalt.com/algorithm), [published pipeline](https://www.commandersalt.com/resources/ingestion_flowchart.html), and [Magda report](https://www.commandersalt.com/details/deck/bf9dad6c497fcac34ff0933f6ad5ac06). Live report fields were rechecked while implementing the comparison. Its current raw scores and adjustments are more detailed than the explanatory flowchart; the flowchart alone is not an executable specification.


## Display preferences and calculated bracket

Main-menu Settings persists display choices in the existing settings table. The default primary score is Power or Bracket. The commander list has an independent Follow default / Power / Bracket override, also adjustable directly above the collection. Salt, win conditions, synergy and interaction are independent optional additions. Local calculations are preferred by default; the preferred source can be changed to CommanderSalt. Supporting numbers retain the selected source's units, while a missing local power value may fall back to the saved report's power. Display preferences never overwrite either analysis.

The calculated local bracket takes the greater of the card-based screen and a band of local power: below 3 → 1, 3–<5 → 2, 5–<7 → 3, 7–<9 → 4, and 9+ → 5. Without a power estimate it uses the card-based screen. This is a local heuristic, not official bracket certification. The original card screen, calculated bracket, CommanderSalt bracket and Moxfield declarations remain separately identified in the detailed view.

Commander collection badges and the commander pickers use the list override; selected seats, turn-order tiles and the final pregame summary use the default. Borrowed decks retain their owner's scores and lose those scores if their commander/partner configuration changes. Rematches reload score preferences before presenting the pod. Power and bracket values are grouped above the four salt/synergy/interaction/win-condition boxes.
