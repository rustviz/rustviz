import React from 'react';
import './ErrorCard.css';

const ISSUES_URL = 'https://github.com/rustviz/rustviz/issues';

// GitHub rejects very long prefilled-issue URLs; past this we leave the
// snippet out of the link and ask the reporter to paste it.
const MAX_ISSUE_URL = 7500;

interface ErrorCardProps {
  err_string: string;
  // Value of the server's X-RustViz-Error-Kind header, if any:
  // "does-not-compile", "unsupported", "timeout", ... Servers that
  // predate the header send none; the card then shows the raw message.
  kind?: string | null;
  // The code that was submitted, for the "report this" link.
  code?: string;
}

/// Split "<intro sentence>\n\n<rustc diagnostics>" (rustviz-lib's
/// does-not-compile message) into its two parts.
function splitIntro(msg: string): [string, string] {
  const i = msg.indexOf('\n\n');
  return i < 0 ? ['', msg] : [msg.slice(0, i), msg.slice(i + 2)];
}

function issueUrl(detail: string, code: string): string {
  const title = `Playground: can't visualize program (${detail})`;
  const withCode =
    `This snippet fails in the playground:\n\n\`\`\`rust\n${code}\n\`\`\`\n\n` +
    `Error: internal error at ${detail}\n`;
  const withoutCode =
    `This snippet fails in the playground (too long for the link; please paste it here):\n\n` +
    `\`\`\`rust\n\n\`\`\`\n\nError: internal error at ${detail}\n`;
  const url = (body: string) =>
    `${ISSUES_URL}/new?title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
  const full = url(withCode);
  return full.length <= MAX_ISSUE_URL ? full : url(withoutCode);
}

const ErrorCard: React.FC<ErrorCardProps> = ({ err_string, kind, code = '' }) => {
  if (kind === 'does-not-compile') {
    const [fullIntro, diagnostics] = splitIntro(err_string);
    // The heading already says it; don't repeat "This program doesn't
    // compile, and ..." right under it.
    const intro = fullIntro.replace(/^This program doesn't compile, and /, '');
    return (
      <div id="err_card">
        <p id="err_header">This program doesn't compile</p>
        {intro && <p className="err_intro">{intro}</p>}
        <pre id="error_message">{diagnostics}</pre>
      </div>
    );
  }

  if (kind === 'unsupported') {
    // "RustViz can't visualize this program yet (internal error at
    // visitor.rs:628: <msg>). Please report it at <url> with your snippet."
    const m = err_string.match(/\(internal error at (.*)\)\. Please report it/s);
    const detail = m ? m[1] : null;
    return (
      <div id="err_card">
        <p id="err_header">RustViz can't visualize this program yet</p>
        {detail ? (
          <>
            <p className="err_intro">
              Your program compiles, but it uses something RustViz doesn't support yet. This is a
              limitation of RustViz, not a problem with your code.
            </p>
            <pre id="error_message">internal error at {detail}</pre>
          </>
        ) : (
          <p className="err_intro">{err_string}</p>
        )}
        <p className="err_report">
          <a href={issueUrl(detail ?? err_string, code)} target="_blank" rel="noreferrer">
            Report this on GitHub
          </a>{' '}
          (opens a pre-filled issue with your snippet) so we can add support for it.
        </p>
      </div>
    );
  }

  return (
    <div id="err_card">
      <p id="err_header">ERROR: </p>
      <pre id="error_message">{err_string}</pre>
    </div>
  );
};

export default ErrorCard;
