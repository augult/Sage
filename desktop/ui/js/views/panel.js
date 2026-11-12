// The pane beside the conversation, and the bar that says what it shows: Economy (what the
// session is and what it has cost), Code (the folder's files, to read and edit) or GitHub (where
// the folder stands with git, and the push). The choice is kept from one session to the next.

import { h, icon } from '../dom.js';
import * as prefs from '../prefs.js';
import { codeView } from './code.js';
import { githubView, waiting } from './github.js';
import { renderInspector } from './inspector.js';

const TABS = [
  { id: 'economy', label: 'Economy', icon: 'chart' },
  { id: 'code', label: 'Code', icon: 'code' },
  { id: 'github', label: 'GitHub', icon: 'github' },
];

let chosen = 'economy';

// How wide the pane is while it shows code, once dragged: the editor takes what is added, the
// tree keeps its width (styles/layout.css clamps it to the window).
const root = document.documentElement;
const keptWidth = Number(prefs.get('code-width'));
if (keptWidth > 0) root.style.setProperty('--code-w', `${keptWidth}px`);

/** The edge of the pane by the sheet, which drags its width while it shows code. Double-click goes back to the default. */
function resizer(pane) {
  const grip = h('div.pane-resize', { role: 'separator', 'aria-orientation': 'vertical', 'aria-label': 'Resize the code editor', title: 'Drag to resize · double-click to reset' });
  grip.addEventListener('pointerdown', (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    grip.setPointerCapture(event.pointerId);
    const [startX, startW] = [event.clientX, pane.getBoundingClientRect().width];
    root.dataset.resizing = 'true';
    const move = (moved) => root.style.setProperty('--code-w', `${Math.round(startW + startX - moved.clientX)}px`);
    const end = () => {
      grip.removeEventListener('pointermove', move);
      delete root.dataset.resizing;
      // What it came to, within the window's limits.
      const width = Math.round(pane.getBoundingClientRect().width);
      root.style.setProperty('--code-w', `${width}px`);
      prefs.set('code-width', String(width));
    };
    grip.addEventListener('pointermove', move);
    grip.addEventListener('lostpointercapture', end, { once: true });
  });
  grip.addEventListener('dblclick', () => {
    root.style.removeProperty('--code-w');
    prefs.set('code-width', '');
  });
  return grip;
}

/**
 * `bar` and `body` are the pane's two parts; `session` is `{ id, title }`.
 * Gives `{ update(model, about), runEnded(), unmount() }`; `about` is what `renderInspector` takes.
 */
export function mountPanel(bar, body, session) {
  const economy = h('div.panel-view');
  const count = h('span.tab-count', { hidden: true });
  const tabs = TABS.map((tab) =>
    h(
      'button.tab',
      { id: `tab-${tab.id}`, type: 'button', role: 'tab', 'aria-label': tab.label, onclick: () => show(tab.id) },
      icon(tab.icon, 14),
      h('span.tab-label', tab.label),
      tab.id === 'github' && count,
    ),
  );
  bar.replaceChildren(h('div.tabs', { role: 'tablist', 'aria-label': 'Pane' }, tabs));
  const grip = resizer(body.parentElement);
  body.parentElement.append(grip);

  // Made when first shown, and kept: a file stays open, and a message typed, while another tab is looked at.
  let code = null;
  const github = githubView(session, {
    onState(state) {
      const n = waiting(state);
      count.textContent = String(n);
      count.hidden = n === 0;
    },
    openFile(path) {
      show('code');
      code.open(path);
    },
    onDiscard: () => code?.refresh(),
  });

  function show(id) {
    chosen = id;
    // The pane is as wide as what it shows needs (styles/layout.css).
    document.documentElement.dataset.panel = id;
    tabs.forEach((tab, at) => tab.setAttribute('aria-selected', String(TABS[at].id === id)));
    if (id === 'code') code ??= codeView(session);
    body.replaceChildren(id === 'economy' ? economy : id === 'code' ? code.el : github.el);
    body.dataset.view = id;
    if (id === 'github') github.load();
  }

  let shown = '';
  show(chosen);
  github.load();

  return {
    update(model, about) {
      const { state, skills } = about;
      const now = [model.sideRev, state.phase, state.outcome, state.verified, state.approvals, ...skills].join('|');
      if (now === shown && economy.firstChild) return;
      shown = now;
      renderInspector(economy, model, about);
    },
    /** A run may have changed the files and left something to push. */
    runEnded() {
      github.load();
      code?.refresh();
    },
    unmount() {
      code?.unmount();
      grip.remove();
      bar.replaceChildren();
      delete document.documentElement.dataset.panel;
      delete body.dataset.view;
    },
  };
}
