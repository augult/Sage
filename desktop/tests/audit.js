// Browser-only presentation regressions. Run after the original lifecycle suite.
// These exercise the real UI with mock IPC, not native/backend behavior.
(() => {
  const params = new URLSearchParams(location.search);
  if (!params.has('audit')) return;
  const results = window.uiAuditResults = { status: 'waiting', checks: [] };
  const settle = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const expect = (ok, message) => { if (!ok) throw new Error(message); };
  const visible = el => el && el.getBoundingClientRect().width > 0;
  async function check(name, run) {
    try { results.checks.push({ name, pass: true, detail: await run() || 'Passed' }); }
    catch (error) { results.checks.push({ name, pass: false, detail: error.message }); }
  }
  async function run() {
    results.status = 'running';
    document.querySelector('#open-settings').click(); await settle();
    document.querySelector('#tab-code').click(); await settle();
    await check('Settings has no horizontal overflow with both panels open', () => {
      const body = document.querySelector('.view-body');
      expect(body.scrollWidth <= body.clientWidth + 1, `${body.scrollWidth}px content in ${body.clientWidth}px view`);
    });
    await check('Provider address and key field remain readable', () => {
      const url = document.querySelector('.provider-url'), input = document.querySelector('.provider input');
      expect(url.getBoundingClientRect().width >= 120, `Provider address width: ${url.getBoundingClientRect().width}px`);
      expect(input.getBoundingClientRect().width >= 120, `Key input width: ${input.getBoundingClientRect().width}px`);
    });
    await check('Settings controls fit their form rows', () => {
      const body = document.querySelector('.view-body').getBoundingClientRect();
      for (const el of document.querySelectorAll('.field-body > *')) {
        const box = el.getBoundingClientRect();
        expect(box.right <= body.right + 1, `${el.className} reaches ${box.right}px past view ${body.right}px`);
      }
    });
    document.querySelector('#add-provider').click(); await settle();
    await check('Provider dialog scrolls without hiding its actions', () => {
      const dialog = document.querySelector('dialog[open]'), form = dialog.querySelector('form');
      expect(form.scrollHeight <= form.clientHeight + 1 || ['auto', 'scroll'].includes(getComputedStyle(form).overflowY), `Form clips ${form.scrollHeight - form.clientHeight}px without scrolling`);
      const foot = dialog.querySelector('.dialog-foot').getBoundingClientRect();
      expect(foot.bottom <= innerHeight - 8 && foot.top >= 0, `Dialog actions off-screen: ${foot.top}..${foot.bottom}`);
    });
    document.querySelector('dialog[open]').close(); await settle();
    document.querySelector('#new-session').click(); await settle();
    await check('Composer actions fit at the minimum workspace width', () => {
      const foot = document.querySelector('.composer-foot').getBoundingClientRect();
      const sides = [...document.querySelectorAll('.composer-side')].map(el => el.getBoundingClientRect());
      expect(sides[0].right <= sides[1].left + 1, `Composer sides overlap by ${sides[0].right - sides[1].left}px`);
      for (const button of [...document.querySelectorAll('.composer-foot button')].filter(visible)) {
        const box = button.getBoundingClientRect();
        expect(box.left >= foot.left - 1 && box.right <= foot.right + 1, `${button.id} clips outside composer`);
      }
    });
    await check('Picker menu stays in the viewport', async () => {
      document.querySelector('#pick-mode').click(); await settle();
      const menu = document.querySelector('#pick-mode-menu'), box = menu.getBoundingClientRect();
      expect(box.left >= 4 && box.right <= innerWidth - 4 && box.top >= 4 && box.bottom <= innerHeight - 4, `Menu outside viewport: ${JSON.stringify(box.toJSON())}`);
      const item = menu.querySelector('.menu-item'); const at = item.getBoundingClientRect();
      expect(item.contains(document.elementFromPoint(at.x + at.width / 2, at.y + at.height / 2)), 'Menu clipped by a scroll container');
      menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    });
    await check('Native title tooltips are absent', () => {
      const titles = [...document.querySelectorAll('[title]')].filter(visible);
      expect(!titles.length, `${titles.length} native titles remain: ${titles.map(el => el.id || el.className).join(', ')}`);
    });
    // Reset after assertions for reproducible screenshots.
    document.querySelector('#tab-economy').click(); document.querySelector('#new-session').click(); await settle();
    results.passed = results.checks.filter(check => check.pass).length;
    results.failed = results.checks.length - results.passed;
    results.status = 'complete';
    window.dispatchEvent(new CustomEvent('ui-audit-complete', { detail: results }));
  }
  window.addEventListener('ui-tests-complete', () => run().catch(error => { results.status = 'error'; results.error = error.message; }), { once: true });
})();
