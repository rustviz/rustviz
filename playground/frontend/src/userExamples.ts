// localStorage-backed user-created example snippets, plus the
// selection model that lets the toolbar dropdown reference either a
// curated example from `examples.ts` or one of these.
//
// Two pieces live here so App.tsx stays focused on UI wiring:
//   * `UserExample` + storage helpers — the persisted shape and
//     read/write functions, defensive against a corrupt/foreign
//     localStorage payload.
//   * Naming helpers — `nextNewExampleName` for the `+` button and
//     `forkedName` for the implicit fork on first edit. Both pick a
//     name that doesn't collide with any existing user example so the
//     dropdown is unambiguous.

import { exampleGroups } from './examples';

const STORAGE_KEY = 'rustviz.playground.userExamples';
const SELECTED_KEY = 'rustviz.playground.selected';

export type UserExample = {
  /** Stable id so renames don't break selection refs. */
  id: string;
  /** Label shown in the dropdown and editable via the rename button. */
  name: string;
  /** Source as last seen in the editor. Updated on every keystroke. */
  code: string;
};

/**
 * What's currently loaded in the editor. `preloaded` references the
 * static `exampleGroups` by indices; `user` references a row of the
 * persisted `UserExample[]` by id.
 */
export type Selection =
  | { kind: 'preloaded'; chapter: number; index: number }
  | { kind: 'user'; id: string };

export const DEFAULT_SELECTION: Selection = {
  kind: 'preloaded',
  chapter: 0,
  index: 0,
};

/** Code template seeded into the editor by the `+` button. */
export const NEW_EXAMPLE_TEMPLATE = `fn main() {

}
`;

export function loadUserExamples(): UserExample[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    // Filter shape defensively — a future schema change or hand-edited
    // localStorage shouldn't blow up the app.
    return parsed.filter(
      (e): e is UserExample =>
        typeof (e as UserExample)?.id === 'string' &&
        typeof (e as UserExample)?.name === 'string' &&
        typeof (e as UserExample)?.code === 'string',
    );
  } catch {
    return [];
  }
}

export function saveUserExamples(examples: UserExample[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(examples));
  } catch {
    // Quota exceeded / storage unavailable. Silently degrade — the
    // user's edits stay in the editor for this session, just not across
    // refreshes. We could surface a warning, but losing one example to
    // a quota cap shouldn't pop a modal.
  }
}

/**
 * Read the persisted selection and validate it's still pointing at
 * something that exists. A stale reference (preloaded index out of
 * range after we trim the curated list, or user id whose example was
 * cleared from localStorage) falls back to the default.
 */
export function loadSelection(userExamples: UserExample[]): Selection {
  try {
    const raw = localStorage.getItem(SELECTED_KEY);
    if (!raw) return DEFAULT_SELECTION;
    const parsed = JSON.parse(raw) as Selection;
    if (
      parsed?.kind === 'preloaded' &&
      typeof parsed.chapter === 'number' &&
      typeof parsed.index === 'number'
    ) {
      const grp = exampleGroups[parsed.chapter];
      if (grp && grp.examples[parsed.index]) return parsed;
    }
    if (parsed?.kind === 'user' && typeof parsed.id === 'string') {
      if (userExamples.some(e => e.id === parsed.id)) return parsed;
    }
  } catch {
    // fall through
  }
  return DEFAULT_SELECTION;
}

export function saveSelection(sel: Selection) {
  try {
    localStorage.setItem(SELECTED_KEY, JSON.stringify(sel));
  } catch {
    // see saveUserExamples
  }
}

/**
 * Resolve the code that belongs to a selection. Caller passes the
 * current `userExamples` so we don't re-read storage on every lookup.
 * Falls back to the default preloaded example if a user example was
 * deleted out from under the selection (defensive — the UI normally
 * keeps these consistent).
 */
export function codeForSelection(sel: Selection, userExamples: UserExample[]): string {
  if (sel.kind === 'user') {
    const found = userExamples.find(e => e.id === sel.id);
    if (found) return found.code;
    // Stale selection — use the default preloaded.
    return exampleGroups[0].examples[0].code;
  }
  return exampleGroups[sel.chapter].examples[sel.index].code;
}

/** Browser-supplied UUIDs when available; random base36 otherwise. */
export function newId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return Math.random().toString(36).slice(2, 10) + Date.now().toString(36);
}

/**
 * Pick the next "New example N" label that doesn't collide with any
 * existing user-example name. Walks the existing names and finds the
 * highest N already in use, then returns N+1.
 */
export function nextNewExampleName(examples: UserExample[]): string {
  let max = 0;
  for (const e of examples) {
    const m = /^New example (\d+)$/.exec(e.name);
    if (m) {
      const n = parseInt(m[1], 10);
      if (n > max) max = n;
    }
  }
  return `New example ${max + 1}`;
}

/**
 * Pick a name for an implicit fork of `base` (the original preloaded
 * example's name). Tries `<base> (copy)` first, then `<base> (copy 2)`,
 * etc., until it finds one not already in `examples`.
 */
export function forkedName(base: string, examples: UserExample[]): string {
  const taken = new Set(examples.map(e => e.name));
  const first = `${base} (copy)`;
  if (!taken.has(first)) return first;
  for (let i = 2; ; i++) {
    const candidate = `${base} (copy ${i})`;
    if (!taken.has(candidate)) return candidate;
  }
}

/**
 * Encode a Selection into the dropdown's `<option value="...">`
 * string and back. Encoding is `kind:rest`:
 *   * `preloaded:<chapterIdx>:<exampleIdx>`
 *   * `user:<id>`
 * Strings only — keeps option values opaque to React.
 */
export function selectionToOptionValue(sel: Selection): string {
  if (sel.kind === 'user') return `user:${sel.id}`;
  return `preloaded:${sel.chapter}:${sel.index}`;
}

export function optionValueToSelection(value: string): Selection | null {
  if (value.startsWith('user:')) {
    return { kind: 'user', id: value.slice('user:'.length) };
  }
  if (value.startsWith('preloaded:')) {
    const [chapter, index] = value.slice('preloaded:'.length).split(':').map(Number);
    if (!Number.isNaN(chapter) && !Number.isNaN(index)) {
      return { kind: 'preloaded', chapter, index };
    }
  }
  return null;
}
