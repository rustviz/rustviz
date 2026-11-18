// Playground side of RustViz's usage logging (rv-telemetry.js). OFF unless
// the build sets VITE_RV_TELEMETRY_ENDPOINT: then index.tsx loads
// /ex-assets/rv-telemetry.js with the config below, and these helpers
// report playground events through it. Without the variable nothing is
// loaded, no header is added to /submit-code, and every helper here is a
// no-op (the pages workflow doesn't set it).
//
// Build-time knobs (Vite env, e.g. in .env.pages or the shell):
//   VITE_RV_TELEMETRY_ENDPOINT      collector origin (unset = off)
//   VITE_RV_TELEMETRY_CONSENT       'notice' (default here) | 'opt-in'
//   VITE_RV_TELEMETRY_COLLECT_CODE  'true' (default here) | 'false'
//   VITE_RV_TELEMETRY_PRIVACY_URL   default '/privacy/'
// The same build also writes privacy/telemetry-config.json (endpoint and
// consent mode; see vite.config.ts), which the privacy page's "Your data"
// section reads to turn recording off or on.
// The defaults are the maintainer's choice for the public playground
// (2026-11-17); rv-telemetry.js's own defaults are the safest ones
// (opt-in, no code text).

type RvTelemetry = {
  isActive(): boolean;
  emit(name: string, props: Record<string, unknown>): unknown;
  ui(action: string, value?: string, sourceFigureId?: string): unknown;
  program(source: string, meta?: { source_figure_id?: string }): string | null;
  programId(source: string): string;
  programOmitted(programId: string): boolean;
  figureId(source: string): string;
  scan(): void;
  flush(): void;
  pageLoadId(): string | null;
};

declare global {
  interface Window {
    rvTelemetry?: RvTelemetry;
    RV_TELEMETRY_CONFIG?: Record<string, unknown>;
  }
}

const env: Record<string, string | undefined> = import.meta.env ?? {};
const ENDPOINT: string = env.VITE_RV_TELEMETRY_ENDPOINT ?? '';

/** True when this build asks before recording (VITE_RV_TELEMETRY_CONSENT=
 *  opt-in). The default, notice mode, draws no banner at all: the Privacy
 *  paragraph in the info panel is the notice. Only opt-in needs a prompt,
 *  so only then does the page reserve a slot for it. */
export const TELEMETRY_OPT_IN = env.VITE_RV_TELEMETRY_CONSENT === 'opt-in';

/** True when this build was configured with a telemetry endpoint. */
export const TELEMETRY_ENABLED = ENDPOINT !== '';

/** Request header carrying the page-load id to /submit-code, so the
 *  compile service's run_result log line can be joined with the client's. */
export const PAGE_LOAD_HEADER = 'X-RV-Page-Load';

/** Load rv-telemetry.js (once) if this build has an endpoint. Resolves
 *  when it has loaded (or failed, or after 3 s), so the first render can
 *  already name its figure; resolves at once when telemetry is off. */
export function loadTelemetry(): Promise<void> {
  if (!TELEMETRY_ENABLED || window.RV_TELEMETRY_CONFIG) return Promise.resolve();
  window.RV_TELEMETRY_CONFIG = {
    endpoint: ENDPOINT,
    consent: TELEMETRY_OPT_IN ? 'opt-in' : 'notice',
    collectCode: env.VITE_RV_TELEMETRY_COLLECT_CODE !== 'false',
    surface: 'playground',
    rvVersion: __PLUGIN_VERSION__,
    privacyUrl: env.VITE_RV_TELEMETRY_PRIVACY_URL ?? '/privacy/',
    footLink: 'none',
    // RustViz never sells or shares the data: only Turn off stops logging.
    respectPrivacySignals: false,
    ...(TELEMETRY_OPT_IN ? { bannerSlot: '#rv-telemetry-slot' } : {}),
  };
  return new Promise(resolve => {
    const s = document.createElement('script');
    s.src = '/ex-assets/rv-telemetry.js';
    s.async = true;
    s.onload = s.onerror = () => resolve();
    setTimeout(resolve, 3000);
    document.head.appendChild(s);
  });
}

function tlm(): RvTelemetry | null {
  const t = TELEMETRY_ENABLED ? window.rvTelemetry : undefined;
  return t && t.isActive() ? t : null;
}

/** figure_id of a program as rendered by this playground build. */
export function figureIdFor(code: string): string | undefined {
  return TELEMETRY_ENABLED && window.rvTelemetry ? window.rvTelemetry.figureId(code) : undefined;
}

export function uiEvent(action: string, value?: string, sourceFigureId?: string): void {
  tlm()?.ui(action, value, sourceFigureId);
}

/** Re-scan the page for figures after the visualization changed. */
export function figuresChanged(): void {
  tlm()?.scan();
}

/** Headers for /submit-code: the page-load id, only while logging is on. */
export function submitHeaders(): Record<string, string> {
  const id = tlm()?.pageLoadId();
  return id ? { [PAGE_LOAD_HEADER]: id } : {};
}

// ---------------------------------------------------------------- edits

const EDIT_EVERY_MS = 10_000;
let pendingChars = 0;
let lastEditEvent = 0;
let lastRunTs = 0;
let editTimer: ReturnType<typeof setTimeout> | null = null;
let latestCode = '';
let sourceCode: string | null = null;

/** The program the editor's text came from (a dropdown example or an
 *  "Open in Playground" link), or null; edits and runs name its figure. */
