# Verification — 2026-09-25

`cargo test --offline --bin commander_pod`: 116 passed, 1 ignored (live network).
`cargo build --offline --bin commander_pod --bin ui_review`: passed.

Application-message tests cover changing life, saving and returning home,
resuming paused, finishing, correcting the result, opening stats, rematching,
and retaining an unsaved game when storage fails. Storage tests cover restoring
photos and game recovery, creating a safety backup, and rejecting unrelated data.
A regression test ensures dismissing Help after recovery keeps timers paused.

The review harness uses only an in-memory sample database, with long player names,
a partner deck, missing art, and saved results. Run `cargo run --bin ui_review -- N`,
where N is 0 (1875×1205), 1 (800×1280), or 2 (1280×800). Each run captures seventeen
states including recovery, game dialogs, search keyboard, result correction,
stats, restore confirmation, rematch setup, partner damage controls, running/paused game boards, and bracket/salt badges.

The layout uses fixed logical dimensions and renders at half scale to fit portrait
reviews on a landscape desktop. Captures assert sufficient window space and crop
to the review viewport. PNG output is compressed and bounded to 1200 pixels wide.
Files are written to `/tmp/commander-finish-N-STATE.png`. These are static rendered
states, not a substitute for physical touch interaction. The harness ignores input
and closes its own process after the final capture.

This pass fixed undersized home cards, inconsistent dialog button styling, an
unsupported undo-log glyph, and timers restarting after restored Help was dismissed.
Plain `cargo run` now selects the main application.

Still requires hands-on validation: physical touchscreen gestures throughout a
full game, live card/deck services, artwork galleries, and file-picker integration.
The current review establishes local behavior and layout checks, not release-wide
product certification.

The circular center control has mouse/touch hit-area tests, including corner
pass-through and Done behavior. Identity clearance checks cover all supported
2–8 player layouts at all three review sizes, using the actual disc dimensions.
