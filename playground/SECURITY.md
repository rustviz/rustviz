# Security model

The RustViz playground accepts arbitrary Rust source from anonymous users on
the public web and feeds it to a real `rustc`.

**What user code can and can't do.** The runner (`playground/runner/rv-runner.sh`)
compiles the snippet as the `src/lib.rs` of a fresh `cargo new --lib` crate
with a generated `Cargo.toml`: no dependencies, no `build.rs`, and the plugin
only runs `cargo check`. A proc macro can't be defined and used in the same
crate, so compiling a snippet does **not** execute user code; executing code
would take a bug in rustc itself. What a snippet *can* do is make rustc read
files (`include!`, `include_str!`) and echo their contents in error messages,
and make rustc burn CPU and memory (const-eval loops, huge types). The
controls below assume a hostile submitter anyway: rustc is a large attack
surface, and defense in depth is cheap here.

## Threat model

**In scope.** A remote attacker submits Rust source to `POST /submit-code`
with intent to: execute arbitrary code on the server, read files outside
the workspace, exfiltrate data over the network, exhaust CPU / memory /
disk / file descriptors, persist a foothold across requests, or pivot to
other tenants on the host.

**Out of scope.** Insider attacks; physical access; vulnerabilities in the
host kernel, container runtime, or Rust toolchain that escape a
properly-configured `docker run` (we trust `docker run --network=none
--read-only --cap-drop=ALL --security-opt=no-new-privileges` to do its job).
Denial-of-service via legitimate but expensive Rust programs is mitigated
but not eliminated; abuse mitigation lives at the rate-limit + caps layer
described below, not at the type-system layer.

## Controls

### 1. Sandboxed plugin execution

**Cloud Run (production).** `RV_RUNNER=sandbox`: the server runs
`rv-runner.sh` under the launcher in `RV_SANDBOX_CMD`, which
`playground/cloudrun/deploy.sh` sets to Cloud Run's built-in
`/usr/local/gcp/bin/sandbox do` (service deployed with `--sandbox-launcher`,
gen2 execution environment). Each request gets a fresh sandbox:

