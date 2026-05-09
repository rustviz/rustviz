// Build-time pre-renderer for the playground's "Loops" / "Ownership" /
// etc. dropdown examples.
//
// For each curated example in `src/examples.ts` we shell out to the
// `rustviz` CLI, capture its `code.svg` + `timeline.svg` outputs, and
// emit a single `src/prerendered.json` keyed by `sha256(code)`. The
// SPA imports that JSON, looks up the user's current source against
// it, and renders directly without bothering the compile API. Result:
// dropdown switches are instant, the cold-start ~45 s wait is reserved
// for novel user code only.
//
// Build-time only. Not invoked at runtime. The output JSON is
// gitignored — every clone regenerates it via the `prebuild` script
// in package.json so stale entries can't sneak past code review.
//
// Failures are fatal: if any curated example fails to render through
// the plugin, the build aborts. Catches plugin regressions in CI;
// also forces local devs to keep the curated set healthy.

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { exampleGroups } from '../src/examples';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const FRONTEND_ROOT = resolve(__dirname, '..');
const OUTPUT_PATH = join(FRONTEND_ROOT, 'src', 'prerendered.json');

type Pair = { code_panel: string; timeline_panel: string };

function sha256Hex(s: string): string {
  return createHash('sha256').update(s, 'utf-8').digest('hex');
}

/**
 * Content-addressable version: a hash of the rendered entry set.
 *
 * This means re-running the build with the same plugin output
 * (unchanged examples + unchanged plugin behaviour) produces the
 * same version string and therefore the same `prerendered.json`
 * file bytes — so the file isn't "dirty" against git just from
 * running a build. A real change to the SVG output anywhere flips
 * the version and invalidates user-side localStorage entries.
 *
 * Stable serialisation: sorted hash keys so insertion order doesn't
 * leak into the version.
 */
function versionFromEntries(entries: Record<string, Pair>): string {
  const sortedHashes = Object.keys(entries).sort();
  const stable = sortedHashes
    .map(h => `${h}:${entries[h].code_panel.length}:${entries[h].timeline_panel.length}:${sha256Hex(entries[h].code_panel)}:${sha256Hex(entries[h].timeline_panel)}`)
    .join('\n');
  return sha256Hex(stable).slice(0, 16);
}

/**
 * Render one snippet through the locally-installed `rustviz` CLI and
 * read back its two SVG panels.
 *
 * `rustviz svg foo.rs -o DIR` writes `foo.code.svg` + `foo.timeline.svg`
 * into DIR. We use a fresh tempdir per example so concurrent runs
 * (none today, but defensive) can't collide and so a previous run's
 * `output.log` from the plugin doesn't leak into the next.
 */
