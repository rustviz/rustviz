// Normalize Rust source before hashing and submitting it. Trailing
// whitespace on a line is invisible in the editor but changes the
// sha256 the cache uses to key entries, so without this normalization
// two snippets that look identical render through the API twice
// (once for each whitespace variant) — and dropdown / cached-user
// snippets miss the cache every time the user copy-pastes code out
// of a terminal that strips through its scrollback buffer or adds
// alignment spaces.
//
// Only trailing-on-each-line is stripped. Leading whitespace
// (indentation) and interior whitespace are load-bearing for Rust
// parsers and for the user's mental model. Trailing newlines at
// end-of-file are also left alone — many `let x = …;` snippets need
// the final newline for `rustc` to accept them as a valid module.
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
