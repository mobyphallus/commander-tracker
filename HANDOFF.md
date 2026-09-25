# Handoff — commander list, on-screen keyboard, card tiles

Written 2026-09-24. Covers the UI work on branch `moxfield-salt`. Everything
described here is **uncommitted** in the working tree.

## Situation

Two agents have been working in this repo at the same time, in one shared
working tree on `moxfield-salt` (branched from `4989e59`):

- **This work** — the commander/deck list, the in-app keyboard, shared card
  tiles, and a game-screen layout fix. Files: `src/cards.rs`,
  `src/keyboard.rs`, `src/screens/players.rs`, `src/screens/setup.rs`,
  `src/screens/game.rs`, `src/style.rs`.
- **The other agent** — Moxfield deck links and salt/bracket scoring. Files:
  `src/moxfield.rs`, `src/salt.rs`, `src/screens/breakdown.rs`, plus the
  `deck_analysis` table in `src/db.rs` and a deck-page view in
  `src/screens/players.rs`.

`players.rs` is **shared** — both sets of changes are interleaved in it.
Check `git diff` before assuming any hunk is yours.

`cargo build` and `cargo test` are green (69 passing, 1 ignored). Three
pre-existing dead-code warnings (`layout.rs:66`, `model.rs` two fields) were
there before either of us and are untouched.

### One thing to clean up

`cargo fmt` was run early in this work and reformatted files neither agent
meant to touch — `rotated.rs`, `screens/game.rs`, `screens/history.rs`,
`screens/stats.rs`, `cache.rs`, `layout.rs`, `art.rs`, `icon.rs`,
`scryfall.rs`, `model.rs`, `app.rs`. It was left alone rather than risk
reverting in-flight work. Worth stripping to formatting-only noise before
committing, once both agents are done.

## iced 0.13 traps — read this before touching any grid or image

Three findings that cost real time. All are confirmed by reading iced's
source, not guessed, and the first two are documented at length in the
module doc of `src/cards.rs`.

**1. An overflowing image inside a scrollable is drawn outside it.**
`ContentFit::Cover` always overflows its bounds, which makes
`image::draw` push its own clip layer — and
`iced_graphics::layer::Stack::push_clip` *replaces* the clip in force
instead of intersecting with the parent's. Inside a scrollable that clip
lands at the scrolled position with nothing holding it to the viewport, so
art gets painted over whatever sits above the list, or disappears while the
list moves. **Rule: every image box is cut to its picture's own proportions
and uses `ContentFit::Contain`, which can never overflow.** `cards.rs` has
a test (`every_art_box_matches_the_shape_of_the_art_in_it`) guarding the
aspect ratios.

**2. A scrollable forces its minimum height onto every tile.**
`Limits::resolve` clamps a `Fixed` (or `Shrink`) height *up* to
`min.height`, and `scrollable::layout` builds its content's limits from its
own `limits.min()`. A wrapping row passes those same limits to each child,
so inside a full-height scrollable every tile is silently stretched to the
height of the whole viewport and one row fills the screen. **Fix: nest the
wrapping row in a `column!`**, which re-issues child limits from
`Size::ZERO`. That is what `cards::grid()` exists for — use it for any
wrapping grid rather than hand-rolling `scrollable(row(..).wrap())`.

**3. There is no IME or virtual-keyboard support at all.**
No `set_ime_allowed`, no `InputMethod` (those land in 0.14). Hence the
hand-built keyboard below.

## What was built

### `src/cards.rs` — shared card tiles

Both the players screen and setup show commanders, so the tiles live here
and take their press `Message` from the caller.

- `deck_tile` — a **square** tile (`TILE = 350.0`, sized so five plus gaps
  span this machine's screen almost exactly; ten decks land in two full
  rows). Art crop on top, name below, mana pips bottom-left of the art,
  Moxfield bracket/salt chips bottom-right. A partner pair is *one* tile,
  two squares wide, showing both cards uncropped.
- `card_tile` — a whole card (portrait, `CARD_W = 246.0`) for search
  results. Keeps card proportions on purpose: squaring one off would cut
  the type line that tells two printings apart.
- `grid` — the wrapping scrollable grid. See trap 2.
- `mana_row` / `DeckMeta` / `identity` / `first_word`.

