import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// Read the workspace's pinned Rust toolchain at build time so the
// playground can show users which rustc version their snippet is being
// compiled against. The same channel is what every Fly Machine ends up
// running via rust-toolchain.toml. We hand it to the SPA via Vite's
// `define` (text replacement at build time) rather than at runtime so
// there's no extra request and the GitHub Pages build doesn't need a
// matching API endpoint.
const rustToolchainPath = resolve(__dirname, '../../rust-toolchain.toml');
const rustToolchainContent = readFileSync(rustToolchainPath, 'utf-8');
const channelMatch = rustToolchainContent.match(/channel\s*=\s*"([^"]+)"/);
const rustVersion = channelMatch ? channelMatch[1] : 'unknown';

// Two build modes:
//   * default (`npm run build`): emits a same-origin SPA at base = '/'
//     intended to be served by rv-serve from frontend/dist/.
//   * pages (`npm run build:pages`, --mode pages): emits an SPA at
//     base = '/playground/' intended to be served by GitHub Pages at
//     https://rustviz.github.io/playground/, with API requests pointed
//     at the Fly origin via VITE_API_BASE in .env.pages.
//
// `npm run dev` proxies API + asset paths to a locally-running rv-serve
// at :8080 so you can iterate without CORS plumbing.
export default defineConfig(({ mode }) => ({
  base: mode === 'pages' ? '/playground/' : '/',
  plugins: [react()],
  define: {
    __RUST_VERSION__: JSON.stringify(rustVersion),
  },
  server: {
    port: 3000,
    proxy: {
      '/submit-code': 'http://127.0.0.1:8080',
      '/ex-assets': 'http://127.0.0.1:8080',
    },
  },
  build: {
    outDir: 'dist',
    sourcemap: true,
  },
}));
