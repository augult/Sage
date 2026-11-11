// The session list in the sidebar, newest first, under a heading per day.

import { h } from '../dom.js';
import { ago } from '../format.js';
import * as store from '../store.js';
import { sessionState, stateGlyph } from './common.js';

const DAY = 86_400_000;

/** The heading a session falls under, by calendar day. */
function dayGroup(ts, now = Date.now()) {
  const midnight = (time) => new Date(time).setHours(0, 0, 0, 0);
  const days = Math.round((midnight(now) - midnight(ts)) / DAY);
  if (days <= 0) return 'Today';
  if (days === 1) return 'Yesterday';
  return days < 7 ? 'Previous 7 days' : 'Older';
}

/** Everything the list shows that can change, as one string: redraw only when it differs. */
export function listSignature(sessions, currentId) {
  const states = sessions.map((meta) => {
    const state = sessionState(meta, store.get(meta.id));
    return [meta.id, state.phase, state.outcome, state.verified, state.approvals, ago(meta.created), dayGroup(meta.created)].join(':');
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
  const rows = [];
  let group = null;
  for (const meta of sessions) {
    const heading = dayGroup(meta.created);
    if (heading !== group) {
      group = heading;
      rows.push(h('div.side-group', heading));
    }
    rows.push(
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
    );
  }
  list.replaceChildren(...rows);
  if (focused) list.querySelector(`[data-session="${CSS.escape(focused)}"]`)?.focus();
}
