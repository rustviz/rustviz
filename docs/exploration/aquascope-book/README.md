# Exploration: RustViz alongside Aquascope in the Brown experimental Rust Book

Status: **parked.** This is an exploration branch, not a proposal to merge. It waits until the RustViz maintainer has discussed it with the Aquascope authors (Will Crichton, Gavin Gray, Shriram Krishnamurthi). After that it is a good student project. The tracking issue is the "Explore: RustViz alongside Aquascope" issue in this repository.

The idea: in a fork of the Aquascope-enabled Rust Book, let authors choose, per example, whether RustViz, Aquascope or both best serve the learning goal. Possibly also a combined view: Aquascope's run-time stack/heap view is consistent with RustViz, and its permissions view is a different presentation of the same borrow-checker facts.

Nothing here was pushed to or sent to the Aquascope or Rust Book repositories. All builds were local, and no data went to their servers.

What was studied: `cognitive-engineering-lab/rust-book` at commit 88250e0 (2026-05-04), `aquascope` v0.4.0, `mdbook-quiz` v0.5.0, and RustViz `main` at about commit 64055f50.

What is on this branch:

- `mdbook-rustviz/`: the preprocessor changes (opt-in, off by default; a book that sets none of the new flags renders exactly as before). New files `src/assets/viz.css` and `src/assets/viz.js`, plus edits to `src/cfg.rs` and `src/preprocessor.rs`.
- This directory: report, `book-toml.diff`, `ch04-markup.diff`, `coverage-table.md`, `consistency.txt`, `scripts/`, `screenshots/`.

The `.diff` files are written against the Rust Book's `book.toml` and `src/ch04-*.md` at the commit above. Their local tool paths were replaced with placeholders. The scripts need `$WORK` (a working directory), `$RUSTVIZ_BIN` and a browser-automation install. Their `$WORK` and `localhost:8000` values are placeholders to edit, not working defaults.

## 1. Repo and build

**Licenses.** rust-book: MIT or Apache-2.0. Aquascope: MIT (Gavin Gray). mdbook-quiz: MIT or Apache-2.0. RustViz: MIT. All are permissive and compatible with a public fork that keeps the notices.

**How the book is built.**

- mdBook 0.5.2. `mdbook-rustviz` already targets the 0.5 API.
- Preprocessors: `quiz`, `aquascope`, two in-repo ones (`trpl-note`, `trpl-listing`) and mdBook's built-in `links`. With no order set in `book.toml`, mdBook's default applies: aquascope, then quiz, then trpl-*.
- The `feedback` JavaScript extension is loaded. `telemetry` and `consent-form` are commented out.
- The book uses Rust 1.90. Aquascope needs a pinned nightly (`nightly-2026-05-01`) with rust-src, rustc-dev, llvm-tools-preview and miri. RustViz also pins a nightly. Today they match because both follow `rustc_plugin` releases.

**How Aquascope blocks are written.** The fence is ` ```aquascope,<ops>[,flags] `. Ops are `interpreter` (run-time stack and heap steps) and `permissions` (compile-time read/write/own). Flags include `stepper`, `boundaries`, `shouldFail`, `horizontal`, `showFlows`, `concreteTypes`, `hideCode`. A leading `#` hides a line, `` `[]` `` marks an interpreter step (L1, L2, ...). Their parser is plain text matching, so it finds ` ```aquascope ` anywhere in a chapter's raw text.

**Counts.** 82 Aquascope blocks (ch04: 55, ch05: 15, ch06: 7, ch08: 4, intro: 1); 34 of them are `shouldFail`. There are also 517 `{{#rustdoc_include}}` listings, each a small cargo project.

**Does building need their server?** No. `mdbook-aquascope` runs `cargo aquascope` per block at build time and embeds the results as `data-*` attributes. Aquascope ships prebuilt binaries only for Linux x86_64, so on macOS arm64 it was built from source (about 65 s). One trap: running `rustup` inside their clone silently installs stable 1.90 because of their `rust-toolchain` file.

## 2. Telemetry and A/B testing

