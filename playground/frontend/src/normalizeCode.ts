// Normalize Rust source before hashing and submitting it. Trailing
// whitespace on a line is invisible in the editor but changes the
// sha256 the cache uses to key entries, so without this normalization
// two snippets that look identical render through the API twice
// (once for each whitespace variant) — and dropdown / cached-user
// snippets miss the cache every time the user copy-pastes code out
// of a terminal that strips through its scrollback buffer or adds
// alignment spaces.
//
// Only trailing spaces/tabs on each line are stripped. Indentation and
// interior whitespace are kept so the rendered code panel matches what
// the user typed, and newlines (including a final one) are untouched so
// line numbers in the visualization line up with the editor. The one
// semantic change is inside a multi-line string literal, where trailing
// spaces are part of the string's value; that doesn't affect ownership
// or borrowing, which is all RustViz visualizes.
//
// The same normalization runs at prerender time so the build-time
// SVG cache's keys are consistent with the runtime ones; see
// `scripts/prerender.ts`.
export function normalizeCode(code: string): string {
  // Match a run of trailing horizontal whitespace at end-of-line.
  // `^` / `$` with `m` flag scope to each line; `[ \t]+` deliberately
  // avoids `\s+` since `\s` also matches `\n` and would eat the
  // line break itself.
  return code.replace(/[ \t]+$/gm, '');
}