- **No network egress** (the launcher's default; we never pass `--allow-egress`).
- **Read-only view of the image**; only a private `/tmp` is writable, and it
  is discarded with the sandbox.
- **No access to the service's env vars, secrets, or the metadata server.**
  Only the variables listed in `RV_SANDBOX_CMD` are visible. Verified with
  `include!("/proc/self/environ")`: none of `K_SERVICE`, `PORT`, `RV_*`
  service settings appear.
- **Wall-clock cap**: `rv-runner.sh` kills the compile after 18 s, and the
  server kills the launcher's process group at 20 s.
- **One compile per instance** (`--concurrency 1`), so a runaway compile
  only affects its own request, and **at most `--max-instances` (3)
  instances**, which is the cost and abuse ceiling.
- The service runs as `rv-runtime`, a service account with no IAM roles.

The launcher is a Cloud Run **Preview** feature. If it changes, the server
fails closed: `RV_RUNNER=sandbox` with an empty `RV_SANDBOX_CMD` is a
configuration error, never "run unsandboxed". Any launcher with the same
"prefix + command" shape (bubblewrap, nsjail) can be substituted.

**Docker (local development).** `rustviz-lib/src/lib.rs::run_docker`,
the backend the playground binary uses by default outside Cloud Run, invokes:

```sh
docker run --rm -i \
  --network=none \
  --read-only \
  --memory=512m --memory-swap=512m \
  --cpus=1 \
  --pids-limit=64 \
  --cap-drop=ALL \
  --security-opt=no-new-privileges \
  --tmpfs=/work:rw,size=128m,mode=1777 \
  --tmpfs=/tmp:rw,size=32m \
  rustviz/rustviz-runner:latest
```

Each request gets a fresh container. Properties:

- **No network.** `--network=none` removes the loopback interface; outbound
  TCP/UDP/DNS all fail closed. A compromised rustc cannot phone home or
  contact internal services.
- **Read-only root FS.** The toolchain and plugin binaries cannot be
  modified, so a successful exploit cannot persist a backdoor.
- **Writable tmpfs only at `/work` and `/tmp`.** Both are size-capped and
  destroyed at container exit. There is nowhere to leave artifacts.
- **Memory cap (512 MiB) with no swap.** Bounds the blast radius of a
  pathologic `Vec::with_capacity` or recursive macro.
- **CPU cap (1 vCPU) and pids cap (64).** Bounds compute and prevents fork
  bombs.
- **All capabilities dropped, no-new-privileges.** Even if the runner
  process is exploited, it cannot escalate.
- **Wall-clock cap (20 s).** Enforced host-side in `run_docker` because
  Docker has no built-in run timeout; if the deadline passes we
  `kill(2)` the container.
- **Container runs as UID 1000 (`runner`)**, never as root inside the
  container.

The runner image (`playground/runner/Dockerfile`) is the *only* image used for this
purpose; it ships only the toolchain + the plugin binaries + a tiny bash
entrypoint. There is no shell access surface in the playground HTTP path.

### 2. Input validation in `playground`

- `actix_web::web::JsonConfig::default().limit(16 KiB)` rejects oversized
  bodies before the handler runs.
- `submit_code` re-checks `payload.code.len() <= 16 KiB` for clarity.
- `actix-governor` rate-limits `/submit-code` per peer IP (5-token bucket,
  refilled at one token per 2 seconds by default; tunable via
  `RV_RATE_SECONDS_PER_REQUEST` / `RV_RATE_BURST`).

### 3. CORS allowlist on the API

The static SPA is served from GitHub Pages at
`https://rustviz.github.io/`, so cross-origin requests to the
Cloud Run API are required and gated by an explicit `actix-cors` allowlist in
`playground/src/main.rs`. The allowlist is the *only* control over which
sites can drive the API from a browser:

- `https://rustviz.github.io` — the Pages origin.
- `http://localhost:3000` / `http://127.0.0.1:3000` — Vite dev server.

Other origins fail the preflight OPTIONS and the browser refuses to send
the request. If a new origin is added (mirror, custom domain), add it
*and only it* — wildcards (`Cors::default().allow_any_origin()`) would
let any site embed the playground and have you absorb its compute cost.

Note that CORS is a browser-enforced control; a determined attacker
can hit `/submit-code` from non-browser clients (curl, scripts) and
bypass it entirely. The rate limiter and the per-request sandbox are the
defenses against that case; CORS just prevents drive-by abuse from
arbitrary websites.

### 4. The `local` backend is dev-only

`RV_RUNNER=local` runs the plugin in-process against a tempdir on the host.
This is convenient for local iteration but **must never be used for a public
deployment**. The `rustviz-lib` library defaults to `local` (the common case is
trusted callers like the CLI or mdbook); the `playground` binary specifically
overrides that default to `docker` at startup if the env var is unset, so
you'd have to actively pass `RV_RUNNER=local` to the playground process to
hit the unsafe path. The production image sets `RV_RUNNER=sandbox`
(`playground/cloudrun/Dockerfile`).

## Operator checklist

Production is configured by `playground/cloudrun/deploy.sh`. To expose the
playground some other way (docker backend on your own host):

1. Build and push the runner image: `docker build -t rustviz/rustviz-runner:latest -f playground/runner/Dockerfile .`
2. Run `playground` with `RV_RUNNER=docker` (the playground's built-in
   default if unset) and `RV_BIND=0.0.0.0:$PORT`.
3. Tune `RV_RATE_SECONDS_PER_REQUEST` and `RV_RATE_BURST` for your expected
   traffic profile. Defaults assume single-user research workloads.
4. Front the service with a reverse proxy that terminates TLS, sets
   `X-Forwarded-For`, and applies its own rate limit / WAF (Cloudflare,
   Caddy, nginx — whatever fits your platform).
5. Run `playground` itself as an unprivileged user with no sudo / docker group
   beyond what's needed to spawn child containers. On a VM, restrict the
   `docker` group membership. (On Cloud Run the image runs as uid 1000 and
   the service account has no IAM roles.)

## Reporting

Email security findings to `comar@umich.edu`. Please do not file public
issues for vulnerabilities that affect deployed instances.
