# RustViz Playground

The playground is a small web app — a CodeMirror editor that POSTs Rust
snippets to a sandboxed backend, which renders them through the RustViz
plugin and returns the two SVG panels. Hosted at
<https://rustviz.github.io/playground/> with the compile API at
<https://rustviz-playground.fly.dev/>.

The same `rustviz-playground` binary works as an all-in-one server (SPA + API
on a single origin); the Pages/Fly split deployed to production is just
a latency optimization for the static page-load.

## Quick start (local)

Requirements: `rustup`, `node` 20+, and (for the sandboxed backend)
`docker` or [Colima](https://github.com/abiosoft/colima). The pinned
nightly toolchain is auto-installed by `rustup` from
`rust-toolchain.toml` at the workspace root.

```sh
git clone https://github.com/rustviz/rustviz
cd rustviz
./setup.sh                  # toolchain, plugin install, frontend build, runner image
cd playground && cargo run --release
open http://127.0.0.1:8080/
```

Iterating on the frontend with hot reload:

```sh
cd playground/frontend && npm run dev   # serves at http://127.0.0.1:3000/
# (proxies /submit-code + /ex-assets to the backend at :8080, so leave
#  `cargo run` running in another terminal)
```

If you don't have Docker installed and just want to poke at the server,
set `RV_RUNNER=local`. **Never** do this on a public deployment — see
[`SECURITY.md`](../SECURITY.md) at the workspace root.

## Deploy

Production runs in two pieces:

- **Static SPA on GitHub Pages**, at <https://rustviz.github.io/playground/>.
  Built from `playground/frontend/` by `.github/workflows/pages.yml` and
  pushed to the `rustviz/playground` repo on every change. Loads instantly
  even when no one has visited recently.
- **Compile API on Fly.io**, at <https://rustviz-playground.fly.dev/>.
  Ten Machines provisioned, all auto-stopping when idle; the edge proxy
  routes traffic to whichever ones are awake and starts more from stopped
  state when concurrency thresholds (`fly.toml::http_service.concurrency`)
  are crossed. dockerd uses the `fuse-overlayfs` storage driver so
  Machines don't need a per-Machine ext4 volume for `/var/lib/docker`
  (see `fly.toml` and `deploy/entrypoint.sh` for context). Idle cost
  ~$2–3/mo total; an HN-spike day adds ~$5–10 of Machine compute.
  Allowed origins for cross-origin requests are listed in
  `playground/src/main.rs::cors`.

### First-time setup (Fly compile API)

```sh
fly auth login                                  # browser OAuth
fly launch --copy-config --no-deploy            # creates the app

# Trigger the runner-image workflow manually for the first publication.
# It also auto-fires on every push to main that touches runner/** or
# rustviz2-plugin/**, but the very first time it has to be kicked off
# by hand because there's nothing in GHCR yet for the deploy to pull.
gh workflow run runner-image.yml --ref main
gh run watch                                    # blocks until the run finishes
                                                # (~30 min first time, ~5 min later)

# Mark the new GHCR package public so Fly Machines can pull without auth:
#   GitHub → Org → Packages → rustviz-runner →
#     Package settings → Change visibility → Public.
# This step has to happen before the next command, otherwise the deploy's
# first-boot `docker pull` fails.

../deploy/deploy.sh                             # two-phase Fly deploy
```

The first boot of each Fly Machine pulls the `rustviz/rustviz-runner`
image from GHCR (~30 s for ~600 MB). It's then cached on the Machine's
local filesystem; subsequent cold starts after auto-stop take ~10 s.

`./deploy/deploy.sh` also ensures the fleet stays at 10 Machines (override
with `RV_FLY_MACHINES=N`). With auto-stop on, idle Machines are free; the
extra capacity exists so the edge proxy has somewhere to spill load when
one Machine gets saturated. No need to manually scale up before posting
the URL somewhere.

### Routine deploys

```sh
../deploy/deploy.sh
```

When you change `runner/**` or `rustviz2-plugin/**`,
`.github/workflows/runner-image.yml` automatically republishes the
sandbox image to GHCR; the next `./deploy/deploy.sh` picks it up on
each Machine's first boot.

Every push to `main` triggers `.github/workflows/build.yml`. The
workflow runs the build + tests first (also on every PR), then on
`main` pushes only a downstream `deploy` job runs `./deploy/deploy.sh`
on a hosted runner (requires a `FLY_API_TOKEN` repo secret). Because
`deploy` declares `needs: build`, a failing test suite blocks the
deploy. The deploy job opens a `deploy-failure`-labelled issue on
failure, in addition to GitHub's default email-on-failure notification.

### First-time setup (GitHub Pages SPA)

```sh
# 1. Create the receiving repo
gh repo create rustviz/playground --public \
  --description "Static front-end for the RustViz playground"

# 2. Enable Pages on rustviz/playground via Settings → Pages →
#    Source: Deploy from a branch → main / root.

# 3. Generate a deploy keypair
ssh-keygen -t ed25519 -f /tmp/playground_deploy_key -N "" -C playground-deploy

# 4. Add the *public* key as a write-enabled deploy key on rustviz/playground
gh api -X POST repos/rustviz/playground/keys \
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
`rustviz/playground` for serving at
<https://rustviz.github.io/playground/>.

### Adding a new SPA origin

If you ever stand up the SPA at another URL (custom domain, mirror), add
that origin to the CORS allowlist in `playground/src/main.rs` and redeploy
the API. The allowlist is the gate — without it the new origin's browsers
will refuse to call `/submit-code`.

### Why a script instead of `fly deploy` directly

`fly.toml` ships with `auto_stop_machines = 'off'` because Fly's
autoscaler stops a Machine after ~40 s of "no incoming traffic", and
our entrypoint takes 5–15 min on a fresh Machine to extract the
~1 GiB runner image through fuse-overlayfs. With `'stop'` set at
deploy time, every fresh Machine would be killed mid-bootstrap before
the playground binary ever binds `:8080` — `fly deploy` would deadlock.
So `'off'` is the only value that lets a fresh deploy actually finish.

The cost-saving auto-stop behavior is applied per-Machine after
deploy. `deploy/deploy.sh` does a destroy-and-recreate every time:

1. Destroys every existing Machine (parallel `fly machine destroy --force`).
2. Runs `fly scale count` and `fly deploy --strategy immediate`, then
   polls `fly status --json` until every freshly-created Machine's
   HTTP check passes.
3. Runs `fly machine update --autostop=stop --skip-health-checks`
   against every Machine in parallel, flipping the per-Machine
   service config via the Machines API. Verifies the config landed;
   doesn't poll for health afterwards (auto-stop kicks in immediately,
   so a poll-for-all-healthy loop is unwinnable).

We destroy and recreate rather than incrementally updating the
existing fleet because every iteration of the in-place approach hit
a different stale-state edge case (stopped Machines that don't
auto-start during deploy, drift between Machines created under
different fly.toml settings, autoscaler racing the post-update
bounce, etc.). Nuking the fleet sidesteps all of it for the cost
of a few minutes of downtime per deploy — acceptable for a research
tool with sparse traffic. End state: every Machine on the new
release with `auto_stop = 'stop'`, fleet idles cheaply (~$2–3/mo
for the IP and Machine baseline). In steady state between deploys
the auto-stop / auto-start cycle is fast (~10 s cold start) thanks
to `persist_rootfs = 'always'` keeping the runner image cached on
each Machine's rootfs.

Pass `--keep-warm` to skip step 3 if you want Machines to never
auto-stop (~$24/mo per always-running Machine).
