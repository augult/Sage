// The pane beside the conversation, and the bar that says what it shows: Economy (what the
// session is and what it has cost), Code (the folder's files, to read and edit) or GitHub (where
// the folder stands with git, and the push). The choice is kept from one session to the next.

import { h, icon } from '../dom.js';
import { codeView } from './code.js';
import { githubView, waiting } from './github.js';
import { renderInspector } from './inspector.js';

const TABS = [
  { id: 'economy', label: 'Economy', icon: 'chart' },
  { id: 'code', label: 'Code', icon: 'code' },
  { id: 'github', label: 'GitHub', icon: 'github' },
];

let chosen = 'economy';

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
      bar.replaceChildren();
      delete document.documentElement.dataset.panel;
      delete body.dataset.view;
    },
  };
}
