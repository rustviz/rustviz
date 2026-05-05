import React, { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import './index.css';
import { extensions } from './setup';
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import axios from 'axios';
import ErrorCard from './ErrorCard';
import { exampleGroups } from './examples';

// API origin. Empty (relative URL) for the default same-origin Fly deploy;
// set to https://rustviz-playground.fly.dev for the GitHub Pages build via
// .env.pages so the SPA hosted on rustviz.github.io can hit the API on Fly.
// rv-serve's CORS allowlist must include the SPA's origin in the latter case.
const API_BASE: string = import.meta.env.VITE_API_BASE ?? '';

declare function helpers(param: string): void;

const defaultExample: string = `
fn main () {
    let mut x = 7;
    let mut z = 6;
    let mut a = & mut x;
    let mut c = & mut z;
    let mut b = & mut a;
    b = & mut c;
    println!("x {}", *a);
    println!("z {}", **b);
}
`.trim();

class Editor {
  private view: EditorView;

  public constructor (
    editorContainer: HTMLElement,
    code: string = defaultExample,
  ) {
    let initial_state = EditorState.create({
      doc: code,
      extensions: extensions
    });
    
    this.view = new EditorView({
      state: initial_state,
      parent: editorContainer
    });
  }

  public getCurrentCode(): string {
    return this.view.state.doc.toString();
  }

  // Replace the entire editor contents in one transaction. Used by the
  // example-picker dropdown when the user selects a preloaded snippet.
  public setCurrentCode(code: string): void {
    this.view.dispatch({
      changes: {
        from: 0,
        to: this.view.state.doc.length,
        insert: code,
      },
    });
  }
}

// Dropdown above the editor that loads a preloaded example into the
// editor when selected. Examples come from `examples.ts`, vendored
// from the rustviz-tutorial repo. We render this via createPortal
// from inside App so the picker has access to the editor instance,
// even though it visually lives in #example-picker (which sits above
// the editor in DOM order, while the rest of the React app mounts
// into #root below the editor).
type ExamplePickerProps = {
  onSelect: (code: string) => void;
};

// The picker stays interactive even while a /submit-code request is in
// flight. Picking a fresh example mid-load aborts the previous request
// (see inflightRef in App) and starts a new one — the user can change
// their mind and the wrong response can never overwrite the right
// one.
const ExamplePicker = ({ onSelect }: ExamplePickerProps) => {
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
      <label htmlFor="example-select">Examples:</label>
      <select id="example-select" defaultValue="" onChange={handleChange}>
        <option value="">— pick a preloaded example —</option>
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

const App = () => {
  const [isLoading, setIsLoading] = useState(false);
  const [isErr, setErr] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [code_svg, setCodeSvg] = useState<string | null>(null);
  const [timeline_svg, setTimelineSvg] = useState<string | null>(null);

  useEffect(() => {
    const editorElement = document.getElementById('editor')!;
    if (editorElement) {
      const newEditor = new Editor(editorElement);
      setEditor(newEditor);
    }
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
        setError(error.response.data); // Extract and set error message
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

  useEffect(() => {
    handleClick(); // Call handleClick when the component mounts
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editor]);


  const handleExampleSelect = (code: string) => {
    if (!editor) return;
    editor.setCurrentCode(code);
    // Auto-fire the visualization. CodeMirror's dispatch is synchronous
    // so getCurrentCode() inside handleClick() will see the just-set
    // code on the next line.
    handleClick();
  };

  // The picker has to mount above the editor in DOM order, but the
  // editor itself lives in vanilla index.html (not inside React). Use
  // createPortal to render the picker into the #example-picker
  // placeholder while keeping it part of App's component tree (so it
  // can call into the editor via state).
  const pickerHost = typeof document !== 'undefined'
    ? document.getElementById('example-picker')
    : null;

  return (
    <div id="page-wrapper" className="page-wrapper">
      {pickerHost && createPortal(
        <ExamplePicker onSelect={handleExampleSelect} />,
        pickerHost
      )}
      <button className="cm-button large-button" id="gen-button" onClick={handleClick} disabled={isLoading}>
        {isLoading ? <>Generating<span className="ellipsis"></span></> : 'Generate Visualization'}
      </button>
      <p className="rust-version">
        Compiles with <code>rustc {__RUST_VERSION__}</code>
      </p>
      {isLoading && (
        <div className="loading-status">
          <p className="loading-message">
            Generating visualization<span className="ellipsis"></span>
          </p>
          <p className="loading-note">
            The first request after a quiet period can take up to ~30 s
            while the compile server wakes up; subsequent requests are
            fast.
          </p>
        </div>
      )}
      {isErr && error ? <ErrorCard err_string={error} /> :
        <div className="page">
          <div className="flex-container vis_block" style={{ marginLeft: '50px' }}>
            <div
              className="ex2 code_panel"
              dangerouslySetInnerHTML={{ __html: code_svg ?? "" }}
            />
            <div
              className="ex2 tl_panel"
              style={{ width: 'auto' }}
              dangerouslySetInnerHTML={{ __html: timeline_svg ?? "" }}
              onMouseEnter={() => helpers('ex2')}
            />
          </div>
        </div>
      }
    </div>
  );
};

export default App;