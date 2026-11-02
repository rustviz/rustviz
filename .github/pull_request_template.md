## What and why

<!-- One concern per PR. Link the issue: "Fixes #N" / "Part of #N". -->

## Before / after

<!-- For plugin changes: the tooltip text (or rendering) a student sees before and after, per case. -->

## Checklist

- [ ] New or changed tooltips are correct Rust semantics, not just "what the plugin prints now"
- [ ] `EXPECTED_TOOLTIPS` entries include `must_not_contain` for the old false text, and fail on the old plugin
- [ ] Reinstalled the plugin before running `cargo test -p rustviz-lib`
- [ ] If this changes what RustViz supports: updated **all three** limitations lists (`README.md`, `rustviz-plugin/README.md`, the playground's Known limitations panel in `playground/frontend/src/App.tsx`)
