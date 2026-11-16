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
