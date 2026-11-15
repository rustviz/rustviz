// Colour theme for the whole page: mdBook's five themes, so the playground
// looks like the tutorial a student came from.
//
// The theme is `data-rv-theme` on <html>. index.css keys the page's colours
// on it, and the two inline SVG panels pick their palette from it too (the
// plugin's stylesheet declares one per theme under the same attribute; see
// rustviz-plugin/src/svg_generator/svg_frontend/themes.rs). index.html sets
// it before first paint from the same storage key, so keep the two in sync.
//
// The default is Navy, mdBook's default dark theme (`preferred-dark-theme`):
// what the tutorial shows a reader whose system is in dark mode, and the
// dark look the RustViz-in-mdBook prototype was judged in. Coal is the
// other dark option, a neutral grey instead of navy's blue-black.

export type ThemeId = 'light' | 'rust' | 'coal' | 'navy' | 'ayu';

export const THEMES: { id: ThemeId; label: string; dark: boolean }[] = [
  { id: 'light', label: 'Light', dark: false },
  { id: 'rust', label: 'Rust', dark: false },
  { id: 'coal', label: 'Coal', dark: true },
  { id: 'navy', label: 'Navy', dark: true },
  { id: 'ayu', label: 'Ayu', dark: true },
];

export const DEFAULT_THEME: ThemeId = 'navy';

const STORAGE_KEY = 'rustviz.theme';

function isTheme(t: unknown): t is ThemeId {
  return THEMES.some(x => x.id === t);
}

export function isDark(t: ThemeId): boolean {
  return THEMES.find(x => x.id === t)?.dark ?? true;
}

/** The viewer's saved theme, else the default. Storage may be missing,
 *  disabled or throw (private windows, blocked site data). */
export function loadTheme(): ThemeId {
  try {
    const t = window.localStorage.getItem(STORAGE_KEY);
    if (isTheme(t)) return t;
  } catch {
    // no storage: use the default
  }
  return DEFAULT_THEME;
}

export function saveTheme(t: ThemeId): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, t);
  } catch {
    // not remembered across visits, but the page still switches
  }
}

export function applyTheme(t: ThemeId): void {
  const root = document.documentElement;
  root.setAttribute('data-rv-theme', t);
  // native controls (select popups, scrollbars) follow too
  root.style.colorScheme = isDark(t) ? 'dark' : 'light';
}
