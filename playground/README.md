# RustViz Playground

The playground is a small web app — a CodeMirror editor that POSTs Rust
snippets to a sandboxed backend, which renders them through the RustViz
plugin and returns the two SVG panels. Hosted at
<https://rustviz.github.io/> with the compile API on Cloud Run at
<https://rustviz-api-816769776225.us-central1.run.app/>.

The same `rustviz-playground` binary works as an all-in-one server (SPA + API
on a single origin); the Pages/Cloud Run split deployed to production is just
a latency optimization for the static page-load.

## Quick start (local)

Requirements: `rustup`, `node` 20+, and (for the sandboxed backend)
`docker` or [Colima](https://github.com/abiosoft/colima). The pinned
nightly toolchain is auto-installed by `rustup` from
`rust-toolchain.toml` at the workspace root.

```sh
git clone https://github.com/rustviz/rustviz
cd rustviz
cargo xtask setup           # toolchain, plugin install, frontend build, runner image
cd playground && cargo run --release
open http://127.0.0.1:8080/
```

Iterating on the frontend with hot reload:

```sh
cd playground/frontend && npm run dev   # serves at http://127.0.0.1:3000/
# (proxies /submit-code + /ex-assets to the backend at :8080, so leave
#  `cargo run` running in another terminal)
```

The playground also loads a program from its URL: `https://<host>/#code=<base64url>`,
where the payload is the UTF-8 source in unpadded base64url (`-`/`_` alphabet). It is put
in the editor, rendered once, and the fragment is then removed. `mdbook-rustviz` emits
these links under each figure; the helpers are in `frontend/src/linkCode.ts`.

If you don't have Docker installed and just want to poke at the server,
set `RV_RUNNER=local`. **Never** do this on a public deployment — see
[`SECURITY.md`](SECURITY.md).

## Deploy

Production runs in two pieces:

- **Static SPA on GitHub Pages**, at <https://rustviz.github.io/>.
  Built from `playground/frontend/` by `.github/workflows/pages.yml` and
  pushed to the `rustviz/rustviz.github.io` repo whenever the frontend or
  the plugin, CLI or rustviz-lib changes (the bundle includes SVGs prerendered
  at build time; `src/prerendered.json` is generated, not tracked). Loads
  instantly even when no one has visited recently.
- **Compile API on Google Cloud Run**, at <https://rustviz-api-816769776225.us-central1.run.app/>.
  See below. Allowed origins for cross-origin requests are listed in
  `playground/src/main.rs::cors`.

### Compile API (Cloud Run)

`playground/cloudrun/` holds a single image (pinned nightly + plugin + server
+ runner script) for Google Cloud Run. Each request compiles inside Cloud
Run's per-request `sandbox do` launcher (no egress, read-only image, private
`/tmp`; see [`SECURITY.md`](SECURITY.md)), so there's no docker-in-docker,
no privileged mode and no fleet to manage. With min-instances 0 and
request-based billing, idle costs nothing; normal use fits in Cloud Run's
free tier. `--max-instances 3` is the cost ceiling.

- **CI:** the `deploy-cloudrun` job in `.github/workflows/build.yml` builds
  the image, pushes it to `us-central1-docker.pkg.dev/rustviz-playground/rustviz/playground:<sha>`
  and runs `playground/cloudrun/deploy.sh`, which deploys and smoke-tests.
  Auth is keyless (GitHub OIDC → Workload Identity Federation →
  `rv-deployer`, accepted only from `rustviz/rustviz` on `main`).
- **Roll back:** `playground/cloudrun/deploy.sh us-central1-docker.pkg.dev/rustviz-playground/rustviz/playground:<older-sha>`
- **Logs:** `gcloud run services logs read rustviz-api --region us-central1`
- **Project:** `rustviz-playground` (owner cyology@gmail.com), with a $5/mo
  budget alert. Artifact Registry keeps the 5 newest images.

History: until Nov 2026 the API ran on Fly.io as a docker-in-docker fleet.
Stray bot connections kept waking ~7 of its 10 Machines, and slow,
crash-prone boots made it worse, costing ~$25–28/mo with no real traffic.
See PR #163.

### First-time setup (GitHub Pages SPA)

```sh
# 1. Create the receiving repo
gh repo create rustviz/rustviz.github.io --public \
  --description "Static front-end for the RustViz playground"

# 2. Enable Pages on rustviz/rustviz.github.io via Settings → Pages →
#    Source: Deploy from a branch → main / root.

# 3. Generate a deploy keypair
ssh-keygen -t ed25519 -f /tmp/playground_deploy_key -N "" -C playground-deploy

# 4. Add the *public* key as a write-enabled deploy key on rustviz/rustviz.github.io
gh api -X POST repos/rustviz/rustviz.github.io/keys \
  -f title=playground-deploy -F read_only=false \
  -f key="$(cat /tmp/playground_deploy_key.pub)"

# 5. Add the *private* key as a secret on rustviz/rustviz
gh secret set PAGES_DEPLOY_KEY --repo rustviz/rustviz < /tmp/playground_deploy_key

# 6. Clean up
rm /tmp/playground_deploy_key /tmp/playground_deploy_key.pub
```

After that, every push to `main` (when the change touches
`playground/frontend/**`) triggers `.github/workflows/pages.yml`, which
builds the SPA in `pages` mode and pushes the `dist/` tree to
`rustviz/rustviz.github.io` for serving at
<https://rustviz.github.io/>.

### Adding a new SPA origin

If you ever stand up the SPA at another URL (custom domain, mirror), add
that origin to the CORS allowlist in `playground/src/main.rs` and redeploy
the API. The allowlist is the gate — without it the new origin's browsers
will refuse to call `/submit-code`.
