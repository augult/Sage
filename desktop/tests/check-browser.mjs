import { once } from 'node:events';
import { createRequire } from 'node:module';

const { chromium } = createRequire(import.meta.url)('playwright');
process.env.HOST = '127.0.0.1';
process.env.PORT = '0';
const { server } = await import('./serve.mjs');

let browser;
try {
  if (!server.listening) await once(server, 'listening');
  const { port } = server.address();
  browser = await chromium.launch({ executablePath: process.env.PLAYWRIGHT_EXECUTABLE });
  for (const [width, height] of [[940, 600], [1320, 860], [1920, 1080], [2560, 1440], [3840, 2160]]) {
    for (const scheme of ['light', 'dark']) {
      const page = await browser.newPage({ viewport: { width, height }, reducedMotion: 'reduce' });
      const label = `${width}×${height} ${scheme}`;
      const errors = [];
      page.on('pageerror', (error) => errors.push(error.message));
      try {
        const view = scheme === 'dark' ? 'session' : 'new';
        await page.goto(`http://127.0.0.1:${port}/?test&audit&rich&scheme=${scheme}&view=${view}`);
        await page.waitForFunction(() => [window.uiTestResults, window.uiAuditResults]
          .some((suite) => suite?.status === 'error') || window.uiAuditResults?.status === 'complete', null, { timeout: 45000 });
        const suites = await page.evaluate(() => [window.uiTestResults, window.uiAuditResults]);
        const failed = suites.flatMap((suite) => suite.checks.filter((check) => !check.pass));
        if (failed.length || errors.length || suites.some((suite) => suite.status !== 'complete')) {
          process.exitCode = 1;
          console.error(label, ...errors, ...failed.map(({ name, detail }) => `${name}: ${detail}`), ...suites.map((suite) => suite.error ?? ''));
        } else {
          console.log(`${label}: ${suites.reduce((count, suite) => count + suite.passed, 0)} checks passed`);
        }
      } catch (error) {
        process.exitCode = 1;
        console.error(label, error.message);
      } finally {
        await page.close();
      }
    }
  }
} finally {
  await browser?.close();
  server.close();
}