export function setSourceCode(code: string | null): void {
  sourceCode = code;
}
function sourceFigure(): string | undefined {
  return sourceCode === null ? undefined : figureIdFor(sourceCode);
}

function sendEdit(): void {
  if (editTimer) { clearTimeout(editTimer); editTimer = null; }
  const t = tlm();
  if (!t || pendingChars === 0) { pendingChars = 0; return; }
  const props: Record<string, unknown> = { chars_changed: Math.min(pendingChars, 1_000_000) };
  if (lastRunTs) props.ms_since_last_run = Math.min(Date.now() - lastRunTs, 86_400_000);
  // With code collection on, the text at this moment as a program record
  // (sent once per distinct text) that the edit names.
  const pid = t.program(latestCode, { source_figure_id: sourceFigure() });
  if (pid && collectsCode()) {
    props.program_id = pid;
    if (t.programOmitted(pid)) props.program_omitted = 'too_large';
  }
  t.emit('edit', props);
  pendingChars = 0;
  lastEditEvent = Date.now();
}

function collectsCode(): boolean {
  const c = window.RV_TELEMETRY_CONFIG;
  return !!c && c.collectCode === true;
}

/** A user edit of `chars` characters (inserted + deleted). Summarised in
 *  at most one `edit` event per 10 s. */
export function noteEdit(code: string, chars: number): void {
  if (!tlm()) return;
  latestCode = code;
  pendingChars += Math.max(chars, 0);
  const wait = lastEditEvent + EDIT_EVERY_MS - Date.now();
  if (wait <= 0) sendEdit();
  else if (!editTimer) editTimer = setTimeout(sendEdit, wait);
}

// ----------------------------------------------------------------- runs

export type RunHandle = { runId: string; start: number; programId: string; figureId: string } | null;

function uuid(): string {
  return crypto.randomUUID ? crypto.randomUUID()
    : 'xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx'.replace(/x/g, () => (Math.random() * 16 | 0).toString(16));
}

/** A run of `code` (Generate, or picking an example). */
export function runStarted(code: string, example: string | undefined): RunHandle {
  const t = tlm();
  if (!t) return null;
  if (pendingChars) sendEdit();
  const runId = uuid();
  // the program's text goes out once, as a `program` record, only with
  // collectCode; the run names it by hash either way
  const programId = t.program(code, { source_figure_id: sourceFigure() }) ?? t.programId(code);
  const figureId = t.figureId(code);
  const props: Record<string, unknown> = {
    run_id: runId,
    program_id: programId,
    figure_id: figureId,
    line_count: Math.min(code.split('\n').length, 100_000),
  };
  // over the collector's 64 KiB source cap: the text wasn't sent
  if (t.programOmitted(programId)) props.program_omitted = 'too_large';
  if (example) props.example = example.slice(0, 64);
  // the figure it was edited from (an unedited example is its own figure)
  const src = sourceFigure();
  if (src && src !== figureId) props.source_figure_id = src;
  t.emit('run', props);
  lastRunTs = Date.now();
  return { runId, start: performance.now(), programId, figureId };
}

export type RunOutcome = {
  status?: number;           // HTTP status, absent for cache hits / network errors
  cached?: boolean;
  errorKind?: string | null; // X-RustViz-Error-Kind
  message?: string;          // the error body (only codes and locations are extracted)
  network?: boolean;
};

/** rustc error codes in a diagnostic (`error[E0382]`), deduplicated. */
export function errorCodes(message: string): string[] {
  const out: string[] = [];
  for (const m of message.matchAll(/error\[(E\d{4})\]/g)) {
    if (!out.includes(m[1])) out.push(m[1]);
    if (out.length >= 32) break;
  }
  return out;
}

/** `expr_visitor.rs:862` from "... (internal error at expr_visitor.rs:862: ...)". */
export function crashLocation(message: string): string | null {
  const m = /\(internal error at ([A-Za-z0-9_.-]+\.rs:\d+)/.exec(message);
  return m ? m[1] : null;
}

export function classifyOutcome(o: RunOutcome): string {
  if (o.cached) return 'ok';
  if (o.network || o.status === undefined) return 'network_error';
  if (o.status >= 200 && o.status < 300) return 'ok';
  if (o.status === 429) return 'rate_limited';
  if (o.errorKind === 'does-not-compile') return 'compile_error';
  if (o.errorKind === 'unsupported') return crashLocation(o.message ?? '') ? 'crash' : 'refused';
  if (o.errorKind === 'timeout' || o.status === 504) return 'timeout';
  return 'server_error';
}

export function runFinished(h: RunHandle, o: RunOutcome): void {
  const t = tlm();
  if (!t || !h) return;
  const outcome = classifyOutcome(o);
  const props: Record<string, unknown> = {
    run_id: h.runId,
    program_id: h.programId,
    figure_id: h.figureId,
    outcome,
    latency_ms: Math.max(0, Math.round(performance.now() - h.start)),
    cached: !!o.cached,
  };
  if (o.status !== undefined) props.http_status = o.status;
  if (o.errorKind && /^[a-z0-9-]{1,64}$/.test(o.errorKind)) props.error_kind = o.errorKind;
  if (outcome === 'compile_error') {
    const codes = errorCodes(o.message ?? '');
    if (codes.length) props.error_codes = codes;
  }
  if (outcome === 'crash') props.crash_at = crashLocation(o.message ?? '');
  t.emit('run_result', props);
}
