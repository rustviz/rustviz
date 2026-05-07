import React, { useEffect, useRef, useState } from 'react';
import './index.css';
import { extensions } from './setup';
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import axios from 'axios';
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';
import ErrorCard from './ErrorCard';
import { exampleGroups } from './examples';
import {
  codeForSelection,
  DEFAULT_SELECTION,
  exampleFilename,
  forkedName,
  importedName,
  loadSelection,
  loadUserExamples,
  newId,
  nextNewExampleName,
  NEW_EXAMPLE_TEMPLATE,
  optionValueToSelection,
  saveSelection,
  saveUserExamples,
  selectionToOptionValue,
  type Selection,
  type UserExample,
} from './userExamples';

// API origin. Empty (relative URL) for the default same-origin Fly deploy;
// set to https://rustviz-playground.fly.dev for the GitHub Pages build via
// .env.pages so the SPA hosted on rustviz.github.io can hit the API on Fly.
// rv-serve's CORS allowlist must include the SPA's origin in the latter case.
const API_BASE: string = import.meta.env.VITE_API_BASE ?? '';

declare function helpers(param: string): void;

// Viewport width threshold (px) below which the layout is considered
// "narrow" — phones in portrait, narrow split windows, etc. Drives
// the initial description-panel size so a first-time mobile visitor
// doesn't see prose eat the editor's screen real estate. After the
// initial render the panel is fully resizable like always.
const NARROW_VIEWPORT_PX = 768;
const initialNarrow =
  typeof window !== 'undefined' &&
  typeof window.matchMedia === 'function' &&
  window.matchMedia(`(max-width: ${NARROW_VIEWPORT_PX}px)`).matches;

// Thin wrapper around CodeMirror's EditorView so the React layer can
// hold a single `Editor` instance across renders without re-creating
// the underlying view (which would lose cursor position, undo
// history, etc.). Owns its own DOM insertion via the constructor's
// `parent` arg.
//
// `onUserChange` fires after every user-driven document change (typing,
// paste, delete) but is suppressed during programmatic `setCurrentCode`
// — the dropdown loading a different example shouldn't look like the
// user editing the current one (which would trigger an implicit fork).
class Editor {
  private view: EditorView;
  // True for the duration of one programmatic `setCurrentCode` call.
  // CodeMirror dispatches transactions synchronously, so we set, dispatch,
  // and clear within the same call frame — the change listener fires
  // in between and sees the flag set.
  private programmatic = false;

  public constructor(
    editorContainer: HTMLElement,
    code: string,
    onUserChange: (code: string) => void,
  ) {
    const updateListener = EditorView.updateListener.of(update => {
      if (!update.docChanged) return;
      if (this.programmatic) return;
      onUserChange(update.state.doc.toString());
    });
    const initial_state = EditorState.create({
      doc: code,
      extensions: [...extensions, updateListener],
    });
    this.view = new EditorView({ state: initial_state, parent: editorContainer });
  }

  public getCurrentCode(): string {
    return this.view.state.doc.toString();
  }

  // Replace the entire editor contents in one transaction. The
  // `programmatic` flag prevents the change listener from interpreting
  // this as user input (which would otherwise look like the user editing
  // a preloaded example, triggering an unwanted implicit fork).
  public setCurrentCode(code: string): void {
    if (code === this.view.state.doc.toString()) return;
    this.programmatic = true;
    try {
      this.view.dispatch({
        changes: { from: 0, to: this.view.state.doc.length, insert: code },
      });
    } finally {
      this.programmatic = false;
    }
  }

  public destroy(): void {
    this.view.destroy();
  }
}

