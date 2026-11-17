# rustviz.github.io

The static frontend of the [RustViz playground](https://rustviz.github.io/).

**This repository is generated; don't edit it here.** Its contents are
the build output of
[`playground/frontend/`](https://github.com/rustviz/rustviz/tree/main/playground/frontend)
in [rustviz/rustviz](https://github.com/rustviz/rustviz), pushed by the
[`pages` workflow](https://github.com/rustviz/rustviz/blob/main/.github/workflows/pages.yml)
on pushes to `main` that touch the frontend, the plugin, the CLI,
`rustviz-lib` or the toolchain. Every push replaces the whole tree, and
each deploy commit names the source commit it was built from.

To build the same thing locally, from a checkout of rustviz/rustviz with
the `rustviz` CLI and plugin installed:

```sh
cd playground/frontend
npm ci
npm run build:pages   # writes dist/
```

Report problems at <https://github.com/rustviz/rustviz/issues>.
