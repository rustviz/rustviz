## What and why

<!-- One concern per PR. Link the issue: "Fixes #N" / "Part of #N". -->

## Before / after

<!-- For plugin changes: the tooltip text (or rendering) a student sees before and after, per case. -->

## Checklist

- [ ] New or changed tooltips are correct Rust semantics, not just "what the plugin prints now"
- [ ] `EXPECTED_TOOLTIPS` entries include `must_not_contain` for the old false text, and fail on the old plugin
- [ ] Reinstalled the plugin before running `cargo test -p rustviz-lib`
- [ ] If this changes what RustViz supports: a fixed gap's issue is closed ("Fixes #N"), a newly found gap is filed as an issue labelled `limitation` (the README and playground panel stay short summaries; touch them only if a top-level gap changes)
- [ ] Crashes: a new crash got an issue labelled `crash`; a crash fix says "Fixes #N" and adds the repro as a fixture (crashes aren't listed in the playground panel)
