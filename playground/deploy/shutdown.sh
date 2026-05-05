#!/usr/bin/env bash
# Destroy all Machines for the RustViz playground Fly app.
#
# Useful for:
#   * a clean deploy.sh test — next deploy starts every Machine from a
#     fresh rootfs and exercises the cold-pull path through fuse-overlayfs
#   * a cost emergency where you want zero compute capacity immediately
#
# Doesn't touch the app itself, the IP, or any other Fly resources. Just
# zeros the Machine count. Re-run ./deploy/deploy.sh to bring the fleet
# back.
#
# Why we don't use `fly scale count 0`:
#   `fly scale count 0` cooperatively acquires each Machine's lease before
#   destroying it. If a previous deploy was interrupted and left a stale
#   lease behind, the command hangs indefinitely on "Waiting on lease for
#   machine ...". `fly machine destroy --force` bypasses the lease, which
#   is what we always want when the operator's intent is "tear it down".
#
# Usage:
#   deploy/shutdown.sh           # interactive confirmation
#   deploy/shutdown.sh --yes     # skip the confirmation prompt

set -euo pipefail

# fly.toml lives next to this script. cd here so fly subcommands
# (machine destroy, list, …) resolve the app name from cwd.
cd "$(dirname "$0")"

YES=0
for arg in "$@"; do
    case "$arg" in
        --yes|-y) YES=1 ;;
        -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
        *) echo "Unknown argument: $arg" >&2; exit 2 ;;
    esac
done

# Resolve fly CLI: locally `brew install flyctl` provides both names,
# but CI installers (e.g. superfly/flyctl-actions) provide only `flyctl`.
if command -v fly >/dev/null 2>&1; then
    FLY=fly
elif command -v flyctl >/dev/null 2>&1; then
    FLY=flyctl
else
    echo "fly CLI not on PATH" >&2
    exit 1
fi
command -v jq  >/dev/null 2>&1 || { echo "jq not on PATH; brew install jq" >&2; exit 1; }
# `fly auth whoami` reads ~/.fly/config.yml (populated by
# `fly auth login`), not FLY_API_TOKEN — so it returns "not logged in"
# in CI even though the rest of flyctl is authenticated via the env
# var. Skip the preflight when the token is set, and strip any
# leading/trailing whitespace from it (a stray trailing newline picked
# up by `gh secret set` from stdin makes flyctl send a malformed
# Authorization header).
if [ -n "${FLY_API_TOKEN:-}" ]; then
    # Strip ONLY CR/LF, not all whitespace — `fly tokens create deploy`
    # returns `FlyV1 fm2_…` and the space between the prefix and the
    # body is part of the token format. See deploy.sh for the longer
    # version of this comment.
    FLY_API_TOKEN=$(printf '%s' "$FLY_API_TOKEN" | tr -d '\r\n')
    export FLY_API_TOKEN
else
    "$FLY" auth whoami >/dev/null 2>&1 || { echo "Not logged in. Run '$FLY auth login' first or set FLY_API_TOKEN." >&2; exit 1; }
fi

APP_NAME=$(awk -F"'" '/^app =/ {print $2; exit}' fly.toml)

# `sort -u` because Fly's API occasionally returns the same Machine id
# twice (same bug deploy.sh's jq query works around with unique_by(.id)).
mapfile -t IDS < <("$FLY" machines list --app "$APP_NAME" --json | jq -r '.[].id' | sort -u)

if [ "${#IDS[@]}" -eq 0 ]; then
    echo "No Machines exist for ${APP_NAME}; nothing to do."
    exit 0
fi

echo "About to force-destroy ${#IDS[@]} Machine(s) for Fly app: ${APP_NAME}"
echo "(The app, IP, and other resources are left intact. Only Machines go.)"
echo "Re-run ./deploy/deploy.sh afterwards to rebuild the fleet."
echo

if [ "$YES" -ne 1 ]; then
    read -rp "Type the app name to confirm: " confirm
    [ "$confirm" = "$APP_NAME" ] || { echo "Confirmation didn't match; aborting." >&2; exit 1; }
fi

# Destroy in parallel — each `fly machine destroy --force` is independent
# and the API handles the concurrency fine. With 10+ Machines this is the
# difference between ~5 s and ~minute of wallclock.
printf '%s\n' "${IDS[@]}" | xargs -P 10 -I {} "$FLY" machine destroy {} --app "$APP_NAME" --force

echo "==> All Machines destroyed."
