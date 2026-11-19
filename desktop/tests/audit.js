// Browser-only presentation regressions. Run after the original lifecycle suite.
// These exercise the real UI with mock IPC, not native/backend behavior.
(() => {
  const params = new URLSearchParams(location.search);
  if (!params.has('audit')) return;
  const results = window.uiAuditResults = { status: 'waiting', checks: [] };
  const settle = async () => {
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    // Entry transforms can emit a final ancestor scroll; wait for finite motion
    // before hovering. A scroll is intentionally supposed to dismiss tooltips.
    const animations = document.getAnimations().filter(animation => Number.isFinite(animation.effect?.getTiming().iterations));
    await Promise.allSettled(animations.map(animation => animation.finished));
    await new Promise(requestAnimationFrame);
  };
  const expect = (ok, message) => { if (!ok) throw new Error(message); };
  const visible = el => el && el.getBoundingClientRect().width > 0;
  async function waitFor(predicate) {
    const deadline = performance.now() + 3000;
    while (performance.now() < deadline) {
      if (predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 20));
    }
    throw new Error('UI did not reach the expected state');
  }
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
    await check('Scrolled Settings action clears the floating inspector toggle', async () => {
      document.querySelector('#settings-keys').scrollIntoView(); await settle();
      const button = document.querySelector('#add-provider'), box = button.getBoundingClientRect();
      const toggle = document.querySelector('#toggle-inspector').getBoundingClientRect();
      expect(toggle.left - box.right >= 8, `Only ${toggle.left - box.right}px of clearance`);
      expect(button.contains(document.elementFromPoint(box.right - 5, box.top + 5)), 'Inspector toggle intercepts Add provider');
    });
    document.querySelector('#add-provider').click(); await settle();
    await check('Provider dialog scrolls without hiding its actions', () => {
      const dialog = document.querySelector('dialog[open]'), form = dialog.querySelector('form');
      expect(form.scrollHeight <= form.clientHeight + 1 || ['auto', 'scroll'].includes(getComputedStyle(form).overflowY), `Form clips ${form.scrollHeight - form.clientHeight}px without scrolling`);
      const foot = dialog.querySelector('.dialog-foot').getBoundingClientRect();
      expect(foot.bottom <= innerHeight - 8 && foot.top >= 0, `Dialog actions off-screen: ${foot.top}..${foot.bottom}`);
    });
    await check('Custom tooltip renders above a modal dialog', async () => {
      const dialog = document.querySelector('dialog[open]'), button = dialog.querySelector('.close');
      const events = [];
      const log = event => events.push(`${event.type}:${event.target.id || event.target.className}`);
      for (const type of ['scroll', 'focusout', 'pointerout']) document.addEventListener(type, log, true);
      button.dispatchEvent(new PointerEvent('pointerover', { bubbles: true, pointerType: 'mouse', relatedTarget: document.body }));
      try { await waitFor(() => document.querySelector('.ui-tooltip:not([hidden])')); }
      catch (error) { throw new Error(`${error.message}; modal events: ${events.join(', ')}; button: ${button.outerHTML}; tip: ${document.querySelector('.ui-tooltip')?.outerHTML}`); }
      finally { for (const type of ['scroll', 'focusout', 'pointerout']) document.removeEventListener(type, log, true); }
      expect(document.querySelector('.ui-tooltip').closest('dialog') === dialog, 'Modal tooltip is outside the active dialog');
      button.dispatchEvent(new PointerEvent('pointerout', { bubbles: true, pointerType: 'mouse', relatedTarget: document.body }));
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
      for (const id of ['effort', 'model', 'folder', 'autonomy']) {
        document.querySelector(`#pick-${id}`).click(); await settle();
        const menu = document.querySelector(`#pick-${id}-menu`), box = menu.getBoundingClientRect();
        expect(box.left >= 4 && box.right <= innerWidth - 4 && box.top >= 4 && box.bottom <= innerHeight - 4, `${id} menu outside viewport: ${JSON.stringify(box.toJSON())}`);
        const item = menu.querySelector('.menu-item'); const at = item.getBoundingClientRect();
        expect(item.contains(document.elementFromPoint(at.x + at.width / 2, at.y + at.height / 2)), `${id} menu clipped by a scroll container`);
        menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      }
    });
    await check('Native title tooltips are absent', () => {
      const titles = [...document.querySelectorAll('[title]')].filter(visible);
      expect(!titles.length, `${titles.length} native titles remain: ${titles.map(el => el.id || el.className).join(', ')}`);
    });
    const tooltip = () => document.querySelector('.ui-tooltip:not([hidden])');
    const enter = el => el.dispatchEvent(new PointerEvent('pointerover', { bubbles: true, pointerType: 'mouse', relatedTarget: document.body }));
    const leave = el => el.dispatchEvent(new PointerEvent('pointerout', { bubbles: true, pointerType: 'mouse', relatedTarget: document.body }));
    const escape = () => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await check('Tooltip waits briefly, then explains effort without a native title', async () => {
      document.activeElement?.blur(); escape();
      const effort = document.querySelector('#pick-effort');
      enter(effort);
      await new Promise(resolve => setTimeout(resolve, 50));
      expect(!tooltip(), 'Tooltip appeared during a quick pass-through');
      await waitFor(tooltip);
      expect(tooltip().textContent.includes('Balanced speed and detail'), tooltip().textContent);
      expect(!document.querySelectorAll('[title]').length, 'Native title appeared while hovered');
      expect(getComputedStyle(tooltip()).pointerEvents === 'none', 'Tooltip intercepts clicks');
      leave(effort); expect(!tooltip(), 'Pointer leave did not dismiss');
    });
    await check('Real edge controls have viewport-clamped tooltips', async () => {
      for (const selector of ['#toggle-sidebar', '#win-close', '#attach']) {
        const el = document.querySelector(selector); enter(el); await waitFor(tooltip);
        const box = tooltip().getBoundingClientRect();
        expect(box.left >= 7 && box.right <= innerWidth - 7 && box.top >= 7 && box.bottom <= innerHeight - 7, `${selector}: ${JSON.stringify(box.toJSON())}`);
        leave(el);
      }
    });
    await check('Tooltip dismisses on Escape, click, scroll and trigger removal', async () => {
      const el = document.querySelector('#attach');
      for (const dismiss of [escape, () => document.dispatchEvent(new Event('click', { bubbles: true })), () => document.dispatchEvent(new Event('scroll'))]) {
        enter(el); await waitFor(tooltip); dismiss(); expect(!tooltip(), 'Dismissal left the tooltip visible');
        expect(!el.getAttribute('aria-describedby')?.includes('ui-tooltip'), 'Stale description remained');
      }
      const transient = document.createElement('button');
      transient.dataset.tooltip = 'Temporary UI'; transient.textContent = 'Temporary';
      transient.style.cssText = 'position:fixed;left:100px;top:100px'; document.body.append(transient);
      enter(transient); await waitFor(tooltip); transient.remove(); await settle();
      expect(!tooltip(), 'Removed trigger left an orphan tooltip');
    });
    await check('Late native titles are migrated without a browser popup', async () => {
      const button = document.createElement('button'); button.textContent = 'Temporary';
      button.setAttribute('title', 'Late tooltip');
      button.style.cssText = 'position:fixed;left:100px;top:100px'; document.body.append(button);
      await settle();
      expect(!button.hasAttribute('title') && button.dataset.tooltip === 'Late tooltip', 'Late native title was not migrated');
      enter(button); await waitFor(tooltip); expect(tooltip().textContent === 'Late tooltip', 'Migrated tooltip did not render');
      button.remove(); await settle();
    });
    document.querySelector('#open-settings').click(); await settle();
    await check('Keyboard tooltip describes the focused radio, not just its wrapper', async () => {
      const radio = document.querySelector('input[name="default-effort"][value="medium"]');
      radio.scrollIntoView({ block: 'center' }); await settle();
      radio.focus({ preventScroll: true }); await waitFor(tooltip);
      expect(radio.getAttribute('aria-describedby')?.includes('ui-tooltip'), 'Focused radio has no tooltip description');
      escape(); expect(!radio.hasAttribute('aria-describedby'), 'Description not removed after Escape');
    });
    document.querySelector('#new-session').click(); await settle();
    await check('Inspector tabs support arrow-key navigation', () => {
      const first = document.querySelector('#tab-economy'); first.click(); first.focus();
      first.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
      expect(document.activeElement.id === 'tab-code' && document.querySelector('#tab-code').getAttribute('aria-selected') === 'true', 'ArrowRight did not select and focus Code');
      document.querySelector('#tab-code').dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true }));
      expect(document.activeElement.id === 'tab-economy', 'Home did not return to Economy');
    });
    if (params.has('rich')) {
      document.querySelector('[data-session="fixture-hello"]').click(); await settle();
      document.querySelector('#tab-code').click();
      await waitFor(() => document.querySelector('.tree-row[data-dir="false"]'));
      document.querySelector('.tree-row[data-dir="false"]').click();
      await waitFor(() => visible(document.querySelector('#code-text')));
      await check('Code tree leaves a readable editor in the narrow inspector', () => {
        const pane = document.querySelector('#inspector-body > .code').getBoundingClientRect();
        const tree = document.querySelector('.code-tree').getBoundingClientRect();
        const editor = document.querySelector('#code-text').getBoundingClientRect();
        expect(tree.width <= pane.width * 0.45, `Tree occupies ${tree.width}/${pane.width}px`);
        expect(editor.width >= 120, `Editor is only ${editor.width}px wide`);
      });
      await check('Many attachments leave the conversation and composer actions usable', async () => {
        const text = document.querySelector('#followup');
        const attach = document.querySelector('#attach');
        text.value = 'First line\nSecond\nThird\nFourth\nFifth';
        text.dispatchEvent(new Event('input', { bubbles: true }));
        attach.dispatchEvent(new CustomEvent('attach', {
          detail: Array.from({ length: 14 }, (_, i) => `/example/a-very-long-attachment-name-${i}.txt`),
        }));
        await settle();
        try {
          const send = document.querySelector('#send-followup');
          const { x, y, width, height } = send.getBoundingClientRect();
          expect(document.querySelector('.timeline-scroll').clientHeight > 0, 'Attachments collapsed the timeline');
          expect(send.contains(document.elementFromPoint(x + width / 2, y + height / 2)), 'Send is not reachable');
        } finally {
          let remove;
          while ((remove = document.querySelector('.attached .file-remove'))) remove.click();
          text.value = '';
          text.dispatchEvent(new Event('input', { bubbles: true }));
        }
      });
      document.querySelector('#tab-github').click();
      await waitFor(() => document.querySelector('.push-row'));
      await check('GitHub long paths stay within the inspector', () => {
        const pane = document.querySelector('#inspector-body').getBoundingClientRect();
        for (const el of document.querySelectorAll('.push-row, .push-target, .git-foot, .commit')) {
          const box = el.getBoundingClientRect();
          expect(box.right <= pane.right + 1 && box.left >= pane.left - 1, `${el.className} overflows inspector`);
        }
      });
      await check('A delayed refresh stays with the file it originally read', async () => {
        const editor = () => document.querySelector('#code-text');
        let finishRefresh;
        window.uiFixture.readFile = (path) => path === 'src/Navigation.js'
          ? 'Second file\n'
          : new Promise((resolve) => { finishRefresh = resolve; });
        try {
          document.querySelector('.push-revert').click();
          document.querySelector('#revert-go').click();
          await waitFor(() => finishRefresh);
          document.querySelector('#tab-code').click();
          document.querySelector('[data-path="src"]').click();
          await waitFor(() => document.querySelector('[data-path="src/Navigation.js"]'));
          document.querySelector('[data-path="src/Navigation.js"]').click();
          await waitFor(() => editor()?.value === 'Second file\n');
          finishRefresh('Late change to the first file\n');
          await settle();
          expect(editor()?.value === 'Second file\n', 'The first file overwrote the second file’s buffer');
        } finally {
          finishRefresh?.('');
          delete window.uiFixture.readFile;
        }
      });
    }
    // Reset after assertions for reproducible screenshots.
    document.querySelector('#tab-economy').click(); document.querySelector('#new-session').click(); await settle();
    results.passed = results.checks.filter(check => check.pass).length;
    results.failed = results.checks.length - results.passed;
    results.status = 'complete';
    window.dispatchEvent(new CustomEvent('ui-audit-complete', { detail: results }));
  }
  window.addEventListener('ui-tests-complete', () => run().catch(error => { results.status = 'error'; results.error = error.message; }), { once: true });
})();