- **Current state: off.** Commit 88250e0 commented out the telemetry and consent-form scripts in `book.toml` ("Not doing any research right now"). The live ch04-01 page loads no telemetry script.
- **What was logged.** A POST to the research endpoint carrying a session ID kept in localStorage, the commit hash, a timestamp and a payload. Endpoints covered quiz answers, quiz bug reports, runtime errors, Aquascope bug reports (with the code and the reader's feedback) and highlight-and-comment feedback. The URL is hard-coded at build time. Logging happens only for builds from `main`/`master` not served from localhost, and does not check consent.
- **Consent.** A modal naming Brown's Cognitive Engineering Lab.
- **A/B assignment.** None in the client code. Interventions were compared over time by commit hash.

**What a fork must do.**

1. Delete the telemetry package or change its endpoint. Do not just leave it commented out, or a later merge can quietly send our students' data into their dataset.
2. Remove or replace the `feedback` extension, which calls the telemetry logger without checking that it exists.
3. Rewrite `src/experiment-intro.md` and its images, which describe their study, publications and Brown branding.
4. Nothing else is needed for mdbook-quiz and Aquascope: they log only through `window.telemetry?.log`, so they stay silent without the script.

In the prototype, headless Chrome blocked every request except to the local server. The only external request attempted was mdBook's own fetch of the Rust Playground crate list. There were no telemetry calls.

## 3. Coverage: RustViz on the Book's code

Method: `scripts/extract.py` strips Aquascope's markers to plain Rust and takes the `src/main.rs` of each listing in chapters 4, 5, 6, 8, 10, 13, 15 and 16; `rustviz html` runs on each. A compile failure counts as intended when the block is `shouldFail`, the fence says `does_not_compile`, or the listing's `output.txt` contains an error. Full table: [`coverage-table.md`](coverage-table.md).

- **Aquascope programs: 48 of 48 that compile render.** The other 34 are deliberate does-not-compile examples, Aquascope's main use. RustViz can't draw them by design, so they stay Aquascope-only.
- **Listings: 132 of 178 render.** 3 are fragments (`rust,ignore`), 7 import a crate from elsewhere in the Book, 34 intentionally don't compile, 2 were refused.
- **The 2 refusals were one crash**: a struct-like enum variant (`enum Message { Move { x: i32, y: i32 } }`) hit an `unwrap()` in the plugin. Filed as rustviz/rustviz#907.
- **Also found and fixed:** the operator actor wording (`item == b' '` read "Copy from item to ==", though `==` borrows its operands): rustviz/rustviz#908.
- "Renders" is not "correct". Tooltips for three ch04 programs were read and are sound, but a real fork needs someone to read all 48 paired diagrams as a Rust teacher before students see them.

## 4. Integration design

The change is a pass in `mdbook-rustviz` that runs before `aquascope` and edits chapter text directly. A chapter is re-serialized through pulldown-cmark only if it contains ` ```rv ` blocks, so unrelated Markdown is untouched.

**Per-block fence flags** (work on ` ```aquascope ` and ` ```rust ` fences, including `{{#rustdoc_include}}` listings, whose hidden `# ` lines are shown in full):

| Flag on the fence | Result |
|---|---|
| none | untouched, byte-for-byte upstream |
| `rustviz` | tabs, Aquascope (or Code) shown first |
| `rustviz-default` | tabs, RustViz shown first |
| `rustviz-only` | RustViz replaces the diagram or code block |
| `rustviz-combo-hover`, `rustviz-combo-rows` | prototype combined views, Aquascope interpreter blocks only |

Our flags are removed before Aquascope or mdBook see the fence, so Aquascope's cache keys equal upstream's. A block RustViz can't draw falls back to exactly what the author wrote, with a warning in the build log. `aquascope-tabs = true` in the `[preprocessor.rustviz]` table is a dev switch that tabs every eligible block.

**Why ordering matters** (`book-toml.diff`). Adding any `before` constraint changes mdBook's default order, so `aquascope` must also be pinned `before = ["quiz", "trpl-listing", "trpl-note"]`. Without that, quiz ran before Aquascope and broke the build, and trpl-* re-fenced Aquascope blocks with four backticks so every diagram came out as a raw `<code>` blob.

**The tab card.** Both views sit in one card with an mdBook-style tab bar. The panes are stacked in one grid cell, so the height never changes between states. The RustViz figure is scaled down (never up, floor 60%) to fit the column; tooltips still work when scaled.

**Dark mode.** The RustViz SVGs are embedded as `<object>`s, so page CSS can't reach inside. `viz.js` copies the theme's code-block colours into each SVG on load and on theme change, darkens the few colours that vanish on dark backgrounds, and loads mdBook's code font. A pure-CSS `invert + hue-rotate` filter was tried and rejected: hues shift (orange to brown, red to pink), so colours stop matching each other. The theming does not work from `file://`, same as the existing tooltip script.

**ch04 markup** (`ch04-markup.diff`; 15 blocks, all render):

| Block | Flag | Reason |
|---|---|---|
| 4.1 #04 `let b = a` (Box) | `rustviz` | RustViz adds that `a` is invalid afterwards and the box is dropped once, via `b`. |
| 4.1 #06 `make_and_drop` | `rustviz` | Shows the drop at the end of the function's scope. |
| 4.1 #07 `add_suffix` | `rustviz` | Prose walks L1-L4, so Aquascope stays default; the timeline shows the whole `first -> name -> full` chain. |
| 4.2 #02 `greet(m1, m2)` returning a tuple | `rustviz-only` | The point is the clumsy move-in/move-out round trip; prose never refers to the diagram. |
| 4.2 #03 `greet(&m1, &m2)` | `rustviz` | Borrows start and end at the call while `m1`/`m2` stay owners. |
| 4.2 #10 `num` used twice, then `push` | `rustviz-default` | A borrow lasts until its last use, which the timeline draws directly. |
| 4.2 #15 `ascii_capitalize` | `rustviz` | The borrow's lifetime differs per branch; RustViz draws branches. |
| 4.4 #01 `first_word` -> `usize` | `rustviz` | The returned `usize` isn't tied to `s`. |
| 4.4 #04 `hello` slice, then `push_str` | `rustviz-default` | Slice-borrow extent, no L-label prose. |
| 4.3 `add_big_strings` x3 | `rustviz` | The point is "shorten the borrow of `dst`"; comparing three timelines shows it. |
| 4.4 Listing 4-7 (`first_word`, bytes) | `rustviz` | Full program; complements the prose. |
| 4.4 `first_word` returning `&str` | `rustviz-default` | The returned slice keeps `s` borrowed, the section's lesson. |
| 4.5 `Document` API | `rustviz-only` | Ownership in signatures; code fully visible in RustViz's panel. |

Deliberately Aquascope-only: memory-layout points RustViz can't show (copies, clone's deep copy, deref and pointee, vector reallocation, slice fat pointers) and permission-model points (R/W/O, flow permission, place granularity such as `a[_]` and `name.0`).

**Screenshots** (`screenshots/`, light and dark): `tabs-add_suffix-{aquascope,rustviz}-{light,dark}.png` show the two toggle states; `flag-rustviz-default-num-*` shows a per-block flag; `combo-A-hover-greet-line3-*` and `combo-B-rows-greet-*` show the combined designs.

## 5. Consistency of the two views

All 48 shared programs were checked automatically and each flagged case by hand (`consistency.txt`, script `scripts/consistency.py`).

Aquascope's permissions view is not a different model: it is computed from the same borrow-checker facts RustViz uses. Where borrows end, it agrees with RustViz in 12 of 13 comparisons, and both move comparisons agree. The interpreter is the run-time view. It agrees with RustViz on moves and drops, but disagrees in pictures about references. 35 of 48 programs show no disagreement. The rest:

1. **A reference outlives its borrow in the picture** (10 instances in 5 programs: 4.2 #04 `r1`, 4.4 #02/#03 `hello`/`world`, 4.5 #01 `a_stack_ref`/`a_box_stack_ref`, 8.1 #01 `n1`/`n2`). Aquascope draws the pointer until the stack frame ends; RustViz ends the borrow at its last use. A combined view has to explain this.
2. **Dead references drawn as dangling** (intro block, 8.1 #01): after `push_str` reallocates, Aquascope draws `hello` as dangling although it is never used again and RustViz ended its borrow earlier.
3. **Partial move** (5.1 #03, struct update syntax). Aquascope greys out all of `user1`; RustViz moves only `user1.username` and drops `user1.email` at scope end, which is what rustc does.
4. **Step granularity** (6.2 #03, `match &opt`). Aquascope restores `opt`'s write permission after the whole match; RustViz ends the borrow inside the `Some` arm. Same fact, different granularity.

No drop-timing disagreements.

## 6. Combined views (prototype, three examples each)

- **A. Hover.** Hovering a timeline line shows Aquascope's snapshot for the latest step at or before it, captioned like "L3: memory after line 8". Works across functions. The snapshot sits underneath because the ~750px column can't fit both side by side. Needs hover (no touch); one step visible at a time. About 3-4 days to harden.
- **B. Rows (recommended).** Each Aquascope step marker becomes a dashed row across both RustViz panels with an L-badge in a gutter; snapshots run in a strip below, and hovering a step highlights its row. Reads well without interaction: one vertical axis, every step visible. Weakness: more than about 3 snapshots overflow the column and scroll sideways. About 5-7 days.

## 7. Effort (worker-days)

| Item | Days |
|---|---|
| Land the tabs properly: tests, README, stop writing SVGs into the book's `src/examples`, stable example IDs | 1.5 |
| RustViz dark mode in the plugin | 1-2 |
| Fork repo, Linux CI and Pages deploy | 1-1.5 |
| Remove telemetry, consent and feedback; rewrite the experiment intro; attribution and "unofficial fork" notice | 0.5 |
| Struct-variant crash fix with corpus fixture (#907) | 0.5 |
| Rust-teacher review of the 48 paired diagrams, plus any listings added (open-ended) | 2-4 |
| **Tabs, credible first version** | **about 7-10**, plus about 0.5 per upstream sync |
| **Combined view B on top** | **5-7** |

**Plugin-side prerequisites** that would replace page-side workarounds: emit colours as CSS variables with a dark palette inside the SVG; emit a `viewBox` and size the code panel to its content; tag code rows and events with their source line (the plugin inserts unnumbered rows between lines, so today the page reads row positions out of the code panel); export per-line facts as data so the page does not parse SVG geometry.

## 8. Risks

- **Nightly mismatch.** The two tools match only while both follow `rustc_plugin` releases. Drift costs a second nightly in CI, not a build conflict, because each tool selects its own toolchain.
- **Upstream churn.** Their Markdown keeps changing, and the book syncs with the official book every few months. Our change is config plus a preprocessor, so merges should mostly touch `book.toml`, but changed diagrams need re-review. Add a CI check that the output never contains `<code>&lt;div class="aquascope-embed"`.
- **Telemetry, ethics and IRB.** Their logging was research under Brown's protocol. A fork must never post to their endpoint, must not reuse their consent text or imply participation in their study, and needs its own IRB approval and its own endpoint for any data it collects.
- **Licensing and attribution.** Keep LICENSE and COPYRIGHT; credit the official book's authors, Brown's Cognitive Engineering Lab (Will Crichton, Gavin Gray, Shriram Krishnamurthi) and Aquascope; remove Brown branding and their domain from deploy; label the fork unofficial (Rust trademark policy).
- **Teaching.** Two visual models side by side can confuse students. Surrounding prose is written for Aquascope's L-labels and permission vocabulary ("s loses W permission"), which RustViz doesn't use, and RustViz adds nothing on the 41% of blocks that don't compile.

## Recommendation

**Coordinate with the Aquascope authors before any fork.** Contact Will Crichton (who maintains the book, mdbook-quiz and Aquascope), with Gavin Gray and Shriram Krishnamurthi, sharing the screenshots and the diff. Questions:

1. Would they accept an opt-in flag or build variant upstream instead of a fork?
2. Are they fine with a public fork under the rustviz org, and what branding and attribution do they want?
3. Do their research or IRB impose constraints? They may resume studies, and a parallel fork could split their audience.

They may also want to compare the two visualizations in a study themselves. Their repository asks that issues go to them, and two "experimental Rust Books" would confuse learners. Only after they are on board: ship per-block tabs first, then build combined view B as an opt-in for interpreter blocks, since B renders their component inside ours.
