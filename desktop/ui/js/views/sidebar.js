// The session list in the sidebar.

import { h } from '../dom.js';
import { ago } from '../format.js';
import * as store from '../store.js';
import { sessionState, stateGlyph } from './common.js';

/** Everything the list shows that can change, as one string: redraw only when it differs. */
export function listSignature(sessions, currentId) {
  const states = sessions.map((meta) => {
    const state = sessionState(meta, store.get(meta.id));
    return [meta.id, state.phase, state.outcome, state.verified, state.approvals, ago(meta.created)].join(':');
  });
  return `${currentId}|${states.join('|')}`;
}

/** Redraws the list, keeping keyboard focus on the session it was on. */
export function renderSessionList(list, sessions, currentId, onOpen) {
  const focused = document.activeElement?.dataset?.session;
  if (!sessions.length) {
    list.replaceChildren(h('p.side-empty', 'No sessions yet.'));
    return;
  }
  list.replaceChildren(
    ...sessions.map((meta) =>
      h(
        'button.session-item',
        { type: 'button', dataset: { session: meta.id }, 'aria-current': meta.id === currentId ? 'page' : null, onclick: () => onOpen(meta.id) },
        stateGlyph(sessionState(meta, store.get(meta.id))),
        h(
          'span.session-text',
          h('span.session-title', meta.title || 'Untitled session'),
          h('span.session-sub', h('span.session-model', meta.model), meta.simulated && h('span.sim', { title: 'Simulated: a scripted stand-in played the model' }, 'SIM'), h('span.session-time', ago(meta.created))),
        ),
      ),
    ),
  );
  if (focused) list.querySelector(`[data-session="${CSS.escape(focused)}"]`)?.focus();
}
