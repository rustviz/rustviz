import React, { useEffect, useRef, useState } from 'react';
import './index.css';
import { extensions } from './setup';
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import axios from 'axios';
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';
import ErrorCard from './ErrorCard';
import { exampleGroups } from './examples';

// API origin. Empty (relative URL) for the default same-origin Fly deploy;
// set to https://rustviz-playground.fly.dev for the GitHub Pages build via
// .env.pages so the SPA hosted on rustviz.github.io can hit the API on Fly.
// rv-serve's CORS allowlist must include the SPA's origin in the latter case.
const API_BASE: string = import.meta.env.VITE_API_BASE ?? '';

declare function helpers(param: string): void;

// Seed the editor with the first dropdown example (Motivation →
// "Hands-on tutorial") so first-time visitors land on a snippet that
// actually exercises ownership, borrowing, and the
// `// rustviz: skip` / `// rustviz: hide` markers — instead of an
// abstract `let mut x = 7; let mut a = &mut x; …` chain that doesn't
// motivate anything. Single source of truth lives in examples.ts.
const defaultExample: string = exampleGroups[0].examples[0].code;

// Thin wrapper around CodeMirror's EditorView so the React layer can
// hold a single `Editor` instance across renders without re-creating
// the underlying view (which would lose cursor position, undo
// history, etc.). Owns its own DOM insertion via the constructor's
// `parent` arg.
class Editor {
  private view: EditorView;

  public constructor(editorContainer: HTMLElement, code: string = defaultExample) {
    const initial_state = EditorState.create({
      doc: code,
      extensions: extensions,
    });
    this.view = new EditorView({ state: initial_state, parent: editorContainer });
  }

  public getCurrentCode(): string {
    return this.view.state.doc.toString();
  }

  // Replace the entire editor contents in one transaction. Used by the
  // example-picker dropdown when the user selects a preloaded snippet.
  public setCurrentCode(code: string): void {
    this.view.dispatch({
      changes: { from: 0, to: this.view.state.doc.length, insert: code },
    });
  }

  public destroy(): void {
    this.view.destroy();
  }
}

// Top-of-page bar. Title on the left, the rustc / plugin version
// stamp on the right (kept here instead of below the editor so it
// reads as masthead metadata rather than competing with the editor /
// visualization for attention).
const TitleBar: React.FC = () => (
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
      <code>rustc {__RUST_VERSION__}</code>
      <code>rustviz-plugin {__PLUGIN_VERSION__}</code>
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
        Closures — captures (whether by reference or by{' '}
        <code>move</code>) aren't drawn as arrows, so the visualization
        silently omits the capture event
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
    <h3>Research paper</h3>
    <p>
      An earlier version of RustViz used hand-written visualization
      directives in source comments and was deployed in classroom
      teaching at the University of Michigan. That version is described
      in our VL/HCC 2022 paper:
    </p>
    <p>
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
      .
    </p>
  </div>
);

type ExamplePickerProps = {
  onSelect: (code: string) => void;
};

// The picker stays interactive even while a /submit-code request is
// in flight. Picking a fresh example mid-load aborts the previous
// request (see inflightRef in App) and starts a new one — the user
// can change their mind and the wrong response can never overwrite
// the right one.
const ExamplePicker: React.FC<ExamplePickerProps> = ({ onSelect }) => {
  const handleChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const value = e.target.value;
    if (!value) return;
    // value encodes "<chapterIndex>:<exampleIndex>" so we don't have
    // to escape the example name into the option's value attribute.
    const [chapterIdx, exampleIdx] = value.split(':').map(Number);
    onSelect(exampleGroups[chapterIdx].examples[exampleIdx].code);
  };

  return (
    <div className="example-picker">
      <label htmlFor="example-select">Example:</label>
      {/* Default to "0:0" — Motivation → "Hands-on tutorial" — to
          match the editor's seed snippet (defaultExample). React's
          `defaultValue` only sets initial state; it doesn't fire
          onChange, so we don't double-load the same code. */}
      <select id="example-select" defaultValue="0:0" onChange={handleChange}>
        {exampleGroups.map((group, gIdx) => (
          <optgroup key={group.chapter} label={group.chapter}>
            {group.examples.map((ex, eIdx) => (
              <option key={ex.name} value={`${gIdx}:${eIdx}`}>
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

  // The CodeMirror editor needs a real DOM node to attach to. Create
  // it once when the host div mounts; tear it down on unmount so
  // React 18 StrictMode's dev-mode double-invocation of this effect
  // doesn't leave a second CodeMirror instance attached.
  const editorHostRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!editorHostRef.current) return;
    const newEditor = new Editor(editorHostRef.current);
    setEditor(newEditor);
    return () => {
      newEditor.destroy();
      setEditor(null);
    };
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

  const handleExampleSelect = (code: string) => {
    if (!editor) return;
    editor.setCurrentCode(code);
    // Auto-fire the visualization. CodeMirror's dispatch is
    // synchronous so getCurrentCode() inside handleClick() will see
    // the just-set code on the next line.
    handleClick();
  };

  return (
    <div className="app-shell">
      <TitleBar />
      <PanelGroup direction="vertical" className="main-split" autoSaveId="rustviz-vertical">
        {/* Top row: prose on the left, code editor on the right. */}
        <Panel defaultSize={40} minSize={15}>
          <PanelGroup direction="horizontal" autoSaveId="rustviz-top-horizontal">
            <Panel defaultSize={35} minSize={15} className="panel description-panel">
              <Description />
            </Panel>
            <PanelResizeHandle className="resize-handle resize-handle-vertical" />
            <Panel defaultSize={65} minSize={25} className="panel editor-panel">
              <div className="editor-toolbar">
                <ExamplePicker onSelect={handleExampleSelect} />
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
                  onMouseEnter={() => helpers('ex2')}
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