function renderOne(code: string, label: string): Pair {
  const dir = mkdtempSync(join(tmpdir(), 'rv-prerender-'));
  try {
    const srcPath = join(dir, 'snippet.rs');
    writeFileSync(srcPath, code);
    try {
      // Capture both streams so we can suppress the CLI's normal
      // "wrote foo.code.svg" chatter (it prints to stderr) but still
      // surface real plugin panics by replaying the captured stderr
      // when the exit code is non-zero.
      execFileSync('rustviz', ['svg', srcPath, '-o', dir], {
        stdio: ['ignore', 'pipe', 'pipe'],
      });
    } catch (e) {
      const stderr = (e as { stderr?: Buffer }).stderr?.toString('utf-8') ?? '';
      const stdout = (e as { stdout?: Buffer }).stdout?.toString('utf-8') ?? '';
      console.error(`\nrustviz CLI failed on example "${label}":`);
      if (stdout) console.error(stdout);
      if (stderr) console.error(stderr);
      throw new Error(
        `rustviz CLI failed on example "${label}". ` +
          `Fix the example or the plugin before continuing — failed ` +
          `renders are fatal here on purpose.`,
      );
    }
    const code_panel = readFileSync(join(dir, 'snippet.code.svg'), 'utf-8');
    const timeline_panel = readFileSync(
      join(dir, 'snippet.timeline.svg'),
      'utf-8',
    );
    return { code_panel, timeline_panel };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/**
 * Reuse path: if `prerendered.json` already covers every current
 * example's hash, skip the per-snippet rustviz invocation entirely.
 *
 * Two reasons this matters:
 *
 *   1. Repeat builds with no example changes complete in <100 ms
 *      instead of ~9 s — useful for `npm run build` in tight loops.
 *   2. Build environments that don't have the rustviz CLI installed
 *      can still pass through the prebuild step as long as a
 *      complete-enough `prerendered.json` is already on disk. This
 *      is what the Fly Dockerfile relies on if its layer cache hits
 *      the prerender stage but not the SPA stage.
 *
 * Returns the inherited entries when all hashes are present, or
 * null if any are missing (in which case main() falls back to the
 * full render path).
 */
function tryReuseExisting(): Record<string, Pair> | null {
  if (!existsSync(OUTPUT_PATH)) return null;
  let parsed: { entries?: Record<string, Pair> };
  try {
    parsed = JSON.parse(readFileSync(OUTPUT_PATH, 'utf-8'));
  } catch {
    return null;
  }
  if (!parsed.entries || typeof parsed.entries !== 'object') return null;

  const reused: Record<string, Pair> = {};
  for (const group of exampleGroups) {
    for (const ex of group.examples) {
      const hash = sha256Hex(ex.code);
      const existing = parsed.entries[hash];
      if (
        !existing ||
        typeof existing.code_panel !== 'string' ||
        typeof existing.timeline_panel !== 'string'
      ) {
        return null;
      }
      reused[hash] = existing;
    }
  }
  return reused;
}

function main(): void {
  const start = Date.now();

  const reused = tryReuseExisting();
  if (reused) {
    console.log(
      `prerender: ${Object.keys(reused).length} snippets all covered by existing JSON; skipping (${Date.now() - start} ms)`,
    );
    return;
  }

  const entries: Record<string, Pair> = {};
  let total = 0;
  let collisions = 0;
  for (const group of exampleGroups) {
    for (const ex of group.examples) {
      total += 1;
      const hash = sha256Hex(ex.code);
      const label = `${group.chapter} / ${ex.name}`;
      if (entries[hash]) {
        // Two curated examples with byte-identical code is a sign of
        // a copy-paste mistake in the dropdown — the second one's
        // entry is a no-op cache-wise. Surface it as a build warning
        // but don't fail; not enforceable cleanly given the dropdown
        // is human-curated.
        console.warn(`prerender: duplicate snippet for ${label} (hash ${hash.slice(0, 8)}…)`);
        collisions += 1;
        continue;
      }
      process.stdout.write(`prerender: ${label.padEnd(50)} `);
      const t0 = Date.now();
      entries[hash] = renderOne(ex.code, label);
      console.log(`${Date.now() - t0} ms`);
    }
  }

  const out = { version: versionFromEntries(entries), entries };
  const serialized = JSON.stringify(out);

  // Skip the write if the contents are byte-identical to what's
  // already on disk. Keeps the working tree clean across no-op
  // builds (and across `npm run build` re-runs that share the same
  // plugin behaviour). The real signal a dev wants is "the file
  // changed because some example's rendering changed" — that still
  // shows up.
  if (existsSync(OUTPUT_PATH)) {
    const existing = readFileSync(OUTPUT_PATH, 'utf-8').trimEnd();
    if (existing === serialized) {
      const totalMs = Date.now() - start;
      console.log(
        `prerender: ${total} snippets, ${collisions} dupes, no content change, ${totalMs} ms total`,
      );
      return;
    }
  }

  writeFileSync(OUTPUT_PATH, serialized);
  const totalMs = Date.now() - start;
  const sizeKb = (serialized.length / 1024).toFixed(0);
  console.log(
    `prerender: ${total} snippets, ${collisions} dupes, ${sizeKb} KB, ${totalMs} ms total → ${OUTPUT_PATH}`,
  );
}

main();
