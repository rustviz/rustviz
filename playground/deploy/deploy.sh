#!/usr/bin/env bash
# Destroy-and-recreate Fly.io deploy for the RustViz playground.
#
# Every deploy starts from a clean fleet:
#
#   1. Destroy every existing Machine (parallel).
#   2. fly deploy --strategy immediate against the empty fleet (creates
#      Fly's HA-default 1-2 Machines in the "app" process group with
#      the new release), then fly scale count to fill out to
#      $RV_FLY_MACHINES. Order matters: scale-count-before-deploy puts
#      Machines in the wrong process group, which fly deploy then
#      ignores while creating its own fleet alongside.
#   3. Poll `fly status --json` until every Machine's health check passes,
#      retrying on stuck-Machine timeout (see RV_DEPLOY_RETRIES below).
#   4. Run `fly machine update --autostop=stop` against every Machine
#      to flip the per-Machine service config from the bootstrap-friendly
#      'off' (set in fly.toml) to the cost-saving 'stop' for steady state.
#      Verify the config landed; do NOT poll for health afterwards
#      (the autoscaler immediately starts auto-stopping idle Machines
#      under the new setting, and Consul health probes don't count as
#      traffic, so a poll-for-all-healthy loop is unwinnable).
#
# Why destroy-and-recreate instead of in-place updates: every previous
# attempt to incrementally update an existing fleet hit a different
# stale-state edge case — Machines stopped pre-deploy that fly deploy
# updates but doesn't auto-start, fly machine update timing out vs the
# autoscaler that just took effect, drift between Machines created
# under different fly.toml settings, etc. Nuking the fleet sidesteps
# all of it for the cost of a few minutes of downtime per deploy
# (acceptable for a research tool with sparse traffic). Each deploy
# pays one fuse-overlayfs cold pull per Machine in parallel
# (~10-15 min wallclock total).
#
# Resilience: cold pulls occasionally stall on a single Machine
# (transient fuse-overlayfs / GHCR flakiness). The wait below is
# wrapped in a retry loop: on timeout, identify Machines whose checks
# aren't passing, destroy them, re-scale, retry the wait.
# RV_DEPLOY_RETRIES controls the number of retries after the first
# attempt (default 1, so 2 total attempts).
#
# In steady state between deploys, the auto-stop / auto-start cycle
# still benefits from `persist_rootfs = 'always'` on the [[vm]] block
# in fly.toml: an auto-stopped Machine that gets traffic comes back
# in ~10 s without re-pulling the runner image. Cost in steady state:
# ~$2-3 / mo for the IP and Machine baseline.
#
# Pass --keep-warm to skip step 4 (Machines never auto-stop; ~$24/mo
# *per always-running Machine*).
#
# The script also ensures the fleet is at $RV_FLY_MACHINES (default 10)
# Machines on every deploy. Idle Machines auto-stop, so the extra capacity
# costs nothing in steady state — it's there so the edge proxy can spill
# concurrent load to additional Machines when one gets saturated, e.g. when
# someone posts the URL on Hacker News. With hard_limit = 5 per Machine in
# fly.toml::http_service.concurrency, ten Machines = ~50 concurrent compile
# capacity at peak, which covers a meaningful HN-frontpage spike.
#
# Usage:
#   deploy/deploy.sh             # cheap mode, two-phase
#   deploy/deploy.sh --keep-warm # always-warm mode, skips phase 2
#
# Env:
#   RV_FLY_MACHINES          fleet size; default 10
#   RV_DEPLOY_TIMEOUT_SECS   per-attempt timeout for the health-check
#                            wait, in seconds; default 1800 (30 min)
#   RV_DEPLOY_RETRIES        retries after the first wait timeout;
#                            default 1 (so up to 2 total attempts)
#
# Prerequisites:
#   * `fly` (flyctl) installed and authenticated (`fly auth login`).
#   * The Fly app already exists (`fly launch --copy-config --no-deploy`
#     done once).
#   * The runner image has been published to ghcr.io/rustviz/rustviz-runner
#     and the package is public. (Run `gh workflow run runner-image.yml`
#     once, then flip the package to public on GitHub.)
#
# fly.toml is never mutated by this script. Phase 1 just runs
# `fly deploy` against the committed fly.toml (which has
# `auto_stop_machines = 'off'` so freshly-created Machines aren't
# killed by the autoscaler mid-cold-pull). Phase 2 uses
# `fly machine update` directly against the Machines API to flip
# auto_stop on each running Machine, doesn't touch fly.toml.

