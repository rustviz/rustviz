// shoot.mjs OUT.png FILE.html [width]: full-page screenshot of a local HTML file (headless Chrome via
// maintainer-tools' playwright, $RV_TOOLS_HOME/review-board/crop-lib.mjs).
import path from 'node:path';
const TOOLS = process.env.RV_TOOLS_HOME || '/Users/cyrus/projects/rustviz/maintainer-tools';
const { launch } = await import(path.join(TOOLS, 'review-board/crop-lib.mjs'));
const [out, file, w] = process.argv.slice(2);
const b = await launch();
const p = await b.newPage({ viewport: { width: +(w || 1600), height: 800 }, deviceScaleFactor: 1.5 });
await p.goto('file://' + path.resolve(file));
await p.waitForTimeout(300);
await p.screenshot({ path: out, fullPage: true });
await b.close();
console.log(out);
