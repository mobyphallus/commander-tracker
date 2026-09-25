# Commander Pod

A touch-first app for the 3000 × 2000 laptop display at 1.6× scale. Design for the table: readable from a seat, forgiving to fingers, and focused on commander art. Smaller windows should reflow without hiding actions; phone packaging is not a target.

## Color and hierarchy

- Page `#101114`; cards `#191B20`; raised controls `#24272E`; borders `#363A44`.
- Primary text `#F3F4F7`; secondary text `#ADB1BD`. Never use structural border colors for readable text.
- Purple `#8B5CF6` identifies selection and progress; lighter purple `#B69AFF` is for accent text and focus. Primary buttons use a darker `#7040CF` fill with light text for contrast.
- Success and danger communicate status. Pair status color with a label or a visible border, never color alone.
- Mana retains its familiar pale W/U/B/R/G colors with dark symbols, independent of the app palette.
- Use flat surfaces and subtle borders. Reserve scrims for text over card art. Do not add purple washes or shadows to every panel.

## Shape, spacing, type

Use `src/style.rs` tokens instead of local colors and decorative values. Spacing uses 4, 8, 16, 24, and 32 logical pixels. Corners use 12, 16, and 20; discs and badges are fully rounded.

Standard controls are 72 logical pixels tall; major actions are 88. Touch targets include their padding. Keep life totals large and table-facing controls at their existing orientation. The type scale separates captions (16), body (18), labels (20), actions (22), subheadings (24), headings (32–36), and display text (48). Life counters use 76.

## Components

- Home uses a full-height dashboard composition: start-game seating preview, recent saved games, and three large destinations. Refresh recent results when returning home.
- Pod size is limited to 2–8. Show the seven choices in balanced rows of four and three. Layout choices use scalable, numbered diagrams with player-facing edge markers.
- Page headers use a labeled back action on the left, followed by a title and a short contextual subtitle. Setup retains its four-step progress header and stable footer.
- In-game, the central timer opens the game menu. Pause/resume and abandon live there; abandon still requires confirmation. Damage entry replaces the timer with a single Done control.
- Each screen has one visually dominant next action. Secondary actions use neutral surfaces; destructive actions use quiet red styling and existing confirmation flows.
- Cards lead with art, then the name. Deck grids calculate column widths from the available space, reserving room for the scrollbar. Partner commanders share one selectable tile.
- Selection has a visible purple outline. Bracket/salt buttons remain separate targets from deck selection, at least one standard control high.
- Dense history and stats views switch to stacked rows below 1100 logical pixels. Long lists scroll; headers and action bars stay reachable.
- Keep search and data loading feedback in stable content slots. Empty states explain what is missing and the next available action. Keyboard panels take layout space so fields remain above them.

## Icons and images

`src/icon.rs` contains original canvas vectors on a 24-unit grid, with rounded strokes. Use 24px for actions, 32px for destinations, and 30px for mana. Keep action labels; do not substitute emoji or platform-dependent icon-font characters. Mana symbols are sun, droplet, skull, flame, tree, and diamond.

Preserve image proportions and use `ContentFit::Contain` inside scrolling grids. Iced 0.13 clipping requires this; see `src/cards.rs`. Keep a column around wrapping rows inside scrollables to avoid stretched tiles.

## Review

Review primarily at the laptop's native logical size (~1875 × 1205), then 1280 × 800 and 800 × 1280 for resize robustness. Include long names, partner decks, missing art, keyboards, loading/error states, selection, rotated seats, and commander damage. Use isolated preview data, never real game records, for interaction testing.