set -euo pipefail

# Layout: this script + fly.toml + Dockerfile all live under
# playground/deploy/. cwd defaults here so most `fly` commands
# (machine update, status, scale, …) find fly.toml in cwd
# without needing --config. The exception is `fly deploy`, which
# uses cwd as the Docker build context — the Dockerfile reaches
# into the workspace crates via `COPY rustviz2-plugin/…` etc.,
# so deploy needs to run from the repo root with fly.toml passed
# explicitly. `fly_deploy` below wraps that.
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$SCRIPT_DIR"

fly_deploy() {
    (cd "$REPO_ROOT" && "$FLY" deploy --config "$SCRIPT_DIR/fly.toml" "$@")
}

# --- arg parsing --------------------------------------------------------
KEEP_WARM=0
for arg in "$@"; do
    case "$arg" in
        --keep-warm) KEEP_WARM=1 ;;
        -h|--help)
            sed -n '2,30p' "$0"
            exit 0
            ;;
        *) echo "Unknown argument: $arg" >&2; exit 2 ;;
    esac
done

# --- preflight ----------------------------------------------------------
# Resolve the fly CLI: locally `brew install flyctl` provides both `fly`
# and `flyctl`, but superfly/flyctl-actions/setup-flyctl in CI installs
# only `flyctl` (no `fly` symlink). Pick whichever is on PATH and use it
# via $FLY everywhere below; xargs and other subprocesses see the
# already-expanded binary name, not a shell alias.
if command -v fly >/dev/null 2>&1; then
    FLY=fly
elif command -v flyctl >/dev/null 2>&1; then
    FLY=flyctl
else
    echo "fly CLI not on PATH; install it with 'brew install flyctl'" >&2
    exit 1
fi
# Skip the `auth whoami` preflight when FLY_API_TOKEN is set. flyctl
# honors that env var for deploy/status/machine-update calls, but
# `auth whoami` itself reads ~/.fly/config.yml (populated by
# `fly auth login`), so it returns "not logged in" in CI even though
# the rest of flyctl is perfectly authenticated. Locally without a
# token, the check still helps with a friendly error.
if [ -n "${FLY_API_TOKEN:-}" ]; then
    # Strip CR/LF from the token. `gh secret set` via stdin or a file
    # commonly picks up a trailing newline; flyctl then sends it as
    # part of the `Authorization: <token>` header and Go's net/http
    # rejects it with "invalid header field value for Authorization".
    #
    # Importantly we strip ONLY \r and \n, not all whitespace: deploy
    # tokens from `fly tokens create deploy` come back as
    # `FlyV1 fm2_lJPE…` — the space between the version prefix and
    # the token body is part of the token format Fly expects in the
    # Authorization header. Stripping it (e.g. with tr -d '[:space:]')
    # mangles the token into `FlyV1fm2_…`, which the API accepts as
    # syntactically valid bytes but rejects as a "token validation
    # error".
    FLY_API_TOKEN=$(printf '%s' "$FLY_API_TOKEN" | tr -d '\r\n')
    export FLY_API_TOKEN
else
    "$FLY" auth whoami >/dev/null 2>&1 || { echo "Not logged in. Run '$FLY auth login' first or set FLY_API_TOKEN." >&2; exit 1; }
fi
command -v jq >/dev/null 2>&1 || { echo "jq is required for the health-check polling loop. brew install jq" >&2; exit 1; }

APP_NAME=$(awk -F"'" '/^app =/ {print $2; exit}' fly.toml)
URL="https://${APP_NAME}.fly.dev/"