Colours come from `style::mana_color` — the card-face colours (W `#FFFBD5`,
U `#AAE0FA`, B `#CBC2BF`, R `#F9AA8F`, G `#9BD3AE`), deliberately outside
the app's violet palette, because a player reads a mana symbol by colour
before they read the letter.

### `src/keyboard.rs` — the app's own keyboard

Types straight into the `String` behind a field rather than synthesising key
events, so a screen only says *which* field is being edited. Generic over
the screen's own field id.

The focus trick worth knowing: **`text_input` captures the press that
focuses it but lets the release through**, so
`mouse_area(text_input(..)).on_release(Focus(field))` gets both a real caret
(and a working physical keyboard) *and* raises the panel. Each screen has a
`keyed_field` helper and a `with_keyboard` wrapper that mounts the panel at
the bottom, taking space from the content rather than floating over it.

Modifier keys are spelled out ("Shift", "Delete") — the app ships no icon
font and U+21E7 / U+232B render as empty boxes.

### `src/screens/players.rs`

Was: saved decks squeezed into `FillPortion(2)` while an *empty* search
panel took `FillPortion(3)`. Now: a full-screen deck grid; search moved to
its own screen behind **+ Add a Deck**; per-deck actions on a bar under the
grid so tiles stay nothing but cards.

Partners: **Set Partner** now searches Scryfall directly, with the player's
own unpaired decks offered alongside. Previously both halves had to be saved
separately and then paired — which is why the database had 36 saved decks
and zero pairs. Removing a pair takes both halves (leaving one behind made
the pair reappear as a phantom deck under the other name).

### `src/screens/game.rs` — the Done collision

The partner damage picker was a plain row pinned to the bottom of a seat
tile in *screen* space, so for every seat along the top of the table it sat
under the Done button that floats dead centre of the board. It is now two
turned `rotated::edge_button` chips at either hand on the player's own edge
— the one part of a tile that can never reach the middle of the table. Each
shows the short name and its running total (`Tymna` / `14 dealt`).
Commander Hate and End Turn stand down while damage is being logged, since
three chips a side is how a long name ends up under its neighbour.

## Verified vs not

Confirmed by screenshot on the real app: the deck grid, square tiles filling
the row, art loading, mana pips, bracket/salt chips (including updating live
after an analysis), selection ring, action bar, the keyboard panel, and the
setup player picker at its new size.

**Not verified — please test:**

1. **The game-screen Done fix.** Needs a saved partner deck *and* a live
   commander-damage log, which could not be reached without writing fake
   data into the real database. Compile-verified, and it reuses the exact
   Start/End edge slots that Hate and End Turn already occupy, so the
   geometry is proven — but nobody has watched it.
2. **A partner pair tile** (the double-wide two-card tile). No pairs existed
   in the database, so it has never actually been drawn.
3. **The printing galleries** (`Art` button; art picking in setup). Changed
   from `Cover` to card-shaped `Contain` as part of trap 1. Reasoned about,
   not looked at.

## Environment notes

- Runs on Hyprland/Wayland at 3000x2000, scale 1.6 → ~1875x1250 logical.
  `style.rs` sizing is tuned for exactly this panel.
- No on-screen keyboard package is installed, and none is needed.
- The app can be driven for screenshots with `ydotool` (daemon running, user
  in `input` group) plus `grim`. It must be on the *visible* workspace to be
  captured — hidden workspaces are not composited. Be aware the owner may be
  using the machine; driving the GUI steals their screen, and a stray click
  once triggered a real Moxfield analysis.

## Stored context

Project memory lives at
`~/.claude/projects/-home-moby-source-commander-pod/memory/`, indexed by
`MEMORY.md`:

- `rule0-bracket-salt-sources.md` — verified Moxfield / Commander Spellbook
  / EDHREC / Scryfall endpoints for bracket and salt. *(Other agent's.)*
- `commandersalt-recovered-formulas.md` — bracket thresholds and per-card
  salt weights recovered from commandersalt.com. *(Other agent's.)*
- `iced-013-layout-traps.md` — the three traps above, in short form.

There is no `CLAUDE.md` in this repo. The commit messages on `main` are
unusually detailed and are worth reading for design intent — `4989e59`
(UI polish) and `cc8ed65` (theme, seat tiles, eliminations) especially.
