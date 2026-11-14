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
  const bounds = () => ({ min: 360, max: Math.max(360, innerWidth - parseFloat(getComputedStyle(root).getPropertyValue('--left-w')) - 360) });
  function describe() {
    const { min, max } = bounds();
    grip.setAttribute('aria-valuemin', String(min));
    grip.setAttribute('aria-valuemax', String(Math.round(max)));
    grip.setAttribute('aria-valuenow', String(Math.round(pane.getBoundingClientRect().width)));
  }
  const grip = h('div.pane-resize', { role: 'separator', tabindex: '0', 'aria-orientation': 'vertical', 'aria-label': 'Code editor width', 'data-tooltip': 'Drag or use arrow keys to resize. Double-click to reset.', onfocus: describe });
  grip.addEventListener('keydown', (event) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const { min, max } = bounds();
    const width = pane.getBoundingClientRect().width;
    const next = event.key === 'Home' ? min : event.key === 'End' ? max : width + (event.key === 'ArrowLeft' ? 24 : -24);
    const value = Math.round(Math.max(min, Math.min(next, max)));
    root.style.setProperty('--code-w', `${value}px`);
    prefs.set('code-width', String(value));
    describe();
  });
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
      describe();
    };
    grip.addEventListener('pointermove', move);
    grip.addEventListener('lostpointercapture', end, { once: true });
  });
  grip.addEventListener('dblclick', () => {
    root.style.removeProperty('--code-w');
    prefs.set('code-width', '');
    describe();
  });
  return grip;
}

/**
 * `bar` and `body` are the pane's two parts; `session` is `{ id, title }`, or absent for the empty pane.
 * Gives `{ update(model, about), runEnded(), unmount() }`; `about` is what `renderInspector` takes.
 */
export function mountPanel(bar, body, session) {
  const economy = h('div.panel-view', !session && h('p.side-note', 'Start or choose a session to see anything here.'));
  const count = h('span.tab-count', { hidden: true });
  const tabs = TABS.map((tab) =>
    h(
      'button.tab',
      { id: `tab-${tab.id}`, type: 'button', role: 'tab', 'aria-label': tab.label, 'aria-controls': 'inspector-body', 'data-tooltip': tab.label, 'data-tooltip-when': 'icon', onclick: () => show(tab.id), onkeydown: (event) => {
        const at = TABS.indexOf(tab);
        const next = { ArrowRight: (at + 1) % TABS.length, ArrowLeft: (at + TABS.length - 1) % TABS.length, Home: 0, End: TABS.length - 1 }[event.key];
        if (next === undefined) return;
        event.preventDefault();
        show(TABS[next].id);
        tabs[next].focus();
      } },
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
  const github = session
    ? githubView(session, {
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
      })
    : null;

  function show(id) {
    chosen = id;
    // The pane is as wide as what it shows needs (styles/layout.css).
    document.documentElement.dataset.panel = id;
    tabs.forEach((tab, at) => { const selected = TABS[at].id === id; tab.setAttribute('aria-selected', String(selected)); tab.tabIndex = selected ? 0 : -1; });
    body.setAttribute('role', 'tabpanel');
    body.setAttribute('aria-labelledby', `tab-${id}`);
    if (session && id === 'code') code ??= codeView(session);
    body.replaceChildren(!session || id === 'economy' ? economy : id === 'code' ? code.el : github.el);
    body.dataset.view = id;
    if (id === 'github') github?.load();
  }

  let shown = '';
  show(chosen);
  github?.load();

  return {
    update(model, about) {
      if (!session) return;
      const { state, skills } = about;
      const now = [model.sideRev, state.phase, state.outcome, state.verified, state.approvals, ...skills].join('|');
      if (now === shown && economy.firstChild) return;
      shown = now;
      renderInspector(economy, model, about);
    },
    /** A run may have changed the files and left something to push. */
    runEnded() {
      github?.load();
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