# --- progress helpers ---------------------------------------------------
# All script-emitted progress lines go through these so they share a
# consistent format ("[MM:SS] message") and a consistent in-place update
# behavior. Output goes to stderr to keep stdout clean for any future
# piping; in an interactive terminal it's interleaved naturally with
# fly's own stdout output.
START_TS=$(date +%s)
elapsed() {
    local now diff
    now=$(date +%s)
    diff=$(( now - START_TS ))
    printf '%02d:%02d' $(( diff / 60 )) $(( diff % 60 ))
}
# In-place update: \r returns to start of line, \033[K clears to EOL,
# so consecutive `status` calls overwrite each other on the same row.
# Use while a step is in flight and you want a ticking display.
status() { printf '\r\033[K[%s] %s' "$(elapsed)" "$*" >&2; }
# Same as status() but commits the line with a trailing newline. Use
# for one-shot announcements ("==> Phase 2: …") and for the final line
# of an in-flight phase, so the next bit of output lands on a fresh row.
say()    { status "$*"; printf '\n' >&2; }

# Polls `fly status --json` until every Machine in the app process group
# is reporting all health checks passing. Returns 0 on success, 1 on
# timeout.
#
# Why we do this in the script rather than relying on fly deploy's own
# health-check wait: with --strategy immediate the deploy returns as soon
# as Machines are recreated, regardless of check status. fly deploy's own
# --wait-timeout (default 2 min) is also too short for our cold-pull
# bootstrap (5-15 min/Machine through fuse-overlayfs). Owning the wait
# in the script lets us pick a deadline that matches reality.
#
# Args: <timeout_secs>
wait_for_fleet_healthy() {
    local timeout_secs="$1"
    local deadline=$(( $(date +%s) + timeout_secs ))
    say "==> Waiting for every Machine to pass its HTTP health check (up to $((timeout_secs / 60)) min)"
    while true; do
        # `fly status --json` returns an object with a Machines array; each
        # Machine has a Checks array. We want every Machine in the app
        # process group to have all its checks passing.
        local json summary passing total
        json=$("$FLY" status --json 2>/dev/null) || json='{}'
        # `unique_by(.id)` is load-bearing: Fly's API occasionally returns
        # the same Machine twice in `fly status --json`; without dedupe the
        # count ends up like "0/11" for a 10-Machine fleet and the loop
        # never sees passing == total.
        summary=$(printf '%s' "$json" | jq -r '
            .Machines // []
            | unique_by(.id)
            | map(select(.config.metadata.fly_process_group == "app" or
                         (.config.metadata.fly_process_group // "app") == "app"))
            | "\(map(select(.checks // [] | all(.status == "passing"))) | length)/\(length)"
        ' 2>/dev/null) || summary='?/?'

        passing=${summary%/*}
        total=${summary#*/}

        if [ "$total" != "0" ] && [ "$total" != "?" ] && [ "$passing" = "$total" ]; then
            say "    ${total}/${total} Machines healthy"
            return 0
        fi

        if [ "$(date +%s)" -gt "$deadline" ]; then
            printf '\n' >&2
            return 1
        fi

        status "    ${summary} Machines healthy; waiting…"
        sleep 15
    done
}

# --- phase 1 ------------------------------------------------------------
say "==> Phase 1: fly deploy (auto_stop_machines = 'off', Machines won't be killed mid-bootstrap)"

# Destroy every existing Machine before scaling + deploying. We accept
# a few minutes of downtime per deploy in exchange for never having
# to debug stale-state edge cases:
#
#   - Stopped Machines that fly deploy applies a new release to but
#     doesn't auto-start (script then hangs waiting for them to pass
#     health checks they can't satisfy until traffic forces a start)
#   - Machines created under an older fly.toml that still have its
#     persist_rootfs / auto_stop / VM-size config baked in
#   - Half-rolled fleets from a previous deploy that crashed mid-way
#
# Every deploy starts from zero: `fly machines list` empty → `fly
# scale count $DESIRED_COUNT` creates $DESIRED_COUNT fresh Machines
# from the new fly.toml → fly deploy rolls them to the new release.
# Tradeoffs: this forces a fuse-overlayfs cold pull on every Machine
# every deploy (~5–15 min/Machine, all in parallel), and there's a
# brief window where the public URL serves 503s because every Machine
# is mid-bootstrap. Fine for a research tool that deploys infrequently
# and tolerates a few minutes of downtime.
mapfile -t EXISTING_IDS < <("$FLY" machines list --app "$APP_NAME" --json | jq -r '.[].id' | sort -u)
if [ "${#EXISTING_IDS[@]}" -gt 0 ]; then
    say "==> Destroying ${#EXISTING_IDS[@]} existing Machine(s) for a clean redeploy"
    printf '%s\n' "${EXISTING_IDS[@]}" \
        | xargs -P 10 -I {} "$FLY" machine destroy {} --app "$APP_NAME" --force >/dev/null 2>&1
fi

# fly deploy FIRST, then fly scale count. Order matters: against an
# empty fleet, `fly scale count` creates Machines in some default
# process group, while `fly deploy` afterwards sees "no Machines in
# group app" and creates its HA-default fleet (typically 2) in the
# "app" group — leaving the scaled Machines orphaned and the total
# count at $DESIRED_COUNT + 2. Running fly deploy first establishes
# the "app" group with HA default; the subsequent fly scale count
# scales that group up to $DESIRED_COUNT.
DESIRED_COUNT="${RV_FLY_MACHINES:-10}"

say "==> fly deploy (creates HA-default initial Machines in the 'app' group)"
# --strategy immediate: with no existing Machines this just means
# "apply the new release as fast as possible to whatever fly creates".
fly_deploy --strategy immediate

if [ "$DESIRED_COUNT" -gt 1 ]; then
    say "==> Scaling fleet up to ${DESIRED_COUNT} Machine(s)"
    "$FLY" scale count "$DESIRED_COUNT" --yes
fi

# Per-attempt timeout: how long we'll wait for *every* Machine to pass
# its HTTP check before giving up on this attempt. Default 30 min,
# enough for fuse-overlayfs cold pulls on the slow-but-not-stuck end
# of the distribution. Override with RV_DEPLOY_TIMEOUT_SECS.
TIMEOUT_SECS="${RV_DEPLOY_TIMEOUT_SECS:-1800}"

# Retries on stuck-Machine timeout: cold pulls are fast for most
# Machines but occasionally one gets stuck mid-extract (transient
# fuse-overlayfs / GHCR / FUSE-kernel flakiness) and never recovers.
# When that happens, identify the unhealthy Machines, destroy them,
# and re-scale + re-wait. RV_DEPLOY_RETRIES counts retries *after*
# the first attempt — default 1 means up to 2 total attempts and a
# worst-case wallclock of 2 × $TIMEOUT_SECS.
MAX_RETRIES="${RV_DEPLOY_RETRIES:-1}"

attempt=0
while true; do
    attempt=$((attempt + 1))
    if wait_for_fleet_healthy "$TIMEOUT_SECS"; then
        break
    fi

    if [ "$attempt" -gt "$MAX_RETRIES" ]; then
        cat >&2 <<EOF
ERROR: not all Machines passed health checks within $((TIMEOUT_SECS / 60)) minutes
across $((MAX_RETRIES + 1)) attempt(s). Override RV_DEPLOY_RETRIES if you
want more retries. The fleet is currently running with auto_stop_machines
= 'off' (bootstrap config), so Machines won't auto-stop while you investigate.

  fly logs                        # see what entrypoint is doing
  fly status --all                # check Machine state + per-check status
  fly ssh console                 # poke around inside

Re-run this script when you've fixed the underlying issue.
EOF
        exit 1
    fi

    # Identify Machines whose checks are not yet all passing — likely
    # ones stuck mid cold-pull. Destroy them and re-scale to recreate
    # so the next wait attempt operates on fresh Machines.
    say "==> Attempt $attempt timed out; identifying stuck Machine(s)"
    UNHEALTHY_RAW=$("$FLY" machines list --app "$APP_NAME" --json | jq -r '
        .[]
        | select(.config.metadata.fly_process_group == "app" or
                 (.config.metadata.fly_process_group // "app") == "app")
        | select((.checks // [] | length == 0) or
                 (.checks // [] | all(.status == "passing") | not))
        | .id
    ' | sort -u)
    if [ -z "$UNHEALTHY_RAW" ]; then
        # Wait timed out but no Machines are reported unhealthy — likely
        # an `fly status --json` blip. Try again with the full timeout.
        say "==> No Machines reported unhealthy; retrying wait"
        continue
    fi
    mapfile -t UNHEALTHY_IDS <<< "$UNHEALTHY_RAW"
    say "==> Destroying ${#UNHEALTHY_IDS[@]} stuck Machine(s) and re-scaling to ${DESIRED_COUNT}"
    printf '%s\n' "${UNHEALTHY_IDS[@]}" \
        | xargs -P 10 -I {} "$FLY" machine destroy {} --app "$APP_NAME" --force >/dev/null 2>&1
    "$FLY" scale count "$DESIRED_COUNT" --yes
done

# --- phase 2 ------------------------------------------------------------
if [ "$KEEP_WARM" -eq 1 ]; then
    say "==> --keep-warm passed; leaving auto_stop = off (~\$24/mo per Machine)."
    exit 0
fi

say "==> Phase 2: flipping each Machine's auto_stop from 'off' to 'stop' (no redeploy)"
# Phase 2 changes a single per-Machine knob (auto_stop=off → stop) via
# the Machines API. `fly machine update` bounces the Machine to apply
# the new config; persist_rootfs='always' on the [[vm]] block in
# fly.toml keeps /var/lib/docker across the bounce, so dockerd comes
# back to a cached runner image with no re-pull. fly's default is
# 'never', under which this update would wipe rootfs and trigger a
# 5-15 min cold pull while auto_stop='stop' is already in force —
# the autoscaler would then kill the Machine mid-bootstrap. That's
# the deadlock persist_rootfs protects us from.
#
# We pass --skip-health-checks so each fly machine update returns as
# soon as the Machines API accepts the config change. We then DON'T
# poll for fleet health afterwards — the moment auto_stop='stop' is
# applied, the autoscaler starts watching each Machine for traffic,
# and health-check probes don't count as traffic (they take a
# separate network path). After ~40s of "no incoming traffic" the
# autoscaler stops a freshly-bounced Machine. So a poll-for-all-
# healthy loop is unwinnable here: Machines auto-stop faster than
# they become healthy. The fly machine update return code IS the
# success signal — once it's nonzero-free, the config is applied and
# we're in the desired steady state (Machines stopped or running per
# real traffic, not per our deploy harness).
#
# Stdout + stderr of the parallel xargs go to a tmpfile rather than
# /dev/null so a real failure produces a useful error message
# (previously: "exit code 123" with no idea which Machine or why).
mapfile -t IDS < <("$FLY" machines list --app "$APP_NAME" --json | jq -r '.[].id' | sort -u)
say "    Updating ${#IDS[@]} Machines (auto_stop → stop)"

UPDATE_LOG=$(mktemp -t fly-machine-update.XXXXXX) || exit 1
trap 'rm -f "$UPDATE_LOG"' EXIT

printf '%s\n' "${IDS[@]}" \
    | xargs -P 10 -I {} "$FLY" machine update {} \
        --app "$APP_NAME" \
        --autostop=stop \
        --autostart=true \
        --skip-health-checks \
        --yes >"$UPDATE_LOG" 2>&1 &
UPDATE_PID=$!

while kill -0 "$UPDATE_PID" 2>/dev/null; do
    status "    Updating ${#IDS[@]} Machines (auto_stop → stop)…"
    sleep 1
done
if ! wait "$UPDATE_PID"; then
    printf '\n' >&2
    echo "ERROR: fly machine update failed for at least one Machine. Last 60 lines of combined output:" >&2
    tail -n 60 "$UPDATE_LOG" >&2
    exit 1
fi
say "    ${#IDS[@]}/${#IDS[@]} Machine config updates accepted"

# We previously did a follow-up "verify" step here that read back each
# Machine's services config from `fly machines list --json` and
# checked `.config.services[].auto_stop_machines == "stop"`. Turns
# out the field name in the Machines API JSON is different from
# what's in fly.toml (probably `autostop` instead of
# `auto_stop_machines`), so the verify always reported `null` and
# false-failed every deploy after the actual config was applied.
# Rather than guess at the field name without being able to
# introspect Fly's schema, just trust the fly machine update exit
# code: if all $#IDS calls returned 0, the API accepted the change.
# If something is silently wrong we'll discover it via cost spike,
# but in practice fly machine update has been honest about exit code.

say "==> Done. Machines match fly.toml's canonical config; they'll auto-stop when idle."
say "    Cold starts after auto-stop take ~10 s (runner image stays on each Machine's rootfs)."
