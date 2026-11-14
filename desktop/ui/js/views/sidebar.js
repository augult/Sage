// The session list in the sidebar: a tab per folder, which opens onto that folder's sessions.
// Folders stand in the order of their newest session, and so do the sessions under each.
// A session is its title, and after it a mark only while it has something to say. Its right-click
// menu opens it, renames it, stops it while it runs and deletes it; F2 and Delete do the second
// and the last from the keyboard.

import { api } from '../api.js';
import { copyText, h } from '../dom.js';
import { folderNames, plainPath, tilde } from '../format.js';
import * as prefs from '../prefs.js';
import * as store from '../store.js';
import { closeButton, contextMenu, openDialog, sessionState, stateMark } from './common.js';
import { chevron } from './tools.js';

const titleOf = (meta) => meta.title || 'Untitled session';

/** The folder a session is listed under. The walkthrough's throwaway workspaces share the one above them. */
function folderOf(meta) {
  const path = plainPath(meta.cwd).replace(/[\\/]+$/, '');
  return meta.simulated && /[\\/]demo[\\/][^\\/]+$/.test(path) ? path.replace(/[\\/][^\\/]+$/, '') : path;
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
    return [meta.id, meta.title, state.phase, state.outcome, state.approvals].join(':');
  });
  return `${currentId}|${states.join('|')}`;
}

/** A dialog's line for what went wrong, and the way to put a problem into it. */
function problemLine() {
  const error = h('div.form-error', { role: 'alert', hidden: true });
  const fail = (problem) => {
    error.textContent = String(problem);
    error.hidden = !problem;
  };
  return [error, fail];
}

/** Asks for another title. `renamed(meta)` runs once the session has it. */
function askRename(meta, renamed) {
  const [error, fail] = problemLine();
  const box = h('input.input', { id: 'session-title', type: 'text', value: meta.title, maxlength: '72', autocomplete: 'off', spellcheck: 'false', 'aria-label': 'Title' });
  const go = h('button.btn.primary', { id: 'rename-session-go', type: 'submit' }, 'Rename');
  openDialog('rename-session', (close) => {
    async function submit(event) {
      event.preventDefault();
      go.disabled = true;
      fail('');
      try {
        const now = await api.renameSession(meta.id, box.value);
        close();
        renamed(now);
      } catch (problem) {
        fail(problem);
        go.disabled = false;
      }
    }
    return [
      h('div.dialog-head', h('h2', 'Rename session'), closeButton(close)),
      h('form.dialog-form', { onsubmit: submit }, h('div.unsaved-body', box, error), h('div.dialog-foot', h('button.btn', { type: 'button', onclick: close }, 'Cancel'), go)),
    ];
  });
  box.focus();
  box.select();
}

/** Asks before a session is deleted. `deleted(id)` runs once it is gone. */
function askDelete(meta, home, deleted) {
  const [error, fail] = problemLine();
  let go;
  openDialog('confirm-delete', (close) => {
    go = h(
      'button.btn.danger',
      {
        id: 'delete-session-go',
        type: 'button',
        async onclick() {
          go.disabled = true;
          fail('');
          try {
            await api.deleteSession(meta.id);
            close();
            deleted(meta.id);
          } catch (problem) {
            fail(problem);
            go.disabled = false;
          }
        },
      },
      'Delete',
    );
    return [
      h('div.dialog-head', h('h2', 'Delete session?'), closeButton(close)),
      h(
        'div.unsaved-body',
        h('p', h('b', titleOf(meta)), ' and its conversation will be deleted. The files in ', h('span.mono', tilde(meta.cwd, home)), ' are not touched.'),
        h('p.muted', 'This cannot be undone.'),
        error,
      ),
      h('div.dialog-foot', h('button.btn', { type: 'button', onclick: close }, 'Cancel'), go),
    ];
  });
  go.focus();
}

/**
 * Redraws the list, keeping keyboard focus on the folder or session it was on. `acts` is what the
 * shell does for it: `open(id)` shows a session, `renamed(meta)` and `deleted(id)` follow one that
 * was renamed or deleted here.
 */
export function renderSessionList(list, sessions, currentId, home, acts) {
  const focused = list.contains(document.activeElement) ? document.activeElement.dataset : {};
  if (!sessions.length) {
    list.replaceChildren(h('p.side-empty', 'No sessions'));
    return;
  }

  /** What may be done with a session now: one that runs is stopped before it is deleted, and one another process has is that one's. */
  function may(meta) {
    const running = sessionState(meta, store.get(meta.id)).phase === 'running';
    const taken = (store.get(meta.id)?.meta ?? meta).why_closed === 'busy';
    return { running, rename: !taken, remove: !taken && !running };
  }

  function menuAt(event, meta) {
    const can = may(meta);
    contextMenu(
      event,
      [
        { label: 'Open', act: () => acts.open(meta.id) },
        { label: 'Rename', act: () => askRename(meta, acts.renamed), disabled: !can.rename },
        'rule',
        { label: 'Copy folder path', act: () => copyText(plainPath(meta.cwd)) },
        'rule',
        can.running && { label: 'Stop', act: () => api.stopSession(meta.id) },
        { label: 'Delete', act: () => askDelete(meta, home, acts.deleted), danger: true, glyph: 'backspace', disabled: !can.remove, title: can.running ? 'Stop it first' : null },
      ],
      { remove: can.remove && (() => askDelete(meta, home, acts.deleted)), back: () => list.querySelector(`[data-session="${CSS.escape(meta.id)}"]`) },
    );
  }

  /** F2 renames the session that has the focus and Delete deletes it, as in the Code tab's tree. */
  function onRowKey(event, meta) {
    const can = may(meta);
    if (event.key === 'F2' && can.rename) {
      event.preventDefault();
      askRename(meta, acts.renamed);
    } else if (event.key === 'Delete' && can.remove) {
      event.preventDefault();
      askDelete(meta, home, acts.deleted);
    } else if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      const box = event.currentTarget.getBoundingClientRect();
      menuAt({ preventDefault: () => event.preventDefault(), clientX: box.left + 24, clientY: box.bottom }, meta);
    }
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
            {
              type: 'button',
              dataset: { session: meta.id },
              'aria-current': meta.id === currentId ? 'page' : null,
              onclick: () => acts.open(meta.id),
              oncontextmenu: (event) => menuAt(event, meta),
              onkeydown: (event) => onRowKey(event, meta),
            },
            h('span.session-title', titleOf(meta)),
            stateMark(states[i]),
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
      busy && h('span.folder-state', stateMark(busy)),
    );
    return h('div.side-folder', toggle, body);
  });
  list.replaceChildren(...tabs);
  const again = focused.session ? `[data-session="${CSS.escape(focused.session)}"]` : focused.folder != null ? `[data-folder="${CSS.escape(focused.folder)}"]` : null;
  if (again) list.querySelector(again)?.focus();
}
