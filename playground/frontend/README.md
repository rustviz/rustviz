# Playground frontend

The RustViz playground's single-page app: Vite + React + TypeScript, with a
CodeMirror editor. It is served at <https://rustviz.github.io/> and, for
local development, by the `rustviz-playground` server in `playground/`
(see [`../README.md`](../README.md) for the quick start).

`cargo xtask setup` at the workspace root runs `npm install` and
`npm run build` here for you. By hand, with `node` 20+:

```sh
npm install
npm run dev          # http://127.0.0.1:3000/, proxies /submit-code and /ex-assets
                     # to a playground server on :8080 (cd playground && cargo run --release)
npm run build        # same-origin build into dist/, served by the playground server
npm run build:pages  # the GitHub Pages build; API calls go to VITE_API_BASE from .env.pages
npx tsx scripts/linkCode.test.ts   # tests for the #code= link helpers
npx tsx scripts/telemetry.test.ts  # tests for the usage-logging client (off by default)
```

Both builds first run `scripts/prerender.ts`, which renders every dropdown
example in `src/examples.ts` through the installed `rustviz` CLI into
`src/prerendered.json` (generated, not tracked). So the `rustviz` CLI and
the plugin must be installed (`cargo xtask setup`, or see [Building from
source](../../README.md#building-from-source)), and a build fails if any
example fails to render.

Files in `public/` are copied into `dist/` as is: `ex-assets/` (tooltip JS
and CSS) and the `README.md` of the deployed
[rustviz.github.io](https://github.com/rustviz/rustviz.github.io) repo.

**Usage logging is off** in every build unless `VITE_RV_TELEMETRY_ENDPOINT`
is set (see `src/telemetry.ts` for it and the other `VITE_RV_TELEMETRY_*`
knobs). Then the page loads `ex-assets/rv-telemetry.js` (a copy of
`rustviz-lib/src/rv-telemetry.js`; keep them identical), shows no
banner (the info pane's Privacy paragraph is the notice; only an
opt-in build shows a prompt) and links `/privacy/` (`public/privacy/index.html`) and a "Your data"
panel from the Privacy section of the info pane. Without it, none of that is
loaded or linked.