// Top-of-page bar. Title on the left, the rustc / plugin version
// stamp on the right (kept here instead of below the editor so it
// reads as masthead metadata rather than competing with the editor /
// visualization for attention).
//
// At narrow widths the tutorial callout and the version codes don't
// fit alongside the title and stars badge — instead of letting them
// overlap (or hiding them outright) they move into a `⋯` overflow
// popover. The popover items are duplicated in the JSX so CSS can
// flip which set is visible without React re-rendering.
const TitleBar: React.FC = () => {
  const detailsRef = useRef<HTMLDetailsElement | null>(null);

  // <details> doesn't close on outside click on its own — wire it up
  // so a tap elsewhere dismisses the popover, matching the platform
  // expectation for a transient menu.
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (detailsRef.current?.open && !detailsRef.current.contains(e.target as Node)) {
        detailsRef.current.open = false;
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  return (
    <header className="titlebar">
      <div className="titlebar-left">
        <h1 className="titlebar-title">
          <a href="https://github.com/rustviz/rustviz/" target="_blank" rel="noreferrer">
            RustViz
          </a>{' '}
          Playground
        </h1>
        <a
          className="titlebar-callout"
          href="https://rustviz.github.io/tutorial/"
          target="_blank"
          rel="noreferrer"
          title="Open the RustViz tutorial in a new tab"
        >
          New to Rust? Read our visual tutorial →
        </a>
      </div>
      <div className="titlebar-meta">
        <code className="titlebar-version">rustc {__RUST_VERSION__}</code>
        <code className="titlebar-version">rustviz-plugin {__PLUGIN_VERSION__}</code>
        {/* Overflow menu — hidden by CSS at wide widths, shown when
            the inline callout / version codes are hidden. The popover
            re-renders the same content so nothing actually disappears,
            it just moves. */}
        <details ref={detailsRef} className="titlebar-overflow">
          <summary
            className="titlebar-overflow-trigger"
            aria-label="More"
            title="More"
          >
            ⋯
          </summary>
          <div className="titlebar-overflow-popover" role="menu">
            {/* Each popover item carries an `…-overflow-target` class
                tied to a specific breakpoint. CSS shows the popover
                item ONLY at widths where its inline counterpart in
                the title bar is hidden — so at 700px (versions
                hidden, callout still inline) the popover shows just
                the versions, and at 480px both. No duplicate UI. */}
            <a
              className="titlebar-overflow-item titlebar-overflow-callout"
              href="https://rustviz.github.io/tutorial/"
              target="_blank"
              rel="noreferrer"
              role="menuitem"
            >
              Read our visual tutorial →
            </a>
            <div
              className="titlebar-overflow-item titlebar-overflow-static titlebar-overflow-version"
              role="none"
            >
              rustc {__RUST_VERSION__}
            </div>
            <div
              className="titlebar-overflow-item titlebar-overflow-static titlebar-overflow-version"
              role="none"
            >
              rustviz-plugin {__PLUGIN_VERSION__}
            </div>
          </div>
        </details>
        {/* shields.io social-style badge updates dynamically; no JS
            fetch needed and the SVG inlines the current star count.
            Rightmost so it's the last thing the eye lands on. */}
        <a
          className="titlebar-stars"
          href="https://github.com/rustviz/rustviz"
          target="_blank"
          rel="noreferrer"
          title="Star us on GitHub"
        >
          <img
            src="https://img.shields.io/github/stars/rustviz/rustviz?style=social&label=Star"
            alt="GitHub stars"
            height={20}
          />
        </a>
      </div>
    </header>
  );
};

// Static prose pane. Same content the index.html used to carry, just
// moved into the React tree so the resizable layout owns every
// region of the page.
const Description: React.FC = () => (
  <div className="description">
    <p>
      Welcome!{' '}
      <a href="https://github.com/rustviz/rustviz/" target="_blank" rel="noreferrer">
        RustViz
      </a>{' '}
      is a tool to visualize an approximation of the compile-time reasoning of{' '}
      <a href="https://www.rust-lang.org/" target="_blank" rel="noreferrer">
        Rust's
      </a>{' '}
      borrow checker.
    </p>
    <p>
      Try it out by writing a Rust program in the editor on the right, then clicking
      the <strong>Generate Visualization</strong> button. The diagram appears in the
      bottom panel.
    </p>
    <h3>Annotation markers</h3>
    <p>You can keep individual items out of the visualization with comment markers:</p>
    <ul>
      <li>
        <code>// rustviz: skip</code> on a <code>let</code> or <code>fn</code> line —
        the item stays in the source but is omitted from the trace.
      </li>
      <li>
        <code>// rustviz: hide</code> on a <code>fn</code> line — additionally
        removes the entire fn from the rendered code panel; call sites still draw
        their arrows.
      </li>
    </ul>
    <h3>Known limitations</h3>
    <p>RustViz is a research tool. It supports a meaningful subset of Rust but not all of it — these features are unsupported or known to misbehave:</p>
    <ul>
      <li>For-loops</li>
      <li>
        Bindings or borrows inside an <code>if</code> or <code>match</code>{' '}
        branch body (the conditional itself can return a value into a{' '}
        <code>let</code>, but tracking events inside the branch isn't supported)
      </li>
      <li>
        Smart-pointer wrappers (<code>Box</code>, <code>Rc</code>,{' '}
        <code>Arc</code>, <code>RefCell</code>) and trait objects (
        <code>Box&lt;dyn T&gt;</code>)
      </li>
      <li>
        Indexing or slicing collections like <code>Vec</code> (string slices
        like <code>&amp;s[..]</code> on a <code>String</code> do work)
      </li>
      <li>
        The <code>?</code> operator (and other desugaring-heavy forms like{' '}
        <code>async</code>/<code>await</code>)
      </li>
      <li>
        Some struct field access patterns: chaining a method onto a field (
        <code>r.field.method()</code>), nested field access (<code>r.a.b</code>),
        and field access through a reference (<code>(&amp;r).field</code>).
        Plain <code>r.field</code> and <code>&amp;r.field</code> work.
      </li>
      <li>
        Inherent methods (<code>impl S {'{ fn ... }'}</code>) are fragile —
        the Rectangle/area pattern (<code>fn area(&amp;self) -&gt; u32 {'{'} self.width * self.height {'}'}</code>)
        works, but minor variants (e.g. a one-field <code>fn get(&amp;self) -&gt; i32 {'{'} self.n {'}'}</code>)
        crash
      </li>
    </ul>
    <p>
      If you find an example that exposes a bug, please open an issue on the{' '}
      <a href="https://github.com/rustviz/rustviz/issues" target="_blank" rel="noreferrer">
        repo
      </a>{' '}
      with the snippet and error message.
    </p>
    <h3>Presentation mode</h3>
    <p>
      The three panels are independently resizable, so you can stage a clean
      presentation view by:
    </p>
    <ul>
      <li>
        Dragging the vertical handle to the left edge to hide this info panel.
      </li>
      <li>
        Dragging the horizontal handle up to hide the editor entirely, or
        most of the way up to leave just the example-picker toolbar visible
        for switching snippets live.
      </li>
    </ul>
    <h3>Other ways to use RustViz</h3>
    <p>Besides this playground, RustViz ships as:</p>
    <ul>
      <li>
        an mdBook preprocessor — for embedding visualizations in a
        book, like our{' '}
        <a href="https://rustviz.github.io/tutorial/" target="_blank" rel="noreferrer">
          visual Rust tutorial
        </a>
      </li>
      <li>
        a <code>rustviz</code> command-line tool — one-shot rendering
        of a <code>.rs</code> file to SVG or self-contained HTML
      </li>
      <li>a Rust library — for programmatic use</li>
    </ul>
    <p>
      Setup and usage for all of them are in the{' '}
      <a href="https://github.com/rustviz/rustviz" target="_blank" rel="noreferrer">
        GitHub repo
      </a>
      .
    </p>
    <h3>Credits</h3>
    <p>
      RustViz is a project of the{' '}
      <a href="https://web.eecs.umich.edu/~comar/" target="_blank" rel="noreferrer">
        Future of Programming Lab
      </a>{' '}
      at the University of Michigan. The plugin is built on{' '}
      <a href="https://github.com/cognitive-engineering-lab/rustc_plugin" target="_blank" rel="noreferrer">
        <code>rustc_plugin</code> / <code>rustc_utils</code>
      </a>{' '}
      by{' '}
      <a href="https://willcrichton.net/" target="_blank" rel="noreferrer">
        Will Crichton
      </a>
      , and ports several MIR / borrow-fact helpers from{' '}
      <a href="https://github.com/cognitive-engineering-lab/aquascope" target="_blank" rel="noreferrer">
        Aquascope
      </a>{' '}
      (another visualization tool for Rust) by{' '}
      <a href="https://gavinleroy.com/" target="_blank" rel="noreferrer">
        Gavin Gray
      </a>
      . RustViz is an independent academic project and has no formal
      affiliation with the Rust project or the Rust Foundation.
    </p>
    <h3>Research paper</h3>
    <p>
      An earlier version of RustViz used hand-written visualization
      directives in source comments and was deployed in classroom
      teaching at the University of Michigan. That version is described
      in our VL/HCC 2022 paper:
    </p>
    <p className="citation">
      Marcelo Almeida, Grant Cole, Ke Du, Gongming Luo, Shulin Pan,
      Yu Pan, Kai Qiu, Vishnu Reddy, Haochen Zhang, Yingying Zhu, and
      Cyrus Omar.{' '}
      <a
        href="https://web.eecs.umich.edu/~comar/rustviz-vlhcc22.pdf"
        target="_blank"
        rel="noreferrer"
      >
        RustViz: Interactively Visualizing Ownership and Borrowing
      </a>
      . In <em>2022 IEEE Symposium on Visual Languages and
      Human-Centric Computing (VL/HCC)</em>, pages 1–10, 2022.{' '}
      <a
        href="https://github.com/rustviz/rustviz/blob/main/rustviz.bib"
        target="_blank"
        rel="noreferrer"
      >
        BibTeX
      </a>
      .
    </p>
  </div>
);

type ExamplePickerProps = {
  selection: Selection;
  userExamples: UserExample[];
  onSelect: (sel: Selection) => void;
};

// The picker stays interactive even while a /submit-code request is
// in flight. Picking a fresh example mid-load aborts the previous
// request (see inflightRef in App) and starts a new one — the user
// can change their mind and the wrong response can never overwrite
// the right one.
//
// Two source groups: the persisted `userExamples` at the top (only
// rendered when non-empty) and the curated `exampleGroups` below.
// Option values are encoded via `selectionToOptionValue` so the
// onChange handler can round-trip a Selection without juggling
// string parsing inline.
const ExamplePicker: React.FC<ExamplePickerProps> = ({ selection, userExamples, onSelect }) => {
  const handleChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const sel = optionValueToSelection(e.target.value);
    if (sel) onSelect(sel);
  };

  return (
    <div className="example-picker">
      <label htmlFor="example-select">Example:</label>
      <select
        id="example-select"
        value={selectionToOptionValue(selection)}
        onChange={handleChange}
      >
        {userExamples.length > 0 && (
          <optgroup label="Your examples">
            {userExamples.map(ex => (
              <option key={ex.id} value={selectionToOptionValue({ kind: 'user', id: ex.id })}>
                {ex.name}
              </option>
            ))}
          </optgroup>
        )}
        {exampleGroups.map((group, gIdx) => (
          <optgroup key={group.chapter} label={group.chapter}>
            {group.examples.map((ex, eIdx) => (
              <option
                key={ex.name}
                value={selectionToOptionValue({ kind: 'preloaded', chapter: gIdx, index: eIdx })}
              >
                {ex.name}
              </option>
            ))}
          </optgroup>
        ))}
      </select>
    </div>
  );
};

const App: React.FC = () => {
  const [isLoading, setIsLoading] = useState(false);
  const [isErr, setErr] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [code_svg, setCodeSvg] = useState<string | null>(null);
  const [timeline_svg, setTimelineSvg] = useState<string | null>(null);

  // Persisted user examples + the current selection. Both hydrated
  // from localStorage on first render (useState initializer runs once)
  // and saved back on every change via the effects below.
  const [userExamples, setUserExamples] = useState<UserExample[]>(() => loadUserExamples());
  const [selection, setSelection] = useState<Selection>(() =>
    loadSelection(loadUserExamples()),
  );

  // Refs mirror the latest state so the editor's change listener — a
  // stable closure captured at editor-mount time — can read current
  // values when deciding whether to fork or update in-place. Without
  // these refs the listener would see the initial state forever.
  const selectionRef = useRef(selection);
  const userExamplesRef = useRef(userExamples);
  useEffect(() => {
    selectionRef.current = selection;
  }, [selection]);
  useEffect(() => {
    userExamplesRef.current = userExamples;
  }, [userExamples]);

  // Persist on every change. localStorage writes are cheap; no need
  // to debounce.
  useEffect(() => {
    saveUserExamples(userExamples);
  }, [userExamples]);
  useEffect(() => {
    saveSelection(selection);
  }, [selection]);

  // Editor change listener. Two cases:
  //   * `selection.kind === 'user'`  →  update the existing user
  //     example's stored code in place (auto-save).
  //   * `selection.kind === 'preloaded'`  →  implicit fork. Create a
  //     new user example named "<original> (copy)", populated with
  //     the just-edited code, and switch the selection to it. The
  //     user's edit cursor stays where it is — we don't touch the
  //     editor view itself.
  const handleEditorChange = (code: string) => {
    const sel = selectionRef.current;
    const examples = userExamplesRef.current;
    if (sel.kind === 'user') {
      const next = examples.map(e => (e.id === sel.id ? { ...e, code } : e));
      setUserExamples(next);
      return;
    }
    const preloadedName = exampleGroups[sel.chapter].examples[sel.index].name;
    const forked: UserExample = {
      id: newId(),
      name: forkedName(preloadedName, examples),
      code,
    };
    setUserExamples([...examples, forked]);
    setSelection({ kind: 'user', id: forked.id });
  };

  // The CodeMirror editor needs a real DOM node to attach to. Create
  // it once when the host div mounts; tear it down on unmount so
  // React 18 StrictMode's dev-mode double-invocation of this effect
  // doesn't leave a second CodeMirror instance attached.
  const editorHostRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!editorHostRef.current) return;
    // Read directly from the initial state values (computed in the
    // useState initializer above). The ESLint deps rule wants
    // selection/userExamples here, but adding them would re-mount the
    // editor on every example switch and lose history. The change
    // listener below uses refs to read the live values instead.
    const initialCode = codeForSelection(selection, userExamples);
    const newEditor = new Editor(editorHostRef.current, initialCode, handleEditorChange);
    setEditor(newEditor);
    return () => {
      newEditor.destroy();
      setEditor(null);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Tracks the AbortController of the in-flight /submit-code request,
  // if any. When a fresh request starts (Generate clicked OR a new
  // example picked from the dropdown mid-loading) we abort the
  // previous one and stash this controller so any later response from
  // the now-stale request can be ignored. Without this, two rapid
  // requests could race and the LATER-resolving one (which is older,
  // because we don't control network ordering) would overwrite the
  // newer result.
  const inflightRef = useRef<AbortController | null>(null);

  const handleClick = async () => {
    if (!editor) return;

    inflightRef.current?.abort();
    const controller = new AbortController();
    inflightRef.current = controller;

    setIsLoading(true);
    const code = editor.getCurrentCode();

    try {
      const response = await axios.post(`${API_BASE}/submit-code`, { code }, {
        signal: controller.signal,
      });
      // A newer request was started while we were waiting; whoever
      // started it owns the UI now. Drop this response on the floor.
      if (inflightRef.current !== controller) return;

      if (response.status === 200) {
        setCodeSvg(response.data.code_panel);
        setTimelineSvg(response.data.timeline_panel);
        setErr(false);
      } else {
        console.error('Error:', response.statusText);
        setError(response.data);
        setErr(true);
      }
    } catch (error) {
      // We aborted this request because a newer one started; not an
      // actual error from the user's perspective.
      if (axios.isCancel(error) || controller.signal.aborted) return;
      if (inflightRef.current !== controller) return;

      if (axios.isAxiosError(error) && error.response) {
        setError(error.response.data);
      } else {
        setError('An unexpected error occurred');
      }
      console.error('An error occurred:', error);
      setErr(true);
    } finally {
      // Only the current (latest) request's resolution should toggle
      // off the loading state — older aborted requests must not
      // race-clear loading while a newer one is still running.
      if (inflightRef.current === controller) {
        setIsLoading(false);
      }
    }
  };

  // Auto-fire the visualization on the seed snippet so the bottom
  // panel never starts empty.
  useEffect(() => {
    handleClick();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editor]);

  // Re-attach hover/tooltip listeners whenever the SVG content
  // changes. Without this, switching examples while the cursor sits
  // over the viz panel leaves the new SVG nodes without listeners
  // (the wrapper div's onMouseEnter doesn't refire), and tooltips
  // silently stop working until the user moves the cursor out and
  // back in. helpers() is idempotent — it tags triggers with a
  // .listener class to skip already-bound nodes.
  useEffect(() => {
    helpers('ex2');
  }, [code_svg, timeline_svg]);

  // Switch to a different example (preloaded or user-created).
  // Setting selection + pushing the new code into the editor are both
  // programmatic — neither should trigger the implicit-fork path.
  const handleExampleSelect = (sel: Selection) => {
    setSelection(sel);
    if (!editor) return;
    editor.setCurrentCode(codeForSelection(sel, userExamples));
    // Auto-fire the visualization. CodeMirror's dispatch is
    // synchronous so getCurrentCode() inside handleClick() will see
    // the just-set code on the next line.
    handleClick();
  };

  // `+` button: create a fresh user example with the next "New
  // example N" name and seed it with an empty fn main() template.
  const handleNewExample = () => {
    const created: UserExample = {
      id: newId(),
      name: nextNewExampleName(userExamples),
      code: NEW_EXAMPLE_TEMPLATE,
    };
    setUserExamples([...userExamples, created]);
    setSelection({ kind: 'user', id: created.id });
    if (!editor) return;
    editor.setCurrentCode(NEW_EXAMPLE_TEMPLATE);
    handleClick();
  };

  // Rename the currently-selected user example. No-op (and disabled
  // in the toolbar) for preloaded examples — those are read-only.
  // Empty / whitespace-only names are rejected; duplicates aren't
  // forbidden because the underlying id stays unique.
  const handleRename = () => {
    if (selection.kind !== 'user') return;
    const current = userExamples.find(e => e.id === selection.id);
    if (!current) return;
    const input = window.prompt('Rename example:', current.name);
    if (input === null) return;
    const trimmed = input.trim();
    if (!trimmed || trimmed === current.name) return;
    setUserExamples(
      userExamples.map(e => (e.id === current.id ? { ...e, name: trimmed } : e)),
    );
  };

  // Delete the currently-selected user example. Confirms first
  // because the action is destructive and there's no undo (the
  // example's code is overwritten in localStorage). After deletion,
  // fall back to the example that was at the same index (or just
  // before, if we deleted the last one); if no user examples are
  // left, fall back to the default preloaded example.
  const handleDelete = () => {
    if (selection.kind !== 'user') return;
    const idx = userExamples.findIndex(e => e.id === selection.id);
    if (idx < 0) return;
    const current = userExamples[idx];
    if (!window.confirm(`Delete "${current.name}"? This can't be undone.`)) return;
    const next = userExamples.filter(e => e.id !== current.id);
    let fallback: Selection;
    if (next.length > 0) {
      const fallbackIdx = Math.min(idx, next.length - 1);
      fallback = { kind: 'user', id: next[fallbackIdx].id };
    } else {
      fallback = DEFAULT_SELECTION;
    }
    setUserExamples(next);
    setSelection(fallback);
    if (!editor) return;
    editor.setCurrentCode(codeForSelection(fallback, next));
    handleClick();
  };

  // Save the editor's current code to the user's disk as a `.rs`
  // file. Filename derives from the selection's name (sanitized for
  // filesystem use). Works for both preloaded and user examples —
  // for preloaded we just emit the original snippet; for user we
  // emit the latest auto-saved code.
  const handleExport = () => {
    if (!editor) return;
    const code = editor.getCurrentCode();
    const sourceName =
      selection.kind === 'user'
        ? userExamples.find(e => e.id === selection.id)?.name ?? 'example'
        : exampleGroups[selection.chapter].examples[selection.index].name;
    const blob = new Blob([code], { type: 'text/x-rust' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = exampleFilename(sourceName);
    document.body.appendChild(a);
    a.click();
    a.remove();
    // Revoke after the click handler has had a tick to start the
    // download — Safari is finicky if we revoke synchronously.
    setTimeout(() => URL.revokeObjectURL(url), 0);
  };

  // Hidden <input type="file"> backing the import button. We click
  // it programmatically from handleImport so the visible toolbar
  // button can carry our own styling/icon.
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const handleImportClick = () => {
    fileInputRef.current?.click();
  };

  // Read the chosen file as text, register it as a new user example,
  // and switch to it. Reusing the same file twice in a row works
  // because we clear the input's value before bailing out.
  const handleImportFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    const code = await file.text();
    const stem = file.name.replace(/\.rs$/i, '');
    const created: UserExample = {
      id: newId(),
      name: importedName(stem, userExamples),
      code,
    };
    setUserExamples([...userExamples, created]);
    setSelection({ kind: 'user', id: created.id });
    if (!editor) return;
    editor.setCurrentCode(code);
    handleClick();
  };

  const canRename = selection.kind === 'user';
  const canDelete = selection.kind === 'user';

  return (
    <div className="app-shell">
      <TitleBar />
      <PanelGroup direction="vertical" className="main-split" autoSaveId="rustviz-vertical">
        {/* Top row: prose on the left, code editor on the right.
            minSize=0 with no `collapsible` means the handle drags
            smoothly all the way down — no snap point. Useful for
            "presentation mode" where you drag until just the
            example-picker toolbar is visible (the editor scrolls
            out of view but the dropdown stays clickable), or all
            the way to 0 to show only the visualization. The handle
            stays grabbable at the top edge of the viz panel for
            dragging back open. */}
        <Panel defaultSize={40} minSize={0}>
          <PanelGroup direction="horizontal" autoSaveId="rustviz-top-horizontal">
            {/* Collapsible so users running pure code demos can drag
                the handle to the left edge and snap the prose pane
                away. minSize is the snap threshold — below 15% the
                panel collapses to 0; the resize handle stays visible
                at the edge so the user can drag it back to expand.
                On a narrow viewport (phones, narrow split windows)
                we start collapsed so the editor + viz get the full
                width on first load — the user can still drag to
                expand. */}
            <Panel
              defaultSize={initialNarrow ? 0 : 35}
              minSize={15}
              collapsible
              className="panel description-panel"
            >
              <Description />
            </Panel>
            <PanelResizeHandle className="resize-handle resize-handle-vertical" />
            <Panel defaultSize={65} minSize={25} className="panel editor-panel">
              <div className="editor-toolbar">
                <ExamplePicker
                  selection={selection}
                  userExamples={userExamples}
                  onSelect={handleExampleSelect}
                />
                <button
                  className="cm-button toolbar-icon-button"
                  onClick={handleNewExample}
                  title="Create a new example (saved in this browser)"
                  aria-label="Create a new example"
                >
                  +
                </button>
                <button
                  className="cm-button toolbar-icon-button"
                  onClick={handleRename}
                  disabled={!canRename}
                  title={
                    canRename
                      ? 'Rename this example'
                      : 'Rename only works on examples you created'
                  }
                  aria-label="Rename this example"
                >
                  {/* Unicode "lower right pencil" (U+270E) renders in
                      the inherited text color across platforms, the
                      same way the `+` glyph above does — no SVG fill
                      / stroke inheritance to debug. */}
                  ✎
                </button>
                <button
                  className="cm-button toolbar-icon-button"
                  onClick={handleDelete}
                  disabled={!canDelete}
                  title={
                    canDelete
                      ? 'Delete this example'
                      : 'Delete only works on examples you created'
                  }
                  aria-label="Delete this example"
                >
                  {/* Lucide "trash-2" — trash can with lid + body
                      vertical strokes. Reads as "delete permanently"
                      more clearly than a bare ✕. */}
                  <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                    <polyline fill="none" points="3 6 5 6 21 6" />
                    <path fill="none" d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
                    <path fill="none" d="M10 11v6" />
                    <path fill="none" d="M14 11v6" />
                    <path fill="none" d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
                  </svg>
                </button>
                <button
                  className="cm-button toolbar-icon-button"
                  onClick={handleExport}
                  title="Save current example as a .rs file"
                  aria-label="Save current example as a .rs file"
                >
                  {/* Lucide "download" — tray with an arrow landing
                      in it. Unambiguous "save to disk" semantics, vs
                      a bare ⬇ which reads like "move down in list".
                      `fill="none"` is set on every node (and again
                      via CSS) because some renderers fill an open
                      path's implied closed region under the default
                      fill rule. */}
                  <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                    <path fill="none" d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                    <polyline fill="none" points="7 10 12 15 17 10" />
                    <line fill="none" x1="12" y1="15" x2="12" y2="3" />
                  </svg>
                </button>
                <button
                  className="cm-button toolbar-icon-button"
                  onClick={handleImportClick}
                  title="Load a .rs file as a new example"
                  aria-label="Load a .rs file as a new example"
                >
                  {/* Lucide "upload" — same tray with the arrow
                      leaving it. Pairs visually with the download
                      icon above. */}
                  <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                    <path fill="none" d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                    <polyline fill="none" points="17 8 12 3 7 8" />
                    <line fill="none" x1="12" y1="3" x2="12" y2="15" />
                  </svg>
                </button>
                <input
                  ref={fileInputRef}
                  type="file"
                  accept=".rs,text/x-rust,text/plain"
                  style={{ display: 'none' }}
                  onChange={handleImportFile}
                />
                <button
                  className="cm-button generate-button"
                  onClick={handleClick}
                  disabled={isLoading}
                >
                  {isLoading ? <>Generating<span className="ellipsis"></span></> : 'Generate Visualization'}
                </button>
              </div>
              <div className="editor-host" ref={editorHostRef} />
            </Panel>
          </PanelGroup>
        </Panel>

        <PanelResizeHandle className="resize-handle resize-handle-horizontal" />

        {/* Bottom row: visualization (or error). Stretches to fill
            whatever vertical space is left. */}
        <Panel defaultSize={60} minSize={15} className="panel viz-panel">
          {isLoading && (
            <div className="loading-status">
              <p className="loading-message">
                Generating visualization<span className="ellipsis"></span>
              </p>
              <p className="loading-note">
                The first request after a quiet period can take up to ~30 s while the
                compile server wakes up; subsequent requests are fast.
              </p>
            </div>
          )}
          {/* Keep the previous render visible while a fresh one is in
              flight — losing it on every Generate click feels worse
              than briefly showing stale arrows. */}
          {isErr && error ? (
            <ErrorCard err_string={error} />
          ) : (
            <div className="viz-content">
              <div className="flex-container vis_block">
                <div
                  className="ex2 code_panel"
                  dangerouslySetInnerHTML={{ __html: code_svg ?? '' }}
                />
                <div
                  className="ex2 tl_panel"
                  style={{ width: 'auto' }}
                  dangerouslySetInnerHTML={{ __html: timeline_svg ?? '' }}
                />
              </div>
            </div>
          )}
        </Panel>
      </PanelGroup>
    </div>
  );
};

export default App;
