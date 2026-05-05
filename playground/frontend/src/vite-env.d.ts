/// <reference types="vite/client" />

// Injected at build time from rust-toolchain.toml's [toolchain].channel
// via Vite's `define` (see vite.config.ts).
declare const __RUST_VERSION__: string;
