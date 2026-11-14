// TEST FIXTURE ONLY. Synthetic IPC responses exercise real desktop UI modules;
// these results say nothing about Rust, filesystem, Git, or provider behavior.
(() => {
  const params = new URLSearchParams(location.search);
  const initialView = params.get('view') === 'session' ? 'session' : 'new';
  const session = { id: 'fixture-hello', title: 'Hello', cwd: '/home/user/Novel', provider: 'fixture', model: 'fixture-model', mode: 'ecosage', effort: 'medium', autonomy: 'auto', simulated: false, created: 1730000000000, updated: 1730000000000, status: 'closed', outcome: null, verified: false, stats: {}, skills: [] };
  const events = [{ type: 'session_start', seq: 1, ts: session.created, title: session.title, cwd: session.cwd, provider: session.provider, model: session.model, mode: session.mode }, { type: 'user', seq: 2, ts: session.created + 1, text: 'Hello', files: [] }];
  const state = { version: 'browser-fixture', home: '/home/user', data_dir: '/home/user/.sage', provider: 'fixture', model: 'fixture-model', effort: 'medium', auto_retry: false, providers: [{ id: 'fixture', name: 'Fixture provider', serves: 'Synthetic browser fixture', base_url: 'https://example.invalid', key_env: '', has_key: true, models: ['fixture-model'], note: 'Not a real connection', custom: false }] };
  const calls = [];
  window.uiFixture = { disclaimer: 'Mock IPC browser fixture; no backend behavior is exercised.', calls, session };
  window.uiTestResults = { status: 'not-run', checks: [] };
  const handlers = {
    ui_hooks: () => ({ view: initialView, theme: 'none', scheme: params.get('scheme') || 'light' }),
    app_state: () => state,
    list_sessions: () => [session],
    session_events: () => events,
    subscribe: () => events,
    list_skills: () => [],
    instructions: () => ({ system: 'Browser fixture system prompt.', system_default: 'Browser fixture system prompt.', system_custom: false, global: '' }),
    git_state: () => ({ repo: false, branch: null, remote: null, url: null, github: false, upstream: null, ahead: 0, files: [], commits: [] }),
    list_dir: () => [],
    pick_folder: () => null,
    pick_files: () => [],
  };
  const noop = async () => {};
  window.__TAURI__ = {
    core: { Channel: class { onmessage = noop; }, invoke: async (command, args) => {
      calls.push({ command, args, view: document.documentElement.dataset.view });
      if (!Object.hasOwn(handlers, command)) throw new Error(`No browser fixture handler: ${command}`);
      return structuredClone(handlers[command](args));
    } },
    window: { getCurrentWindow: () => ({ minimize: noop, toggleMaximize: noop, close: noop, onCloseRequested: noop, isMaximized: async () => false }) },
  };

  if (!params.has('test')) return;
  const results = window.uiTestResults = { status: 'running', disclaimer: window.uiFixture.disclaimer, initialView, viewport: { width: innerWidth, height: innerHeight }, checks: [] };
  const sleep = (ms) => new Promise(resolve => setTimeout(resolve, ms));
  async function waitFor(predicate, label) {
    const deadline = performance.now() + 5000;
    while (performance.now() < deadline) { if (predicate()) return; await sleep(25); }
    throw new Error(`Timed out: ${label}`);
  }
  const settle = async () => { await new Promise(requestAnimationFrame); await new Promise(requestAnimationFrame); await sleep(30); };
  const expect = (condition, detail) => { if (!condition) throw new Error(detail); };
  const rect = (selector) => { const el = document.querySelector(selector); expect(el, `Missing ${selector}`); return el.getBoundingClientRect(); };
  async function check(name, run) { try { const detail = await run(); results.checks.push({ name, pass: true, detail: detail ?? 'Passed' }); } catch (error) { results.checks.push({ name, pass: false, detail: error.message }); } }
  async function go(selector, view) { const button = document.querySelector(selector); expect(button, `Missing ${selector}`); button.click(); await waitFor(() => document.documentElement.dataset.view === view, view); await settle(); }
  const panelCounts = () => ({ tabs: document.querySelectorAll('#inspector-tabs [role="tab"]').length, tablists: document.querySelectorAll('#inspector-tabs [role="tablist"]').length, resizers: document.querySelectorAll('#inspector > .pane-resize').length });
  const expectOnePanel = () => { const count = panelCounts(); expect(count.tabs === 3 && count.tablists === 1 && count.resizers === 1, `Expected 3 tabs / 1 tablist / 1 resizer, got ${JSON.stringify(count)}`); return count; };
  async function run() {
    await waitFor(() => document.querySelector('#session-list [data-session]') && document.querySelector('#main').children.length, 'app mounted');
    await document.fonts.ready;
    await settle();
    if (document.documentElement.dataset.inspector !== 'on') { document.querySelector('#toggle-inspector').click(); await settle(); }
    await go('#new-session', 'new');
    await check('No-session inspector has one complete panel', () => JSON.stringify(expectOnePanel()));
    for (const id of ['economy', 'code', 'github']) await check(`No-session ${id} tab is clickable and empty`, async () => {
      const button = document.querySelector(`#tab-${id}`);
      expect(button && !button.disabled, `Missing or disabled ${id} tab`);
      const box = button.getBoundingClientRect();
      expect(box.width > 0 && box.height > 0 && button.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)), `${id} tab is not visible/hit-testable`);
      button.click(); await settle();
      expect(button.getAttribute('aria-selected') === 'true' && document.querySelector('#inspector-body').dataset.view === id, `${id} tab did not select`);
      expect(document.querySelector('#inspector-body').textContent.includes('Start or choose a session to see anything here.'), 'Missing no-session empty text');
      return 'Selected and showed no-session text';
    });
    await check('No-session never asks for files or Git state', () => {
      const invalid = calls.filter(call => ['list_dir', 'git_state'].includes(call.command) && call.view !== 'session');
      expect(!invalid.length, `Unexpected no-session IPC: ${JSON.stringify(invalid)}`);
      return 'No list_dir/git_state calls outside a session';
    });
    await check('New/settings/session navigation keeps one panel and resizer', async () => {
      const snapshots = [];
      for (const [selector, view] of [['#open-settings', 'settings'], ['#new-session', 'new'], ['[data-session="fixture-hello"]', 'session'], ['#open-settings', 'settings'], ['[data-session="fixture-hello"]', 'session'], ['#new-session', 'new']]) {
        await go(selector, view); snapshots.push({ view, ...expectOnePanel() });
      }
      return snapshots;
    });
    await go('[data-session="fixture-hello"]', 'session');
    await waitFor(() => document.querySelector('.timeline .user-text'), 'Hello user bubble');
    await settle();
    await check('Active-session tabs still render their own content', async () => {
      for (const id of ['economy', 'code', 'github', 'economy']) {
        const button = document.querySelector(`#tab-${id}`);
        button.click(); await settle();
        const body = document.querySelector('#inspector-body');
        expect(button.getAttribute('aria-selected') === 'true' && body.dataset.view === id && body.children.length > 0, `${id} did not render its session view`);
        expect(!body.textContent.includes('Start or choose a session'), `${id} incorrectly rendered the sessionless placeholder`);
      }
      return 'Economy, Code and GitHub render for Hello; returned to Economy';
    });
    await check('Hello session title aligns with the visible folder chevron', () => {
      const title = rect('.session-item .session-title'), chev = rect('.folder-toggle svg.chev path');
      const delta = Math.abs(title.left - chev.left); expect(delta <= 0.1, `Title left ${title.left}px; visible chevron left ${chev.left}px; difference ${delta}px`); return { titleLeft: title.left, chevronLeft: chev.left, delta };
    });
    await check('Sidebar rows have symmetric outer insets of at least 12px', () => {
      const sidebar = rect('#sidebar');
      return ['#new-session', '.folder-toggle', '.session-item', '#open-settings'].map(selector => {
        const row = rect(selector), left = row.left - sidebar.left, right = sidebar.right - row.right;
        expect(left >= 12 && right >= 12 && Math.abs(left - right) <= 1, `${selector}: ${left}px left / ${right}px right`);
        return { selector, left, right };
      });
    });
    await check('File menu uncheckable rows have symmetric padding', async () => {
      document.querySelector('#menu-file').click(); await settle();
      const rows = [...document.querySelectorAll('.bar-menu .menu-item')]; expect(rows.length > 0, 'File menu did not open');
      const measured = rows.map(row => { const style = getComputedStyle(row); const left = parseFloat(style.paddingLeft), right = parseFloat(style.paddingRight); expect(!row.hasAttribute('aria-checked') && Math.abs(left - right) <= 1, `${row.textContent}: ${left}px left / ${right}px right`); return { text: row.querySelector('.menu-text').textContent, left, right }; });
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return measured;
    });
    await check('View menu preserves checkbox gutter', async () => {
      document.querySelector('#menu-view').click(); await settle();
      const rows = [...document.querySelectorAll('.bar-menu [role="menuitemcheckbox"]')]; expect(rows.length > 0, 'Missing checkbox rows');
      const measured = rows.map(row => { const style = getComputedStyle(row), left = parseFloat(style.paddingLeft), right = parseFloat(style.paddingRight); expect(left >= right + 12, `${row.textContent}: checkbox gutter removed (${left}px / ${right}px)`); return { text: row.querySelector('.menu-text').textContent, left, right }; });
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return measured;
    });
    await check('First Hello bubble clears inspector toggle by at least 12px', () => {
      const bubble = rect('.timeline .user-text'), toggle = rect('#toggle-inspector');
      const overlaps = bubble.left < toggle.right && bubble.right > toggle.left && bubble.top < toggle.bottom && bubble.bottom > toggle.top;
      const gapX = Math.max(toggle.left - bubble.right, bubble.left - toggle.right, 0), gapY = Math.max(toggle.top - bubble.bottom, bubble.top - toggle.bottom, 0), gap = Math.hypot(gapX, gapY);
      expect(!overlaps && gap >= 12, `Bubble overlaps=${overlaps}; nearest gap=${gap.toFixed(2)}px`);
      return { overlap: overlaps, gap, bubble: bubble.toJSON(), toggle: toggle.toJSON() };
    });
    await check('Chat stays horizontally clear of toggle while scrolling', () => {
      const bubble = rect('.timeline .user-text'), toggle = rect('#toggle-inspector');
      const horizontalGap = toggle.left - bubble.right;
      expect(horizontalGap >= 12, `Only ${horizontalGap}px of horizontal clearance; a scrolled message could crowd the toggle`);
      return { horizontalGap };
    });
    await check('No-session IPC remains clean after lifecycle', () => {
      const invalid = calls.filter(call => ['list_dir', 'git_state'].includes(call.command) && call.view !== 'session');
      expect(!invalid.length, `Unexpected IPC ${JSON.stringify(invalid)}`); return `${calls.length} mock IPC calls inspected`;
    });
    // Preserve requested landing view after assertions for repeatable screenshots.
    await go(initialView === 'session' ? '[data-session="fixture-hello"]' : '#new-session', initialView);
    results.passed = results.checks.filter(check => check.pass).length;
    results.failed = results.checks.length - results.passed;
    results.status = 'complete';
    document.documentElement.dataset.uiTests = results.failed ? 'failed' : 'passed';
    window.dispatchEvent(new CustomEvent('ui-tests-complete', { detail: results }));
  }
  window.addEventListener('load', () => run().catch(error => { results.status = 'error'; results.error = error.message; document.documentElement.dataset.uiTests = 'error'; }));
})();
