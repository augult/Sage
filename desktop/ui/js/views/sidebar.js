// The session list in the sidebar: a tab per folder, which opens onto that folder's sessions.
// Folders stand in the order of their newest session, and so do the sessions under each.

import { h } from '../dom.js';
import { ago, plainPath, tilde } from '../format.js';
import * as prefs from '../prefs.js';
import * as store from '../store.js';
import { sessionState, stateGlyph } from './common.js';
import { chevron } from './tools.js';

/** The folder a session is listed under. The walkthrough's throwaway workspaces share the one above them. */
function folderOf(meta) {
  const path = plainPath(meta.cwd).replace(/[\\/]+$/, '');
  return meta.simulated && /[\\/]demo[\\/][^\\/]+$/.test(path) ? path.replace(/[\\/][^\\/]+$/, '') : path;
}

/** A name per folder: its own, with its parent's in front where two folders share one. */
function folderNames(paths) {
  const tail = (path, parts) => path.split(/[\\/]/).filter(Boolean).slice(-parts).join('/') || path || 'No folder';
  const taken = new Map();
  for (const path of paths) taken.set(tail(path, 1), (taken.get(tail(path, 1)) ?? 0) + 1);
  return new Map(paths.map((path) => [path, tail(path, taken.get(tail(path, 1)) > 1 ? 2 : 1)]));
}

// The folders the user has shut, kept between launches. Every other one is open.
function shutFolders() {
  try {
    return new Set(JSON.parse(prefs.get('folded', '[]')));
  } catch {
    return new Set();
  }
}

const keepShut = (folders) => prefs.set('folded', JSON.stringify([...folders]));

// The session the list last marked as the open one.
let marked = null;

/** Everything the list shows that can change, as one string: redraw only when it differs. */
export function listSignature(sessions, currentId) {
  const states = sessions.map((meta) => {
    const state = sessionState(meta, store.get(meta.id));
    return [meta.id, state.phase, state.outcome, state.verified, state.approvals, ago(meta.created)].join(':');
  });
  return `${currentId}|${states.join('|')}`;
}

/** Redraws the list, keeping keyboard focus on the folder or session it was on. */
export function renderSessionList(list, sessions, currentId, home, onOpen) {
  const focused = list.contains(document.activeElement) ? document.activeElement.dataset : {};
  if (!sessions.length) {
    list.replaceChildren(h('p.side-empty', 'No sessions'));
    return;
  }
  const folders = new Map();
  for (const meta of sessions) {
    const path = folderOf(meta);
    if (!folders.has(path)) folders.set(path, []);
    folders.get(path).push(meta);
  }
  const names = folderNames([...folders.keys()]);
  const shut = shutFolders();
  // Going to a session opens the folder it is in.
  if (currentId !== marked) {
    marked = currentId;
    const current = sessions.find((meta) => meta.id === currentId);
    if (current && shut.delete(folderOf(current))) keepShut(shut);
  }

  const tabs = [...folders].map(([path, metas]) => {
    const states = metas.map((meta) => sessionState(meta, store.get(meta.id)));
    // What a shut folder must still say: a session in it is waiting for an answer, or running.
    const busy = states.find((state) => state.approvals > 0) ?? states.find((state) => state.phase === 'running');
    const open = !shut.has(path);
    const body = h(
      `div.fold${open ? '.is-open' : ''}`,
      h(
        'div.fold-inner',
        metas.map((meta, i) =>
          h(
            'button.session-item',
            { type: 'button', dataset: { session: meta.id }, 'aria-current': meta.id === currentId ? 'page' : null, onclick: () => onOpen(meta.id) },
            stateGlyph(states[i]),
            h(
              'span.session-text',
              h('span.session-title', meta.title || 'Untitled session'),
              h('span.session-sub', h('span.session-model', meta.model), meta.simulated && h('span.sim', { title: 'Simulated' }, 'SIM'), h('span.session-time', ago(meta.created))),
            ),
          ),
        ),
      ),
    );
    body.inert = !open;
    const toggle = h(
      'button.folder-toggle',
      {
        type: 'button',
        title: tilde(path, home),
        dataset: { folder: path },
        'aria-expanded': String(open),
        // In place, so that the chevron turns and the sessions slide rather than jump.
        onclick: () => {
          const now = toggle.getAttribute('aria-expanded') !== 'true';
          toggle.setAttribute('aria-expanded', String(now));
          body.classList.toggle('is-open', now);
          body.inert = !now;
          const kept = shutFolders();
          if (now) kept.delete(path);
          else kept.add(path);
          keepShut(kept);
        },
      },
      chevron(),
      h('span.folder-name', names.get(path)),
      busy && h('span.folder-state', stateGlyph(busy)),
      h('span.folder-count', String(metas.length)),
    );
    return h('div.side-folder', toggle, body);
  });
  list.replaceChildren(...tabs);
  const again = focused.session ? `[data-session="${CSS.escape(focused.session)}"]` : focused.folder != null ? `[data-folder="${CSS.escape(focused.folder)}"]` : null;
  if (again) list.querySelector(again)?.focus();
}
