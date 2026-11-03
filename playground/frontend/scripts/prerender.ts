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
// gitignored (see the repo-root .gitignore) — every clone regenerates
// it via the `prebuild` script in package.json so stale entries can't
// sneak past code review. It records which plugin rendered it
// (`pluginFingerprint`), and a rebuild only reuses it when that still
// matches the installed plugin.
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
import { homedir, tmpdir } from 'node:os';
import { delimiter, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { exampleGroups } from '../src/examples';
import { normalizeCode } from '../src/normalizeCode';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const FRONTEND_ROOT = resolve(__dirname, '..');
const OUTPUT_PATH = join(FRONTEND_ROOT, 'src', 'prerendered.json');

type Pair = { code_panel: string; timeline_panel: string };

function sha256Hex(s: string): string {
  return createHash('sha256').update(s, 'utf-8').digest('hex');
}

const PATH_DIRS = (process.env.PATH ?? '').split(delimiter).filter(Boolean);
const CARGO_BIN = join(process.env.CARGO_HOME ?? join(homedir(), '.cargo'), 'bin');
/** Where `cargo rv-plugin` looks: `$CARGO_HOME/bin` first unless it's
 *  already on PATH, then PATH in order (cargo's `search_directories`). */
const CARGO_SUBCOMMAND_DIRS = PATH_DIRS.includes(CARGO_BIN) ? PATH_DIRS : [CARGO_BIN, ...PATH_DIRS];

function findExecutable(name: string, dirs: string[]): string | null {
  const exts = process.platform === 'win32' ? ['.exe', ''] : [''];
  for (const dir of dirs) {
    for (const ext of exts) {
      const p = join(dir, name + ext);
      if (existsSync(p)) return p;
    }
  }
  return null;
}

/**
 * Identity of the plugin + renderer that the SVGs come from:
 * `<plugin crate version>+<pinned toolchain>+<digest of the installed
 * rustviz / cargo-rv-plugin / rv-plugin-driver binaries>`.
 *
 * The crate version alone (2.0.0) doesn't move when plugin behaviour
 * changes, so the binary digest is what actually catches "same
 * examples, different plugin". It goes into the manifest's `version`
 * (which invalidates users' localStorage render cache) and gates
 * `tryReuseExisting`. A spurious change (e.g. a byte-different rebuild
 * of the same source) only costs one re-render / cache flush.
 */
function pluginFingerprint(): string {
  const read = (rel: string, re: RegExp) => {
    try {
      return re.exec(readFileSync(resolve(FRONTEND_ROOT, rel), 'utf-8'))?.[1] ?? 'unknown';
    } catch {
      return 'unknown';
    }
  };
  const crateVersion = read('../../rustviz-plugin/Cargo.toml', /^version\s*=\s*"([^"]+)"/m);
  const toolchain = read('../../rust-toolchain.toml', /channel\s*=\s*"([^"]+)"/);
  const digest = createHash('sha256');
  const cargoPlugin = findExecutable('cargo-rv-plugin', CARGO_SUBCOMMAND_DIRS);
  // rustc_plugin looks for the driver next to cargo-rv-plugin.
  const driver = findExecutable('rv-plugin-driver', [
    ...(cargoPlugin ? [dirname(cargoPlugin)] : []),
    ...CARGO_SUBCOMMAND_DIRS,
  ]);
  for (const [name, path] of [
    ['rustviz', findExecutable('rustviz', PATH_DIRS)],
    ['cargo-rv-plugin', cargoPlugin],
    ['rv-plugin-driver', driver],
  ] as const) {
    digest.update(`${name}\0`);
    digest.update(path ? readFileSync(path) : 'missing');
    digest.update('\0');
  }
  return `${crateVersion}+${toolchain}+${digest.digest('hex').slice(0, 16)}`;
}

/**
 * Content-addressable version: a hash of the rendered entry set and
 * of the plugin fingerprint.
 *
 * Re-running the build with the same examples and the same installed
 * plugin produces the same version string (and the same file bytes,
 * so the write below is skipped). A change to the SVG output anywhere,
 * or to the plugin even if no curated rendering changed, flips the
 * version and invalidates user-side localStorage entries, which were
 * rendered by the compile API's (equally updated) plugin.
 *
 * Stable serialisation: sorted hash keys so insertion order doesn't
 * leak into the version.
 */
function versionFromEntries(entries: Record<string, Pair>, plugin: string): string {
  const sortedHashes = Object.keys(entries).sort();
  const stable = sortedHashes
    .map(h => `${h}:${entries[h].code_panel.length}:${entries[h].timeline_panel.length}:${sha256Hex(entries[h].code_panel)}:${sha256Hex(entries[h].timeline_panel)}`)
    .concat(`plugin:${plugin}`)
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
 * Reuse path: if `prerendered.json` was rendered by the same plugin
 * (same `pluginFingerprint`) and already covers every current
 * example's hash, skip the per-snippet rustviz invocation entirely,
 * so repeat builds with no example or plugin changes complete in
 * <100 ms instead of ~9 s.
 *
 * Returns the inherited entries when reusable, or null (in which
 * case main() falls back to the full render path).
 */
function tryReuseExisting(plugin: string): Record<string, Pair> | null {
  if (!existsSync(OUTPUT_PATH)) return null;
  let parsed: { plugin?: string; entries?: Record<string, Pair> };
  try {
    parsed = JSON.parse(readFileSync(OUTPUT_PATH, 'utf-8'));
  } catch {
    return null;
  }
  // A file from an older plugin (or one written before the
  // fingerprint existed) would ship stale dropdown SVGs.
  if (parsed.plugin !== plugin) return null;
  if (!parsed.entries || typeof parsed.entries !== 'object') return null;

  const reused: Record<string, Pair> = {};
  for (const group of exampleGroups) {
    for (const ex of group.examples) {
      // Match the runtime cache: hash the normalized source so a
      // dropdown click and a paste-with-trailing-spaces collapse to
      // the same key.
      const hash = sha256Hex(normalizeCode(ex.code));
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

  const plugin = pluginFingerprint();
  const reused = tryReuseExisting(plugin);
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
      const normalized = normalizeCode(ex.code);
      const hash = sha256Hex(normalized);
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
      // Render the normalized form so the SVG that comes back
      // corresponds to exactly what `rustviz svg` would emit for the
      // user's submission (with their trailing whitespace stripped).
      entries[hash] = renderOne(normalized, label);
      console.log(`${Date.now() - t0} ms`);
    }
  }

  const out = { version: versionFromEntries(entries, plugin), plugin, entries };
  const serialized = JSON.stringify(out);

  // Skip the write if the contents are byte-identical to what's
  // already on disk, so a no-op rebuild doesn't touch the file's
  // mtime (and retrigger Vite's watcher in dev).
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
