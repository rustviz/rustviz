# Rust Book corpus

127 short programs in the style of *The Rust Programming Language*
(chapters 4–15, plus extras), each reviewed by hand against real Rust
semantics. The test is `rustviz-lib/tests/book.rs`; its `BOOK` table says,
per snippet, what the plugin on `main` gets right and what it gets wrong.

File names start with the probe id (`c4_09`, `m_13`, `x16`, ...) that
issues and PRs cite. `c<N>` is Book chapter N, `m_` is misc, `x` is crash
minimizations and controls. Don't renumber or rewrite a snippet, since that
breaks those references. Add a new file instead.

## Categories

| status    | meaning |
|-----------|---------|
| `Correct` | Renders, and every tooltip reviewed is true. |
| `Wrong`   | Renders, but some tooltip is false. The false strings are in `known_wrong`. |
| `Missing` | Renders and nothing shown is false, but an event is missing or drawn off-canvas (`off_canvas: true`). `note` says which. |
| `Crash`   | The plugin panics today. `note` gives the panic site. (None on main 4c8f552.) |

Each entry also has

- `must_contain`: correct tooltips that must stay;
- `must_not_contain`: false tooltips a fix removed, which must not come back;
- `known_wrong`: false tooltips main still prints.

A string written `"L13 text"` only matches a tooltip drawn on source line
13. Use this when the same text is right on one line and wrong on
another, e.g. a borrow that ends too early.

## What fails and what only prints a note

The test **fails** when:

- a snippet that renders now crashes, errors, draws outside the SVG (unless
  `off_canvas` is set), or has no timeline annotations;
- a `must_contain` string is missing;
- a `must_not_contain` string is present.

It **passes but prints a note** (on stderr, visible without
`--nocapture`) when:

- a `known_wrong` string is no longer shown;
- a `Crash` snippet now renders;
- an `off_canvas` snippet is now fully on canvas.

Every run ends with a summary like
`book corpus: 127 render (74 correct, 39 known-wrong, 14 missing), 0 known crashes; 0 failure(s), 0 note(s)`
(the counts on main 4c8f552).

## When a fix lands

The notes tell you which snippets your change affected. For each one:

1. Dump what the plugin prints now:
   `RV_BOOK_DUMP=/tmp/book RV_BOOK_ONLY=c4_09 cargo test -p rustviz-lib --test book`
   writes `/tmp/book/c4_09_first_word_str.txt`, one `L<line> <text>` per
   tooltip.
2. Read the new tooltips as a Rust teacher would. Move each `known_wrong`
   string that's gone to `must_not_contain`. Add the corrected text to
   `must_contain`.
3. Update `status`: `Correct` if nothing false or missing is left (then
   `known_wrong` must be empty), and edit `note`. For a crash that now
   renders, review all of its tooltips, then give it a status and lists
   like any other snippet.
4. Clear `off_canvas` if the note says everything is on canvas.

Then confirm the new `must_not_contain` entries fail on the old plugin, as
for `corpus.rs`. If a change makes a `known_wrong` string disappear but the
replacement is still false, put the new false text in `known_wrong`.

The test runs the installed plugin (`cargo rv-plugin` on PATH), so
reinstall after plugin changes: `cargo install --locked --path rustviz-plugin`.
`RV_BOOK_JOBS` sets the number of parallel runs (default 4).
