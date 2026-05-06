/// <reference types="vite/client" />

// Injected at build time from rust-toolchain.toml's [toolchain].channel
// via Vite's `define` (see vite.config.ts).
declare const __RUST_VERSION__: string;

// Injected at build time from rustviz-plugin/Cargo.toml's [package].version.
// This is the version of the plugin the playground's compile backend was
// built against; it bumps in lockstep with the published rustviz-plugin
// crate via scripts/bump-version.sh.
declare const __PLUGIN_VERSION__: string;
